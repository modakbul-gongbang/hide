//! The one-time move of this Mac's state folder from `~/.local/state/hide`
//! to `~/.hide/state` (PRD hide-home-layout D-05).
//!
//! `hide connect` runs it before it looks for a daemon, and only for the
//! default state folder: a folder HIDE_STATE_DIR or XDG_STATE_HOME chose is
//! never moved (D-04). The whole folder is renamed at once, so there is never
//! a copy in two places or a half-moved folder; a daemon running from the old
//! folder is stopped first, because it keeps writing there, and the folder
//! moves only once its instance lock is free: a daemon that holds it but did
//! not answer as itself is never signalled, and nothing moves. When the new
//! folder already exists nothing is merged: the old folder stays as it is
//! and the daemon's boot logs both paths (`log_left_behind`). Every later run
//! finds no old folder and does nothing.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

/// What the move did.
#[derive(Debug, Eq, PartialEq)]
pub enum Moved {
    /// There was no old folder (or it was not a folder this account owns).
    Nothing,
    /// The old folder was renamed into place.
    Moved { stopped_pid: Option<u32> },
    /// Both folders exist; the new one is used and the old one stays.
    LeftBehind,
}

/// Moves `legacy` to `target` once. `stop` ends the daemon a legacy state
/// file names when that daemon answers as itself, and returns its pid; a
/// daemon that will not stop, or any process still holding the folder's
/// instance lock afterwards, is an error and nothing moves.
pub fn move_legacy(
    legacy: &Path,
    target: &Path,
    stop: impl FnOnce(&Path) -> Result<Option<u32>, String>,
) -> Result<Moved, String> {
    if !own_folder(legacy)? {
        return Ok(Moved::Nothing);
    }
    // Checked before any lock is taken, so a legacy folder left beside the
    // new one is not touched at all (B5).
    if std::fs::symlink_metadata(target).is_ok() {
        return Ok(Moved::LeftBehind);
    }
    // The legacy folder's own connect lock: an older `hide connect` takes the
    // same one, so neither starts a daemon there while it moves. The folder
    // itself is never made, so one another connect just moved is not made
    // again.
    let _lock = match lock_in_folder(&legacy.join("connect.lock")) {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Moved::Nothing),
        Err(error) => return Err(format!("{} could not be locked: {error}", legacy.display())),
    };
    // Rechecked under the lock: another connect may have moved it meanwhile.
    if !own_folder(legacy)? {
        return Ok(Moved::Nothing);
    }
    if std::fs::symlink_metadata(target).is_ok() {
        return Ok(Moved::LeftBehind);
    }
    let parent = target
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", target.display()))?;
    private_parent(parent)?;
    let stopped_pid = stop(legacy)?;
    // The lock is the liveness test: a pid or a `/health` answer can be stale
    // or slow, but a running daemon always holds it. Held across the rename,
    // so no older daemon starts in the folder meanwhile.
    let _instance = match instance_lock(legacy) {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            return Err(format!(
                "a hided still runs from {} and did not answer as itself, so nothing was moved; quit it and connect again",
                legacy.display()
            ));
        }
        Err(error) => {
            return Err(format!(
                "{} could not be locked: {error}",
                crate::state_file::lock_path(legacy).display()
            ));
        }
    };
    std::fs::rename(legacy, target).map_err(|error| {
        format!(
            "{} could not be moved to {}: {error}",
            legacy.display(),
            target.display()
        )
    })?;
    Ok(Moved::Moved { stopped_pid })
}

/// Logged once per daemon start while an old folder is left beside the one
/// in use (B5); nothing reaches the screen.
pub fn log_left_behind(legacy: Option<&Path>, state_dir: &Path) {
    let Some(legacy) = legacy else { return };
    if std::fs::symlink_metadata(legacy).is_ok() {
        herdr_core::diagnostic!(serde_json::json!({
            "component": "hided",
            "kind": "state.legacy_left",
            "legacy": legacy.display().to_string(),
            "state_dir": state_dir.display().to_string(),
            "message": "both state folders exist; the new one is used and the old one is left untouched",
        }));
    }
}

/// Whether `path` is a real folder this account owns. A link or another
/// account's folder is not moved, whatever it leads to; one another account
/// can write to is refused, since its state file names the pid that `stop`
/// signals.
fn own_folder(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_dir() && metadata.uid() == euid() && metadata.mode() & 0o022 != 0 =>
        {
            Err(format!(
                "{} can be written by other accounts, so Hide does not move it",
                path.display()
            ))
        }
        Ok(metadata) => Ok(metadata.is_dir() && metadata.uid() == euid()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("{} could not be read: {error}", path.display())),
    }
}

/// `~/.hide`, made 0700 when missing; an existing one must be a real folder
/// this account owns that no other account can write to, since the state
/// folder holds the daemon's token.
fn private_parent(parent: &Path) -> Result<(), String> {
    match std::fs::DirBuilder::new().mode(0o700).create(parent) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!(
                "{} could not be created: {error}",
                parent.display()
            ));
        }
    }
    let metadata = std::fs::symlink_metadata(parent)
        .map_err(|error| format!("{} could not be read: {error}", parent.display()))?;
    if !metadata.is_dir() || metadata.uid() != euid() || metadata.mode() & 0o022 != 0 {
        return Err(format!(
            "{} is not a private folder of this account, so the state folder is not moved into it",
            parent.display()
        ));
    }
    Ok(())
}

