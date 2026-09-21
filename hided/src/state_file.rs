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

pub fn acquire_lock(dir: &Path) -> io::Result<File> {
    fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
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
}
