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
//! machine whose kit has not answered yet refuses a start with a next action.
//!
//! A transition path: it goes away with the kit part once openai/codex#48500
//! runs a daemon's hooks in each window's environment (D-26).

use hide_kit::{ComponentId, ComponentState};

use crate::model::KitSnapshot;

pub(crate) const NO_DAEMON: &str = "--no-daemon";

/// What a machine's kit last read about its Codex's shared daemon.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum CodexDaemon {
    /// No answer, no Codex there, or a failed capability read.
    #[default]
    Unknown,
    /// The actual binary was read and has no shared daemon setting.
    Unsupported,
    /// That Codex has the shared daemon and the `--no-daemon` flag.
    Present,
}

impl CodexDaemon {
    /// The machine's answer from its kit's last report.
    pub(crate) fn from_kit(kit: &KitSnapshot) -> Self {
        if kit.unavailable.is_some() {
            return Self::Unknown;
        }
        let Some(part) = kit
            .components
            .iter()
            .find(|part| part.id == ComponentId::CodexPerPane)
        else {
            return Self::Unknown;
        };
        match part.state {
            // These states come only from an actual daemon setting read.
            ComponentState::Installed
            | ComponentState::Off
            | ComponentState::NotInstalled
            | ComponentState::Removed => Self::Present,
            ComponentState::Absent => match part.codex_daemon {
                Some(true) => Self::Present,
                Some(false) => Self::Unsupported,
                None => Self::Unknown,
            },
            ComponentState::Failed | ComponentState::Outdated => Self::Unknown,
        }
    }
}

/// `args` for a start of `kind`, with `--no-daemon` first for a Codex that
/// has the daemon, once (the caller may already carry it).
pub(crate) fn start_arguments(
    kind: &str,
    daemon: CodexDaemon,
    args: Vec<String>,
) -> Result<Vec<String>, String> {
    if !kind.eq_ignore_ascii_case("codex") {
        return Ok(args);
    }
    match daemon {
        CodexDaemon::Unknown => Err("Codex 지원 여부를 확인하지 못했습니다. Settings에서 대상 머신의 키트를 다시 확인한 뒤 실행하세요.".to_owned()),
        CodexDaemon::Unsupported => Ok(args),
        CodexDaemon::Present if args.iter().take_while(|arg| arg.as_str() != "--").any(|arg| arg == NO_DAEMON) => Ok(args),
        CodexDaemon::Present => Ok(std::iter::once(NO_DAEMON.to_owned()).chain(args).collect()),
    }
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
                codex_daemon: None,
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
            Ok(strings(&["--no-daemon", "resume", "abc"]))
        );
        assert_eq!(
            start_arguments("codex", present, strings(&["--", "fix the bug"])),
            Ok(strings(&["--no-daemon", "--", "fix the bug"]))
        );
        // The operator's switch is about a Codex started by hand.
        let off = CodexDaemon::from_kit(&kit(ComponentState::Off));
        assert_eq!(
            start_arguments("codex", off, Vec::new()),
            Ok(strings(&["--no-daemon"]))
        );
        // Never twice.
        assert_eq!(
            start_arguments("codex", present, strings(&["--no-daemon"])),
            Ok(strings(&["--no-daemon"]))
        );
    }

    #[test]
    fn only_a_confirmed_older_codex_gets_no_flag() {
        let mut older = kit(ComponentState::Absent);
        older.components[0].codex_daemon = Some(false);
        assert_eq!(
            start_arguments(
                "codex",
                CodexDaemon::from_kit(&older),
                strings(&["resume", "a"])
            ),
            Ok(strings(&["resume", "a"]))
        );
        older.components[0].codex_daemon = Some(true);
        assert_eq!(
            start_arguments("codex", CodexDaemon::from_kit(&older), vec![]),
            Ok(strings(&["--no-daemon"]))
        );
        assert_eq!(
            start_arguments("claude", CodexDaemon::Unknown, strings(&["--resume", "a"])),
            Ok(strings(&["--resume", "a"]))
        );
    }

    #[test]
    fn unread_failed_missing_and_outdated_capability_refuse_the_common_start_boundary() {
        for kit in [
            kit(ComponentState::Absent),
            kit(ComponentState::Failed),
            kit(ComponentState::Outdated),
            KitSnapshot::default(),
        ] {
            for args in [vec![], strings(&["resume", "a"]), strings(&["--no-daemon"])] {
                let error = crate::wire::agent_start_params(
                    "pane",
                    "agent",
                    "codex",
                    args,
                    CodexDaemon::from_kit(&kit),
                )
                .unwrap_err();
                assert!(
                    error.contains("Settings") && error.contains("대상 머신"),
                    "{error}"
                );
            }
        }
    }

    #[test]
    fn a_prompt_that_mentions_the_flag_is_not_a_process_flag() {
        assert_eq!(
            start_arguments(
                "codex",
                CodexDaemon::Present,
                strings(&["--", "--no-daemon"])
            ),
            Ok(strings(&["--no-daemon", "--", "--no-daemon"]))
        );
    }
}
