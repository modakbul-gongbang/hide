use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use hide_platform::fs::{Access, atomic, private};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 2;
pub const MAX_CLIENTS: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonState {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub socket: Option<String>,
    pub started_at: String,
    /// When the daemon's process started, as the platform layer reports it,
    /// written with the pid so a later reader can tell the daemon from
    /// another process that reused the pid. A state written by an earlier
    /// build has none, and its pid is judged by liveness alone.
    #[serde(default)]
    pub pid_started: Option<u64>,
}

/// The state folder, and any parent made with it such as `~/.hide`, is made
/// private: it holds the daemon's token. An existing folder keeps its mode.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    private::create_dir_all(dir)
}

pub fn state_path(dir: &Path) -> PathBuf {
    dir.join("hided.json")
}

pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join("hided.lock")
}

/// Writes the state whole and private: a reader sees the previous state or
/// this one, never part of either.
pub fn write_state(dir: &Path, state: &DaemonState) -> io::Result<PathBuf> {
    create_private_dir(dir)?;
    let path = state_path(dir);
    atomic::write_file(&path, &serde_json::to_vec_pretty(state)?, Access::Private)?;
    Ok(path)
}

pub fn read_state(dir: &Path) -> io::Result<Option<DaemonState>> {
    let path = state_path(dir);
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Removes the state file when it still names `pid`, the daemon a caller
/// just stopped. The instance lock file is never unlinked here: a daemon
/// started since holds its lock on that file, and unlinking it would let a
/// third daemon take a fresh one beside it.
pub fn forget_daemon(dir: &Path, pid: u32) {
    if read_state(dir)
        .ok()
        .flatten()
        .is_some_and(|state| state.pid == pid)
    {
        let _ = fs::remove_file(state_path(dir));
    }
}

/// Serializes `hide connect`s on one state folder: held from looking at the
/// running daemon until the one to attach to answers. The file persists;
/// the lock is released with the returned handle.
pub fn lock_connect(dir: &Path) -> io::Result<File> {
    create_private_dir(dir)?;
    let file = private::open_own_file(&dir.join("connect.lock"), true)?;
    file.lock()?;
    Ok(file)
}

pub fn new_token() -> String {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).expect("getrandom");
    hex::encode(bytes)
}

/// This daemon host's identity, kept in its state directory: the first start
/// writes one and every later start reads it back, so a browser's unsaved
/// drafts name the host that held them (PRD S5.5 B9-B12) and a daemon
/// restart, a new port or a new token is still the same host. Written to a
/// temporary file and renamed, so an interrupted first start leaves no torn id.
pub fn host_id(dir: &Path) -> io::Result<String> {
    create_private_dir(dir)?;
    let path = dir.join("host-id");
    match fs::read_to_string(&path) {
        Ok(text) if is_host_id(text.trim()) => return Ok(text.trim().to_owned()),
        Ok(text) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} does not hold a host id ({} bytes); refusing to replace it",
                    path.display(),
                    text.len()
                ),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).expect("getrandom");
    let id = format!("host-{}", hex::encode(bytes));
    let staging = dir.join(format!("host-id.{}.tmp", std::process::id()));
    // A start that died here left its staging file; this pid is ours now.
    let _ = fs::remove_file(&staging);
    let mut file = private::create_new_file(&staging)?;
    file.write_all(id.as_bytes())?;
    file.sync_all()?;
    // Another start that won the race keeps its id; this one reads it.
    match fs::hard_link(&staging, &path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            let _ = fs::remove_file(&staging);
            return Err(error);
        }
    }
    let _ = fs::remove_file(&staging);
    let stored = fs::read_to_string(&path)?;
    Ok(stored.trim().to_owned())
}

fn is_host_id(text: &str) -> bool {
    text.strip_prefix("host-")
        .is_some_and(|hex| hex.len() == 32 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

pub fn acquire_lock(dir: &Path) -> io::Result<File> {
    create_private_dir(dir)?;
    let file = private::open_own_file(&lock_path(dir), true)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(fs::TryLockError::WouldBlock) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another hide instance holds the lock",
        )),
        Err(fs::TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_file_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_state(
            dir.path(),
            &DaemonState {
                pid: 1,
                port: 9,
                token: "aa".into(),
                socket: None,
                started_at: "now".into(),
                pid_started: None,
            },
        )
        .unwrap();
        assert!(private::is_private(&path).unwrap());
    }

    #[test]
    fn a_host_keeps_its_id_across_starts_and_a_damaged_id_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let first = host_id(dir.path()).unwrap();
        assert!(is_host_id(&first));
        assert_eq!(host_id(dir.path()).unwrap(), first);
        fs::write(dir.path().join("host-id"), "garbage").unwrap();
        assert!(host_id(dir.path()).is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("host-id")).unwrap(),
            "garbage"
        );
    }
}
