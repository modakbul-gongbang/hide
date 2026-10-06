//! Codex's shared app-server daemon, read and switched through Codex's own
//! `codex features` command (PRD overview-request-view D-21, D-22, D-25).
//!
//! A Codex attached to the shared daemon runs its hooks in the daemon's
//! environment, not the pane's, so Herdr never learns that pane's session and
//! Hide cannot read it (openai/codex#48500). Turning `daemon_auto_start` off
//! makes every Codex the operator starts by hand run in its own pane. This is
//! the only code that changes that setting, and it changes it only through
//! `codex features disable|enable daemon_auto_start`: Codex's
//! `config.toml` is never written here.
//!
//! A running daemon is never stopped (D-23); the setting reaches each Codex
//! started after it, which [`DaemonState::running`] lets the kit say.
//!
//! This is a transition path: it goes away with the kit part that calls it
//! once openai/codex#48500 runs a daemon's hooks in each window's
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DaemonState {
    pub setting: DaemonSetting,
    /// A shared daemon answers right now; Codex sessions already attached to
    /// it stay there until it goes down.
    pub running: bool,
}

/// The `codex` this machine's account would run, or `None` when Hide finds
/// none on the login PATH or in the usual install folders.
pub fn find_codex(home: &Path) -> Option<PathBuf> {
    crate::diagnosis::runtime_binary(AgentRuntime::Codex, home)
}

/// Reads the setting and whether a daemon is running, changing nothing.
pub fn read(codex: &Path, home: &Path, stop: &AtomicBool) -> Result<DaemonState, String> {
    let setting = read_setting(codex, home, stop)?;
    let running = match setting {
        DaemonSetting::Unsupported => false,
        DaemonSetting::On | DaemonSetting::Off => daemon_running(codex, home, stop)?,
    };
    Ok(DaemonState { setting, running })
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

/// Turns the shared daemon off for every Codex started from now on.
pub fn turn_off(codex: &Path, home: &Path, stop: &AtomicBool) -> Result<(), String> {
    switch(codex, home, "disable", stop)
}

/// Gives the shared daemon back to Codex: the operator turned the kit part
/// off (D-24).
pub fn turn_on(codex: &Path, home: &Path, stop: &AtomicBool) -> Result<(), String> {
    switch(codex, home, "enable", stop)
}

fn switch(codex: &Path, home: &Path, verb: &str, stop: &AtomicBool) -> Result<(), String> {
    let finished = run(codex, home, &["features", verb, DAEMON_FEATURE], stop)?;
    if finished.succeeded() {
        Ok(())
    } else {
        Err(failed(codex, &format!("features {verb}"), &finished))
    }
}

/// `codex app-server daemon version` answers `"status":"running"` with exit 0
/// while a daemon serves its control socket, and fails when none does; it
/// never starts one.
pub fn daemon_running(codex: &Path, home: &Path, stop: &AtomicBool) -> Result<bool, String> {
    let finished = run(codex, home, &["app-server", "daemon", "version"], stop)?;
    Ok(finished.succeeded() && parse_daemon_status(&finished.stdout))
}

fn parse_daemon_status(stdout: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .ok()
        .and_then(|value| {
            value
                .get("status")?
                .as_str()
                .map(|status| status == "running")
        })
        .unwrap_or(false)
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
fn run(codex: &Path, home: &Path, args: &[&str], stop: &AtomicBool) -> Result<Finished, String> {
    let mut command = Command::new(codex);
    command
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("CODEX_HOME", home.join(".codex"))
        .env(
            "PATH",
            crate::diagnosis::cli_path(home).ok_or_else(|| {
                format!("a folder under {} cannot be put on a PATH", home.display())
            })?,
        );
    let name = format!("codex {}", args.join(" "));
    run_to_end(&mut command, DEADLINE, stop).map_err(|failure| match failure {
        RunFailure::Start(error) => format!("{} could not start: {error}", codex.display()),
        RunFailure::Wait(error) => format!("{name} could not be waited on: {error}"),
        RunFailure::TimedOut => format!(
            "{name} did not answer within {} seconds and was stopped",
            DEADLINE.as_secs()
        ),
        RunFailure::Stopped => format!("{name} was stopped because Hide is quitting"),
    })
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
            crate::program_version(&shim, home.path()).as_deref(),
            Some("0.160.0")
        );
    }

    #[test]
    fn only_a_running_status_is_a_running_daemon() {
        assert!(parse_daemon_status(r#"{"status":"running","pid":1}"#));
        assert!(!parse_daemon_status(r#"{"status":"stopped"}"#));
        assert!(!parse_daemon_status("Error: failed to connect"));
    }
}
