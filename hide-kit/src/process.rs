//! The one way the kit runs another program: as an owned child that leads its
//! own tree, with a deadline, and killed with its whole tree when the deadline passes or
//! the caller stops waiting (engineering rule 14; practice `process.md`).
//!
//! Every child the kit starts (`herdr plugin uninstall`, `node --version`,
//! `hcoord daemon ensure`) is short-lived, so none outlives the call that
//! started it, and a raised stop flag ends it at once, so the daemon that owns
//! the kit can quit without waiting out a deadline. The waiting, the output cap
//! and the tree's end are `hide_platform::process::run_to_end`'s; this module
//! adds the environment the kit's children get and the kit's reasons.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub use hide_platform::process::Finished;
use hide_platform::process::{RunFailure, run_to_end};

/// Runs `program` with `args` and `env` added to an empty-ish environment
/// that keeps only `PATH` and `HOME`, and waits at most `deadline`, or
/// until `stop` is raised.
///
/// The environment is built rather than inherited: a daemon launched from a
/// Herdr pane carries that pane's `HERDR_*` identity, and a child that
/// inherited it would act on the operator's Herdr instead of the target's.
pub fn run(
    program: &Path,
    args: &[&str],
    env: &[(String, String)],
    home: &Path,
    deadline: Duration,
    stop: &AtomicBool,
) -> Result<Finished, String> {
    let mut command = Command::new(program);
    command.args(args).env_clear().env("HOME", home).env(
        "PATH",
        std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
    );
    for (key, value) in env {
        command.env(key, value);
    }
    let name = program.display().to_string();
    run_to_end(&mut command, deadline, stop).map_err(|failure| match failure {
        RunFailure::Start(error) => format!("{name} could not start: {error}"),
        RunFailure::Wait(error) => format!("{name} could not be waited on: {error}"),
        RunFailure::TimedOut => format!(
            "{name} did not finish within {} seconds and was stopped",
            deadline.as_secs()
        ),
        RunFailure::Stopped => format!("{name} was stopped because Hide is quitting"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Instant;

    #[test]
    fn a_child_past_its_deadline_is_stopped_with_its_group() {
        let home = tempfile::tempdir().unwrap();
        let marker = home.path().join("grandchild-survived");
        let script = format!("(sleep 2; touch '{}') & sleep 30", marker.display());
        let started = Instant::now();
        let result = run(
            Path::new("/bin/sh"),
            &["-c", &script],
            &[],
            home.path(),
            Duration::from_millis(300),
            &AtomicBool::new(false),
        );
        assert!(result.unwrap_err().contains("did not finish"));
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(2500));
        assert!(!marker.exists(), "the grandchild outlived the deadline");
    }

    #[test]
    fn output_and_exit_code_come_back_and_the_environment_is_not_inherited() {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: tests in this module do not read this variable concurrently.
        unsafe { std::env::set_var("HIDE_KIT_TEST_LEAK", "leaked") };
        let finished = run(
            Path::new("/bin/sh"),
            &["-c", "printf '%s|%s|%s' \"$HOME\" \"$HIDE_KIT_TEST_LEAK\" \"$GIVEN\"; echo oops >&2; exit 3"],
            &[("GIVEN".to_owned(), "yes".to_owned())],
            home.path(),
            Duration::from_secs(5),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(finished.code, Some(3));
        assert_eq!(finished.stdout, format!("{}||yes", home.path().display()));
        assert_eq!(finished.last_error_line(), "oops");
    }
}
