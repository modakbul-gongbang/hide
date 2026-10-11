//! Account-wide login agents: a program the system keeps running for the
//! account, starts at its login and starts again when it fails. A moved HOME
//! never changes which of the system's agents are the account's. The system's
//! commands are injectable ([`UserAgents::fixture`]) so a fixture never
//! reaches the real ones.
//!
//! launchd is the one system with login agents here (macOS, `launchd`).
//! Every other system answers `ErrorKind::Unsupported` with the reason to
//! each question that needs login agents, and has none installed, so there is
//! nothing to read, unload, remove or forget.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

mod launchd;

const NO_LOGIN_AGENTS: &str = "this system has no login agents";
const NO_POWER_SETTINGS: &str = "this system has no power settings to read";

#[derive(Clone, Debug)]
pub struct UserAgents {
    system: System,
}

#[derive(Clone, Debug)]
enum System {
    Launchd(launchd::Launchd),
    /// No login agents: nothing is installed or unloaded and no command
    /// runs.
    None,
}

/// What an agent needs of the account's login to run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Login {
    /// It runs whether or not anyone is logged in.
    NotNeeded,
    /// It runs only while someone is logged in, and someone is.
    LoggedIn,
    /// It runs only while someone is logged in, and no one is.
    LoggedOut,
}

impl UserAgents {
    pub fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                system: System::Launchd(launchd::Launchd::current()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        Self::none()
    }

    /// A system with no login agents. A test fixture's HOME gets this, so
    /// nothing a test runs reaches the account's real login agents.
    pub fn none() -> Self {
        Self {
            system: System::None,
        }
    }

    /// An explicit external-system fixture, never selected by an environment
    /// variable: the system's programs are the stand-ins in `stand_ins`, under
    /// the names the implementation runs (launchd's `launchctl` and `pmset`),
    /// and its agents are a domain of the fixture's own.
    pub fn fixture(stand_ins: &Path) -> Self {
        Self {
            system: System::Launchd(launchd::Launchd::fixture(stand_ins)),
        }
    }

    /// Whether this system keeps login agents; `Unsupported` says why not.
    pub fn supported(&self) -> io::Result<()> {
        match &self.system {
            System::Launchd(_) => Ok(()),
            System::None => Err(unsupported(NO_LOGIN_AGENTS)),
        }
    }

    /// What an agent needs of the account's login, and whether it has it
    /// now.
    pub fn login(&self, home: &Path, stop: &AtomicBool) -> io::Result<Login> {
        match &self.system {
            System::Launchd(launchd) => launchd.login(home, stop),
            System::None => Err(unsupported(NO_LOGIN_AGENTS)),
        }
    }

    /// How long the machine waits on power before it sleeps on its own,
    /// which stops every agent; `None` when it never does.
    pub fn sleeps_on_power(&self, home: &Path, stop: &AtomicBool) -> io::Result<Option<Duration>> {
        match &self.system {
            System::Launchd(launchd) => launchd.sleeps_on_power(home, stop),
            System::None => Err(unsupported(NO_POWER_SETTINGS)),
        }
    }

    /// Installs `agent` for `home`'s account, which starts it and starts it
    /// at each login; one installed already under its label is replaced.
    pub fn install(
        &self,
        agent: &LoginAgent<'_>,
        home: &Path,
        stop: &AtomicBool,
    ) -> io::Result<()> {
        match &self.system {
            System::Launchd(launchd) => launchd.install(agent, home, stop),
            System::None => Err(unsupported(NO_LOGIN_AGENTS)),
        }
    }

    /// Starts `agent` once where an installed agent runs, never again at a
    /// login and never after it exits, until [`unload`] ends it. What the
    /// system needs to run it is kept in `folder`, a private folder the
    /// caller made and keeps until then. One loaded already under its label
    /// is replaced.
    ///
    /// [`unload`]: Self::unload
    pub fn start_once(
        &self,
        agent: &LoginAgent<'_>,
        folder: &Path,
        home: &Path,
        stop: &AtomicBool,
    ) -> io::Result<()> {
        match &self.system {
            System::Launchd(launchd) => launchd.start_once(agent, folder, home, stop),
            System::None => Err(unsupported(NO_LOGIN_AGENTS)),
        }
    }

