//! The helper and its stand-in `hide` as a test runs them against the hook's
//! budget: one file each per build, started once per process before a test
//! is given it, and hard linked into a test's folder (`stand_ins`).

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

use crate::stand_ins;

/// The helper cargo built, ready to run, with the `hide` it asks when cargo
/// built one beside it.
pub fn hook() -> &'static Path {
    let hook = Path::new(env!("CARGO_BIN_EXE_hide-agent-hooks"));
    stand_ins::ready(hook);
    let sibling = hook.with_file_name(format!("hide{}", std::env::consts::EXE_SUFFIX));
    if sibling.exists() {
        stand_ins::ready(&sibling);
    }
    hook
}

/// A private folder for one test, on the filesystem that holds the helper and
/// the stand-ins, so [`link`] can put them in it.
#[cfg(unix)]
pub fn folder() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

/// Puts `program`, from [`hook`] or [`stand_in`], at `at` as a hard link.
#[cfg(unix)]
pub fn link(program: &Path, at: &Path) {
    std::fs::hard_link(program, at).unwrap();
}

/// The stand-in whose text is `body`, ready to run.
#[cfg(unix)]
pub fn stand_in(body: &str) -> PathBuf {
    stand_ins::stand_in(
        &Path::new(env!("CARGO_TARGET_TMPDIR")).join("stand-ins"),
        body,
    )
}
