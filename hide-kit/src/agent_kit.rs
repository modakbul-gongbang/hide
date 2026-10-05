//! What the kit does for each agent the operator switched on: the skill stub
//! and, for an agent with a guidance hook, that hook (issue #517).
//!
//! It follows the rules every other part follows (`crate` docs): what
//! another tool wrote is never touched, a piece that failed does not stop
//! the others and says why, and a piece the kit installed and the operator
//! then took away stays away until Reinstall. The record's `agents` map is
//! the operator's choice; its `installed` set holds `hook:<agent>` and
//! `skill:<folder>` for what the kit has put down.
//!
//! Claude Code and Codex keep their hook as a kit part ([`HookSupport::Part`])
//! because Memory, pane diagnosis and the device rows read it there. Their
//! switch gates that part here ([`part_gate`]); everything else about the
//! part is the code that already ran.

use std::path::Path;

use hide_agent_hooks::guidance::GuidanceAgent;
use hide_agent_hooks::{HookStatus, InstallFailure};

use crate::agents::{
    ADAPTERS, AgentAdapter, AgentReport, Detection, HookSupport, PieceReport, SkillDir,
    SkillObserved, adapter_of_part, availability, install_skill, observe_skill, program_version,
    remove_skill, skill_location,
};
use crate::record::Record;
use crate::{ComponentId, ComponentState, KitTarget, Observed, RemoveOutcome, Scope};

/// A kit part's id, state, reason and location, as the agent whose hook it
/// is reports it.
pub(crate) type PartView = (ComponentId, ComponentState, Option<String>, Option<String>);

fn hook_code(adapter: &AgentAdapter) -> String {
    format!("hook:{}", adapter.id)
}

fn skill_code(dir: SkillDir) -> String {
    format!("skill:{}", dir.code())
}

/// Whether the agent is on: the operator's choice made in `scope` now, else
/// the one on record, else what its adapter does without a choice.
pub(crate) fn enabled(record: &Record, scope: &Scope, adapter: &AgentAdapter) -> bool {
    if scope.agent_off.contains(adapter.id) {
        return false;
    }
    if scope.agent_on.contains(adapter.id) {
        return true;
    }
    record
        .agent_choice(adapter.id)
        .unwrap_or(adapter.default_on)
}

/// A machine the kit has never been applied to waits for the operator's first
/// choice before any agent gets anything: the agents that are on by default
/// are recorded off, the record is marked as awaiting the choice, and an
/// operator's switch on in the same pass still wins. A machine with a record
/// keeps what it had, so an upgrade changes nothing. The mark lives in the
/// record, so it outlasts a quit between the hold and the answer.
/// True when this pass changed the record.
pub(crate) fn hold_for_onboarding(record: &mut Record) -> bool {
    if !record.is_fresh() {
        return false;
    }
    for adapter in ADAPTERS.iter().filter(|adapter| adapter.default_on) {
        record.set_agent_choice(adapter.id, false);
    }
    record.await_choice()
}

/// A scope that names an agent is the operator's explicit choice, which
/// answers the first-run question (the choices themselves are recorded by
/// [`apply`]).
fn answers_choice(scope: &Scope) -> bool {
    !scope.agent_on.is_empty() || !scope.agent_off.is_empty()
}

/// What the agent switch says about the kit part that is its hook.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PartGate {
    /// The agent is on, so the part may be installed.
    pub(crate) enabled: bool,
    /// The operator is switching the agent off in this scope.
    pub(crate) turning_off: bool,
    /// The operator is switching the agent on in this scope.
    pub(crate) turning_on: bool,
}

