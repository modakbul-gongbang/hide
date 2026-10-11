//! Where a pane's hook finds the Memory store of the daemon that runs it.
//!
//! A daemon started with a moved state folder (`HIDE_STATE_DIR`,
//! `XDG_STATE_HOME`) writes its store there, and a hook takes no path from
//! its environment. The daemon therefore leaves a record in the default state
//! folder, which the hook already trusts for the default store, naming its
//! Herdr socket and its state folder. The hook looks the record up by the
//! socket Herdr puts in every pane (`HERDR_SOCKET_PATH`), used only as the
//! record's name and never opened: the folder it reads comes from the record
//! the daemon wrote, under the account's own private folder.
//!
//! The record names one daemon per Herdr socket. A daemon never replaces the
//! record of a live daemon, a record of a dead one is removed when any daemon
//! registers, and a daemon removes its own on every exit it can run code on.

use std::io::Read;
use std::path::{Path, PathBuf};

use hide_platform::fs::Access;
use hide_platform::fs::atomic::write_file;
use hide_platform::fs::private::{
    create_dir_all, is_private, open_own_file, owned_by_current_user, restrict_to_owner,
};
use hide_platform::process;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The folder under the default state folder that holds the records.
pub const REGISTRY_DIR: &str = "daemons";
/// Records one folder holds; a daemon that would make more is not registered.
pub const REGISTRY_LIMIT: usize = 32;
const RECORD_LIMIT_BYTES: u64 = 4096;

#[derive(Debug, Deserialize, Serialize)]
struct Record {
    herdr_socket: String,
    state_dir: String,
    pid: u32,
    /// `process::start_time(pid)`, so a recycled pid is not a live daemon.
    started: u64,
}

impl Record {
    fn alive(&self) -> bool {
        process::is_alive(self.pid)
            && process::start_time(self.pid).is_ok_and(|started| started == self.started)
    }
}

/// Why a daemon was not registered. The hook then reads the default store.
#[derive(Debug)]
pub enum RegisterError {
    Io(std::io::Error),
    /// The state folder is not an absolute path.
    NotAbsolute,
    /// A live daemon already holds the record for this Herdr socket.
    HeldBy(u32),
    /// The folder already holds [`REGISTRY_LIMIT`] records of live daemons.
    Full,
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::NotAbsolute => write!(formatter, "the state folder is not an absolute path"),
            Self::HeldBy(pid) => write!(formatter, "daemon {pid} holds this Herdr socket's record"),
            Self::Full => write!(formatter, "{REGISTRY_LIMIT} daemons are registered"),
        }
    }
}

