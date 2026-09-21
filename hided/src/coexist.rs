//! Whether the Swift shell already holds the Herdr socket hided is about to
//! attach to (PRD B5, D-05).
//!
//! The rule is about one socket, not about the Swift app's existence: the
//! shell attached to the operator's default socket while hided attaches to an
//! isolated one is the ordinary e2e and measurement arrangement and must run
//! without a prompt. The earlier version read "any socket set" as "Swift is
//! attached" and refused every isolated run that had no TTY.
//!
//! The socket the shell holds is read from its own environment: the shell
//! resolves it with one rule (`RuntimeEnvironment.herdrSocketPath`, an
//! absolute `HERDR_SOCKET_PATH` override, else `$HOME/.config/herdr/herdr.sock`),
//! so applying the same rule to the same variables names the same file. The
//! environment comes from the kernel (`KERN_PROCARGS2`), NUL-separated, so a
//! value is never split on a space the way `ps -E` output would be.

use std::path::{Path, PathBuf};

pub const SWIFT_EXECUTABLE: &str = "HerdrMacOS";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwiftShell {
    pub pid: u32,
    /// The socket the shell resolved, or why it could not be read.
    pub socket: Result<PathBuf, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Coexist {
    /// No Swift shell process on this machine.
    NoSwiftShell,
    /// Swift runs, but on another socket or hided attaches to none.
    Isolated { swift: SwiftShell },
    /// Both would attach to the same socket: warn, and refuse without a TTY.
    SharedSocket { swift: SwiftShell, socket: PathBuf },
    /// Swift runs and its socket could not be read; treated like a shared
    /// socket because the answer is unknown, and the reason is shown.
    Unknown { swift: SwiftShell, reason: String },
}

/// Decides the coexistence outcome from the socket hided will attach to and
/// the Swift shell found on the machine, if any.
pub fn classify(target: Option<&Path>, swift: Option<SwiftShell>) -> Coexist {
    let Some(swift) = swift else {
        return Coexist::NoSwiftShell;
    };
    let Some(target) = target else {
        return Coexist::Isolated { swift };
    };
    match &swift.socket {
        Ok(held) if same_file(held, target) => Coexist::SharedSocket {
            socket: held.clone(),
            swift,
        },
        Ok(_) => Coexist::Isolated { swift },
        Err(reason) => Coexist::Unknown {
            reason: reason.clone(),
            swift,
        },
    }
}

fn same_file(left: &Path, right: &Path) -> bool {
    let resolve = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    resolve(left) == resolve(right)
}

/// The Swift shell's socket rule applied to its environment entries.
///
/// Mirrors `RuntimeEnvironment.herdrSocketPath` in the shell: an absolute
/// `HERDR_SOCKET_PATH` wins, otherwise the default under the shell's `HOME`.
pub fn socket_from_environment<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<PathBuf, String> {
    let mut override_path = None;
    let mut home = None;
    for (key, value) in entries {
        match key {
            "HERDR_SOCKET_PATH" if value.starts_with('/') => override_path = Some(value),
            "HOME" if !value.is_empty() => home = Some(value),
            _ => {}
        }
    }
    if let Some(path) = override_path {
        return Ok(PathBuf::from(path));
    }
    home.map(|home| Path::new(home).join(".config/herdr/herdr.sock"))
        .ok_or_else(|| {
            "the Swift shell's environment carries neither HERDR_SOCKET_PATH nor HOME".to_owned()
        })
}

/// Finds one running Swift shell and the socket it resolved.
///
/// Only macOS runs the shell, so every other platform answers `None`.
#[cfg(target_os = "macos")]
pub fn find_swift_shell() -> Option<SwiftShell> {
    let pid = macos::pids()
        .into_iter()
        .find(|pid| macos::executable_name(*pid).as_deref() == Some(SWIFT_EXECUTABLE))?;
    Some(SwiftShell {
        pid,
        socket: socket_of(pid),
    })
}

#[cfg(not(target_os = "macos"))]
pub fn find_swift_shell() -> Option<SwiftShell> {
    None
}

/// The socket a process resolved by the Swift rule, read from its environment.
#[cfg(target_os = "macos")]
pub fn socket_of(pid: u32) -> Result<PathBuf, String> {
    let environment = macos::environment(pid)?;
    socket_from_environment(
        environment
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    )
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_int, c_void};
    use std::io;

    /// Every pid the kernel lists, or none when the query fails.
    pub fn pids() -> Vec<u32> {
        // SAFETY: a null buffer asks for the count; the second call receives a
        // buffer of at least that many pids and returns how many were written.
        unsafe {
            let count = libc::proc_listallpids(std::ptr::null_mut(), 0);
            if count <= 0 {
                return Vec::new();
            }
            let mut buffer = vec![0 as libc::pid_t; count as usize + 16];
            let byte_len = (buffer.len() * std::mem::size_of::<libc::pid_t>()) as c_int;
            let written = libc::proc_listallpids(buffer.as_mut_ptr() as *mut c_void, byte_len);
            if written <= 0 {
                return Vec::new();
            }
            buffer.truncate(written as usize);
            buffer
                .into_iter()
                .filter(|pid| *pid > 0)
                .map(|pid| pid as u32)
                .collect()
        }
    }

    /// The file name of a process's executable, or `None` when the kernel
    /// refuses (another user's process, or one that exited meanwhile).
    pub fn executable_name(pid: u32) -> Option<String> {
        let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the buffer is at least PROC_PIDPATHINFO_MAXSIZE bytes, which
        // is the size proc_pidpath documents as sufficient.
        let written = unsafe {
            libc::proc_pidpath(
                pid as c_int,
                buffer.as_mut_ptr() as *mut c_void,
                buffer.len() as u32,
            )
        };
        if written <= 0 {
            return None;
        }
        let path = String::from_utf8_lossy(&buffer[..written as usize]).into_owned();
        path.rsplit('/').next().map(str::to_owned)
    }

    /// A process's environment as it was at exec, from `KERN_PROCARGS2`.
    ///
    /// The buffer is `argc`, the executable path, NUL padding, `argc`
    /// NUL-terminated arguments, then NUL-terminated `KEY=VALUE` entries.
    pub fn environment(pid: u32) -> Result<Vec<(String, String)>, String> {
        let raw = procargs(pid)
            .map_err(|error| format!("cannot read the environment of pid {pid}: {error}"))?;
        parse_procargs(&raw)
            .ok_or_else(|| format!("pid {pid} has a malformed KERN_PROCARGS2 buffer"))
    }

    fn procargs(pid: u32) -> io::Result<Vec<u8>> {
        // A size query on KERN_PROCARGS2 answers the argument block only and
        // leaves the environment out, so the buffer is sized by kern.argmax,
        // the kernel's own bound on the whole block.
        let mut argmax_mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        let mut argmax: c_int = 0;
        let mut argmax_len = std::mem::size_of::<c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as c_int];
        // SAFETY: each output pointer is paired with the exact length of the
        // buffer it points at, and the kernel writes back the used length.
        unsafe {
            if libc::sysctl(
                argmax_mib.as_mut_ptr(),
                argmax_mib.len() as u32,
                &mut argmax as *mut c_int as *mut c_void,
                &mut argmax_len,
                std::ptr::null_mut(),
                0,
            ) != 0
            {
                return Err(io::Error::last_os_error());
            }
            let mut size = argmax.max(0) as libc::size_t;
            let mut buffer = vec![0u8; size];
            if libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                buffer.as_mut_ptr() as *mut c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            ) != 0
            {
                return Err(io::Error::last_os_error());
            }
            buffer.truncate(size);
            Ok(buffer)
        }
    }

    pub(super) fn parse_procargs(raw: &[u8]) -> Option<Vec<(String, String)>> {
        let argc = c_int::from_ne_bytes(raw.get(..4)?.try_into().ok()?);
        let mut rest = &raw[4..];
        // Executable path, then the padding that follows it.
        let exe_end = rest.iter().position(|byte| *byte == 0)?;
        rest = &rest[exe_end..];
        while rest.first() == Some(&0) {
            rest = &rest[1..];
        }
        for _ in 0..argc.max(0) {
            let end = rest.iter().position(|byte| *byte == 0)?;
            rest = &rest[end + 1..];
        }
        let mut entries = Vec::new();
        while !rest.is_empty() {
            let end = rest
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(rest.len());
            let entry = &rest[..end];
            rest = if end < rest.len() {
                &rest[end + 1..]
            } else {
                &[]
            };
            if entry.is_empty() {
                break;
            }
            let text = String::from_utf8_lossy(entry);
            if let Some((key, value)) = text.split_once('=') {
                entries.push((key.to_owned(), value.to_owned()));
            }
        }
        Some(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swift(socket: Result<&str, &str>) -> SwiftShell {
        SwiftShell {
            pid: 4242,
            socket: socket.map(PathBuf::from).map_err(str::to_owned),
        }
    }

    #[test]
    fn no_swift_shell_never_warns() {
        assert_eq!(
            classify(
                Some(Path::new("/Users/example/.config/herdr/herdr.sock")),
                None
            ),
            Coexist::NoSwiftShell
        );
    }

    #[test]
    fn an_isolated_socket_never_warns_while_swift_holds_the_default() {
        let shell = swift(Ok("/Users/example/.config/herdr/herdr.sock"));
        let outcome = classify(Some(Path::new("/tmp/hide-e2e-1.sock")), Some(shell.clone()));
        assert_eq!(outcome, Coexist::Isolated { swift: shell });
    }

    #[test]
    fn no_target_socket_never_warns() {
        let shell = swift(Ok("/Users/example/.config/herdr/herdr.sock"));
        assert_eq!(
            classify(None, Some(shell.clone())),
            Coexist::Isolated { swift: shell }
        );
    }

    #[test]
    fn the_same_socket_warns() {
        let shell = swift(Ok("/Users/example/.config/herdr/herdr.sock"));
        let outcome = classify(
            Some(Path::new("/Users/example/.config/herdr/herdr.sock")),
            Some(shell.clone()),
        );
        assert_eq!(
            outcome,
            Coexist::SharedSocket {
                swift: shell,
                socket: PathBuf::from("/Users/example/.config/herdr/herdr.sock"),
            }
        );
    }

    #[test]
    fn swift_on_an_override_socket_warns_only_for_that_socket() {
        let shell = swift(Ok("/tmp/swift-private.sock"));
        assert!(matches!(
            classify(
                Some(Path::new("/tmp/swift-private.sock")),
                Some(shell.clone())
            ),
            Coexist::SharedSocket { .. }
        ));
        assert!(matches!(
            classify(
                Some(Path::new("/Users/example/.config/herdr/herdr.sock")),
                Some(shell)
            ),
            Coexist::Isolated { .. }
        ));
    }

    #[test]
    fn an_unreadable_swift_environment_is_unknown_not_isolated() {
        let shell = swift(Err("cannot read the environment of pid 4242: EPERM"));
        assert!(matches!(
            classify(Some(Path::new("/tmp/hide-e2e-1.sock")), Some(shell)),
            Coexist::Unknown { .. }
        ));
    }

    #[test]
    fn socket_rule_matches_the_swift_shell() {
        assert_eq!(
            socket_from_environment([("HOME", "/Users/example")]).unwrap(),
            PathBuf::from("/Users/example/.config/herdr/herdr.sock")
        );
        assert_eq!(
            socket_from_environment([
                ("HOME", "/Users/example"),
                ("HERDR_SOCKET_PATH", "/tmp/private.sock"),
            ])
            .unwrap(),
            PathBuf::from("/tmp/private.sock")
        );
        // A relative override is ignored by the shell, so it is ignored here.
        assert_eq!(
            socket_from_environment([
                ("HOME", "/Users/example"),
                ("HERDR_SOCKET_PATH", "rel.sock")
            ])
            .unwrap(),
            PathBuf::from("/Users/example/.config/herdr/herdr.sock")
        );
        assert!(socket_from_environment([("PATH", "/usr/bin")]).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_a_procargs_buffer() {
        let mut raw = 2i32.to_ne_bytes().to_vec();
        raw.extend_from_slice(
            b"/bin/sleep\x00\x00\x00sleep\x0030\x00HOME=/Users/example\x00HERDR_SOCKET_PATH=/tmp/a b.sock\x00\x00",
        );
        let entries = macos::parse_procargs(&raw).unwrap();
        assert_eq!(
            entries,
            vec![
                ("HOME".to_owned(), "/Users/example".to_owned()),
                ("HERDR_SOCKET_PATH".to_owned(), "/tmp/a b.sock".to_owned()),
            ]
        );
    }

    /// Re-entered as the child of the test below: with the marker set it
    /// sleeps so the parent can read its environment; otherwise it is a no-op.
    #[cfg(target_os = "macos")]
    #[test]
    fn child_holds_a_socket() {
        if std::env::var_os("HIDE_COEXIST_CHILD").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    /// The kernel read against a real child: the socket the child was given
    /// is the socket read back, and a spaced value survives whole. The child
    /// is this test binary, because macOS hides the environment of a platform
    /// binary such as /bin/sleep from KERN_PROCARGS2.
    #[cfg(target_os = "macos")]
    #[test]
    fn reads_the_socket_a_live_process_resolved() {
        let exe = std::env::current_exe().expect("test executable");
        let mut child = std::process::Command::new(&exe)
            .args([
                "--exact",
                "coexist::tests::child_holds_a_socket",
                "--nocapture",
            ])
            .env("HIDE_COEXIST_CHILD", "1")
            .env("HOME", "/Users/example")
            .env("HERDR_SOCKET_PATH", "/tmp/coexist test.sock")
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn child test");
        std::thread::sleep(std::time::Duration::from_millis(200));
        let socket = socket_of(child.id());
        let name = macos::executable_name(child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(socket.unwrap(), PathBuf::from("/tmp/coexist test.sock"));
        assert_eq!(
            name,
            exe.file_name().map(|n| n.to_string_lossy().into_owned())
        );
    }
}
