//! Hide's install kit: the one list of what Hide puts on a machine, and how
//! each part is installed, judged and removed.
//!
//! The same code runs for this Mac, inside `hided`, and for a device, inside
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

mod cli;
mod device;
mod hcoord;
mod hooks;
mod labels;
mod local;
mod payload;
pub mod process;
mod record;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

pub use device::{CURRENT, device_target};
pub use hcoord::{HcoordRuntime, NODE_MINIMUM, find_node};
pub use labels::{LABELS_PLUGIN_ID, labels_home};
pub use local::{STANDALONE_REASON, bundled_kit_dir, local_target};
pub use record::kit_state_dir;

/// One part of the kit.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentId {
    /// The `hide` command, linked in the account's command folder.
    Cli,
    /// Hide's entries in `~/.claude/settings.json`.
    ClaudeCodeHook,
    /// Hide's entries in `~/.codex/hooks.json`.
    CodexHook,
    /// The agent labels Herdr plugin, `hide.agent-context-labels`.
    Labels,
    /// The `hcoord` command and its daemon.
    Hcoord,
}

impl ComponentId {
    pub const ALL: [ComponentId; 5] = [
        Self::Cli,
        Self::ClaudeCodeHook,
        Self::CodexHook,
        Self::Labels,
        Self::Hcoord,
    ];

    /// The stable name the wire and the record carry.
    pub fn code(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::ClaudeCodeHook => "claude_code_hook",
            Self::CodexHook => "codex_hook",
            Self::Labels => "labels",
            Self::Hcoord => "hcoord",
        }
    }

    /// The name the operator reads.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cli => "hide command",
            Self::ClaudeCodeHook => "Claude Code hook",
            Self::CodexHook => "Codex hook",
            Self::Labels => "Agent labels plugin",
            Self::Hcoord => "hcoord",
        }
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
}

/// Every part's state on one machine, in [`ComponentId::ALL`] order.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct KitReport {
    pub components: Vec<ComponentReport>,
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
                })
                .collect(),
        }
    }
}

/// Where the kit acts on one machine.
#[derive(Clone, Debug)]
pub struct KitTarget {
    /// The account's home, whose configuration files the hooks go into.
    pub home: PathBuf,
    /// The folder holding this build's parts: `hide`, `hide-agent-hooks`,
    /// `agent-context-labels/` and `hcoord/dist/`. It is the path the hooks,
    /// the `hide` link and the hcoord shim name, so it must outlive the
    /// process: the app bundle's `Contents/Resources` on this Mac, the helper
    /// root's `current` link on a device (D-11).
    pub kit_dir: PathBuf,
    /// Where `hide` is linked (`~/.local/bin` unless the daemon was told
    /// otherwise).
    pub cli_dir: PathBuf,
    /// Folders whose links are Hide's own besides an app bundle's
    /// `Contents/Resources`, such as a device helper root, so a `hide` link
    /// into one of them is replaced rather than left as the operator's.
    pub owned_roots: Vec<PathBuf>,
    /// The Herdr server the plugin is linked in.
    pub herdr_socket: PathBuf,
    /// The `herdr` CLI, needed only to take out a GitHub install of the
    /// labels plugin; `None` when the machine has none Hide can find.
    pub herdr_bin: Option<PathBuf>,
    /// What runs hcoord on this machine, or why nothing can.
    pub hcoord: Result<HcoordRuntime, String>,
    /// Raised by the kit's owner when it is going away: a child the kit is
    /// waiting on is ended and no further part is started.
    pub stop: Arc<AtomicBool>,
}

/// What an apply is allowed to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Scope {
    /// The launch or connection pass: install what was never installed and
    /// replace what is outdated, and leave alone whatever the operator took
    /// away.
    Automatic,
    /// The operator pressed Reinstall: also bring back these parts when they
    /// were taken away. Parts that are in place are not touched (B8).
    Reinstall(Vec<ComponentId>),
}