/// The gate of a part an agent switch governs, `None` for any other part.
/// The hook parts follow their agent's switch in every way; Codex's
/// per-pane setting follows only whether Codex is on, so a Mac that has not
/// answered the first-run choice, or has Codex off, is not written to, while
/// the part's own switch keeps its own choices.
pub(crate) fn part_gate(record: &Record, scope: &Scope, part: ComponentId) -> Option<PartGate> {
    if part == ComponentId::CodexPerPane {
        let codex = crate::agents::adapter("codex")?;
        return Some(PartGate {
            enabled: enabled(record, scope, codex),
            turning_off: false,
            turning_on: false,
        });
    }
    let adapter = adapter_of_part(part)?;
    Some(PartGate {
        enabled: enabled(record, scope, adapter),
        turning_off: scope.agent_off.contains(adapter.id),
        turning_on: scope.agent_on.contains(adapter.id),
    })
}

/// Whether a part that is gone is the operator's switch rather than a
/// removal by hand: only when the agent that governs it is off.
pub(crate) fn part_is_off(record: &Record, part: ComponentId) -> bool {
    part_gate(record, &Scope::default(), part).is_some_and(|gate| !gate.enabled)
}

// --- A guidance hook -----------------------------------------------------------

fn helper(target: &KitTarget) -> std::path::PathBuf {
    target.kit_dir.join(hide_agent_hooks::HELPER_BINARY_NAME)
}

fn observe_guidance(
    target: &KitTarget,
    adapter: &AgentAdapter,
    agent: GuidanceAgent,
    detection: &Detection,
) -> Observed {
    use hide_agent_hooks::guidance;
    if let Err(why) = agent.supported_here() {
        return Observed::Unsupported(format!(
            "{}'s hook is not written here: {why}",
            adapter.label
        ));
    }
    let observed = match guidance::status(agent, &target.home) {
        HookStatus::RuntimeAbsent => {
            return Observed::Absent(format!("{} is not set up on this machine", adapter.label));
        }
        // Reading a file never says Off: the kit's record does (`HookStatus::Off`).
        HookStatus::NotInstalled | HookStatus::Off => Observed::Missing,
        HookStatus::Outdated { version } => Observed::Stale(format!(
            "version {version} of the hook is there; this build writes version {}",
            guidance::GUIDANCE_VERSION
        )),
        HookStatus::Failed {
            reason: InstallFailure::HelperMissing { helper, .. },
        } => Observed::Stale(format!("the hook points at {helper}, which is gone")),
        HookStatus::Failed { reason } => return Observed::Blocked(reason.message()),
        HookStatus::Installed { .. } => {
            let wanted = helper(target);
            match guidance::installed_helper_path(agent, &target.home) {
                Some(found) if Path::new(&found) == wanted => Observed::Current,
                Some(found) => Observed::Stale(format!(
                    "the hook runs {found}, not this build's {}",
                    wanted.display()
                )),
                None => Observed::Stale("the hook's command is not one Hide wrote".to_owned()),
            }
        }
    };
    if matches!(observed, Observed::Current) {
        return observed;
    }
    if !helper(target).is_file() {
        return Observed::Blocked(format!(
            "this build has no hook helper at {}",
            helper(target).display()
        ));
    }
    match version_gate(adapter, detection) {
        Ok(()) => observed,
        // Nothing a Reinstall can change: the agent is too old, or its
        // version cannot be read, so the row says why and is not repairable.
        Err(reason) => Observed::Unsupported(reason),
    }
}

/// An agent with a documented minimum version gets the hook only when its
/// CLI answers a version at or above it; one with none has no gate.
fn version_gate(adapter: &AgentAdapter, detection: &Detection) -> Result<(), String> {
    let Some(minimum) = adapter.min_version else {
        return Ok(());
    };
    let version = detection.executable(adapter).and_then(program_version);
    match version {
        Some(version) if hide_agent_hooks::version_at_least(&version, minimum) => Ok(()),
        Some(version) => Err(format!(
            "{} {version} is older than {minimum}, which Hide's hook needs; update it",
            adapter.label
        )),
        None => Err(format!(
            "Hide could not read {}'s version, and its hook needs {minimum} or newer; the skill is in place",
            adapter.label
        )),
    }
}

