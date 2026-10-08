//! What the kernel says about a process asking for a Workspace credential
//! (PRD core-host-node D-21): which pane's shell it descends from, where it
//! works, whether a process is still the one that started, and the local
//! socket a pane's process asks on, whose system reports the caller's pid.
//!
//! These are facts of the machine the caller runs on, so they are the node's.
//! hided's node role reads them and the credential is issued from them.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use hide_platform::fs::private;

pub use hide_host::pane_peer::{
    PaneIdentity, descends_from, inspect, inspect_until, process_start,
};

/// The listener a pane's process asks for a capability on: a Unix socket or
/// a named pipe, whose system reports the caller's pid.
pub type BootstrapListener = hide_platform::ipc::LocalListener;

/// The longest bootstrap socket path the record may hold: a short `/tmp`
/// path on Unix, the account's temporary folder on Windows.
const BOOTSTRAP_RECORD_CAP: u64 = 1024;

/// The folder `peer` works in, in the wire spelling, read from the kernel
/// and resolved to its real path; never from what the caller says.
pub fn caller_directory(peer: i32) -> Result<String, &'static str> {
    let cwd = hide_host::pane_peer::process_cwd(peer).ok_or("caller_unavailable")?;
    let canonical =
        hide_platform::fs::identity::canonical(&cwd).map_err(|_| "caller_unavailable")?;
    hide_platform::path::to_wire(&canonical).map_err(|_| "caller_unavailable")
}

/// Where the bootstrap's private folder goes. A Unix socket path is limited
/// to about a hundred bytes, so a short folder under `/tmp` there, whose
/// random name another user cannot reserve; a pipe name has no such limit,
/// so the account's own temporary folder on Windows.
fn bootstrap_parent() -> PathBuf {
    if cfg!(windows) {
        std::env::temp_dir()
    } else {
        PathBuf::from("/tmp")
    }
}

/// Binds the bootstrap socket in a new private folder and records its path
/// in `state_dir`. `token` names the folder and the record's staging file;
/// each call must answer a new random value.
pub fn bind(
    state_dir: &Path,
    token: impl Fn() -> String,
) -> Result<(BootstrapListener, PathBuf), String> {
    let parent = bootstrap_parent();
    let directory = (0..8)
        .find_map(|_| {
            let candidate = parent.join(format!("hide-pane-{}", &token()[..24]));
            match private::create_dir(&candidate) {
                Ok(()) => Some(Ok(candidate)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error.to_string())),
            }
        })
        .unwrap_or_else(|| Err("pane bootstrap directory collision limit".to_owned()))?;
    let path = directory.join("b.sock");
    let result = (|| {
        let listener = BootstrapListener::bind(&path).map_err(|error| error.to_string())?;
        private::restrict_to_owner(&path).map_err(|error| error.to_string())?;
        let record = bootstrap_socket_record(state_dir);
        let staging = record.with_extension(format!("{}.tmp", &token()[..16]));
        let mut file = private::create_new_file(&staging).map_err(|error| error.to_string())?;
        let published = (|| {
            let text = path.to_str().ok_or("pane bootstrap path is not text")?;
            file.write_all(text.as_bytes())
                .map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            fs::rename(&staging, &record).map_err(|error| error.to_string())
        })();
        if published.is_err() {
            let _ = fs::remove_file(staging);
        }
        published?;
        Ok((listener, path.clone()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&directory);
    }
    result
}

pub fn bootstrap_socket_record(state_dir: &Path) -> PathBuf {
    state_dir.join("pane-capabilities/bootstrap-socket")
}

/// The bootstrap socket the running daemon published, trusted only when the
/// record and the folder the socket sits in are this account's own and
/// private.
pub fn bootstrap_socket_path(state_dir: &Path) -> Result<PathBuf, String> {
    let record = bootstrap_socket_record(state_dir);
    let file = private::open_own_file(&record, false).map_err(|_| "hide_unavailable".to_owned())?;
    let metadata = file.metadata().map_err(|_| "hide_unavailable".to_owned())?;
    if !private::is_private(&record).unwrap_or(false) || metadata.len() > BOOTSTRAP_RECORD_CAP {
        return Err("invalid_bootstrap_socket_record".to_owned());
    }
    let mut bytes = Vec::new();
    file.take(BOOTSTRAP_RECORD_CAP)
        .read_to_end(&mut bytes)
        .map_err(|_| "invalid_bootstrap_socket_record".to_owned())?;
    let path = PathBuf::from(
        String::from_utf8(bytes).map_err(|_| "invalid_bootstrap_socket_record".to_owned())?,
    );
    let directory = path.parent().ok_or("invalid_bootstrap_socket_record")?;
    let metadata = fs::symlink_metadata(directory)
        .map_err(|_| "invalid_bootstrap_socket_record".to_owned())?;
    if !path.is_absolute()
        || !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || !private::owned_by_current_user(directory).unwrap_or(false)
        || !private::is_private(directory).unwrap_or(false)
    {
        return Err("invalid_bootstrap_socket_record".to_owned());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Windows cannot read a process's working directory (`cwd_of`).
    #[cfg(unix)]
    #[test]
    fn caller_directory_reads_the_callers_working_directory() {
        let expected = hide_platform::path::to_wire(
            &hide_platform::fs::identity::canonical(&std::env::current_dir().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(caller_directory(std::process::id() as i32), Ok(expected));
    }

    #[test]
    fn a_caller_that_is_gone_is_unavailable() {
        assert_eq!(caller_directory(i32::MAX), Err("caller_unavailable"));
    }
}
