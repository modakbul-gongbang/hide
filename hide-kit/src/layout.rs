//! Where Hide keeps its own files on a machine (PRD hide-home-layout D-01).
//!
//! Everything Hide owns sits under `~/.hide`: the daemon's state folder, a
//! device's helper root, the kit's record and copies, the hook counters and
//! retirement records. Outside it stay only what another program reads at a place
//! it chose (the hook entries and the `hide` link on `PATH`). The
//! legacy spellings are kept here too, for the one-time moves that read them
//! and the kit passes that retire them; nothing else may name them.

use std::path::{Path, PathBuf};

/// The folder under HOME that holds everything Hide owns.
pub const HIDE_HOME: &str = ".hide";

/// A device's helper root unless the daemon was started with another, as a
/// consent records it; `~` is the device account's home (D-11).
pub const HELPER_ROOT: &str = "~/.hide/host-helper";

/// The helper root builds before this layout installed under. A consent for
/// it is carried to [`HELPER_ROOT`] without asking (D-12), and the kit pass
/// retires it once the helper runs from the new root (D-13).
pub const LEGACY_HELPER_ROOT: &str = "~/.local/share/hide/host-helper";

pub fn hide_home(home: &Path) -> PathBuf {
    home.join(HIDE_HOME)
}

/// The daemon's state folder when nothing relocates it (D-04).
pub fn default_state_dir(home: &Path) -> PathBuf {
    hide_home(home).join("state")
}

/// The state folder builds before this layout used by default; moved once to
/// [`default_state_dir`] by `hide connect` (D-05).
pub fn legacy_state_dir(home: &Path) -> PathBuf {
    home.join(".local/state/hide")
}

