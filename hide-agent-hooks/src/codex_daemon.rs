//! Codex's shared app-server daemon, read and switched through Codex's own
//! `codex features` command (PRD overview-request-view D-21, D-22, D-25;
//! settings-cleanup D-12, D-14).
//!
//! A Codex attached to the shared daemon runs its hooks in the daemon's
//! environment, not the pane's, so Herdr never learns that pane's session and
//! Hide cannot read it (openai/codex#48500). Turning `daemon_auto_start` off
//! makes every Codex the operator starts by hand run in its own pane. Only
//! the operator's own request turns it off: no install pass does. This is the
//! only code that changes that setting, and it changes it only through
//! `codex features disable daemon_auto_start`: Codex's `config.toml` is never
//! written here.
//!
//! A running daemon is never stopped (D-23); the setting reaches each Codex
//! started after it.
//!
//! This is a transition path: it goes away with the Settings link that calls
//! it once openai/codex#48500 runs a daemon's hooks in each window's
//! environment (D-26).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use hide_platform::process::{Finished, RunFailure, run_to_end};

use crate::runtime::AgentRuntime;

/// The feature `codex features` names the shared daemon by.
pub const DAEMON_FEATURE: &str = "daemon_auto_start";

/// How long one `codex features` or `codex app-server daemon version` call
/// may take; each answers in well under a second.
const DEADLINE: Duration = Duration::from_secs(5);

/// What Codex on one machine says about its shared daemon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonSetting {
    /// This Codex has no `daemon_auto_start` feature: an older Codex that
    /// starts no shared daemon, so there is nothing to turn off.
    Unsupported,
    /// Codex starts the shared daemon on its own.
    On,
    /// Every Codex runs in its own process.
    Off,
}

/// The `codex` this machine's account would run, or `None` when Hide finds
/// none on the login PATH or in the usual install folders.
pub fn find_codex(home: &Path) -> Option<PathBuf> {
    crate::diagnosis::runtime_binary(AgentRuntime::Codex, home)
}

/// Reads the actual binary's feature capability without querying or starting
/// a daemon and without changing the account's setting.
pub fn read_setting(codex: &Path, home: &Path, stop: &AtomicBool) -> Result<DaemonSetting, String> {
    let listed = run(codex, home, &["features", "list"], stop)?;
    if !listed.succeeded() {
        return Err(failed(codex, "features list", &listed));
    }
    parse_feature_list(&listed.stdout)
}

/// Why a switch did not happen, as the operator's popover tells it: the
/// message is for the log, never for the screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchError {
    pub failure: SwitchFailure,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwitchFailure {
    /// The `codex` program could not be started.
    CouldNotStart,
    /// Codex ran and exited unsuccessfully.
    Refused,
    /// Codex did not answer in time and was stopped.
    TimedOut,
    /// Hide was quitting and stopped the child.
    Stopped,
}

/// Turns the shared daemon off for every Codex started from now on. The
/// setting is changed only when Codex says it did: a refusal, a timeout and
/// a stop leave it as it was.
pub fn turn_off(codex: &Path, home: &Path, stop: &AtomicBool) -> Result<(), SwitchError> {
    let finished =
        run(codex, home, &["features", "disable", DAEMON_FEATURE], stop).map_err(|error| {
            SwitchError {
                failure: error.failure,
                message: error.message,
            }
        })?;
    if finished.succeeded() {
        Ok(())
    } else {
        Err(SwitchError {
            failure: SwitchFailure::Refused,
            message: failed(codex, "features disable", &finished),
        })
    }
}

/// The `daemon_auto_start` row of `codex features list`, whose columns are the
/// feature's name, its stage and whether it is on.
fn parse_feature_list(stdout: &str) -> Result<DaemonSetting, String> {
    let Some(row) = stdout
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .find(|columns| columns.first() == Some(&DAEMON_FEATURE))
    else {
        return Ok(DaemonSetting::Unsupported);
    };
    match row.last() {
        Some(&"true") => Ok(DaemonSetting::On),
        Some(&"false") => Ok(DaemonSetting::Off),
        _ => Err(format!(
            "codex features list printed a {DAEMON_FEATURE} row Hide cannot read: {}",
            row.join(" ")
        )),
    }
}

/// Codex is run with the account's HOME and its own `.codex` named outright,
/// in an environment built rather than inherited, so a `CODEX_HOME` the
/// launching shell carried never redirects which setting is changed. Its
/// `PATH` is the one it was found on ([`crate::diagnosis::cli_path`]), so a
/// Codex installed by pnpm, a script that runs `node`, finds `node` there.
fn run(codex: &Path, home: &Path, args: &[&str], stop: &AtomicBool) -> Result<Finished, RunError> {
    let mut command = Command::new(codex);
    command
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("CODEX_HOME", home.join(".codex"))
        .env(
            "PATH",
            crate::diagnosis::cli_path(home).ok_or_else(|| RunError {
                failure: SwitchFailure::CouldNotStart,
                message: format!("a folder under {} cannot be put on a PATH", home.display()),
            })?,
        );
    let name = format!("codex {}", args.join(" "));
    run_to_end(&mut command, DEADLINE, stop).map_err(|failure| match failure {
        RunFailure::Start(error) => RunError {
            failure: SwitchFailure::CouldNotStart,
            message: format!("{} could not start: {error}", codex.display()),
        },
        RunFailure::Wait(error) => RunError {
            failure: SwitchFailure::CouldNotStart,
            message: format!("{name} could not be waited on: {error}"),
        },
        RunFailure::TimedOut => RunError {
            failure: SwitchFailure::TimedOut,
            message: format!(
                "{name} did not answer within {} seconds and was stopped",
                DEADLINE.as_secs()
            ),
        },
        RunFailure::Stopped => RunError {
            failure: SwitchFailure::Stopped,
            message: format!("{name} was stopped because Hide is quitting"),
        },
    })
}