/// Locks `path`, making the file inside its existing folder but never the
/// folder: a missing folder is `NotFound`.
fn lock_in_folder(path: &Path) -> io::Result<File> {
    use std::os::fd::AsRawFd;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    // SAFETY: flock on a descriptor `file` owns; dropping it releases.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

/// The legacy folder's instance lock, taken without waiting; a missing lock
/// file means no daemon ever ran there and is created inside the folder.
fn instance_lock(legacy: &Path) -> io::Result<File> {
    use std::os::fd::AsRawFd;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(crate::state_file::lock_path(legacy))?;
    // SAFETY: flock on a descriptor `file` owns; dropping it releases.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            io::Error::from(io::ErrorKind::WouldBlock)
        } else {
            error
        });
    }
    Ok(file)
}

fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_with(home: &Path) -> std::path::PathBuf {
        let legacy = home.join(".local/state/hide");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("core-state.json"), "{\"projects\":[1]}").unwrap();
        legacy
    }

    #[test]
    fn the_whole_folder_moves_once_and_a_second_run_does_nothing() {
        let home = tempfile::tempdir().unwrap();
        let legacy = legacy_with(home.path());
        let target = home.path().join(".hide/state");
        let moved = move_legacy(&legacy, &target, |_| Ok(Some(42))).unwrap();
        assert_eq!(
            moved,
            Moved::Moved {
                stopped_pid: Some(42)
            }
        );
        assert!(!legacy.exists());
        assert_eq!(
            std::fs::read_to_string(target.join("core-state.json")).unwrap(),
            "{\"projects\":[1]}"
        );
        let mode = std::fs::metadata(home.path().join(".hide")).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let again = move_legacy(&legacy, &target, |_| panic!("nothing to stop")).unwrap();
        assert_eq!(again, Moved::Nothing);
        assert!(
            !legacy.exists(),
            "a second run must not make the old folder again"
        );
    }

    #[test]
    fn an_existing_new_folder_is_used_and_the_old_one_left_whole() {
        let home = tempfile::tempdir().unwrap();
        let legacy = legacy_with(home.path());
        let target = home.path().join(".hide/state");
        std::fs::create_dir_all(&target).unwrap();
        let moved = move_legacy(&legacy, &target, |_| panic!("no daemon is stopped")).unwrap();
        assert_eq!(moved, Moved::LeftBehind);
        assert!(legacy.join("core-state.json").is_file());
        assert!(
            !legacy.join("connect.lock").exists(),
            "the old folder is not touched"
        );
        assert!(!target.join("core-state.json").exists());
    }

    #[test]
    fn a_process_still_holding_the_instance_lock_leaves_everything_in_place() {
        let home = tempfile::tempdir().unwrap();
        let legacy = legacy_with(home.path());
        let target = home.path().join(".hide/state");
        // A daemon that did not answer as itself: `stop` finds nothing to signal.
        let held = crate::state_file::acquire_lock(&legacy).unwrap();
        let error = move_legacy(&legacy, &target, |_| Ok(None)).unwrap_err();
        assert!(error.contains("still runs"), "{error}");
        assert!(legacy.join("core-state.json").is_file());
        assert!(!target.exists());
        drop(held);
        assert_eq!(
            move_legacy(&legacy, &target, |_| Ok(None)).unwrap(),
            Moved::Moved { stopped_pid: None }
        );
    }

    #[test]
    fn a_folder_other_accounts_can_write_is_not_moved() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let legacy = legacy_with(home.path());
        std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o777)).unwrap();
        let target = home.path().join(".hide/state");
        let error = move_legacy(&legacy, &target, |_| panic!("nothing is signalled")).unwrap_err();
        assert!(error.contains("other accounts"), "{error}");
        assert!(legacy.join("core-state.json").is_file());
    }

    #[test]
    fn a_daemon_that_will_not_stop_leaves_everything_in_place() {
        let home = tempfile::tempdir().unwrap();
        let legacy = legacy_with(home.path());
        let target = home.path().join(".hide/state");
        let error =
            move_legacy(&legacy, &target, |_| Err("pid 7 did not stop".to_owned())).unwrap_err();
        assert!(error.contains("pid 7"));
        assert!(legacy.join("core-state.json").is_file());
        assert!(!target.exists());
    }

    #[test]
    fn a_linked_old_folder_is_never_moved() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = home.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(home.path().join(".local/state")).unwrap();
        let legacy = home.path().join(".local/state/hide");
        std::os::unix::fs::symlink(&elsewhere, &legacy).unwrap();
        let target = home.path().join(".hide/state");
        assert_eq!(
            move_legacy(&legacy, &target, |_| panic!("nothing to stop")).unwrap(),
            Moved::Nothing
        );
        assert!(elsewhere.exists() && !target.exists());
    }

    #[test]
    fn a_hide_folder_others_can_write_is_not_moved_into() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let legacy = legacy_with(home.path());
        let hide = home.path().join(".hide");
        std::fs::create_dir_all(&hide).unwrap();
        std::fs::set_permissions(&hide, std::fs::Permissions::from_mode(0o777)).unwrap();
        let error =
            move_legacy(&legacy, &hide.join("state"), |_| panic!("not reached")).unwrap_err();
        assert!(error.contains("not a private folder"), "{error}");
        assert!(legacy.join("core-state.json").is_file());
    }
}
