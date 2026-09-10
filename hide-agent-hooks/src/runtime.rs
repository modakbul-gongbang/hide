//! Which agent runtimes Hide instruments, and the vocabulary it writes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The marker Hide stamps into every hook entry it installs.
///
/// It is carried as the `--source` argument of the helper invocation, so one
/// substring in the command line answers both questions the diagnosis asks:
/// whether Hide owns this entry, and which version of the entry it is (PRD
/// D-31, D-57). Nothing outside this crate may write it.
pub const HOOK_SOURCE_NAME: &str = "hide-subagents";

/// The hook helper's file name, as the bundle ships it. The app looks for it
/// beside its own executable and hands the path to [`crate::install`], so the
/// name is stated once here rather than in the app and the build script both.
pub const HELPER_BINARY_NAME: &str = "hide-agent-hooks";

/// The version of the installed entry. Raise it when the command Hide writes
/// changes shape, so an older entry is reported as outdated and the operator
/// is offered a reinstall rather than being silently left with a hook that
/// reports nothing.
pub const HOOK_VERSION: u32 = 1;

/// The token that says this pane's session is running Hide's hook at all.
///
/// It is written once at `SessionStart` and carries the hook version, so a
/// pane with no children still proves it is instrumented. Without it, "this
/// agent is working alone" and "Hide cannot see this agent's children" would
/// be the same empty screen (PRD B21, B23, D-61).
pub const INSTRUMENTED_TOKEN: &str = "hide_hooks";

/// The count of subagents this pane's session has running right now.
pub const WORKING_TOKEN: &str = "hide_sub_working";

/// The count of subagents this pane's session has finished.
pub const DONE_TOKEN: &str = "hide_sub_done";

/// The count of subagents this pane's session reports as blocked.
///
/// No shipped adapter observes it, so it is never written today. The reader
/// treats an absent count as unknown and draws the uninstrumented mark rather
/// than a zero (PRD B32, D-53).
pub const BLOCKED_TOKEN: &str = "hide_sub_blocked";

/// `hide-subagents@1`, the exact value the helper hands Herdr as its source.
pub fn hook_source_id() -> String {
    format!("{HOOK_SOURCE_NAME}@{HOOK_VERSION}")
}

/// Reads the version out of a `hide-subagents@N` source id.
///
/// Returns `None` for anything that is not Hide's marker, which is how a
/// third party's entry is left alone.
pub fn parse_source_version(value: &str) -> Option<u32> {
    let rest = value.strip_prefix(HOOK_SOURCE_NAME)?;
    let digits = rest.strip_prefix('@')?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Finds Hide's marker anywhere in a hook entry's command line.
///
/// The command is the only field both runtimes agree on, so ownership is read
/// from it rather than from a key of Hide's own invention that a runtime's
/// settings validator might reject.
pub fn marker_version_in(command: &str) -> Option<u32> {
    let mut cursor = command;
    while let Some(index) = cursor.find(HOOK_SOURCE_NAME) {
        let candidate = &cursor[index..];
        let end = candidate
            .find(|character: char| {
                !character.is_ascii_alphanumeric() && character != '-' && character != '@'
            })
            .unwrap_or(candidate.len());
        if let Some(version) = parse_source_version(&candidate[..end]) {
            return Some(version);
        }
        cursor = &cursor[index + HOOK_SOURCE_NAME.len()..];
    }
    None
}

/// An agent runtime whose global hook configuration Hide can instrument.
///
/// Adding a runtime is adding a variant here and the three answers below; no
/// other file in the product learns a new configuration format (PRD D-47).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentRuntime {
    ClaudeCode,
    Codex,
}

impl AgentRuntime {
    pub const ALL: [AgentRuntime; 2] = [Self::ClaudeCode, Self::Codex];

    /// The stable identifier used in state, diagnostics and the wire.
    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    /// Reads an id back. The wire carries the id, so this is the one place
    /// that turns it into a runtime rather than each caller matching strings.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|runtime| runtime.id() == id)
    }

    /// The name the operator sees.
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|runtime| runtime.id() == value)
    }

    /// The directory whose presence means this runtime is set up on this Mac.
    ///
    /// Hide installs nothing for a runtime the operator does not use: writing
    /// `~/.codex/hooks.json` on a machine with no Codex would create a file
    /// for a tool that is not there.
    pub fn home_directory(self, home: &Path) -> PathBuf {
        match self {
            Self::ClaudeCode => home.join(".claude"),
            Self::Codex => home.join(".codex"),
        }
    }

    /// The file whose `hooks` object Hide appends to.
    pub fn config_path(self, home: &Path) -> PathBuf {
        match self {
            Self::ClaudeCode => self.home_directory(home).join("settings.json"),
            Self::Codex => self.home_directory(home).join("hooks.json"),
        }
    }
}

/// The hook events Hide registers.
///
/// These four are exactly the events both shipped runtimes declare, verified
/// against the installed Claude Code binary and the Codex configuration on
/// 2026-09-09 (PRD D-09). An event only one runtime has is not registered:
/// a key a runtime does not know is a key its settings validator may reject,
/// and the sweep at `Stop` already covers what `SessionEnd` would.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookEvent {
    /// A new session took over this pane: the pane's counts start again.
    SessionStart,
    /// One subagent started.
    SubagentStart,
    /// One subagent finished.
    SubagentStop,
    /// The turn ended, so nothing this session spawned is still running. This
    /// is what clears a count left behind by a `SubagentStop` that never
    /// arrived (PRD B31, D-53).
    Stop,
}

impl HookEvent {
    pub const ALL: [HookEvent; 4] = [
        Self::SessionStart,
        Self::SubagentStart,
        Self::SubagentStop,
        Self::Stop,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::Stop => "Stop",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.name() == value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_is_found_with_its_version_and_only_for_hides_own_entries() {
        let command = "'/Applications/hide.app/Contents/MacOS/hide-agent-hooks' hook \
                       --event SubagentStart --source hide-subagents@1";
        assert_eq!(marker_version_in(command), Some(1));
        assert_eq!(
            marker_version_in("/opt/other/hook.sh --source other-tool@4"),
            None
        );
        assert_eq!(
            marker_version_in("/opt/other/hide-subagents-lookalike.sh"),
            None
        );
    }

    #[test]
    fn a_later_version_is_read_back_verbatim() {
        assert_eq!(parse_source_version("hide-subagents@12"), Some(12));
        assert_eq!(parse_source_version("hide-subagents@"), None);
        assert_eq!(parse_source_version("hide-subagents"), None);
    }

    #[test]
    fn every_runtime_answers_a_distinct_config_path_under_the_given_home() {
        let home = Path::new("/Users/example");
        assert_eq!(
            AgentRuntime::ClaudeCode.config_path(home),
            Path::new("/Users/example/.claude/settings.json")
        );
        assert_eq!(
            AgentRuntime::Codex.config_path(home),
            Path::new("/Users/example/.codex/hooks.json")
        );
    }
}
