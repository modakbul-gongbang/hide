//! What the machine taking the core says about itself before a move (PRD
//! core-host-node-move B3, D-04, D-25, D-26), answered by `hided core-move
//! inspect` there: its Herdr server runs, `gh` is signed in, the account's
//! GUI session runs (the only one its login item runs in), and the machine
//! does not sleep by itself on power. A failing check says what it found;
//! hide changes none of it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::control::{CheckId, FailedCheck};

/// How long one check's program may take.
const PROGRAM_WITHIN: Duration = Duration::from_secs(5);

/// The programs the `gh`, session and power checks run: the system's, or a
/// fixture's stand-ins (`env::fixture_preflight_programs`).
pub struct Programs {
    /// `None` finds `gh` on this account's PATH.
    gh: Option<PathBuf>,
    session: Option<hide_platform::user_agents::UserAgents>,
    pmset: Option<PathBuf>,
}

impl Programs {
    pub fn for_this_machine() -> Self {
        if let Some(folder) = crate::env::fixture_preflight_programs() {
            return Self {
                gh: Some(folder.join("gh")),
                session: Some(hide_platform::user_agents::UserAgents::fixture(
                    folder.join("launchctl"),
                    "gui/fixture".to_owned(),
                )),
                pmset: Some(folder.join("pmset")),
            };
        }
        Self {
            gh: None,
            session: cfg!(target_os = "macos")
                .then(hide_platform::user_agents::UserAgents::current),
            pmset: cfg!(target_os = "macos").then(|| "/usr/bin/pmset".into()),
        }
    }
}

/// Where this machine's Herdr is, as its daemon resolves it.
pub struct Herdr<'a> {
    pub bin: Option<&'a Path>,
    pub socket: Option<&'a str>,
}

/// Every check that fails on this machine, in the dialog's order.
pub fn failing(home: &Path, herdr: &Herdr<'_>, programs: &Programs) -> Vec<FailedCheck> {
    let stop = AtomicBool::new(false);
    [
        (CheckId::Herdr, herdr_running(home, herdr, &stop)),
        (CheckId::Gh, gh_signed_in(home, programs, &stop)),
        (CheckId::GuiSession, gui_session(home, programs, &stop)),
        (CheckId::Sleep, sleep_off(home, programs, &stop)),
    ]
    .into_iter()
    .filter_map(|(check, result)| result.err().map(|detail| FailedCheck { check, detail }))
    .collect()
}

fn run(
    mut command: Command,
    stop: &AtomicBool,
) -> Result<hide_platform::process::Finished, String> {
    let program = command.get_program().to_string_lossy().into_owned();
    hide_platform::process::run_to_end(&mut command, PROGRAM_WITHIN, stop)
        .map_err(|failure| format!("{program} did not answer: {failure:?}"))
}

/// `herdr status server --json` for the server this machine's core would
/// own says it runs.
fn herdr_running(home: &Path, herdr: &Herdr<'_>, stop: &AtomicBool) -> Result<(), String> {
    let bin = herdr
        .bin
        .ok_or("no herdr on this machine: neither HERDR_BIN_PATH nor PATH names one")?;
    let mut command = Command::new(bin);
    command
        .args(["status", "server", "--json"])
        .env(hide_platform::host::HOME_VARIABLE, home);
    if let Some(socket) = herdr.socket {
        command.env(crate::env::HERDR_SOCKET_PATH, socket);
    }
    let finished = run(command, stop)?;
    if finished.code != Some(0) {
        return Err(format!(
            "herdr status server exited {:?}: {}",
            finished.code,
            finished.last_error_line()
        ));
    }
    let status: hide_node::ssh::RemoteHerdrServerStatus =
        serde_json::from_str(finished.stdout.trim())
            .map_err(|error| format!("herdr status server was not readable: {error}"))?;
    if !status.running {
        return Err(format!("no Herdr server answers at {}", status.socket));
    }
    Ok(())
}

/// `gh auth status` exits 0: the core reads GitHub with this machine's
/// login once it runs here (PRD core-host-node-remote-core D-16).
fn gh_signed_in(home: &Path, programs: &Programs, stop: &AtomicBool) -> Result<(), String> {
    // `gh` runs with the PATH it was found on, which its git and credential
    // helpers need; a stand-in is run as it is.
    let (gh, path) = match &programs.gh {
        Some(gh) => (gh.clone(), None),
        None => {
            let path = hide_platform::programs::account_path()
                .ok_or("this account's PATH could not be read")?;
            let gh = hide_platform::host::find_program(&path, "gh")
                .ok_or("gh is not installed on this machine")?;
            (gh, Some(path))
        }
    };
    let mut command = Command::new(gh);
    command
        .args(["auth", "status"])
        .env(hide_platform::host::HOME_VARIABLE, home);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let finished = run(command, stop)?;
    if finished.code != Some(0) {
        return Err("gh is not signed in".to_owned());
    }
    Ok(())
}

