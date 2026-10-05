//! Hide's install kit: the one list of what Hide puts on a machine, and how
//! each part is installed, judged and removed.
//!
//! The same code runs for this machine, inside `hided`, and for a device, inside
//! `hide-host-helper`; only the [`KitTarget`] differs, so a device gets
//! exactly what this Mac gets (PRD device-parity D-09, D-10). Adding a part is
//! adding a [`ComponentId`] and its three answers here; nothing else in the
//! product learns how to install it.
//!
//! Three rules hold for every part:
//!
//! - what another tool wrote is never touched: a hook entry without Hide's
//!   marker, a `hide` that is not Hide's link, a plugin config folder (D-15);
//! - a part that failed does not stop the others, and says why (D-18);
//! - a part the kit installed and the operator then took away stays away
//!   until the operator asks for it again with Reinstall (D-20, D-26). The
//!   kit's own record of what it installed is what tells "taken away" from
//!   "never installed"; `~/.hide/agent-hooks/installed-once` from before the
//!   kit is not such a record (B20).

mod agent_kit;
pub mod agents;
mod cli;
mod codex_per_pane;
mod coordination_retirement;
mod device;
mod hooks;
mod labels;
pub mod layout;
pub mod legacy;
mod local;
pub mod process;
mod record;
pub mod retirement_inspection;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use agents::{AgentAdapter, AgentReport, Availability, HookSupport, PieceReport, SkillDir};
pub use coordination_retirement::preflight as retirement_preflight;
pub use device::{CURRENT, device_target};
pub use labels::{HCOORD_PLUGIN_ID, LABELS_PLUGIN_ID, Retirement, labels_home, plugin_state_dir};
pub use legacy::is_build_name;
pub use local::{STANDALONE_REASON, bundled_kit_dir, local_target};
pub use record::kit_state_dir;

/// One part of the kit.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentId {
    /// The `hide` command, installed in the account's command folder.
    Cli,
    /// Hide's entries in `~/.claude/settings.json`.
    ClaudeCodeHook,
    /// Hide's entries in `~/.codex/hooks.json`.
    CodexHook,
    /// One-release retirement of the old coordination installation.
    CoordinationRetirement,
    /// Codex's shared daemon turned off, so each Codex runs in its own pane
    /// (PRD overview-request-view D-21).
    CodexPerPane,
}

impl ComponentId {
    pub const ALL: [ComponentId; 5] = [
        Self::Cli,
        Self::ClaudeCodeHook,
        Self::CodexHook,
        Self::CoordinationRetirement,
        Self::CodexPerPane,
    ];

    /// The stable name the wire and the record carry.
    pub fn code(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::ClaudeCodeHook => "claude_code_hook",
            Self::CodexHook => "codex_hook",
            Self::CoordinationRetirement => "coordination_retirement",
            Self::CodexPerPane => "codex_per_pane",
        }
    }

    /// The name the operator reads.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cli => "hide command",
            Self::ClaudeCodeHook => "Claude Code hook",
            Self::CodexHook => "Codex hook",
            Self::CoordinationRetirement => "Coordination retirement",
            Self::CodexPerPane => "Codex를 pane마다 실행",
        }
    }

    /// Whether the operator can turn the part off from its row, which then
    /// stays off until they turn it on (D-24). Every other part is taken
    /// away by hand and brought back with Reinstall.
    pub fn can_turn_off(self) -> bool {
        matches!(self, Self::CodexPerPane)
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.code() == code)
    }
}

/// What one part is on the machine now.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentState {
    /// This build's part is in place.
    Installed,
    /// A part is there, but not this build's: an older version, another
    /// path, or a copy installed by hand. The next launch or connection
    /// replaces it.
    Outdated,
    /// Never installed here.
    NotInstalled,
    /// The kit installed it and it has since gone; only Reinstall brings it
    /// back.
    Removed,
    /// It could not be installed or judged; `reason` says why.
    Failed,
    /// The machine has nothing for this part to attach to, such as an agent
    /// runtime that is not set up there.
    Absent,
    /// The operator turned the part off, or undid it outside Hide; no pass
    /// puts it back and nothing asks them to (D-24, B36).
    Off,
}