fn install_guidance(target: &KitTarget, agent: GuidanceAgent) -> Result<(), String> {
    hide_agent_hooks::guidance::install(agent, &target.home, &helper(target))
        .map(|_| ())
        .map_err(|failure| failure.message())
}

fn remove_guidance(target: &KitTarget, agent: GuidanceAgent) -> RemoveOutcome {
    match hide_agent_hooks::guidance::remove(agent, &target.home) {
        Ok(outcome) if outcome.removed_entries > 0 => RemoveOutcome::Removed,
        Ok(_) => RemoveOutcome::Absent,
        Err(failure) => RemoveOutcome::Failed {
            reason: failure.message(),
        },
    }
}

// --- One pass ------------------------------------------------------------------

fn dir_state(
    observed: &SkillObserved,
    recorded: bool,
    failure: Option<String>,
) -> (ComponentState, Option<String>) {
    if let Some(reason) = failure {
        return (ComponentState::Failed, Some(reason));
    }
    match observed {
        SkillObserved::Current => (ComponentState::Installed, None),
        SkillObserved::Stale => (
            ComponentState::Outdated,
            Some("an older version of the skill is there".to_owned()),
        ),
        SkillObserved::Edited => (
            ComponentState::Outdated,
            Some(
                "the skill was edited after Hide wrote it; Reinstall puts Hide's text back"
                    .to_owned(),
            ),
        ),
        SkillObserved::Missing if recorded => (
            ComponentState::Removed,
            Some("taken out after Hide installed it; Reinstall puts it back".to_owned()),
        ),
        SkillObserved::Missing => (ComponentState::NotInstalled, None),
        SkillObserved::Foreign => (
            ComponentState::Absent,
            Some(format!(
                "a skill named {} that Hide did not write is already there; Hide left it alone",
                crate::agents::SKILL_NAME
            )),
        ),
        SkillObserved::Unreadable(reason) => (ComponentState::Failed, Some(reason.clone())),
    }
}

/// Judges every agent without changing anything.
pub(crate) fn status(
    target: &KitTarget,
    record: &Result<Record, String>,
    parts: &[PartView],
) -> Vec<AgentReport> {
    let empty = Record::default();
    let scope = Scope::default();
    let readable = record.as_ref().unwrap_or(&empty);
    let detection = Detection::probe(&target.home);
    let dirs = observe_dirs(target, readable, &scope, &detection);
    ADAPTERS
        .iter()
        .map(|adapter| {
            let on = enabled(readable, &scope, adapter);
            report_agent(
                target,
                adapter,
                readable,
                on,
                &dirs,
                &detection,
                None,
                record.is_ok(),
                parts,
            )
        })
        .collect()
}

