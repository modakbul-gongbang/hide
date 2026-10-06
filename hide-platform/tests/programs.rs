//! The one search for an agent's CLI: the install kit and Hide AI both ask it,
//! so a program is found, or not, the same way for both.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use hide_platform::programs;

fn program(folder: &Path, name: &str) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    let path = folder.join(name);
    fs::write(&path, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A stand-in login shell whose startup files put `extra` on the `PATH`.
fn shell_adding(folder: &Path, extra: &Path) -> PathBuf {
    let shell = folder.join("shell");
    fs::write(
        &shell,
        format!(
            "#!/bin/sh\nPATH=\"{}:$PATH\"\nexport PATH\n[ \"$1\" = -ilc ] || exit 64\neval \"$2\"\n",
            extra.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&shell, fs::Permissions::from_mode(0o755)).unwrap();
    shell
}

#[test]
fn a_program_only_the_login_shells_path_reaches_is_found() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let shells_folder = root.path().join("installer-bin");
    let installed = program(&shells_folder, "hide-fixture-agent-a");
    let shell = shell_adding(root.path(), &shells_folder);

    let found = programs::find_cli_with(
        &home,
        Some(&shell),
        &AtomicBool::new(false),
        "hide-fixture-agent-a",
    );
    assert_eq!(found, Some(installed));
    assert_eq!(
        programs::find_cli_with(&home, None, &AtomicBool::new(false), "hide-fixture-agent-a"),
        None,
        "with no shell to ask, that folder is not searched"
    );
}

#[test]
fn a_program_in_an_install_folder_the_daemons_path_misses_is_found() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let installed = program(&home.join(".local/bin"), "hide-fixture-agent-b");

    assert_eq!(
        programs::find_cli_with(&home, None, &AtomicBool::new(false), "hide-fixture-agent-b"),
        Some(installed)
    );
    assert_eq!(
        programs::find_cli_with(&home, None, &AtomicBool::new(false), "hide-fixture-absent"),
        None
    );
}
