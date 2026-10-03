//! Owned process entrypoints for the CLI daemon and short-lived file openers.

use std::io;
use std::process::{Child, Command, Stdio};

use std::ffi::OsStr;
use std::path::Path;

#[cfg(unix)]
use hide_platform::process;
use hide_platform::process::OwnedChild;
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::time::{Duration, Instant};

/// Cap on resident hided children this CLI owns. Crossing it is a failure.
pub const MAX_DAEMON_CHILDREN: usize = 1;

/// Starts a deliberately detached `hided` daemon from the short-lived CLI.
/// The file-opener ownership path below has the opposite lifetime policy.
/// The daemon leads its own process group, so an interrupt sent to the
/// terminal job or host that ran the CLI (`pnpm dev`, a desktop app killing
/// a timed-out `hide connect`) never reaches it: its lifetime is its own.
pub fn spawn_owned(command: &mut Command) -> io::Result<Child> {
    hide_platform::process::detach(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// A short-lived CLI opener this daemon answers for: stopping it, dropping it
/// and this daemon's death without Drop each end the helper and everything it
/// started. On Windows the job object [`OwnedChild`] puts the helper in does
/// that by itself, because the system ends the job when its last handle
/// closes. No Unix system ends a whole process group when its owner dies, so
/// there a private supervisor holds the helper's group and watches an owner
/// channel whose EOF reaches it however hided died.
pub struct OwnedOpener {
    #[cfg(unix)]
    supervisor: OwnedChild,
    #[cfg(unix)]
    owner: Option<UnixStream>,
    #[cfg(windows)]
    helper: OwnedChild,
}

#[cfg(windows)]
impl OwnedOpener {
    pub fn try_wait(&mut self) -> io::Result<bool> {
        Ok(self.helper.try_wait()?.is_some())
    }

    /// Ends what the helper left in its job too, as the Unix supervisor ends
    /// the helper's group when the helper returns. Safe to call more than once.
    pub fn stop(&mut self) {
        let _ = self.helper.kill_tree();
        let _ = self.helper.wait();
    }
}

#[cfg(unix)]
impl OwnedOpener {
    pub fn supervisor_pid(&self) -> u32 {
        self.supervisor.id()
    }

    pub fn try_wait(&mut self) -> io::Result<bool> {
        let Some(owner) = self.owner.as_mut() else {
            return Ok(true);
        };
        let mut byte = [0];
        match owner.read(&mut byte) {
            Ok(0) => Ok(true),
            Ok(_) => Err(io::Error::other("unexpected opener supervisor message")),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn stop(&mut self) {
        // End the tree ourselves even if the supervisor crashed before its
        // watcher ran. Ending it more than once is safe, so explicit stop
        // followed by Drop is too.
        self.owner.take();
        let _ = self.supervisor.kill_tree();
        let until = Instant::now() + Duration::from_millis(250);
        while Instant::now() < until {
            match self.supervisor.try_wait() {
                Ok(Some(_)) => return,
                Err(_) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let _ = self.supervisor.kill_tree();
        let _ = self.supervisor.wait();
    }
}

impl Drop for OwnedOpener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Hands a default association utility to the OS. Tokio's process driver
/// attempts to reap a short-lived utility after its handle is dropped; one
/// that becomes the registered application may live for that app's lifetime.
#[cfg(unix)]
pub fn handoff_default_opener(program: &OsStr, path: &Path) -> io::Result<()> {
    let mut command = tokio::process::Command::new(program);
    command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _app = command.spawn()?;
    Ok(())
}

/// Starts an explicit CLI override as an [`OwnedOpener`]. On Unix that is the
/// same hided binary (`supervisor_exe`) in its internal supervisor mode, which
/// watches this owner's socket; Windows starts the helper itself and never
/// runs `supervisor_exe`.
#[cfg(windows)]
pub fn spawn_opener(
    _supervisor_exe: &Path,
    program: &OsStr,
    path: &Path,
) -> io::Result<OwnedOpener> {
    let mut command = Command::new(program);
    command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(OwnedOpener {
        helper: OwnedChild::spawn(&mut command)?,
    })
}

#[cfg(unix)]
pub fn spawn_opener(
    supervisor_exe: &Path,
    program: &OsStr,
    path: &Path,
) -> io::Result<OwnedOpener> {
    let (owner, supervisor_socket) = UnixStream::pair()?;
    let inherited_fd = supervisor_socket.as_raw_fd();
    let mut command = Command::new(supervisor_exe);
    command
        .arg("--open-helper")
        .arg(inherited_fd.to_string())
        .arg(program)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // UnixStream::pair uses close-on-exec. Only the supervisor receives its
    // end; the parent end stays close-on-exec and the launched app receives no
    // liveness descriptor.
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(inherited_fd, libc::F_SETFD, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    // The supervisor leads its own tree, which is the CLI helper's too.
    let supervisor = OwnedChild::spawn(&mut command)?;
    drop(supervisor_socket);
    let mut opener = OwnedOpener {
        supervisor,
        owner: Some(owner),
    };
    let owner = opener.owner.as_mut().expect("owner channel");
    owner.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut accepted = [0];
    if owner.read_exact(&mut accepted).is_err() || accepted != [1] {
        opener.stop();
        return Err(io::Error::other(
            "opener supervisor could not start the CLI helper",
        ));
    }
    owner.set_read_timeout(None)?;
    // EOF now observes supervisor completion without reaping its PID. That
    // keeps its process-group ID reserved until stop signals the group.
    owner.set_nonblocking(true)?;
    Ok(opener)
}

/// Invoked only by hided's private `--open-helper` mode. An owner death closes
/// the socket even after SIGKILL; the watcher then kills the still-running CLI
/// process group. Default-app handoff does not enter this helper.
#[cfg(unix)]
pub fn run_opener_helper(args: &[OsString]) -> Result<(), String> {
    if args.len() != 4 || args[0] != "--open-helper" {
        return Err("invalid opener helper arguments".into());
    }
    let fd: i32 = args[1]
        .to_str()
        .and_then(|value| value.parse().ok())
        .filter(|fd| *fd >= 3)
        .ok_or("invalid opener helper descriptor")?;
    let mut owner = unsafe { UnixStream::from_raw_fd(fd) };
    // The actual CLI must never inherit the liveness channel.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error().to_string());
    }
    let mut command = Command::new(&args[2]);
    command
        .arg(&args[3])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = owner.write_all(&[0]);
            return Err(error.to_string());
        }
    };
    #[cfg(debug_assertions)]
    if let Some(milliseconds) = std::env::var("HIDE_OPEN_HELPER_TEST_PAUSE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value <= 3_000)
    {
        std::thread::sleep(Duration::from_millis(milliseconds));
    }
    let group = std::process::id();
    let mut watcher_owner = match owner.try_clone() {
        Ok(socket) => socket,
        Err(error) => {
            let _ = process::kill_tree(group);
            let _ = child.wait();
            return Err(error.to_string());
        }
    };
    let watcher = std::thread::Builder::new()
        .name("hide-open-owner".into())
        .spawn(move || {
            let mut byte = [0];
            // Any read or EOF means the daemon stopped. The CLI cannot inherit
            // this close-on-exec descriptor.
            let _ = watcher_owner.read(&mut byte);
            let _ = process::kill_tree(group);
        });
    if let Err(error) = watcher {
        let _ = process::kill_tree(group);
        let _ = child.wait();
        return Err(error.to_string());
    }
    // Acceptance follows watcher creation. The parent's process-group fallback
    // covers the scheduling gap before the new thread gets CPU time.
    if owner.write_all(&[1]).is_err() {
        let _ = process::kill_tree(group);
        let _ = child.wait();
        return Err("opener owner disappeared before acceptance".into());
    }
    let result = child.wait().map_err(|error| error.to_string());
    // An explicit override is a CLI helper, not a registered default app.
    // It may have forked and returned, so close its process group on exit too.
    let _ = process::kill_tree(group);
    result.map(|_| ())
}