/// The account's GUI session runs, which the core's login item needs.
fn gui_session(home: &Path, programs: &Programs, stop: &AtomicBool) -> Result<(), String> {
    let agents = programs
        .session
        .as_ref()
        .ok_or("this system has no login session for the core to run in")?;
    match agents.session_present(home, stop) {
        Ok(true) => Ok(()),
        Ok(false) => Err("no one is logged in to this machine's desktop".to_owned()),
        Err(error) => Err(format!("launchctl did not answer: {error}")),
    }
}

/// `pmset -g custom` says the machine never sleeps on its own on power.
fn sleep_off(home: &Path, programs: &Programs, stop: &AtomicBool) -> Result<(), String> {
    let pmset = programs
        .pmset
        .as_ref()
        .ok_or("this system has no power settings hide can read")?;
    let mut command = Command::new(pmset);
    command
        .args(["-g", "custom"])
        .env_clear()
        .env(hide_platform::host::HOME_VARIABLE, home);
    let finished = run(command, stop)?;
    if finished.code != Some(0) {
        return Err(format!("pmset exited {:?}", finished.code));
    }
    match ac_sleep_minutes(&finished.stdout) {
        Some(0) => Ok(()),
        Some(minutes) => Err(format!("sleeps on power after {minutes} min")),
        None => Err("pmset reports no sleep setting on power".to_owned()),
    }
}

/// The `sleep` value of `pmset -g custom`'s `AC Power:` section.
fn ac_sleep_minutes(text: &str) -> Option<u32> {
    let mut on_power = false;
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) {
            on_power = line.trim() == "AC Power:";
            continue;
        }
        let mut words = line.split_whitespace();
        if on_power && words.next() == Some("sleep") {
            return words.next()?.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: &str = "Battery Power:\n lidwake              1\n sleep                1\n displaysleep         2\nAC Power:\n lidwake              1\n sleep                0 (sleep prevented by sharingd)\n displaysleep         10\n";

    #[test]
    fn only_the_power_section_s_sleep_counts() {
        assert_eq!(ac_sleep_minutes(LAPTOP), Some(0));
        assert_eq!(
            ac_sleep_minutes("AC Power:\n displaysleep         10\n sleep                10\n"),
            Some(10)
        );
        assert_eq!(
            ac_sleep_minutes("Battery Power:\n sleep                0\n"),
            None
        );
        assert_eq!(
            ac_sleep_minutes("AC Power:\n sleep                never\n"),
            None
        );
    }

    #[cfg(unix)]
    fn stand_in(dir: &Path, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Each program's answer is read as the check it is, and one that does
    /// not answer is a failing check rather than a pass.
    #[cfg(unix)]
    #[test]
    fn herdr_and_power_checks_fail_on_what_their_programs_answer() {
        let dir = tempfile::tempdir().unwrap();
        let stop = AtomicBool::new(false);
        let running = stand_in(
            dir.path(),
            "herdr-running",
            r#"echo '{"running":true,"socket":"/tmp/h.sock"}'"#,
        );
        let stopped = stand_in(
            dir.path(),
            "herdr-stopped",
            r#"echo '{"running":false,"socket":"/tmp/h.sock"}'"#,
        );
        let herdr = |bin: &Path| {
            herdr_running(
                dir.path(),
                &Herdr {
                    bin: Some(bin),
                    socket: None,
                },
                &stop,
            )
        };
        assert_eq!(herdr(&running), Ok(()));
        assert_eq!(
            herdr(&stopped),
            Err("no Herdr server answers at /tmp/h.sock".to_owned())
        );
        assert!(
            herdr_running(
                dir.path(),
                &Herdr {
                    bin: None,
                    socket: None
                },
                &stop
            )
            .is_err()
        );

        let programs = |pmset: &str| Programs {
            gh: None,
            session: None,
            pmset: Some(stand_in(dir.path(), "pmset", &format!("printf '{pmset}'"))),
        };
        assert_eq!(
            sleep_off(dir.path(), &programs("AC Power:\\n sleep 0\\n"), &stop),
            Ok(())
        );
        assert_eq!(
            sleep_off(dir.path(), &programs("AC Power:\\n sleep 1\\n"), &stop),
            Err("sleeps on power after 1 min".to_owned())
        );
        let none = Programs {
            gh: None,
            session: None,
            pmset: None,
        };
        assert!(sleep_off(dir.path(), &none, &stop).is_err());
        assert!(gui_session(dir.path(), &none, &stop).is_err());

        let gh = |script: &str| Programs {
            gh: Some(stand_in(dir.path(), "gh", script)),
            session: Some(hide_platform::user_agents::UserAgents::fixture(
                stand_in(dir.path(), "launchctl", script),
                "gui/fixture".to_owned(),
            )),
            pmset: None,
        };
        assert_eq!(gh_signed_in(dir.path(), &gh("exit 0"), &stop), Ok(()));
        assert_eq!(gui_session(dir.path(), &gh("exit 0"), &stop), Ok(()));
        assert_eq!(
            gh_signed_in(dir.path(), &gh("exit 1"), &stop),
            Err("gh is not signed in".to_owned())
        );
        assert!(gui_session(dir.path(), &gh("exit 113"), &stop).is_err());
    }
}
