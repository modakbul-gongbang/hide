//! One label generator per Herdr server (PRD labels-in-hided D-10).
//!
//! Every hided that follows the same Herdr server would otherwise analyze
//! the same turns. The worker that holds an exclusive `flock` on the
//! server's lock file generates; any other shows provider names, logs once
//! that it is standing by, and tries again every thirty seconds, so it
//! takes over when the holder exits. The lock is advisory and released by
//! the kernel with its process, so a crashed holder blocks nobody.
//!
//! A local server's lock sits beside its socket, the one place every daemon
//! that reaches the server shares whatever HOME it runs with, so a daemon
//! started with a private HOME but the operator's socket stands by too. A
//! device's lock sits under the daemon's HOME, which owns the registration.

use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;
use sha2::{Digest, Sha256};

const RETRY: Duration = Duration::from_secs(30);

/// The lock of the local Herdr server listening at `socket`.
pub(crate) fn local_lock_path(socket: &Path) -> PathBuf {
    let socket = std::fs::canonicalize(socket).unwrap_or_else(|_| socket.to_path_buf());
    let name = socket
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "herdr.sock".to_owned());
    socket.with_file_name(format!("{name}.hide-label-generator.lock"))
}

/// The lock of a registered device's server, in the daemon's state folder,
/// so an isolated daemon never writes the operator's (PRD hide-home-layout
/// D-06).
pub(crate) fn device_lock_path(state_dir: &Path, device_id: &str) -> PathBuf {
    let digest = Sha256::digest(format!("device:{device_id}").as_bytes());
    hide_kit::layout::label_generators(state_dir).join(format!("{digest:x}.lock"))
}

pub(crate) struct GeneratorLock {
    path: Option<PathBuf>,
    target: String,
    held: Option<File>,
    next_attempt: Option<Instant>,
}

impl GeneratorLock {
    /// `path` `None` (a device worker with no home) holds the role
    /// unconditionally: there is no shared place another daemon could
    /// coordinate through.
    pub(crate) fn new(path: Option<PathBuf>, target: &str) -> Self {
        Self {
            path,
            target: target.to_owned(),
            held: None,
            next_attempt: None,
        }
    }

    /// Whether this worker generates now. Returns `(held, took_over)`, where
    /// `took_over` means it was standing by until now, so what changed while
    /// another daemon generated has to be caught up.
    pub(crate) fn ensure(&mut self, now: Instant) -> (bool, bool) {
        let Some(path) = self.path.as_deref() else {
            return (true, false);
        };
        if self.held.is_some() {
            return (true, false);
        }
        if self.next_attempt.is_some_and(|at| now < at) {
            return (false, false);
        }
        let first = self.next_attempt.is_none();
        self.next_attempt = Some(now + RETRY);
        match try_lock(path) {
            Ok(Some(file)) => {
                record_holder(&file);
                self.held = Some(file);
                if !first {
                    crate::diagnostic!(json!({
                        "component": "labels",
                        "kind": "generator.acquired",
                        "target": self.target,
                    }));
                }
                (true, !first)
            }
            Ok(None) => {
                if first {
                    crate::diagnostic!(json!({
                        "component": "labels",
                        "kind": "generator.standby",
                        "target": self.target,
                        "holder": holder(path),
                    }));
                }
                (false, false)
            }
            Err(error) => {
                if first {
                    crate::diagnostic!(json!({
                        "component": "labels",
                        "kind": "generator.lock_failed",
                        "target": self.target,
                        "message": error.to_string(),
                    }));
                }
                (false, false)
            }
        }
    }

    pub(crate) fn held(&self) -> bool {
        self.path.is_none() || self.held.is_some()
    }
}

/// The holder writes who it is into the lock, so a daemon standing by can
/// say which process generates for its server: a stray daemon on the same
/// Herdr is otherwise invisible.
fn record_holder(file: &File) {
    use std::io::{Seek, Write};
    let mut file = file;
    let holder = json!({"pid": std::process::id()}).to_string();
    let _ = file
        .set_len(0)
        .and_then(|()| file.seek(std::io::SeekFrom::Start(0)))
        .and_then(|_| file.write_all(holder.as_bytes()));
}

/// The pid the holder wrote, and nothing else of the file; `null` when it
/// cannot be read.
fn holder(path: &Path) -> serde_json::Value {
    use std::io::Read;
    let mut bytes = Vec::new();
    let read = open_private(path, false).and_then(|file| file.take(64).read_to_end(&mut bytes));
    read.ok()
        .and_then(|_| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|holder| holder.get("pid").and_then(serde_json::Value::as_u64))
        .map_or(serde_json::Value::Null, serde_json::Value::from)
}

/// Opens the lock file itself, never what a link at its path names, and
/// only when it is a regular file of this user: the lock can sit in a
/// directory other users write (a socket under `/tmp`), where a planted
/// link would otherwise have its target truncated.
fn open_private(path: &Path, create: bool) -> std::io::Result<File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = std::fs::OpenOptions::new()
        .create(create)
        .truncate(false)
        .read(true)
        .write(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    // SAFETY: geteuid has no preconditions and no memory effects.
    if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::other(
            "the lock is not this user's regular file",
        ));
    }
    Ok(file)
}

fn try_lock(path: &Path) -> std::io::Result<Option<File>> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let file = open_private(path, true)?;
    // SAFETY: `flock` on a descriptor this function owns; no memory is
    // shared with the call.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(Some(file));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_planted_at_the_lock_path_is_neither_followed_nor_held() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("herdr.sock");
        let target = dir.path().join("operator-file.json");
        std::fs::write(&target, r#"{"secret":"kept"}"#).unwrap();
        std::os::unix::fs::symlink(&target, local_lock_path(&socket)).unwrap();

        let mut lock = GeneratorLock::new(Some(local_lock_path(&socket)), "local");
        assert_eq!(lock.ensure(Instant::now()), (false, false));
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            r#"{"secret":"kept"}"#
        );
        assert_eq!(holder(&local_lock_path(&socket)), serde_json::Value::Null);
    }

    #[test]
    fn a_second_worker_for_the_same_server_stands_by_until_the_first_ends() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("herdr.sock");
        let mut first = GeneratorLock::new(Some(local_lock_path(&socket)), "local");
        let mut second = GeneratorLock::new(Some(local_lock_path(&socket)), "local");
        let mut other = GeneratorLock::new(
            Some(local_lock_path(&dir.path().join("other.sock"))),
            "local",
        );
        let now = Instant::now();
        assert_eq!(first.ensure(now), (true, false));
        assert_eq!(second.ensure(now), (false, false));
        assert_eq!(
            holder(&local_lock_path(&socket)),
            std::process::id(),
            "the standby can name the process that generates"
        );
        assert_eq!(other.ensure(now), (true, false));
        drop(first);
        // Inside the retry window nothing is tried; after it, it takes over.
        assert_eq!(second.ensure(now + Duration::from_secs(1)), (false, false));
        assert_eq!(second.ensure(now + RETRY), (true, true));
    }
}
