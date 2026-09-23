//! Owned process entrypoints for the CLI daemon and short-lived file openers.

use std::io;
use std::process::{Child, Command, Stdio};

#[cfg(unix)]
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::time::{Duration, Instant};

/// Cap on resident hided children this CLI owns. Crossing it is a failure.
pub const MAX_DAEMON_CHILDREN: usize = 1;

/// Starts a deliberately detached `hided` daemon from the short-lived CLI.
/// The file-opener ownership path below has the opposite lifetime policy.
pub fn spawn_owned(command: &mut Command) -> io::Result<Child> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// A short-lived CLI opener and the private owner channel to its supervisor.
/// The channel's EOF also reaches the supervisor when hided dies without Drop.
#[cfg(unix)]
pub struct OwnedOpener {
    supervisor: Child,
    owner: Option<UnixStream>,
}

#[cfg(unix)]
impl OwnedOpener {
    pub fn try_wait(&mut self) -> io::Result<bool> {
        Ok(self.supervisor.try_wait()?.is_some())
    }

    pub fn stop(&mut self) {
        // EOF makes the supervisor end its CLI process group first. A stuck
        // supervisor is then killed and reaped by this owner.
        self.owner.take();
        let until = Instant::now() + Duration::from_millis(250);
        while Instant::now() < until {
            match self.supervisor.try_wait() {
                Ok(Some(_)) => return,
                Err(_) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let _ = self.supervisor.kill();
        let _ = self.supervisor.wait();
    }
}

#[cfg(unix)]
impl Drop for OwnedOpener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the same hided binary in its internal opener-supervisor mode. The
/// supervisor alone starts the CLI utility and watches this owner's socket.
#[cfg(unix)]
pub fn spawn_opener(
    supervisor_exe: &Path,
    program: &OsStr,
    path: &Path,
    override_program: bool,
) -> io::Result<OwnedOpener> {
    let (owner, supervisor_socket) = UnixStream::pair()?;
    let inherited_fd = supervisor_socket.as_raw_fd();
    let mut command = Command::new(supervisor_exe);
    command
        .arg("--open-helper")
        .arg(inherited_fd.to_string())
        .arg(if override_program {
            "override"
        } else {
            "default"
        })
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
    let supervisor = command.spawn()?;
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
    Ok(opener)
}

/// Invoked only by hided's private `--open-helper` mode. An owner death closes
/// the socket even after SIGKILL; the watcher then kills the still-running CLI
/// process group. Successful default-app handoff is not killed after exit.
#[cfg(unix)]
pub fn run_opener_helper(args: &[OsString]) -> Result<(), String> {
    if args.len() != 5 || args[0] != "--open-helper" {
        return Err("invalid opener helper arguments".into());
    }
    let fd: i32 = args[1]
        .to_str()
        .and_then(|value| value.parse().ok())
        .filter(|fd| *fd >= 3)
        .ok_or("invalid opener helper descriptor")?;
    let override_program = match args[2].to_str() {
        Some("override") => true,
        Some("default") => false,
        _ => return Err("invalid opener helper mode".into()),
    };
    let mut owner = unsafe { UnixStream::from_raw_fd(fd) };
    // The actual CLI must never inherit the liveness channel.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error().to_string());
    }
    let mut command = Command::new(&args[3]);
    command
        .arg(&args[4])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = owner.write_all(&[0]);
            return Err(error.to_string());
        }
    };
    let group = child.id() as i32;
    if owner.write_all(&[1]).is_err() {
        let _ = unsafe { libc::killpg(group, libc::SIGKILL) };
        let _ = child.wait();
        return Err("opener owner disappeared before acceptance".into());
    }
    let watcher = std::thread::Builder::new()
        .name("hide-open-owner".into())
        .spawn(move || {
            let mut byte = [0];
            // Any read or EOF means the owner stopped. The supervisor is the only
            // process with this socket end, so a closed daemon cannot be missed.
            let _ = owner.read(&mut byte);
            let _ = unsafe { libc::killpg(group, libc::SIGKILL) };
        });
    if let Err(error) = watcher {
        let _ = unsafe { libc::killpg(group, libc::SIGKILL) };
        let _ = child.wait();
        return Err(error.to_string());
    }
    let result = child.wait().map_err(|error| error.to_string());
    // An explicit override is a CLI helper, not a registered default app.
    // It may have forked and returned, so close its process group on exit too.
    if override_program || result.is_err() {
        let _ = unsafe { libc::killpg(group, libc::SIGKILL) };
    }
    result.map(|_| ())
}