impl ComponentState {
    /// Whether the machine row offers Reinstall for this part.
    pub fn needs_attention(self) -> bool {
        matches!(
            self,
            Self::Outdated | Self::NotInstalled | Self::Removed | Self::Failed
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComponentReport {
    pub id: ComponentId,
    pub state: ComponentState,
    /// Why the part is not installed, in one sentence; absent when it is.
    #[serde(default)]
    pub reason: Option<String>,
    /// The file, link or folder the part lives at on that machine.
    #[serde(default)]
    pub location: Option<String>,
    /// The actual Codex binary supports the shared daemon setting. Unknown
    /// on a missing or failed probe; independent of this part's switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_daemon: Option<bool>,
}

/// Every part's state on one machine, in [`ComponentId::ALL`] order.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct KitReport {
    pub components: Vec<ComponentReport>,
    /// Every agent Hide has an adapter for, in the adapters' order (issue
    /// #517); empty in a report from a build that predates them.
    #[serde(default)]
    pub agents: Vec<AgentReport>,
    /// What an apply took out of the retired labels plugin; empty when there
    /// was nothing of it (PRD labels-in-hided D-12).
    #[serde(default, skip_serializing_if = "Retirement::is_empty")]
    pub labels_retirement: Retirement,
    /// What an apply took off the machine from older layouts: the
    /// standalone hcoord Herdr plugin and the folders `~/.hide` replaced;
    /// empty when there was nothing (PRD hide-home-layout D-13, D-14).
    #[serde(default, skip_serializing_if = "Retirement::is_empty")]
    pub legacy_retirement: Retirement,
}

impl KitReport {
    pub fn component(&self, id: ComponentId) -> Option<&ComponentReport> {
        self.components.iter().find(|report| report.id == id)
    }

    /// The same report for a machine the kit cannot run on, with one reason
    /// for every part (a standalone daemon, a platform with no package).
    pub fn unavailable(reason: &str) -> Self {
        Self {
            components: ComponentId::ALL
                .into_iter()
                .map(|id| ComponentReport {
                    id,
                    state: ComponentState::Failed,
                    reason: Some(reason.to_owned()),
                    location: None,
                    codex_daemon: None,
                })
                .collect(),
            agents: Vec::new(),
            labels_retirement: Retirement::default(),
            legacy_retirement: Retirement::default(),
        }
    }
}

/// Where the kit acts on one machine.
#[derive(Clone, Debug)]
pub struct KitTarget {
    /// The account's home, whose configuration files the hooks go into.
    pub home: PathBuf,
    /// The folder holding `hide` and `hide-agent-hooks`. The hooks and
    /// command link name this folder, so it must outlive the
    /// process: the desktop package's resources on this machine, the helper
    /// root's `current` link on a device (D-11).
    pub kit_dir: PathBuf,
    /// Where the `hide` command is installed (`~/.local/bin` unless the daemon was told
    /// otherwise).
    pub cli_dir: PathBuf,
    /// Folders whose links are Hide's own besides an app bundle's
    /// `Contents/Resources`, such as a device helper root, so a `hide` link
    /// into one of them is replaced rather than left as the operator's.
    pub owned_roots: Vec<PathBuf>,
    /// The machine's Herdr server, which the retired labels plugin is taken
    /// out of.
    pub herdr_socket: PathBuf,
    /// The `herdr` CLI, needed only to take out a GitHub install of the
    /// retired labels plugin; `None` when the machine has none Hide can find.
    pub herdr_bin: Option<PathBuf>,
    /// The machine's `codex`, `None` when Hide finds none.
    pub codex: Option<PathBuf>,
    /// Legacy relocation, inspected only by the one-release retirement.
    pub legacy_coordination_home: Option<PathBuf>,
    /// Account login-agent boundary. Fixtures inject a stand-in command.
    pub user_agents: hide_platform::user_agents::UserAgents,
    /// Registered checkout roots included in retirement preflight.
    pub retirement_projects: Vec<PathBuf>,
    /// Folders of older layouts the pass takes off this machine (D-13).
    pub legacy: Vec<legacy::Legacy>,
    /// Raised by the kit's owner when it is going away: a child the kit is
    /// waiting on is ended and no further part is started.
    pub stop: Arc<AtomicBool>,
}

/// What an apply is allowed to do. The launch or connection pass is the
/// empty scope: it installs what was never installed and replaces what is
/// outdated, and leaves alone whatever the operator took away or turned off.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Scope {
    /// Parts the operator asked back, with Reinstall or by turning one on:
    /// they are installed again even when they were taken away. Parts that
    /// are in place are not touched (B8).
    pub restore: BTreeSet<ComponentId>,
    /// Parts the operator turned off ([`ComponentId::can_turn_off`]).
    pub turn_off: BTreeSet<ComponentId>,
    /// Agents the operator switched on, by adapter id: the choice is
    /// recorded and each of the agent's pieces is installed again, even one
    /// that was taken away (issue #517).
    pub agent_on: BTreeSet<String>,
    /// Agents the operator switched off: their marked entries and stubs come
    /// out and no pass puts them back.
    pub agent_off: BTreeSet<String>,
}

impl Scope {
    /// The launch or connection pass.
    pub fn automatic() -> Self {
        Self::default()
    }

