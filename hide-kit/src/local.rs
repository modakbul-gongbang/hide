//! This machine's kit target: the parts ship in the running desktop package's
//! resources folder.
//!
//! Only a daemon running from a desktop package has a folder that outlives it, so
//! any other `hided` (a development build, a standalone daemon for a browser)
//! installs nothing and says so (B11, D-19): what the kit writes is a path the
//! operator's own configuration keeps, and a build directory is deleted by the
//! next build (the 2026-09-10 hook incident).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::KitTarget;

/// Why a development or standalone daemon installs nothing on this machine.
pub const STANDALONE_REASON: &str = if cfg!(target_os = "macos") {
    "This Hide daemon is not running from the installed app, so it installs nothing on this Mac; open the Hide app to install"
} else {
    "This Hide daemon is not running from an unpacked desktop package, so it installs nothing on this machine; open the packaged Hide app to install"
};

/// The resources folder of the desktop package `executable` runs from.
/// A plain Cargo binary, or a folder merely named `resources`, has no kit.
pub fn bundled_kit_dir(executable: &Path) -> Option<PathBuf> {
    let resources = executable.parent()?;
    if resources.file_name() == Some(OsStr::new("resources")) {
        let app = resources.parent()?;
        return (resources.join("app.asar").is_file()
            && app
                .join(format!("hide{}", std::env::consts::EXE_SUFFIX))
                .is_file())
        .then(|| resources.to_path_buf());
    }
    let contents = resources.parent()?;
    let bundle = contents.parent()?;
    (resources.file_name() == Some(OsStr::new("Resources"))
        && contents.file_name() == Some(OsStr::new("Contents"))
        && bundle.extension() == Some(OsStr::new("app")))
    .then(|| resources.to_path_buf())
}

/// This machine's target for a desktop package whose resources are `kit_dir`.
pub fn local_target(
    kit_dir: &Path,
    home: &Path,
    herdr_socket: &Path,
    stop: Arc<AtomicBool>,
) -> KitTarget {
    let herdr = kit_dir.join(format!("herdr{}", std::env::consts::EXE_SUFFIX));
    let relocated = std::env::var_os("HCOORD_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    KitTarget {
        home: home.to_path_buf(),
        kit_dir: kit_dir.to_path_buf(),
        cli_dir: home.join(".local").join("bin"),
        // A `hide` linked into a device helper root is Hide's too: this Mac
        // may once have been another Mac's device, under either layout.
        owned_roots: vec![
            crate::layout::helper_root(home),
            crate::layout::legacy_helper_root(home),
        ],
        herdr_socket: herdr_socket.to_path_buf(),
        herdr_bin: herdr.is_file().then(|| herdr.clone()),
        codex: hide_agent_hooks::codex_daemon::find_codex(home),
        login_shell: crate::agents::login_shell(),
        legacy_coordination_home: relocated,
        user_agents: hide_platform::user_agents::UserAgents::current(),
        retirement_projects: Vec::new(),
        legacy: crate::legacy::local(home),
        stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_daemon_inside_an_app_bundle_has_a_kit_folder() {
        assert_eq!(
            bundled_kit_dir(Path::new("/Applications/hide.app/Contents/Resources/hided")),
            Some(PathBuf::from("/Applications/hide.app/Contents/Resources"))
        );
        // A Cargo build directory is deleted by the next build; a hook that
        // named one on 2026-09-10 failed every session afterwards.
        for elsewhere in [
            "/work/hide/target/debug/deps/hided",
            "/work/hide/target/release/hided",
            "/tmp/Contents/Resources/hided",
            "/Applications/hide.app/Resources/hided",
            "/Applications/hide.app/Contents/MacOS/hided",
        ] {
            assert_eq!(bundled_kit_dir(Path::new(elsewhere)), None, "{elsewhere}");
        }
    }

    #[test]
    fn unpacked_package_requires_both_the_shell_archive_and_electron() {
        let dir = tempfile::tempdir().unwrap();
        let resources = dir.path().join("renamed package/resources");
        std::fs::create_dir_all(&resources).unwrap();
        let daemon = resources.join(format!("hided{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(bundled_kit_dir(&daemon), None);
        std::fs::write(resources.join("app.asar"), "").unwrap();
        assert_eq!(bundled_kit_dir(&daemon), None);
        let electron = resources
            .parent()
            .unwrap()
            .join(format!("hide{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&electron, "").unwrap();
        assert_eq!(bundled_kit_dir(&daemon), Some(resources.clone()));
        std::fs::remove_file(resources.join("app.asar")).unwrap();
        assert_eq!(bundled_kit_dir(&daemon), None);
        let target = local_target(
            &resources,
            dir.path(),
            Path::new("isolated.sock"),
            Arc::default(),
        );
        assert_eq!(target.cli_dir, dir.path().join(".local/bin"));
        std::fs::write(
            resources.join(format!("herdr{}", std::env::consts::EXE_SUFFIX)),
            "",
        )
        .unwrap();
        assert!(
            local_target(
                &resources,
                dir.path(),
                Path::new("isolated.sock"),
                Arc::default()
            )
            .herdr_bin
            .is_some()
        );
    }
}