impl Scope {
    fn restores(&self, id: ComponentId) -> bool {
        match self {
            Self::Automatic => false,
            Self::Reinstall(ids) => ids.contains(&id),
        }
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
}

fn observe(id: ComponentId, target: &KitTarget) -> Observed {
    match id {
        ComponentId::Cli => cli::observe(target),
        ComponentId::ClaudeCodeHook => {
            hooks::observe(target, hide_agent_hooks::AgentRuntime::ClaudeCode)
        }
        ComponentId::CodexHook => hooks::observe(target, hide_agent_hooks::AgentRuntime::Codex),
        ComponentId::Labels => labels::observe(target),
        ComponentId::Hcoord => hcoord::observe(target),
    }
}

fn install(id: ComponentId, target: &KitTarget) -> Result<(), String> {
    match id {
        ComponentId::Cli => cli::install(target),
        ComponentId::ClaudeCodeHook => {
            hooks::install(target, hide_agent_hooks::AgentRuntime::ClaudeCode)
        }
        ComponentId::CodexHook => hooks::install(target, hide_agent_hooks::AgentRuntime::Codex),
        ComponentId::Labels => labels::install(target),
        ComponentId::Hcoord => hcoord::install(target),
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
        ComponentId::Labels => labels_home(&target.home).display().to_string(),
        ComponentId::Hcoord => hcoord::shim_path(&target.home).display().to_string(),
    }
}

fn report(
    id: ComponentId,
    target: &KitTarget,
    observed: Observed,
    recorded: bool,
    failure: Option<String>,
) -> ComponentReport {
    let (state, reason) = match (failure, observed) {
        (Some(failure), _) => (ComponentState::Failed, Some(failure)),
        (None, Observed::Current) => (ComponentState::Installed, None),
        (None, Observed::Stale(reason)) => (ComponentState::Outdated, Some(reason)),
        (None, Observed::Missing) if recorded => (
            ComponentState::Removed,
            Some("taken out after Hide installed it; Reinstall puts it back".to_owned()),
        ),
        (None, Observed::Missing) => (ComponentState::NotInstalled, None),
        (None, Observed::Blocked(reason)) => (ComponentState::Failed, Some(reason)),
        (None, Observed::Absent(reason)) => (ComponentState::Absent, Some(reason)),
    };
    ComponentReport {
        id,
        state,
        reason,
        location: Some(location(id, target)),
    }
}

/// Judges every part without changing anything.
pub fn status(target: &KitTarget) -> KitReport {
    let record = record::load(&target.home);
    let recorded = |id| record.as_ref().is_ok_and(|record| record.contains(id));
    KitReport {
        components: ComponentId::ALL
            .into_iter()
            .map(|id| report(id, target, observe(id, target), recorded(id), None))
            .collect(),
    }
}

/// Installs what `scope` allows and answers what every part is afterwards.
///
/// Every part is tried whatever happened to the ones before it. A second
/// apply of the same build changes nothing (engineering rule 11), except that
/// hcoord's daemon is asked to converge each time, as the desktop host did on
/// every launch.
pub fn apply(target: &KitTarget, scope: &Scope) -> KitReport {
    let loaded = record::load(&target.home);
    let (mut record, record_failure) = match loaded {
        Ok(record) => (record, None),
        Err(reason) => (record::Record::default(), Some(reason)),
    };
    let mut changed = false;
    let mut components = Vec::with_capacity(ComponentId::ALL.len());
    for id in ComponentId::ALL {
        // The owner is quitting: what is done is recorded, and the rest waits
        // for the next launch or connection.
        if target.stop.load(Ordering::Relaxed) {
            break;
        }
        let observed = observe(id, target);
        let recorded = record.contains(id);
        let install_now = match &observed {
            Observed::Stale(_) => true,
            Observed::Missing => scope.restores(id) || (!recorded && record_failure.is_none()),
            Observed::Current | Observed::Blocked(_) | Observed::Absent(_) => false,
        };
        let mut failure = None;
        if install_now {
            match install(id, target) {
                Ok(()) => {}
                Err(reason) => failure = Some(reason),
            }
        } else if matches!(observed, Observed::Missing) && !recorded {
            // Missing, never recorded, and the record could not be read: the
            // kit cannot tell whether the operator took it away, so it does
            // not put it back on a guess (engineering rule 4).
            failure = record_failure.clone();
        }
        let after = if install_now {
            observe(id, target)
        } else {
            observed
        };
        if failure.is_none() && matches!(after, Observed::Current) && id == ComponentId::Hcoord {
            failure = hcoord::ensure(target).err();
        }
        if matches!(after, Observed::Current) && !record.contains(id) {
            record.insert(id);
            changed = true;
        }
        let recorded_now = record.contains(id);
        components.push(report(id, target, after, recorded_now, failure));
    }
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
    KitReport { components }
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
}

/// Takes Hide's parts off a machine that is being removed from Hide: the
/// hook entries carrying Hide's marker, the labels plugin Hide linked, the
/// `hide` link when it is Hide's, and the record. hcoord stays, because other
/// tools on that machine drive agents through it (D-16).
pub fn remove(target: &KitTarget) -> RemoveReport {
    let mut components = Vec::new();
    for id in ComponentId::ALL {
        let outcome = match id {
            ComponentId::Cli => cli::remove(target),
            ComponentId::ClaudeCodeHook => {
                hooks::remove(target, hide_agent_hooks::AgentRuntime::ClaudeCode)
            }
            ComponentId::CodexHook => hooks::remove(target, hide_agent_hooks::AgentRuntime::Codex),
            ComponentId::Labels => labels::remove(target),
            ComponentId::Hcoord => RemoveOutcome::Kept {
                reason: "hcoord stays, because other tools on this machine use it".to_owned(),
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
    RemoveReport { components }
}

/// Replaces `path` with `contents` through a temporary file beside it, so a
/// failure part way leaves the old file whole.
/// `base` joined with `parts`, each folder checked in turn: the kit keeps
/// code there that launchd and Herdr run as this account, so a folder that
/// belongs to another account or that others can write to is refused rather
/// than trusted. With `create`, a missing folder is made 0700; without it, a
/// missing folder ends the check, since nothing below it can be there yet.
pub(crate) fn private_dirs(base: &Path, parts: &[&str], create: bool) -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    // SAFETY: geteuid has no preconditions and cannot fail.
    let account = unsafe { libc::geteuid() };
    let mut folder = base.to_path_buf();
    for part in parts {
        folder.push(part);
        if create {
            match std::fs::DirBuilder::new().mode(0o700).create(&folder) {
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
        let metadata = match std::fs::metadata(&folder) {
            Ok(metadata) => metadata,
            Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(base.join(parts.join("/")));
            }
            Err(error) => return Err(format!("{} could not be read: {error}", folder.display())),
        };
        if !metadata.is_dir() {
            return Err(format!("{} is not a folder", folder.display()));
        }
        if metadata.uid() != account || metadata.mode() & 0o022 != 0 {
            return Err(format!(
                "{} can be changed by another account, so Hide keeps nothing it runs there",
                folder.display()
            ));
        }
    }
    Ok(folder)
}

pub(crate) fn write_atomically(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("{} could not be created: {error}", parent.display()))?;
    let temporary = parent.join(format!(
        ".{}.hide-kit-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file"),
        std::process::id()
    ));
    let _ = std::fs::remove_file(&temporary);
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    written.map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!("{} could not be written: {error}", path.display())
    })
}

#[cfg(test)]
mod tests;