    pub fn reinstall(parts: impl IntoIterator<Item = ComponentId>) -> Self {
        Self {
            restore: parts.into_iter().collect(),
            ..Self::default()
        }
    }

    pub fn turn_off(parts: impl IntoIterator<Item = ComponentId>) -> Self {
        Self {
            turn_off: parts.into_iter().collect(),
            ..Self::default()
        }
    }

    /// The operator's per-agent choices: `on` switched on (or Reinstall of an
    /// agent that is on), `off` switched off.
    pub fn agents<'a>(
        on: impl IntoIterator<Item = &'a str>,
        off: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        Self {
            agent_on: on.into_iter().map(str::to_owned).collect(),
            agent_off: off.into_iter().map(str::to_owned).collect(),
            ..Self::default()
        }
        .settled()
    }

    /// An agent named both ways keeps its later choice; the caller passes
    /// the later one in `agent_off` or `agent_on` last, so here the switch
    /// off wins only when both were asked in one request.
    fn settled(mut self) -> Self {
        let both: Vec<String> = self
            .agent_on
            .intersection(&self.agent_off)
            .cloned()
            .collect();
        for id in both {
            self.agent_on.remove(&id);
        }
        self
    }

    pub fn is_automatic(&self) -> bool {
        self.restore.is_empty()
            && self.turn_off.is_empty()
            && self.agent_on.is_empty()
            && self.agent_off.is_empty()
    }

    /// Two requests for one machine as one; for a part named by both, the
    /// later choice wins.
    pub fn merge(mut self, later: Scope) -> Scope {
        for id in later.restore {
            self.turn_off.remove(&id);
            self.restore.insert(id);
        }
        for id in later.turn_off {
            self.restore.remove(&id);
            self.turn_off.insert(id);
        }
        for id in later.agent_on {
            self.agent_off.remove(&id);
            self.agent_on.insert(id);
        }
        for id in later.agent_off {
            self.agent_on.remove(&id);
            self.agent_off.insert(id);
        }
        self
    }

    fn restores(&self, id: ComponentId) -> bool {
        self.restore.contains(&id)
    }
}

/// What one part looks like before the kit decides anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Observed {
    Current,
    /// There, but not this build's; the sentence says what is there.
    Stale(String),
    Missing,
    /// Neither installed nor installable as things stand.
    Blocked(String),
    /// Nothing on the machine for the part to attach to.
    Absent(String),
    /// A Codex was read and has no shared daemon setting.
    Unsupported(String),
    /// A supported Codex was read, but its account is not set up yet.
    SupportedAbsent(String),
}

fn observe(id: ComponentId, target: &KitTarget) -> Observed {
    match id {
        ComponentId::Cli => cli::observe(target),
        ComponentId::ClaudeCodeHook => {
            hooks::observe(target, hide_agent_hooks::AgentRuntime::ClaudeCode)
        }
        ComponentId::CodexHook => hooks::observe(target, hide_agent_hooks::AgentRuntime::Codex),
        ComponentId::CoordinationRetirement => coordination_retirement::observe(target),
        ComponentId::CodexPerPane => codex_per_pane::observe(target),
    }
}

