use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

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
}

pub fn state_path(dir: &Path) -> PathBuf {
    dir.join("hided.json")
}

pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join("hided.lock")
}

pub fn write_state(dir: &Path, state: &DaemonState) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = state_path(dir);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(serde_json::to_vec_pretty(state)?.as_slice())?;
    file.sync_all()?;
    let mut permissions = file.metadata()?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&path, permissions)?;
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

pub fn remove_state(dir: &Path) {
    let _ = fs::remove_file(state_path(dir));
    let _ = fs::remove_file(lock_path(dir));
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
    fs::create_dir_all(dir)?;
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
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&staging)?;
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
    fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(lock_path(dir))?;
    let result = unsafe { libc::flock(use_raw_fd(&file), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another hide instance holds the lock",
        ));
    }
    Ok(file)
}

fn use_raw_fd(file: &File) -> i32 {
    use std::os::fd::AsRawFd;
    file.as_raw_fd()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_file_is_mode_600() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_state(
            dir.path(),
            &DaemonState {
                pid: 1,
                port: 9,
                token: "aa".into(),
                socket: None,
                started_at: "now".into(),
            },
        )
        .unwrap();
        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
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
