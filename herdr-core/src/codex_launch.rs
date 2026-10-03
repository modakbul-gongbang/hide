//! `--no-daemon` on every Codex Hide starts (PRD overview-request-view D-20).
//!
//! A Codex attached to the shared app-server daemon runs its hooks in the
//! daemon's environment, so Herdr never learns that pane's session and Hide
//! cannot read it (openai/codex#48500). Every start Hide makes, a new agent,
//! a start from an issue or PR, a reopen, a fork and a wake, goes through
//! [`crate::wire::agent_start_params`], which takes the machine's answer here
//! and puts the flag first. Whether the operator turned the kit's
//! `Codex를 pane마다 실행` off does not matter: that switch is about a Codex
//! started by hand (D-24).
//!
//! An older Codex has no daemon and refuses the flag, so the flag goes only
//! where the machine's kit read a Codex that has the daemon setting; a
//! machine whose kit has not answered yet starts Codex as it always did.
//!
//! A transition path: it goes away with the kit part once openai/codex#48500
//! runs a daemon's hooks in each window's environment (D-26).

use hide_kit::{ComponentId, ComponentState};

use crate::model::KitSnapshot;

pub(crate) const NO_DAEMON: &str = "--no-daemon";

/// What a machine's kit last read about its Codex's shared daemon.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum CodexDaemon {
    /// No Codex there, one without the daemon, or no answer yet.
    #[default]
    Unknown,
    /// That Codex has the shared daemon and the `--no-daemon` flag.
    Present,
}

impl CodexDaemon {
    /// The machine's answer from its kit's last report.
    pub(crate) fn from_kit(kit: &KitSnapshot) -> Self {
        let state = kit
            .components
            .iter()
            .find(|part| part.id == ComponentId::CodexPerPane)
            .map(|part| part.state);
        match state {
            // Each of these read a `daemon_auto_start` setting, on or off.
            Some(
                ComponentState::Installed
                | ComponentState::Off
                | ComponentState::NotInstalled
                | ComponentState::Removed,
            ) => Self::Present,
            // Nothing to attach to, or the setting could not be read.
            Some(ComponentState::Absent | ComponentState::Failed | ComponentState::Outdated)
            | None => Self::Unknown,
        }
    }
}

/// `args` for a start of `kind`, with `--no-daemon` first for a Codex that
/// has the daemon, once (the caller may already carry it).
pub(crate) fn start_arguments(kind: &str, daemon: CodexDaemon, args: Vec<String>) -> Vec<String> {
    let codex = kind.eq_ignore_ascii_case("codex");
    if !codex || daemon != CodexDaemon::Present || args.iter().any(|arg| arg == NO_DAEMON) {
        return args;
    }
    std::iter::once(NO_DAEMON.to_owned()).chain(args).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::KitComponentSnapshot;

    fn kit(state: ComponentState) -> KitSnapshot {
        KitSnapshot {
            components: vec![KitComponentSnapshot {
                id: ComponentId::CodexPerPane,
                label: ComponentId::CodexPerPane.label().to_owned(),
                state,
                reason: None,
                location: None,
            }],
            ..KitSnapshot::default()
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn a_codex_with_the_daemon_starts_without_it_on_every_kind_of_start() {
        let present = CodexDaemon::from_kit(&kit(ComponentState::Installed));
        assert_eq!(
            start_arguments("codex", present, strings(&["resume", "abc"])),
            strings(&["--no-daemon", "resume", "abc"])
        );
        assert_eq!(
            start_arguments("codex", present, strings(&["--", "fix the bug"])),
            strings(&["--no-daemon", "--", "fix the bug"])
        );
        // The operator's switch is about a Codex started by hand.
        let off = CodexDaemon::from_kit(&kit(ComponentState::Off));
        assert_eq!(
            start_arguments("codex", off, Vec::new()),
            strings(&["--no-daemon"])
        );
        // Never twice.
        assert_eq!(
            start_arguments("codex", present, strings(&["--no-daemon"])),
            strings(&["--no-daemon"])
        );
    }

    #[test]
    fn an_older_codex_or_an_unread_machine_or_another_agent_gets_no_flag() {
        for kit in [
            kit(ComponentState::Absent),
            kit(ComponentState::Failed),
            KitSnapshot::default(),
        ] {
            assert_eq!(
                start_arguments(
                    "codex",
                    CodexDaemon::from_kit(&kit),
                    strings(&["resume", "a"])
                ),
                strings(&["resume", "a"])
            );
        }
        assert_eq!(
            start_arguments("claude", CodexDaemon::Present, strings(&["--resume", "a"])),
            strings(&["--resume", "a"])
        );
    }
}