fn install(id: ComponentId, target: &KitTarget) -> Result<(), String> {
    match id {
        ComponentId::Cli => cli::install(target),
        ComponentId::ClaudeCodeHook => {
            hooks::install(target, hide_agent_hooks::AgentRuntime::ClaudeCode)
        }
        ComponentId::CodexHook => hooks::install(target, hide_agent_hooks::AgentRuntime::Codex),
        ComponentId::CoordinationRetirement => coordination_retirement::install(target).map(|_| ()),
        ComponentId::CodexPerPane => codex_per_pane::install(target),
    }
}

/// Undoes a part the operator turned off; only the parts
/// [`ComponentId::can_turn_off`] names have an undo.
fn turn_off(id: ComponentId, target: &KitTarget) -> Result<(), String> {
    match id {
        ComponentId::CodexPerPane => codex_per_pane::turn_off(target),
        // An agent's switch turns its hook part off (`agent_kit::part_gate`).
        ComponentId::ClaudeCodeHook => {
            hooks::turn_off(target, hide_agent_hooks::AgentRuntime::ClaudeCode)
        }
        ComponentId::CodexHook => hooks::turn_off(target, hide_agent_hooks::AgentRuntime::Codex),
        ComponentId::Cli | ComponentId::CoordinationRetirement => {
            Err(format!("{} cannot be turned off", id.label()))
        }
    }
}

fn location(id: ComponentId, target: &KitTarget) -> String {
    match id {
        ComponentId::Cli => cli::link_path(target).display().to_string(),
        ComponentId::ClaudeCodeHook => hide_agent_hooks::AgentRuntime::ClaudeCode
            .config_path(&target.home)
            .display()
            .to_string(),
        ComponentId::CodexHook => hide_agent_hooks::AgentRuntime::Codex
            .config_path(&target.home)
            .display()
            .to_string(),
        ComponentId::CoordinationRetirement => coordination_retirement::location(target)
            .display()
            .to_string(),
        ComponentId::CodexPerPane => codex_per_pane::location(target).display().to_string(),
    }
}

fn report(
    id: ComponentId,
    target: &KitTarget,
    observed: Observed,
    recorded: bool,
    switched_off: bool,
    failure: Option<String>,
) -> ComponentReport {
    let codex_daemon = (id == ComponentId::CodexPerPane)
        .then_some(match &observed {
            Observed::Current | Observed::Missing | Observed::SupportedAbsent(_) => Some(true),
            Observed::Unsupported(_) => Some(false),
            _ => None,
        })
        .flatten();
    let (state, reason) = match (failure, observed) {
        (Some(failure), _) => (ComponentState::Failed, Some(failure)),
        (None, Observed::Current) if id == ComponentId::CodexPerPane => {
            (ComponentState::Installed, codex_per_pane::note(target))
        }
        (None, Observed::Current) => (ComponentState::Installed, None),
        (None, Observed::Stale(reason)) => (ComponentState::Outdated, Some(reason)),
        // Gone after Hide applied it: the operator turned it off or undid it
        // by hand, and either way it is theirs now (B36).
        (None, Observed::Missing) if recorded && (id.can_turn_off() || switched_off) => {
            (ComponentState::Off, None)
        }
        (None, Observed::Missing) if recorded => (
            ComponentState::Removed,
            Some("taken out after Hide installed it; Reinstall puts it back".to_owned()),
        ),
        (None, Observed::Missing) => (ComponentState::NotInstalled, None),
        (None, Observed::Blocked(reason)) => (ComponentState::Failed, Some(reason)),
        (
            None,
            Observed::Absent(reason)
            | Observed::Unsupported(reason)
            | Observed::SupportedAbsent(reason),
        ) => (ComponentState::Absent, Some(reason)),
    };
    ComponentReport {
        id,
        state,
        reason,
        location: Some(location(id, target)),
        codex_daemon,
    }
}

/// How long a kit waits for another kit changing the same account.
const LOCK_DEADLINE: Duration = Duration::from_secs(90);

