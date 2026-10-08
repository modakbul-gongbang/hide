//! A device's kit target, as `hided node serve` builds it on the device.
//!
//! The core uploads each build's parts into `<root>/<version>/` beside the
//! helper, and the helper points `<root>/current` at its own version before
//! it applies the kit, so the hooks, the `hide` link name
//! a path that survives the next build (B16). The `herdr` CLI is the device's own.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::KitTarget;

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
    let relocated = std::env::var_os("HCOORD_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    KitTarget {
        home: home.to_path_buf(),
        kit_dir: root.join(CURRENT),
        cli_dir: cli_dir.to_path_buf(),
        // The old layout's root too, so its `hide` link is re-pointed rather
        // than left as the operator's (D-13).
        owned_roots: vec![root.to_path_buf(), crate::layout::legacy_helper_root(home)],
        herdr_socket: herdr_socket.to_path_buf(),
        herdr_bin,
        codex: hide_agent_hooks::codex_daemon::find_codex(home),
        login_shell: crate::agents::login_shell(),
        legacy_coordination_home: relocated,
        user_agents: hide_platform::user_agents::UserAgents::current(),
        retirement_projects: Vec::new(),
        legacy: crate::legacy::device(home, root),
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
