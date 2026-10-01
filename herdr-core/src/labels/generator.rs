//! One label generator per Herdr server (PRD labels-in-hided D-10).
//!
//! Every hided that follows the same Herdr server would otherwise analyze
//! the same turns. The worker that holds an exclusive `flock` on a file
//! named after the server generates; any other shows provider names, logs
//! once that it is standing by, and tries again every thirty seconds, so it
//! takes over when the holder exits. The lock is advisory and released by
//! the kernel with its process, so a crashed holder blocks nobody.

use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;
use sha2::{Digest, Sha256};

const RETRY: Duration = Duration::from_secs(30);

/// Where generator locks live under a home.
pub(crate) fn lock_dir(home: &Path) -> PathBuf {
    home.join(".local/state/hide/label-generators")
}

pub(crate) struct GeneratorLock {
    path: Option<PathBuf>,
    target: String,
    held: Option<File>,
    next_attempt: Option<Instant>,
}

impl GeneratorLock {
    /// `dir` `None` (no home) holds the role unconditionally: there is no
    /// shared place another daemon could coordinate through.
    pub(crate) fn new(dir: Option<&Path>, server_key: &str, target: &str) -> Self {
        let path = dir.map(|dir| {
            let digest = Sha256::digest(server_key.as_bytes());
            dir.join(format!("{digest:x}.lock"))
        });
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

fn try_lock(path: &Path) -> std::io::Result<Option<File>> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
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
    fn a_second_worker_for_the_same_server_stands_by_until_the_first_ends() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = GeneratorLock::new(Some(dir.path()), "/tmp/herdr.sock", "local");
        let mut second = GeneratorLock::new(Some(dir.path()), "/tmp/herdr.sock", "local");
        let mut other = GeneratorLock::new(Some(dir.path()), "/tmp/other.sock", "local");
        let now = Instant::now();
        assert_eq!(first.ensure(now), (true, false));
        assert_eq!(second.ensure(now), (false, false));
        assert_eq!(other.ensure(now), (true, false));
        drop(first);
        // Inside the retry window nothing is tried; after it, it takes over.
        assert_eq!(second.ensure(now + Duration::from_secs(1)), (false, false));
        assert_eq!(second.ensure(now + RETRY), (true, true));
    }
}