/// One kit at a time changes an account's files. Two registrations of one
/// machine connect together at launch, and a Mac that is also another Mac's
/// device runs its own kit beside the device's; their copies, hook files and
/// retirement would otherwise interleave. The lock is an advisory lock on a
/// file in the kit's private folder, so it ends with the process that held it.
pub(crate) struct AccountLock {
    _lock: hide_platform::fs::lock::Lock,
}

pub(crate) fn lock_account(target: &KitTarget) -> Result<AccountLock, String> {
    use hide_platform::fs::lock::{Mode, Waited, lock_file};
    let path = record::private_state_dir(&target.home, true)?.join(".lock");
    let file = hide_platform::fs::private::open_or_create_file(&path)
        .map_err(|error| format!("{} could not be opened: {error}", path.display()))?;
    match lock_file(file, Mode::Exclusive, LOCK_DEADLINE, &|| {
        target.stop.load(Ordering::Relaxed)
    }) {
        Ok(Waited::Locked(lock)) => Ok(AccountLock { _lock: lock }),
        Ok(Waited::Cancelled) => Err("Hide is quitting".to_owned()),
        Ok(Waited::TimedOut) => Err(format!(
            "another Hide was still changing this account's kit after {} seconds",
            LOCK_DEADLINE.as_secs()
        )),
        Err(error) => Err(format!("{} could not be locked: {error}", path.display())),
    }
}

/// Judges every part and every agent without changing anything.
pub fn status(target: &KitTarget) -> KitReport {
    if let Err(reason) = record::private_state_dir(&target.home, false) {
        return retirement_blocked(target, reason);
    }
    let record = record::load(&target.home);
    let recorded = |id| record.as_ref().is_ok_and(|record| record.contains(id));
    let switched_off = |id| {
        record
            .as_ref()
            .is_ok_and(|record| agent_kit::part_is_off(record, id))
    };
    let components: Vec<ComponentReport> = ComponentId::ALL
        .into_iter()
        .map(|id| {
            report(
                id,
                target,
                observe(id, target),
                recorded(id),
                switched_off(id),
                None,
            )
        })
        .collect();
    let agents = agent_kit::status(target, &record, &agent_kit::part_views(&components));
    KitReport {
        components,
        agents,
        labels_retirement: Retirement::default(),
        legacy_retirement: Retirement::default(),
    }
}

