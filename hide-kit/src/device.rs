//! A device's kit target, as `hide-host-helper` builds it on the device.
//!
//! The core uploads each build's parts into `<root>/<version>/` beside the
//! helper, and the helper points `<root>/current` at its own version before
//! it applies the kit, so the hooks, the `hide` link and the hcoord copy name
//! a path that survives the next build (B16). hcoord runs on a Node the
//! device already has, and the `herdr` CLI is the device's own.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::{HcoordRuntime, KitTarget, find_node};

/// The name of the link under the helper root that leads to the running
/// build's folder.
pub const CURRENT: &str = "current";

/// The kit target for a device whose helper root is `root`. `cli_dir` and
/// `herdr_socket` come from the device's consent and registration.
pub fn device_target(
    root: &Path,
    home: &Path,
    cli_dir: &Path,
    herdr_socket: &Path,
    stop: Arc<AtomicBool>,
) -> KitTarget {
    let herdr_bin = find_herdr(home);
    let hcoord = find_node(home, &stop).map(|program| HcoordRuntime {
        program,
        // hcoord calls `herdr` for lineage; a daemon started from a
        // non-login SSH shell would not find the one the operator uses.
        env: herdr_bin
            .iter()
            .map(|herdr| ("HERDR_BIN_PATH".to_owned(), herdr.display().to_string()))
            .chain(crate::hcoord::home_override())
            .collect(),
    });
    KitTarget {
        home: home.to_path_buf(),
        kit_dir: root.join(CURRENT),
        cli_dir: cli_dir.to_path_buf(),
        owned_roots: vec![root.to_path_buf()],
        herdr_socket: herdr_socket.to_path_buf(),
        herdr_bin,
        hcoord,
        stop,
    }
}

/// The device's `herdr`: the first on `PATH`, then the usual install
/// folders. An SSH exec channel runs with a short `PATH`, so the folders a
/// login shell would add are looked in too.
fn find_herdr(home: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|folder| folder.join("herdr")));
    }
    candidates.extend([
        home.join(".local/bin/herdr"),
        home.join(".cargo/bin/herdr"),
        PathBuf::from("/opt/homebrew/bin/herdr"),
        PathBuf::from("/usr/local/bin/herdr"),
    ]);
    candidates.into_iter().find(|candidate| candidate.is_file())
}