/// `HIDE_STATE_DIR` wins; a set `XDG_STATE_HOME` keeps `$XDG_STATE_HOME/hide`
/// as before and is never moved; otherwise `~/.hide/state` (D-04). Empty
/// values count as unset, as the callers read them.
pub fn state_dir(
    home: &Path,
    hide_state_dir: Option<&str>,
    xdg_state_home: Option<&str>,
) -> PathBuf {
    if let Some(dir) = hide_state_dir.filter(|value| !value.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(xdg) = xdg_state_home.filter(|value| !value.is_empty()) {
        return Path::new(xdg).join("hide");
    }
    default_state_dir(home)
}

/// [`state_dir`] from this process's environment, for a program that has no
/// environment registry of its own (the device helper).
pub fn state_dir_from_process(home: &Path) -> PathBuf {
    let read = |key: &str| std::env::var(key).ok();
    state_dir(
        home,
        read("HIDE_STATE_DIR").as_deref(),
        read("XDG_STATE_HOME").as_deref(),
    )
}

/// Where a device's helper and the `hide` command beside it meet (D-06).
pub fn workspace_bridges(state_dir: &Path) -> PathBuf {
    state_dir.join("workspace-bridges")
}

/// Durable local requests and watches; its owner is the core delivery worker.
pub fn delivery_ledger(state_dir: &Path) -> PathBuf {
    state_dir.join("delivery-ledger.json")
}

/// The last GitHub answer per project, restored when the daemon starts; its
/// owner is the core's GitHub store.
pub fn github_snapshot(state_dir: &Path) -> PathBuf {
    state_dir.join("github-snapshot.json")
}

/// The link record: which sessions made or worked on which pull request,
/// kept after a pane closes; its owner is the core's link worker.
pub fn links_store(state_dir: &Path) -> PathBuf {
    state_dir.join("links.sqlite3")
}

/// The Software Factory store: Factories, Tasks, questions, records and
/// events; its owner is the core's Factory engine thread.
pub fn factory_store(state_dir: &Path) -> PathBuf {
    state_dir.join("factory.sqlite3")
}

/// The Factory's private files beside its store: PRD attachments by hash and
/// verify log tails.
pub fn factory_files(state_dir: &Path) -> PathBuf {
    state_dir.join("factory-files")
}

/// Where this machine's core runs when it is not here: present only on a
/// node whose core a move placed on another machine; its owner is the
/// daemon's start (`hided::placement`).
pub fn core_placement(state_dir: &Path) -> PathBuf {
    state_dir.join("core-placement.json")
}

/// The record of a core move this machine drives, written before each step
/// that cannot be undone; its owner is the move driver
/// (`hided::core_move::journal`).
pub fn core_move_journal(state_dir: &Path) -> PathBuf {
    state_dir.join("core-move.json")
}

/// The record on a machine taking or giving the core in a move: a core
/// waiting for its first link, active, stopped for the move, or retired;
/// its owner is `hided core-move` (`hided::core_move::handover`).
pub fn core_handover(state_dir: &Path) -> PathBuf {
    state_dir.join("core-handover.json")
}

/// The login item that runs the core on `state_dir` after a move placed it
/// on this machine (`hided::login_item`): one per state folder, so a folder
/// other than the account's default never takes the default's item.
pub fn core_login_item(home: &Path, state_dir: &Path) -> String {
    const LABEL: &str = "dev.withhide.core";
    if state_dir == default_state_dir(home) {
        return LABEL.to_owned();
    }
    use sha2::Digest;
    let digest = sha2::Sha256::digest(state_dir.to_string_lossy().as_bytes());
    format!("{LABEL}.{}", hex_prefix(&digest))
}

fn hex_prefix(digest: &[u8]) -> String {
    digest
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Copies a move makes of this machine's brain state, one folder per move.
pub fn move_staging(state_dir: &Path) -> PathBuf {
    state_dir.join("move-staging")
}

/// Where the machine taking the core receives a move's copy.
pub fn move_incoming(state_dir: &Path) -> PathBuf {
    state_dir.join("move-incoming")
}

/// The brain state a machine held before its core moved away, the latest
/// move's only.
pub fn moved_out(state_dir: &Path) -> PathBuf {
    state_dir.join("moved-out")
}

/// `~/rest` of a helper root spelling under `home`; an absolute spelling as
/// it is.
pub fn expand_home(spelling: &str, home: &Path) -> PathBuf {
    match spelling.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(spelling),
    }
}

pub fn helper_root(home: &Path) -> PathBuf {
    expand_home(HELPER_ROOT, home)
}

pub fn legacy_helper_root(home: &Path) -> PathBuf {
    expand_home(LEGACY_HELPER_ROOT, home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_folder_is_under_hide_unless_something_relocates_it() {
        let home = Path::new("/Users/example");
        assert_eq!(
            state_dir(home, None, None),
            PathBuf::from("/Users/example/.hide/state")
        );
        assert_eq!(
            state_dir(home, Some(""), Some("")),
            state_dir(home, None, None)
        );
        assert_eq!(
            state_dir(home, None, Some("/xdg")),
            PathBuf::from("/xdg/hide")
        );
        assert_eq!(
            state_dir(home, Some("/isolated"), Some("/xdg")),
            PathBuf::from("/isolated")
        );
    }

    #[test]
    fn a_state_folder_other_than_the_default_has_a_login_item_of_its_own() {
        let home = Path::new("/Users/op");
        let default = core_login_item(home, &default_state_dir(home));
        let other = core_login_item(home, Path::new("/private/tmp/seat/state"));
        assert_eq!(default, "dev.withhide.core");
        assert!(
            other.starts_with("dev.withhide.core.") && other != default,
            "{other}"
        );
        assert_eq!(
            other,
            core_login_item(home, Path::new("/private/tmp/seat/state"))
        );
    }

    #[test]
    fn every_hide_default_sits_under_one_folder() {
        let home = Path::new("/home/me");
        for path in [
            default_state_dir(home),
            helper_root(home),
            crate::kit_state_dir(home),
        ] {
            assert!(path.starts_with("/home/me/.hide"), "{}", path.display());
        }
    }
}