/// Installs what `scope` allows and answers what every part is afterwards.
///
/// Every part is tried whatever happened to the ones before it. A second
/// apply of the same build changes nothing (engineering rule 11).
/// Retirement preflight precedes even creation of the account lock.
pub fn apply(target: &KitTarget, scope: &Scope) -> KitReport {
    if let Err(reason) = retirement_preflight(target) {
        return retirement_blocked(target, reason);
    }
    let _lock = match lock_account(target) {
        Ok(lock) => lock,
        Err(reason) => return KitReport::unavailable(&reason),
    };
    let loaded = record::load(&target.home);
    let (mut record, record_failure) = match loaded {
        Ok(record) => (record, None),
        Err(reason) => (record::Record::default(), Some(reason)),
    };
    let (mut legacy_retirement, retirement_failure) = match coordination_retirement::install(target)
    {
        Ok(report) => (report, None),
        Err(reason) => (Retirement::default(), Some(reason)),
    };
    let labels_retirement = labels::retire(target);
    let mut changed = false;
    if retirement_failure.is_none() && !record.has_retired(HCOORD_PLUGIN_ID) {
        record.mark_retired(HCOORD_PLUGIN_ID);
        changed = true;
    }
    let mut components = Vec::with_capacity(ComponentId::ALL.len());
    for id in ComponentId::ALL {
        // The owner is quitting: what is done is recorded, and the rest waits
        // for the next launch or connection.
        if target.stop.load(Ordering::Relaxed) {
            break;
        }
        let observed = observe(id, target);
        if id == ComponentId::CoordinationRetirement {
            if matches!(observed, Observed::Current) && !record.contains(id) {
                record.insert(id);
                changed = true;
            }
            components.push(report(
                id,
                target,
                observed,
                record.contains(id),
                false,
                retirement_failure.clone(),
            ));
            continue;
        }
        let recorded = record.contains(id);
        // An agent's switch governs the hook part that is its hook: off, the
        // part is never installed or replaced; switched off now, it comes
        // out; switched on now, it is Reinstall's (issue #517).
        let gate = agent_kit::part_gate(&record, scope, id);
        let agent_off = gate.is_some_and(|gate| !gate.enabled);
        let turning_off = scope.turn_off.contains(&id) || gate.is_some_and(|gate| gate.turning_off);
        let restoring = scope.restores(id) || gate.is_some_and(|gate| gate.turning_on);
        let install_now = !turning_off
            && !agent_off
            && match &observed {
                Observed::Stale(_) => true,
                Observed::Missing => restoring || (!recorded && record_failure.is_none()),
                Observed::Current
                | Observed::Blocked(_)
                | Observed::Absent(_)
                | Observed::Unsupported(_)
                | Observed::SupportedAbsent(_) => false,
            };
        let undo_now = turning_off && matches!(observed, Observed::Current | Observed::Stale(_));
        let mut failure = None;
        if install_now {
            match install(id, target) {
                Ok(()) => {}
                Err(reason) => failure = Some(reason),
            }
        } else if undo_now {
            failure = turn_off(id, target).err();
        } else if turning_off || agent_off {
            // Nothing of it is in place, so it is already off.
        } else if matches!(observed, Observed::Missing) && !recorded {
            // Missing, never recorded, and the record could not be read: the
            // kit cannot tell whether the operator took it away, so it does
            // not put it back on a guess (engineering rule 4).
            failure = record_failure.clone();
        }
        let after = if install_now || undo_now {
            observe(id, target)
        } else {
            observed
        };
        // A part the operator turned off is recorded too, so the next pass
        // reads it as theirs rather than as never installed.
        let applied = matches!(after, Observed::Current)
            || (turning_off && matches!(after, Observed::Missing));
        if applied && !record.contains(id) {
            record.insert(id);
            changed = true;
        }
        if matches!(after, Observed::Current) && id == ComponentId::Cli {
            changed |= record.remember_cli(cli::wanted(target));
        }
        let recorded_now = record.contains(id);
        let off = agent_off || agent_kit::part_is_off(&record, id);
        components.push(report(id, target, after, recorded_now, off, failure));
    }
    let (agents, agents_changed) = agent_kit::apply(
        target,
        scope,
        &mut record,
        record_failure.as_ref(),
        &agent_kit::part_views(&components),
    );
    changed |= agents_changed;
    if changed
        && record_failure.is_none()
        && let Err(reason) = record::save(&target.home, &record)
    {
        // The parts are in place; only the memory of having installed them
        // is not, so the next pass reinstalls rather than leaving one out.
        // Each part's row stays as it is.
        for component in &mut components {
            if component.state == ComponentState::Installed {
                component.reason = Some(format!("Installed, but {reason}"));
            }
        }
    }
    // Last, so a hook or link the pass just moved off a legacy folder no
    // longer names it.
    let folders = legacy::retire(target);
    legacy_retirement.removed.extend(folders.removed);
    legacy_retirement.failures.extend(folders.failures);
    KitReport {
        components,
        agents,
        labels_retirement,
        legacy_retirement,
    }
}

/// A preflight refusal, without inspecting any other part or external service.
pub fn retirement_blocked(target: &KitTarget, reason: String) -> KitReport {
    let mut report = KitReport::unavailable(&reason);
    if let Some(component) = report
        .components
        .iter_mut()
        .find(|component| component.id == ComponentId::CoordinationRetirement)
    {
        component.location = Some(
            coordination_retirement::location(target)
                .display()
                .to_string(),
        );
    }
    report
}

