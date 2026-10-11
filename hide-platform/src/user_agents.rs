//! Account-wide login agents. A moved HOME never changes the launchd domain.
//! The command boundary is injectable so a fixture never reaches launchd.
//!
//! A login agent runs in the account's GUI session (`gui/<uid>`): launchd
//! starts it when the session starts and when it is installed, restarts it
//! when it fails or is killed, at most every ten seconds, and leaves it
//! stopped after it exits successfully.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// How long one `launchctl` command may take that waits on nothing but
/// launchd: `print`, `bootstrap`, `kickstart`.
const COMMAND_WITHIN: Duration = Duration::from_secs(5);
/// How long a `bootout` may take: it returns once the job has ended, which
/// launchd allows its default `ExitTimeOut` (20 s) before it kills the job,
/// so a job that takes its time to stop is not reported as still loaded.
const STOPPED_WITHIN: Duration = Duration::from_secs(30);

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

    /// A system with no login agents: nothing is installed or unloaded and
    /// no command runs. A test fixture's HOME gets this, so nothing a test
    /// runs reaches the account's real launchd domain.
    pub fn none() -> Self {
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

    /// Whether the account's GUI session runs, the only one a login agent
    /// runs in.
    pub fn session_present(&self, home: &Path, stop: &AtomicBool) -> io::Result<bool> {
        let result = self.run(&["print", &self.domain], home, stop)?;
        Ok(result.code == Some(0))
    }

    /// Writes `agent`'s property list under `home` and loads it into the
    /// GUI session, which starts it; one loaded already is replaced.
    pub fn install(
        &self,
        agent: &LoginAgent<'_>,
        home: &Path,
        stop: &AtomicBool,
    ) -> io::Result<()> {
        if self.command.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "this system has no login agents",
            ));
        }
        self.unload(agent.label, home, stop)?;
        let path = Self::plist(home, agent.label);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        crate::fs::atomic::write_file(&path, agent.plist().as_bytes(), crate::fs::Access::Private)?;
        let path = path.to_string_lossy().into_owned();
        let result = self.run(&["bootstrap", &self.domain, &path], home, stop)?;
        if result.code != Some(0) {
            return Err(io::Error::other(format!(
                "bootstrap failed (exit {:?}): {}",
                result.code,
                result.stderr.trim()
            )));
        }
        Ok(())
    }

    /// Loads `agent` into the GUI session from a property list written at
    /// `list`, outside the login agents folder, so no later login loads it
    /// again: launchd starts it once and never restarts it, and [`unload`]
    /// ends it. One loaded already under its label is replaced.
    ///
    /// [`unload`]: Self::unload
    pub fn start_once(
        &self,
        agent: &LoginAgent<'_>,
        list: &Path,
        home: &Path,
        stop: &AtomicBool,
    ) -> io::Result<()> {
        if self.command.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "this system has no login agents",
            ));
        }
        self.unload(agent.label, home, stop)?;
        crate::fs::atomic::write_file(list, agent.once().as_bytes(), crate::fs::Access::Private)?;
        let list = list.to_string_lossy().into_owned();
        let result = self.run(&["bootstrap", &self.domain, &list], home, stop)?;
        if result.code != Some(0) {
            return Err(io::Error::other(format!(
                "bootstrap failed (exit {:?}): {}",
                result.code,
                result.stderr.trim()
            )));
        }
        Ok(())
    }

    /// Starts the loaded agent `label` when it is not running.
    pub fn kickstart(&self, label: &str, home: &Path, stop: &AtomicBool) -> io::Result<()> {
        let name = format!("{}/{label}", self.domain);
        let result = self.run(&["kickstart", &name], home, stop)?;
        if result.code != Some(0) {
            return Err(io::Error::other(format!(
                "kickstart failed (exit {:?}): {}",
                result.code,
                result.stderr.trim()
            )));
        }
        Ok(())
    }

    /// How the loaded job `label` last exited, once it is not running:
    /// `None` while it runs or has never exited, or when it is not loaded.
    pub fn last_exit(
        &self,
        label: &str,
        home: &Path,
        stop: &AtomicBool,
    ) -> io::Result<Option<String>> {
        if self.command.is_none() {
            return Ok(None);
        }
        let result = self.run(&["print", &format!("{}/{label}", self.domain)], home, stop)?;
        if result.code != Some(0) {
            return Ok(None);
        }
        Ok(last_exit_of(&result.stdout))
    }

    pub fn is_loaded(&self, label: &str, home: &Path, stop: &AtomicBool) -> io::Result<bool> {
        if self.command.is_none() {
            return Ok(false);
        }
        self.loaded(&format!("{}/{label}", self.domain), home, stop)
    }

    /// Unloads the agent `label`, which ends its process, and removes its
    /// property list.
    pub fn remove(&self, label: &str, home: &Path, stop: &AtomicBool) -> io::Result<()> {
        self.unload(label, home, stop)?;
        match std::fs::remove_file(Self::plist(home, label)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn unload(&self, label: &str, home: &Path, stop: &AtomicBool) -> io::Result<()> {
        if self.command.is_none() {
            return Ok(());
        }
        let name = format!("{}/{label}", self.domain);
        if !self.loaded(&name, home, stop)? {
            return Ok(());
        }
        let result = self.run_within(&["bootout", &name], home, stop, STOPPED_WITHIN)?;
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
        self.run_within(args, home, stop, COMMAND_WITHIN)
    }

    fn run_within(
        &self,
        args: &[&str],
        home: &Path,
        stop: &AtomicBool,
        within: Duration,
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
        crate::process::run_to_end(&mut command, within, stop)
            .map_err(|error| io::Error::other(format!("login agent command failed: {error:?}")))
    }
}

/// One login agent: the program launchd keeps running in the account's GUI
/// session.
pub struct LoginAgent<'a> {
    pub label: &'a str,
    pub program: &'a Path,
    pub arguments: &'a [&'a str],
    pub environment: &'a [(&'a str, &'a str)],
    /// Where its standard output and error go.
    pub log: &'a Path,
}