struct RunError {
    failure: SwitchFailure,
    message: String,
}

impl From<RunError> for String {
    fn from(error: RunError) -> String {
        error.message
    }
}

fn failed(codex: &Path, what: &str, finished: &Finished) -> String {
    let line = finished.last_error_line();
    let code = finished
        .code
        .map_or_else(|| "a signal".to_owned(), |code| format!("code {code}"));
    if line.is_empty() {
        format!("{} {what} exited with {code}", codex.display())
    } else {
        format!("{} {what} exited with {code}: {line}", codex.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_row_is_read_by_its_last_column() {
        let listed = "apps                 stable  true\n\
                      daemon_auto_start    stable  false\n\
                      realtime_conversation stable true\n";
        assert_eq!(parse_feature_list(listed), Ok(DaemonSetting::Off));
        assert_eq!(
            parse_feature_list("daemon_auto_start  stable  true\n"),
            Ok(DaemonSetting::On)
        );
        assert_eq!(
            parse_feature_list("apps stable true\n"),
            Ok(DaemonSetting::Unsupported)
        );
        assert!(parse_feature_list("daemon_auto_start stable maybe\n").is_err());
    }

    /// The Mac mini's layout on 2026-10-06: pnpm 11 puts the `codex` script
    /// in `~/Library/pnpm/bin`, the script runs `node` by name, `node` is in
    /// `~/.local/bin`, and the device helper's own `PATH` holds neither. The
    /// names are unique so a CLI on the test machine's `PATH` cannot answer.
    #[cfg(unix)]
    #[test]
    fn a_codex_installed_by_pnpm_11_is_found_and_runs_with_its_node() {
        use std::os::unix::fs::PermissionsExt;

        let home = tempfile::tempdir().unwrap();
        let script = |path: PathBuf, body: &str| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let shim = script(
            home.path().join("Library/pnpm/bin/hide-fixture-codex"),
            "#!/bin/sh\nexec hide-fixture-node \"$@\"\n",
        );
        script(
            home.path().join(".local/bin/hide-fixture-node"),
            concat!(
                "#!/bin/sh\n",
                "case \"$1 $2\" in\n",
                "  '--version ') echo 'codex-cli 0.160.0' ;;\n",
                "  'features list') echo 'daemon_auto_start  stable  false' ;;\n",
                "  *) exit 2 ;;\n",
                "esac\n",
            ),
        );

        let found = crate::find_binary("hide-fixture-codex", home.path());
        assert_eq!(found.as_deref(), Some(shim.as_path()));
        assert_eq!(
            read_setting(&shim, home.path(), &AtomicBool::new(false)),
            Ok(DaemonSetting::Off)
        );
        assert_eq!(
            crate::program_version(&shim, &crate::cli_path(home.path()).unwrap()).as_deref(),
            Some("0.160.0")
        );
    }

    /// A stand-in `codex` whose `features` command keeps the setting in a
    /// file beside it, so the test never touches an account's real one.
    #[cfg(unix)]
    fn stand_in(home: &Path, disable: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let bin = home.join("bin/hide-fixture-codex-switch");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(home.join("setting"), "true").unwrap();
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\ncase \"$1 $2\" in\n  'features list') echo \"daemon_auto_start  stable  $(cat '{0}/setting')\" ;;\n  'features disable') {disable} ;;\n  *) exit 2 ;;\nesac\n",
                home.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    #[cfg(unix)]
    #[test]
    fn turning_off_changes_the_setting_only_when_codex_says_it_did() {
        let home = tempfile::tempdir().unwrap();
        let stop = AtomicBool::new(false);

        let refusing = stand_in(home.path(), "echo 'not allowed' >&2; exit 3");
        let error = turn_off(&refusing, home.path(), &stop).expect_err("refused");
        assert_eq!(error.failure, SwitchFailure::Refused);
        assert!(error.message.contains("not allowed"), "{}", error.message);
        assert_eq!(
            read_setting(&refusing, home.path(), &stop),
            Ok(DaemonSetting::On),
            "a refusal leaves the setting as it was"
        );

        let working = stand_in(
            home.path(),
            &format!("echo false > '{}/setting'", home.path().display()),
        );
        assert_eq!(turn_off(&working, home.path(), &stop), Ok(()));
        assert_eq!(
            read_setting(&working, home.path(), &stop),
            Ok(DaemonSetting::Off)
        );

        let missing = home.path().join("bin/no-such-codex");
        assert_eq!(
            turn_off(&missing, home.path(), &stop)
                .expect_err("not there")
                .failure,
            SwitchFailure::CouldNotStart
        );
    }
}