    /// Starts the loaded agent `label` when it is not running.
    pub fn start(&self, label: &str, home: &Path, stop: &AtomicBool) -> io::Result<()> {
        match &self.system {
            System::Launchd(launchd) => launchd.start(label, home, stop),
            System::None => Err(unsupported(NO_LOGIN_AGENTS)),
        }
    }

    /// How the loaded agent `label` last exited, once it is not running:
    /// `None` while it runs or has never exited, or when it is not loaded.
    pub fn last_exit(
        &self,
        label: &str,
        home: &Path,
        stop: &AtomicBool,
    ) -> io::Result<Option<String>> {
        match &self.system {
            System::Launchd(launchd) => launchd.last_exit(label, home, stop),
            System::None => Ok(None),
        }
    }

    /// Whether the system holds the agent `label` now: running it, or
    /// ready to start it again.
    pub fn is_loaded(&self, label: &str, home: &Path, stop: &AtomicBool) -> io::Result<bool> {
        match &self.system {
            System::Launchd(launchd) => launchd.is_loaded(label, home, stop),
            System::None => Ok(false),
        }
    }

    /// Whether the next login starts the agent `label`: it was installed
    /// and neither removed nor forgotten since.
    pub fn is_installed(&self, label: &str, home: &Path) -> io::Result<bool> {
        match &self.system {
            System::Launchd(_) => launchd::Launchd::is_installed(label, home),
            System::None => Ok(false),
        }
    }

    /// Unloads the agent `label`, which ends its process, and returns once
    /// the system no longer holds it.
    pub fn unload(&self, label: &str, home: &Path, stop: &AtomicBool) -> io::Result<()> {
        match &self.system {
            System::Launchd(launchd) => launchd.unload(label, home, stop),
            System::None => Ok(()),
        }
    }

    /// Unloads the agent `label`, which ends its process, and uninstalls it.
    pub fn remove(&self, label: &str, home: &Path, stop: &AtomicBool) -> io::Result<()> {
        match &self.system {
            System::Launchd(launchd) => {
                launchd.unload(label, home, stop)?;
                launchd::Launchd::forget(label, home)
            }
            System::None => Ok(()),
        }
    }

    /// Uninstalls the agent `label` and leaves it loaded: the next login
    /// starts nothing of it, and what runs now goes on until it exits or is
    /// unloaded. The agent's own process can ask this of itself, where its
    /// own unload would stop it midway.
    pub fn forget(&self, label: &str, home: &Path) -> io::Result<()> {
        match &self.system {
            System::Launchd(_) => launchd::Launchd::forget(label, home),
            System::None => Ok(()),
        }
    }
}

fn unsupported(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, reason)
}

/// One login agent: the program the system keeps running for the account.
pub struct LoginAgent<'a> {
    pub label: &'a str,
    pub program: &'a Path,
    pub arguments: &'a [&'a str],
    pub environment: &'a [(&'a str, &'a str)],
    /// Where its standard output and error go.
    pub log: &'a Path,
}

/// The account's home from the user database, independent of HOME overrides.
pub fn account_home() -> io::Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::ffi::CStr;
        use std::os::unix::ffi::OsStrExt;
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let mut bytes = vec![0_u8; 64 * 1024];
        // The initialized buffer outlives the passwd pointers; copy before returning.
        let code = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                record.as_mut_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                &mut result,
            )
        };
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code));
        }
        if result.is_null() {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let record = unsafe { record.assume_init() };
        if record.pw_dir.is_null() {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let home = unsafe { CStr::from_ptr(record.pw_dir) };
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(home.to_bytes())))
    }
    #[cfg(windows)]
    crate::host::home_dir()
}
