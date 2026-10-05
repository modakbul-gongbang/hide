//! Account-wide login agents. A moved HOME never changes the launchd domain.
//! The command boundary is injectable so a fixture never reaches launchd.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct UserAgents {
    command: Option<PathBuf>,
    domain: String,
}

impl UserAgents {
    pub fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            // getuid has no failure case and changes no process state.
            let uid = unsafe { libc::getuid() };
            Self {
                command: Some("/bin/launchctl".into()),
                domain: format!("gui/{uid}"),
            }
        }
        #[cfg(not(target_os = "macos"))]
        Self {
            command: None,
            domain: String::new(),
        }
    }

    /// An explicit external-system fixture, never selected by an environment variable.
    pub fn fixture(command: PathBuf, domain: String) -> Self {
        Self {
            command: Some(command),
            domain,
        }
    }

    pub fn plist(home: &Path, label: &str) -> PathBuf {
        home.join("Library/LaunchAgents")
            .join(format!("{label}.plist"))
    }

    pub fn unload(&self, label: &str, home: &Path, stop: &AtomicBool) -> io::Result<()> {
        if self.command.is_none() {
            return Ok(());
        }
        let name = format!("{}/{label}", self.domain);
        if !self.loaded(&name, home, stop)? {
            return Ok(());
        }
        let result = self.run(&["bootout", &name], home, stop)?;
        if result.code != Some(0) && self.loaded(&name, home, stop)? {
            return Err(io::Error::other(format!(
                "bootout failed (exit {:?})",
                result.code
            )));
        }
        if self.loaded(&name, home, stop)? {
            return Err(io::Error::other(
                "the login agent is still loaded after bootout",
            ));
        }
        Ok(())
    }

    fn loaded(&self, name: &str, home: &Path, stop: &AtomicBool) -> io::Result<bool> {
        let result = self.run(&["print", name], home, stop)?;
        match result.code {
            Some(0) => Ok(true),
            Some(113) => Ok(false),
            code => Err(io::Error::other(format!(
                "login agent inspection failed (exit {code:?})"
            ))),
        }
    }

    fn run(
        &self,
        args: &[&str],
        home: &Path,
        stop: &AtomicBool,
    ) -> io::Result<crate::process::Finished> {
        let mut command = Command::new(
            self.command
                .as_ref()
                .ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))?,
        );
        command
            .args(args)
            .env_clear()
            .env(crate::host::HOME_VARIABLE, home);
        crate::process::run_to_end(&mut command, Duration::from_secs(5), stop)
            .map_err(|error| io::Error::other(format!("login agent command failed: {error:?}")))
    }
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
