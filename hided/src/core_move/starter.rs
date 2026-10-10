//! How the machine taking the core starts and stops it (PRD
//! core-host-node-move B9, Q14). In production the account's login item
//! starts it, so it outlives the SSH session that placed it and comes back
//! after a login; a test fixture's HOME starts it as a detached process, so
//! no test reaches launchd.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::env::{FIXTURE_HOME_MARKER, HIDE_CORE_STARTER};

/// How long a started core has to bind its attach socket.
const STARTED_WITHIN: Duration = Duration::from_secs(30);
/// How long a stopped core has to end after it was asked, before it is
/// killed.
const STOPPED_WITHIN: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub enum CoreStarter {
    /// The account's login item (`login_item`).
    LoginItem { home: PathBuf },
    /// A detached `hided`, only in a fixture HOME.
    Fixture { program: PathBuf },
}

impl CoreStarter {
    /// The starter for this account: the fixture only when
    /// `HIDE_CORE_STARTER=fixture` and HOME is a fixture HOME
    /// (`env::fixture_home`), the login item otherwise.
    pub fn for_account(home: &Path) -> Result<Self, String> {
        let asked = std::env::var_os(HIDE_CORE_STARTER);
        if asked.as_deref() == Some(std::ffi::OsStr::new("fixture")) {
            if !crate::env::fixture_home(home) {
                return Err(format!(
                    "{HIDE_CORE_STARTER}=fixture is refused: {} is not a fixture HOME ({FIXTURE_HOME_MARKER} under /tmp)",
                    home.display()
                ));
            }
            let program = std::env::current_exe()
                .map_err(|error| format!("this hided has no path: {error}"))?;
            return Ok(Self::Fixture { program });
        }
        Ok(Self::LoginItem {
            home: home.to_path_buf(),
        })
    }

    /// Starts the core on `state_dir` and waits until it takes links.
    pub fn start(&self, state_dir: &Path) -> Result<u32, String> {
        let stale = std::fs::read(crate::attach::attach_record(state_dir)).ok();
        let mut exited: Box<dyn FnMut() -> Option<String>> = Box::new(|| None);
        match self {
            Self::Fixture { program } => {
                let log = log_file(state_dir)?;
                let mut command = std::process::Command::new(program);
                command.env("HIDE_STATE_DIR", state_dir);
                hide_platform::process::detach(&mut command)
                    .map_err(|error| format!("the core could not be detached: {error}"))?;
                command
                    .stdin(std::process::Stdio::null())
                    .stdout(log.try_clone().map_err(|error| error.to_string())?)
                    .stderr(log);
                let mut child = command
                    .spawn()
                    .map_err(|error| format!("the core did not start: {error}"))?;
                exited = Box::new(move || {
                    child
                        .try_wait()
                        .ok()
                        .flatten()
                        .map(|status| format!("the core exited at its start: {status}"))
                });
            }
            Self::LoginItem { home } => crate::login_item::start(home, state_dir)?,
        }
        wait_for_core(state_dir, stale.as_deref(), &mut *exited)
    }

    /// Stops the core on `state_dir` and removes its starter, and answers
    /// only once no core of that folder runs.
    pub fn stop(&self, state_dir: &Path) -> Result<(), String> {
        if let Self::LoginItem { home } = self {
            crate::login_item::remove(home)?;
        }
        let Some(running) = crate::state_file::read_state(state_dir)
            .map_err(|error| format!("the core's state could not be read: {error}"))?
        else {
            return Ok(());
        };
        stop_pid(running.pid)
    }
}

fn log_file(state_dir: &Path) -> Result<std::fs::File, String> {
    let logs = state_dir.join("Logs");
    hide_platform::fs::private::create_dir_all(&logs).map_err(|error| error.to_string())?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("core.log"))
        .map_err(|error| format!("the core's log could not be opened: {error}"))
}

/// Waits until the core in `state_dir` records an attach socket other than
/// `stale` and answers on it, and answers its pid; `exited` names a core
/// that ended before that.
// Another process's start and end announce nothing to this one: its pid and
// its records are read until they say so.
#[allow(clippy::disallowed_methods)] // a production wait, not test code
fn wait_for_core(
    state_dir: &Path,
    stale: Option<&[u8]>,
    exited: &mut dyn FnMut() -> Option<String>,
) -> Result<u32, String> {
    let deadline = Instant::now() + STARTED_WITHIN;
    loop {
        if let Some(exit) = exited() {
            return Err(exit);
        }
        let record = std::fs::read(crate::attach::attach_record(state_dir)).ok();
        if record.is_some()
            && record.as_deref() != stale
            && let Ok(Some(running)) = crate::state_file::read_state(state_dir)
            && hide_platform::process::is_alive(running.pid)
        {
            return Ok(running.pid);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the core did not take links within {}s",
                STARTED_WITHIN.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Asks `pid` to stop, kills it past [`STOPPED_WITHIN`], and answers once
/// it is gone.
#[allow(clippy::disallowed_methods)] // a production wait, not test code
fn stop_pid(pid: u32) -> Result<(), String> {
    if !hide_platform::process::is_alive(pid) {
        return Ok(());
    }
    hide_platform::process::terminate(pid)
        .map_err(|error| format!("the core {pid} could not be stopped: {error}"))?;
    let deadline = Instant::now() + STOPPED_WITHIN;
    while hide_platform::process::is_alive(pid) {
        if Instant::now() >= deadline {
            hide_platform::process::kill_tree(pid)
                .map_err(|error| format!("the core {pid} could not be killed: {error}"))?;
            let killed = Instant::now() + STOPPED_WITHIN;
            while hide_platform::process::is_alive(pid) {
                if Instant::now() >= killed {
                    return Err(format!("the core {pid} is still running after a kill"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