impl From<std::io::Error> for RegisterError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// A daemon's record, removed when this is dropped.
#[derive(Debug)]
pub struct Registration {
    path: PathBuf,
    pid: u32,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // Only the record this daemon wrote: a later daemon of the same
        // socket may have replaced a record it found dead.
        if read_record(&self.path).is_some_and(|record| record.pid == self.pid) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn registry(default_state_dir: &Path) -> PathBuf {
    default_state_dir.join(REGISTRY_DIR)
}

fn record_path(default_state_dir: &Path, herdr_socket: &str) -> PathBuf {
    let name: String = Sha256::digest(herdr_socket.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    registry(default_state_dir).join(format!("{name}.json"))
}

fn read_record(path: &Path) -> Option<Record> {
    let file = open_own_file(path, false).ok()?;
    if !is_private(path).ok()? {
        return None;
    }
    let mut raw = Vec::new();
    file.take(RECORD_LIMIT_BYTES + 1)
        .read_to_end(&mut raw)
        .ok()?;
    if raw.len() as u64 > RECORD_LIMIT_BYTES {
        return None;
    }
    serde_json::from_slice(&raw).ok()
}

/// Records that the daemon running here keeps its store in `state_dir`, for
/// the panes of the Herdr at `herdr_socket`.
///
/// Nothing is written, and `Ok(None)` answers, when `state_dir` is the default
/// state folder: a hook reads that one without a record.
pub fn register(
    default_state_dir: &Path,
    herdr_socket: &str,
    state_dir: &Path,
) -> Result<Option<Registration>, RegisterError> {
    if !state_dir.is_absolute() {
        return Err(RegisterError::NotAbsolute);
    }
    if same_folder(state_dir, default_state_dir) {
        return Ok(None);
    }
    let folder = registry(default_state_dir);
    create_dir_all(&folder)?;
    restrict_to_owner(&folder)?;
    let path = record_path(default_state_dir, herdr_socket);
    let mut live = 0;
    for entry in std::fs::read_dir(&folder)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        match read_record(&entry_path) {
            Some(record) if record.alive() => {
                if entry_path == path {
                    return Err(RegisterError::HeldBy(record.pid));
                }
                live += 1;
            }
            // Dead, unreadable or not the account's own: not a record to keep.
            _ => {
                let _ = std::fs::remove_file(&entry_path);
            }
        }
    }
    if live >= REGISTRY_LIMIT {
        return Err(RegisterError::Full);
    }
    let pid = std::process::id();
    let record = Record {
        herdr_socket: herdr_socket.to_owned(),
        state_dir: state_dir.display().to_string(),
        pid,
        started: process::start_time(pid)?,
    };
    let body = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
    write_file(&path, &body, Access::Private)?;
    Ok(Some(Registration { path, pid }))
}

/// The state folder of the daemon that runs the Herdr at `herdr_socket`, when
/// one registered it from a folder other than the default.
///
/// Every check fails closed to `None`, and the caller reads the default store:
/// the record is the account's own private regular file with one name, names
/// this socket, and names an absolute folder that is a real directory the
/// account owns and nobody else can enter.
pub fn resolve(default_state_dir: &Path, herdr_socket: &str) -> Option<PathBuf> {
    let record = read_record(&record_path(default_state_dir, herdr_socket))?;
    if record.herdr_socket != herdr_socket {
        return None;
    }
    let state_dir = PathBuf::from(record.state_dir);
    if !state_dir.is_absolute() {
        return None;
    }
    let metadata = std::fs::symlink_metadata(&state_dir).ok()?;
    if !metadata.is_dir()
        || !owned_by_current_user(&state_dir).ok()?
        || !is_private(&state_dir).ok()?
    {
        return None;
    }
    Some(state_dir)
}

fn same_folder(left: &Path, right: &Path) -> bool {
    left == right || hide_platform::fs::identity::same_file(left, right).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_dir(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        create_dir_all(&path).unwrap();
        restrict_to_owner(&path).unwrap();
        path
    }

    #[test]
    fn a_relocated_daemon_is_found_by_its_herdr_socket_and_its_record_goes_with_it() {
        let root = tempfile::tempdir().unwrap();
        let default = private_dir(root.path(), "default");
        let moved = private_dir(root.path(), "moved");
        let registration = register(&default, "/run/herdr-a.sock", &moved)
            .unwrap()
            .expect("a moved folder is registered");
        assert_eq!(resolve(&default, "/run/herdr-a.sock"), Some(moved));
        assert_eq!(
            resolve(&default, "/run/herdr-b.sock"),
            None,
            "another Herdr's panes read the default store"
        );
        drop(registration);
        assert_eq!(resolve(&default, "/run/herdr-a.sock"), None);
    }

    #[test]
    fn the_default_state_folder_needs_no_record() {
        let root = tempfile::tempdir().unwrap();
        let default = private_dir(root.path(), "default");
        assert!(
            register(&default, "/run/herdr.sock", &default)
                .unwrap()
                .is_none()
        );
        assert!(!default.join(REGISTRY_DIR).exists());
    }

    #[test]
    fn a_live_daemons_record_is_not_replaced_and_a_dead_ones_is() {
        let root = tempfile::tempdir().unwrap();
        let default = private_dir(root.path(), "default");
        let first = private_dir(root.path(), "first");
        let second = private_dir(root.path(), "second");
        let _held = register(&default, "/run/herdr.sock", &first)
            .unwrap()
            .unwrap();
        assert!(matches!(
            register(&default, "/run/herdr.sock", &second),
            Err(RegisterError::HeldBy(pid)) if pid == std::process::id()
        ));
        // A record whose daemon ended: the pid is not running.
        let path = record_path(&default, "/run/herdr.sock");
        let dead = Record {
            herdr_socket: "/run/herdr.sock".into(),
            state_dir: first.display().to_string(),
            pid: u32::MAX - 1,
            started: 1,
        };
        write_file(&path, &serde_json::to_vec(&dead).unwrap(), Access::Private).unwrap();
        let replaced = register(&default, "/run/herdr.sock", &second)
            .unwrap()
            .unwrap();
        assert_eq!(resolve(&default, "/run/herdr.sock"), Some(second));
        drop(replaced);
    }

    #[test]
    fn a_record_that_is_not_the_accounts_private_file_naming_a_private_folder_is_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let default = private_dir(root.path(), "default");
        let moved = private_dir(root.path(), "moved");
        let socket = "/run/herdr.sock";
        let write = |state_dir: &str, record_socket: &str| {
            let record = Record {
                herdr_socket: record_socket.into(),
                state_dir: state_dir.into(),
                pid: std::process::id(),
                started: process::start_time(std::process::id()).unwrap(),
            };
            create_dir_all(&registry(&default)).unwrap();
            write_file(
                &record_path(&default, socket),
                &serde_json::to_vec(&record).unwrap(),
                Access::Private,
            )
            .unwrap();
        };
        write(&moved.display().to_string(), socket);
        assert_eq!(resolve(&default, socket), Some(moved.clone()));
        // The record names another socket than the one it is filed under.
        write(&moved.display().to_string(), "/run/other.sock");
        assert_eq!(resolve(&default, socket), None);
        // A relative folder is no path to trust.
        write("moved", socket);
        assert_eq!(resolve(&default, socket), None);
        // A folder that does not exist.
        write(&root.path().join("gone").display().to_string(), socket);
        assert_eq!(resolve(&default, socket), None);
        // A symbolic link in place of the folder.
        #[cfg(unix)]
        {
            let link = root.path().join("link");
            std::os::unix::fs::symlink(&moved, &link).unwrap();
            write(&link.display().to_string(), socket);
            assert_eq!(resolve(&default, socket), None);
        }
        // A folder other accounts can enter.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let open = root.path().join("open");
            std::fs::create_dir(&open).unwrap();
            std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
            write(&open.display().to_string(), socket);
            assert_eq!(resolve(&default, socket), None);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_record_other_accounts_can_read_or_a_link_in_its_place_is_not_followed() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let default = private_dir(root.path(), "default");
        let moved = private_dir(root.path(), "moved");
        let socket = "/run/herdr.sock";
        let _held = register(&default, socket, &moved).unwrap().unwrap();
        let path = record_path(&default, socket);
        assert_eq!(resolve(&default, socket), Some(moved.clone()));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(resolve(&default, socket), None);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let real = root.path().join("real.json");
        std::fs::rename(&path, &real).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();
        assert_eq!(resolve(&default, socket), None);
    }

