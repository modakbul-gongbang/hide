//! Handing a pane's counts to Herdr as display-only metadata.
//!
//! `herdr pane report-metadata` is the whole integration: Herdr stores the
//! tokens on the pane, returns them in `PaneInfo`, and Hide's ordinary
//! `session.snapshot` carries them to the core. No socket method, event
//! subscription or poll is added by this path (PRD D-08, D-27, D-33).

use std::io;
use std::path::PathBuf;
use std::process::Command;

use crate::counters::PaneCounters;
use crate::runtime::{DONE_TOKEN, HOOK_VERSION, INSTRUMENTED_TOKEN, WORKING_TOKEN};

/// The Herdr executable to report through.
///
/// Herdr sets `HERDR_BIN_PATH` in every pane it owns, so a hook running
/// inside a pane always has the exact binary that owns the session. The
/// fallback is only for a hook fired outside one, where reporting is a no-op
/// anyway because there is no pane id.
pub fn herdr_binary() -> PathBuf {
    std::env::var_os("HERDR_BIN_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("herdr"))
}

/// The arguments `herdr` is invoked with, as a value a test can inspect.
///
/// Building them apart from running them is what lets the contract be
/// asserted without a Herdr server (engineering rule 12).
pub fn report_arguments(pane_id: &str, source: &str, counters: PaneCounters) -> Vec<String> {
    vec![
        "pane".to_owned(),
        "report-metadata".to_owned(),
        "--source".to_owned(),
        source.to_owned(),
        "--token".to_owned(),
        format!("{INSTRUMENTED_TOKEN}={HOOK_VERSION}"),
        "--token".to_owned(),
        format!("{WORKING_TOKEN}={}", counters.working),
        "--token".to_owned(),
        format!("{DONE_TOKEN}={}", counters.done),
        pane_id.to_owned(),
    ]
}

/// Reports one pane's counts. The caller decides what a failure means; the
/// hook entry point swallows it, because the operator-visible outcome of a
/// hook that cannot reach Herdr is the pane reading as uninstrumented, not a
/// broken agent turn (PRD B32).
pub fn report(pane_id: &str, source: &str, counters: PaneCounters) -> io::Result<()> {
    let status = Command::new(herdr_binary())
        .args(report_arguments(pane_id, source, counters))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "herdr pane report-metadata exited with {status}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_names_the_pane_the_source_and_all_three_tokens() {
        let arguments = report_arguments(
            "w7B:pM",
            "hide-subagents@1",
            PaneCounters {
                working: 2,
                done: 5,
            },
        );
        assert_eq!(
            &arguments[..4],
            &["pane", "report-metadata", "--source", "hide-subagents@1"]
        );
        assert!(arguments.contains(&"hide_hooks=1".to_owned()));
        assert!(arguments.contains(&"hide_sub_working=2".to_owned()));
        assert!(arguments.contains(&"hide_sub_done=5".to_owned()));
        assert_eq!(arguments.last().unwrap(), "w7B:pM");
        assert!(
            !arguments
                .iter()
                .any(|argument| argument.starts_with("hide_sub_blocked")),
            "a count no adapter observes is never reported as a number"
        );
    }

    #[test]
    fn an_instrumented_session_with_no_children_still_proves_it_is_instrumented() {
        let arguments = report_arguments("p1", "hide-subagents@1", PaneCounters::default());
        assert!(
            arguments.contains(&format!("{INSTRUMENTED_TOKEN}={HOOK_VERSION}")),
            "a pane working alone is a different screen from a pane Hide cannot see"
        );
    }
}
