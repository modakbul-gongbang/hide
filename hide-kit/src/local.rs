//! This Mac's kit target: the parts ship in the running app bundle's
//! `Contents/Resources`, and hcoord runs on the app's own executable in Node
//! mode.
//!
//! Only a daemon running from an app bundle has a folder that outlives it, so
//! any other `hided` (a development build, a standalone daemon for a browser)
//! installs nothing and says so (B11, D-19): what the kit writes is a path the
//! operator's own configuration keeps, and a build directory is deleted by the
//! next build (the 2026-09-10 hook incident).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::{HcoordRuntime, KitTarget};

/// Why this machine's row installs nothing: on a Mac, a daemon outside the
/// app; elsewhere every daemon, because only a macOS app bundle has the
/// folder [`bundled_kit_dir`] recognizes, so a Windows or Linux package
/// installs nothing either.
pub const STANDALONE_REASON: &str = if cfg!(target_os = "macos") {
    "This Hide daemon is not running from the installed app, so it installs nothing on this Mac; open the Hide app to install"
} else {
    "Hide installs its kit only from its macOS app, so it installs nothing on this machine"
};

/// The `Contents/Resources` folder of the app bundle `executable` runs
/// from, or `None` when it does not run from one.
pub fn bundled_kit_dir(executable: &Path) -> Option<PathBuf> {
    let resources = executable.parent()?;
    let contents = resources.parent()?;
    let bundle = contents.parent()?;
    (resources.file_name() == Some(OsStr::new("Resources"))
        && contents.file_name() == Some(OsStr::new("Contents"))
        && bundle.extension() == Some(OsStr::new("app")))
    .then(|| resources.to_path_buf())
}

/// This Mac's target for a bundle whose `Contents/Resources` is `kit_dir`.
pub fn local_target(
    kit_dir: &Path,
    home: &Path,
    herdr_socket: &Path,
    stop: Arc<AtomicBool>,
) -> KitTarget {
    let herdr = kit_dir.join("herdr");
    let relocated = crate::layout::hcoord_home_override();
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
        hcoord: app_executable(kit_dir).map(|program| HcoordRuntime {
            program,
            env: [
                ("ELECTRON_RUN_AS_NODE".to_owned(), "1".to_owned()),
                ("HERDR_BIN_PATH".to_owned(), herdr.display().to_string()),
            ]
            .into_iter()
            .chain(crate::hcoord::relocation_env(relocated.as_deref()))
            .collect(),
        }),
        codex: hide_agent_hooks::codex_daemon::find_codex(home),
        hcoord_home: relocated,
        legacy: crate::legacy::local(home),
        stop,
    }
}

/// The app's own executable, `Contents/MacOS/<CFBundleExecutable>`, which
/// runs hcoord's JavaScript with `ELECTRON_RUN_AS_NODE` set.
fn app_executable(kit_dir: &Path) -> Result<PathBuf, String> {
    let contents = kit_dir
        .parent()
        .ok_or_else(|| format!("{} has no app bundle around it", kit_dir.display()))?;
    let plist = contents.join("Info.plist");
    let text = std::fs::read_to_string(&plist)
        .map_err(|error| format!("{} could not be read: {error}", plist.display()))?;
    let name = plist_string(&text, "CFBundleExecutable")
        .ok_or_else(|| format!("{} names no executable", plist.display()))?;
    let program = contents.join("MacOS").join(name);
    if program.is_file() {
        Ok(program)
    } else {
        Err(format!(
            "the app executable {} is missing",
            program.display()
        ))
    }
}

/// The string value of `key` in an XML property list.
fn plist_string(text: &str, key: &str) -> Option<String> {
    let after = &text[text.find(&format!("<key>{key}</key>"))?..];
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")?;
    let value = after[start..start + end].trim();
    (!value.is_empty() && !value.contains('/')).then(|| value.to_owned())
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
    fn the_app_executable_is_read_from_the_bundle_plist() {
        let dir = tempfile::tempdir().unwrap();
        let contents = dir.path().join("hide.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::create_dir_all(contents.join("Resources")).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            "<plist><dict><key>CFBundleName</key><string>x</string>\n<key>CFBundleExecutable</key>\n  <string>hide</string></dict></plist>",
        )
        .unwrap();
        std::fs::write(contents.join("MacOS/hide"), "").unwrap();
        let target = local_target(
            &contents.join("Resources"),
            dir.path(),
            Path::new("/nowhere.sock"),
            Arc::default(),
        );
        let runtime = target.hcoord.unwrap();
        assert_eq!(runtime.program, contents.join("MacOS/hide"));
        assert!(
            runtime
                .env
                .contains(&("ELECTRON_RUN_AS_NODE".to_owned(), "1".to_owned()))
        );
        assert_eq!(target.cli_dir, dir.path().join(".local/bin"));
    }
}