    #[test]
    fn registrations_stop_at_the_limit_after_dead_records_are_swept() {
        let root = tempfile::tempdir().unwrap();
        let default = private_dir(root.path(), "default");
        let moved = private_dir(root.path(), "moved");
        create_dir_all(&registry(&default)).unwrap();
        let record_of = |socket: &str, pid: u32, started: u64| {
            let record = Record {
                herdr_socket: socket.into(),
                state_dir: moved.display().to_string(),
                pid,
                started,
            };
            write_file(
                &record_path(&default, socket),
                &serde_json::to_vec(&record).unwrap(),
                Access::Private,
            )
            .unwrap();
        };
        let me = std::process::id();
        let started = process::start_time(me).unwrap();
        for n in 0..REGISTRY_LIMIT {
            record_of(&format!("/run/live-{n}.sock"), me, started);
        }
        assert!(matches!(
            register(&default, "/run/new.sock", &moved),
            Err(RegisterError::Full)
        ));
        // Dead records free their places.
        for n in 0..REGISTRY_LIMIT {
            record_of(&format!("/run/live-{n}.sock"), u32::MAX - 1, 1);
        }
        let registration = register(&default, "/run/new.sock", &moved)
            .unwrap()
            .unwrap();
        let remaining = std::fs::read_dir(registry(&default)).unwrap().count();
        assert_eq!(remaining, 1, "only the new daemon's record is left");
        drop(registration);
    }
}