/// The skill folders' states, with a folder wanted only while an agent that
/// reads it is on and set up here.
fn observe_dirs(
    target: &KitTarget,
    record: &Record,
    scope: &Scope,
    detection: &Detection,
) -> Vec<(SkillDir, SkillObserved, bool)> {
    SkillDir::ALL
        .into_iter()
        .map(|dir| {
            let wanted = ADAPTERS.iter().any(|adapter| {
                adapter.skill_dir == dir
                    && adapter.skill_supported()
                    && enabled(record, scope, adapter)
                    && detection.detected(adapter)
                    && dir.writable(&target.home)
            });
            (dir, observe_skill(dir, &target.home), wanted)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn report_agent(
    target: &KitTarget,
    adapter: &AgentAdapter,
    record: &Record,
    on: bool,
    dirs: &[(SkillDir, SkillObserved, bool)],
    detection: &Detection,
    failures: Option<&AgentFailures>,
    record_readable: bool,
    parts: &[PartView],
) -> AgentReport {
    let detected = detection.detected(adapter);
    // The switch works while either piece can be put here: Codex on a system
    // with no documented skill folder still has its hook to switch off.
    let availability = availability(
        detected,
        adapter.skill_supported(),
        adapter.hook_supported(),
    );
    let skill = if !on {
        let location = skill_location(adapter.skill_dir, &target.home);
        // A switch-off whose removal failed, or that left Hide's stub behind,
        // is not Off: the stub is still read by the agent.
        let left = failures
            .and_then(|failures| failures.skill.get(&adapter.skill_dir).cloned())
            .or_else(|| left_behind(record, adapter, dirs, &location));
        match left {
            Some(reason) => PieceReport {
                state: ComponentState::Failed,
                reason: Some(reason),
                location: Some(location),
            },
            None => PieceReport {
                state: ComponentState::Off,
                reason: None,
                location: Some(location),
            },
        }
    } else if !detected || !adapter.skill_supported() {
        PieceReport {
            state: ComponentState::Absent,
            reason: Some(if detected {
                format!(
                    "{}'s documentation gives no skill folder for this system",
                    adapter.label
                )
            } else {
                format!("{} is not set up on this machine", adapter.label)
            }),
            location: None,
        }
    } else {
        let observed = dirs
            .iter()
            .find(|(dir, _, _)| *dir == adapter.skill_dir)
            .map(|(_, observed, _)| observed.clone())
            .unwrap_or(SkillObserved::Missing);
        let recorded = record.contains_piece(&skill_code(adapter.skill_dir));
        let failure = failures.and_then(|failures| failures.skill.get(&adapter.skill_dir).cloned());
        let (state, reason) = if observed == SkillObserved::Missing
            && failure.is_none()
            && !adapter.skill_dir.writable(&target.home)
        {
            // Hide does not create an agent's own folder, so a Reinstall
            // cannot help until the agent has made it.
            (
                ComponentState::Absent,
                Some(format!(
                    "{} has not created its own folder yet; the skill is put in once it has",
                    adapter.label
                )),
            )
        } else {
            dir_state(&observed, recorded, failure)
        };
        let reason = match (state, record_readable) {
            (ComponentState::NotInstalled, false) => {
                Some("Hide's record of what it installed could not be read".to_owned())
            }
            _ => reason,
        };
        PieceReport {
            state,
            reason,
            location: Some(skill_location(adapter.skill_dir, &target.home)),
        }
    };
    let hook = match adapter.hook {
        HookSupport::None => None,
        HookSupport::Part(part) => {
            let found = parts.iter().find(|(id, ..)| *id == part);
            Some(match found {
                Some((_, state, reason, location)) => PieceReport {
                    state: *state,
                    reason: reason.clone(),
                    location: location.clone(),
                },
                None => PieceReport {
                    state: ComponentState::Absent,
                    reason: None,
                    location: None,
                },
            })
        }
        HookSupport::Guidance(agent) => Some(guidance_piece(
            target, adapter, agent, record, on, detection, failures,
        )),
    };
    AgentReport {
        id: adapter.id.to_owned(),
        label: adapter.label.to_owned(),
        availability,
        enabled: on,
        skill,
        hook,
        doc_url: adapter.doc_url.to_owned(),
    }
}

/// Hide's skill stub is still in the folder of an agent the operator switched
/// off, with no agent that is on and set up reading it: the removal failed
/// or never ran, and the row must not read Off over it.
fn left_behind(
    record: &Record,
    adapter: &AgentAdapter,
    dirs: &[(SkillDir, SkillObserved, bool)],
    location: &str,
) -> Option<String> {
    if record.agent_choice(adapter.id) != Some(false)
        || !record.contains_piece(&skill_code(adapter.skill_dir))
    {
        return None;
    }
    let (_, observed, wanted) = dirs.iter().find(|(dir, _, _)| *dir == adapter.skill_dir)?;
    (!wanted && matches!(observed, SkillObserved::Current | SkillObserved::Stale)).then(|| {
        format!(
            "Hide's skill is still in {location}; switch the agent on and off again to remove it"
        )
    })
}

fn guidance_piece(
    target: &KitTarget,
    adapter: &AgentAdapter,
    agent: GuidanceAgent,
    record: &Record,
    on: bool,
    detection: &Detection,
    failures: Option<&AgentFailures>,
) -> PieceReport {
    let location = Some(agent.config_path(&target.home).display().to_string());
    if !on {
        let left = failures
            .and_then(|failures| failures.hook.get(adapter.id).cloned())
            .or_else(|| {
                matches!(
                    hide_agent_hooks::guidance::status(agent, &target.home),
                    HookStatus::Installed { .. } | HookStatus::Outdated { .. }
                )
                .then(|| {
                    "Hide's entry is still in the file; switch the agent on and off again to remove it"
                        .to_owned()
                })
            });
        return match left {
            Some(reason) => PieceReport {
                state: ComponentState::Failed,
                reason: Some(reason),
                location,
            },
            None => PieceReport {
                state: ComponentState::Off,
                reason: None,
                location,
            },
        };
    }
    if !detection.detected(adapter) {
        return PieceReport {
            state: ComponentState::Absent,
            reason: Some(format!("{} is not set up on this machine", adapter.label)),
            location: None,
        };
    }
    let recorded = record.contains_piece(&hook_code(adapter));
    let (state, reason) = match failures.and_then(|failures| failures.hook.get(adapter.id)) {
        Some(reason) => (ComponentState::Failed, Some(reason.clone())),
        None => match observe_guidance(target, adapter, agent, detection) {
            Observed::Current => (ComponentState::Installed, None),
            Observed::Stale(reason) => (ComponentState::Outdated, Some(reason)),
            Observed::Missing if recorded => (
                ComponentState::Removed,
                Some("taken out after Hide installed it; Reinstall puts it back".to_owned()),
            ),
            Observed::Missing => (ComponentState::NotInstalled, None),
            Observed::Blocked(reason) => (ComponentState::Failed, Some(reason)),
            Observed::Absent(reason)
            | Observed::Unsupported(reason)
            | Observed::SupportedAbsent(reason) => (ComponentState::Absent, Some(reason)),
        },
    };
    PieceReport {
        state,
        reason,
        location,
    }
}

/// Why a piece that was tried failed, by agent and folder.
#[derive(Default)]
struct AgentFailures {
    hook: std::collections::BTreeMap<&'static str, String>,
    skill: std::collections::BTreeMap<SkillDir, String>,
}

/// Applies the operator's per-agent choices in `scope` and what the pass
/// owes every agent that is on; answers every agent afterwards. `parts` are
/// the kit parts' reports, for the agents whose hook is one.
pub(crate) fn apply(
    target: &KitTarget,
    scope: &Scope,
    record: &mut Record,
    record_failure: Option<&String>,
    parts: &[PartView],
) -> (Vec<AgentReport>, bool) {
    let mut changed = false;
    let mut failures = AgentFailures::default();
    let record_readable = record_failure.is_none();
    // Looked for once: every decision below reads this, not the PATH.
    let detection = Detection::probe(&target.home);

    // What the operator chose now is the record's, whatever else this pass
    // can or cannot do for the agent. A switch for an agent that is not set
    // up here has nothing to switch.
    if record_readable && answers_choice(scope) {
        changed |= record.answer_choice();
    }
    for adapter in ADAPTERS {
        let detected = detection.detected(adapter);
        if record_readable {
            if scope.agent_on.contains(adapter.id) && detected {
                changed |= record.set_agent_choice(adapter.id, true);
            } else if scope.agent_off.contains(adapter.id) {
                changed |= record.set_agent_choice(adapter.id, false);
            }
        }
    }

    // Guidance hooks.
    for adapter in ADAPTERS {
        let HookSupport::Guidance(agent) = adapter.hook else {
            continue;
        };
        let on = enabled(record, scope, adapter);
        let detected = detection.detected(adapter);
        let code = hook_code(adapter);
        if !on {
            if scope.agent_off.contains(adapter.id) {
                match hide_agent_hooks::guidance::remove(agent, &target.home) {
                    Ok(_) => changed |= record.forget_piece(&code),
                    Err(failure) => {
                        failures.hook.insert(adapter.id, failure.message());
                    }
                }
            }
            continue;
        }
        if !detected {
            continue;
        }
        let observed = observe_guidance(target, adapter, agent, &detection);
        let recorded = record.contains_piece(&code);
        let restore = scope.agent_on.contains(adapter.id);
        let install_now = match &observed {
            Observed::Stale(_) => true,
            Observed::Missing => restore || (!recorded && record_readable),
            _ => false,
        };
        if install_now && let Err(reason) = install_guidance(target, agent) {
            failures.hook.insert(adapter.id, reason);
            continue;
        }
        let after = if install_now {
            observe_guidance(target, adapter, agent, &detection)
        } else {
            observed
        };
        if matches!(after, Observed::Current) && record_readable {
            changed |= record.insert_piece(&code);
        }
    }

    // Skill folders, shared by every agent that reads one.
    for dir in SkillDir::ALL {
        let readers = || {
            ADAPTERS
                .iter()
                .filter(move |adapter| adapter.skill_dir == dir)
        };
        let wanted = readers().any(|adapter| {
            adapter.skill_supported()
                && enabled(record, scope, adapter)
                && detection.detected(adapter)
                && dir.writable(&target.home)
        });
        let switched_off = readers().any(|adapter| scope.agent_off.contains(adapter.id));
        let switched_on = readers()
            .any(|adapter| scope.agent_on.contains(adapter.id) && detection.detected(adapter));
        let code = skill_code(dir);
        if wanted {
            let observed = observe_skill(dir, &target.home);
            let recorded = record.contains_piece(&code);
            let install_now = match &observed {
                SkillObserved::Stale => true,
                // The operator's edit is theirs until Reinstall.
                SkillObserved::Edited => switched_on,
                SkillObserved::Missing => switched_on || (!recorded && record_readable),
                _ => false,
            };
            if install_now && let Err(reason) = install_skill(dir, &target.home) {
                failures.skill.insert(dir, reason);
                continue;
            }
            let after = if install_now {
                observe_skill(dir, &target.home)
            } else {
                observed
            };
            if matches!(after, SkillObserved::Current) && record_readable {
                changed |= record.insert_piece(&code);
            }
        } else if switched_off {
            match remove_skill(dir, &target.home) {
                Ok(_) => changed |= record.forget_piece(&code),
                Err(reason) => {
                    failures.skill.insert(dir, reason);
                }
            }
        }
    }

    let dirs = observe_dirs(target, record, scope, &detection);
    let reports = ADAPTERS
        .iter()
        .map(|adapter| {
            let on = enabled(record, scope, adapter);
            report_agent(
                target,
                adapter,
                record,
                on,
                &dirs,
                &detection,
                Some(&failures),
                record_readable,
                parts,
            )
        })
        .collect();
    (reports, changed)
}

/// Takes every Hide piece off a machine: each guidance hook's marked
/// entries and each skill stub that is Hide's.
pub(crate) fn remove(target: &KitTarget) -> Vec<(String, RemoveOutcome)> {
    let mut outcomes = Vec::new();
    for adapter in ADAPTERS {
        if let HookSupport::Guidance(agent) = adapter.hook {
            outcomes.push((
                format!("hook:{}", adapter.id),
                remove_guidance(target, agent),
            ));
        }
    }
    for dir in SkillDir::ALL {
        let outcome = match remove_skill(dir, &target.home) {
            Ok(true) => RemoveOutcome::Removed,
            Ok(false) => RemoveOutcome::Absent,
            Err(reason) => RemoveOutcome::Failed { reason },
        };
        outcomes.push((skill_code(dir), outcome));
    }
    outcomes
}

/// The kit parts' reports, in the shape [`apply`] and [`status`] take.
pub(crate) fn part_views(components: &[crate::ComponentReport]) -> Vec<PartView> {
    components
        .iter()
        .map(|part| {
            (
                part.id,
                part.state,
                part.reason.clone(),
                part.location.clone(),
            )
        })
        .collect()
}