impl LoginAgent<'_> {
    /// Its property list: started at load and at each start of the GUI
    /// session, restarted when it fails or is killed, never after a
    /// successful exit, at most every ten seconds.
    pub fn plist(&self) -> String {
        self.property_list(concat!(
            "<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>",
            "<key>ThrottleInterval</key><integer>10</integer>",
        ))
    }

    /// Its property list as a job launchd runs once at load and never
    /// restarts ([`UserAgents::start_once`]).
    fn once(&self) -> String {
        self.property_list("<key>KeepAlive</key><false/>")
    }

    fn property_list(&self, restarts: &str) -> String {
        let string = |value: &str| format!("<string>{}</string>", escape(value));
        let mut arguments = string(&self.program.to_string_lossy());
        for argument in self.arguments {
            arguments.push_str(&string(argument));
        }
        let environment: String = self
            .environment
            .iter()
            .map(|(key, value)| format!("<key>{}</key>{}", escape(key), string(value)))
            .collect();
        let log = string(&self.log.to_string_lossy());
        format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
                "<plist version=\"1.0\"><dict>",
                "<key>Label</key>{label}",
                "<key>ProgramArguments</key><array>{arguments}</array>",
                "<key>EnvironmentVariables</key><dict>{environment}</dict>",
                "<key>LimitLoadToSessionType</key><string>Aqua</string>",
                "<key>RunAtLoad</key><true/>",
                "{restarts}",
                "<key>StandardOutPath</key>{log}",
                "<key>StandardErrorPath</key>{log}",
                "</dict></plist>\n"
            ),
            label = string(self.label),
            arguments = arguments,
            environment = environment,
            restarts = restarts,
            log = log,
        )
    }
}

/// The exit `launchctl print` reports for a job that is not running and has
/// exited: its `last exit code` line.
pub fn last_exit_of(printed: &str) -> Option<String> {
    let field = |name: &str| {
        printed.lines().find_map(|line| {
            line.trim()
                .strip_prefix(name)
                .and_then(|rest| rest.trim_start().strip_prefix('='))
                .map(|value| value.trim().to_owned())
        })
    };
    if field("state")?.as_str() != "not running" {
        return None;
    }
    field("last exit code").filter(|code| !code.starts_with('('))
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