/// What removing the kit did to one part.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RemoveOutcome {
    Removed,
    /// Nothing of Hide's was there.
    Absent,
    /// Left in place on purpose; the sentence says why.
    Kept {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoveReport {
    pub components: Vec<(ComponentId, RemoveOutcome)>,
    /// What removing took off for the agents: each guidance hook
    /// (`hook:<agent>`) and each skill folder (`skill:<folder>`).
    #[serde(default)]
    pub agents: Vec<(String, RemoveOutcome)>,
}

/// Takes Hide's parts off a machine that is being removed from Hide: the
/// hook entries carrying Hide's marker, the `hide` link when it is Hide's,
/// and the record. Preserved retirement folders stay in place.
pub fn remove(target: &KitTarget) -> RemoveReport {
    let _lock = match lock_account(target) {
        Ok(lock) => lock,
        Err(reason) => {
            return RemoveReport {
                components: ComponentId::ALL
                    .into_iter()
                    .map(|id| {
                        let reason = reason.clone();
                        (id, RemoveOutcome::Failed { reason })
                    })
                    .collect(),
                agents: Vec::new(),
            };
        }
    };
    let mut components = Vec::new();
    for id in ComponentId::ALL {
        let outcome = match id {
            ComponentId::Cli => cli::remove(target),
            ComponentId::ClaudeCodeHook => {
                hooks::remove(target, hide_agent_hooks::AgentRuntime::ClaudeCode)
            }
            ComponentId::CodexHook => hooks::remove(target, hide_agent_hooks::AgentRuntime::Codex),
            ComponentId::CoordinationRetirement => RemoveOutcome::Kept {
                reason: "retirement preserves the old ledger folder".to_owned(),
            },
            // Hide cannot tell its own switch from the operator's choice, and
            // a Codex left per pane harms nothing.
            ComponentId::CodexPerPane => RemoveOutcome::Kept {
                reason: format!(
                    "Codex keeps running per pane; `codex features enable {}` gives it its shared daemon back",
                    hide_agent_hooks::codex_daemon::DAEMON_FEATURE
                ),
            },
        };
        components.push((id, outcome));
    }
    if let Err(reason) = record::forget(&target.home) {
        components.push((
            ComponentId::Cli,
            RemoveOutcome::Failed {
                reason: format!("the install record stayed: {reason}"),
            },
        ));
    }
    let agents = agent_kit::remove(target);
    RemoveReport { components, agents }
}

/// `base` joined with `parts`, each folder checked in turn: the kit keeps
/// code there that launchd and Herdr run as this account, so a folder that
/// is a link, belongs to another account or that others can write to is
/// refused rather than trusted. With `create`, a missing folder is made 0700;
/// without it, a missing folder ends the check, since nothing below it can be
/// there yet.
pub(crate) fn private_dirs(base: &Path, parts: &[&str], create: bool) -> Result<PathBuf, String> {
    use hide_platform::fs::private;
    let mut folder = base.to_path_buf();
    for part in parts {
        folder.push(part);
        if create {
            match private::create_dir(&folder) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!(
                        "{} could not be created: {error}",
                        folder.display()
                    ));
                }
            }
        }
        let metadata = match std::fs::symlink_metadata(&folder) {
            Ok(metadata) => metadata,
            Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(base.join(parts.join("/")));
            }
            Err(error) => return Err(format!("{} could not be read: {error}", folder.display())),
        };
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "{} is a link; Hide requires an owned real folder",
                folder.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!("{} is not a folder", folder.display()));
        }
        let trusted = private::owned_by_current_user(&folder)
            .and_then(|owned| Ok(owned && !private::others_can_modify(&folder)?))
            .map_err(|error| format!("{} could not be checked: {error}", folder.display()))?;
        if !trusted {
            return Err(format!(
                "{} can be changed by another account, so Hide keeps nothing it runs there",
                folder.display()
            ));
        }
    }
    Ok(folder)
}

/// Replaces `path` with `contents` through a temporary file beside it, so a
/// failure part way leaves the old file whole.
pub(crate) fn write_atomically(
    path: &Path,
    contents: &[u8],
    access: hide_platform::fs::Access,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("{} could not be created: {error}", parent.display()))?;
    hide_platform::fs::atomic::write_file(path, contents, access)
        .map(drop)
        .map_err(|error| format!("{} could not be written: {error}", path.display()))
}

// The suite drives the kit against a stand-in Herdr on a Unix socket and with
// shell scripts for the programs it installs, so it runs where those do; the
// Windows kit is tested with the device helper that installs it there.
#[cfg(all(test, unix))]
mod tests;
