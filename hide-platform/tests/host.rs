//! The contract of `hide_platform::host`, stated as what a caller observes.
//! The same file runs on macOS, Linux and Windows. The socket Herdr listens
//! on by default is checked against the pinned Herdr itself, in
//! `hide-herdr-client/tests/real_herdr.rs`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use hide_platform::host;

#[test]
fn the_home_folder_is_an_absolute_folder_that_exists() {
    let home = host::home_dir().unwrap();
    assert!(home.is_absolute(), "{}", home.display());
    assert!(home.is_dir(), "{} is not a folder", home.display());
}

#[test]
fn the_state_folder_is_the_accounts_own() {
    let state = host::state_dir().unwrap();
    assert!(state.is_absolute(), "{}", state.display());
    if std::env::var_os("XDG_STATE_HOME").is_none() {
        let home = host::home_dir().unwrap();
        assert!(
            state.starts_with(&home),
            "{} is not under {}",
            state.display(),
            home.display()
        );
    }
}

/// A folder holding a file named like the program that the system would not
/// run: no execute bit on Unix, no extension on Windows.
fn decoy(name: &str) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    fs::write(folder.path().join(name), b"not a program").unwrap();
    folder
}

fn path_with_first(first: &Path) -> std::ffi::OsString {
    let mut folders = vec![first.to_path_buf()];
    folders.extend(std::env::split_paths(&host::login_path().unwrap()));
    std::env::join_paths(folders).unwrap()
}

#[test]
fn git_is_found_on_the_login_path_and_runs() {
    let git = host::find_program(&host::login_path().unwrap(), "git").expect("git on PATH");
    assert!(git.is_absolute(), "{}", git.display());
    let output = Command::new(&git).arg("--version").output().unwrap();
    assert!(
        output.status.success(),
        "{} --version failed",
        git.display()
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("git version"));
}

#[test]
fn a_file_the_system_would_not_run_is_passed_over() {
    let decoy = decoy("git");
    let git = host::find_program(&path_with_first(decoy.path()), "git").expect("git on PATH");
    assert!(
        !git.starts_with(decoy.path()),
        "{} is not a program",
        git.display()
    );
}

#[test]
fn a_missing_program_is_not_found() {
    assert_eq!(
        host::find_program(
            &host::login_path().unwrap(),
            "hide-platform-no-such-program"
        ),
        None
    );
}

#[test]
fn the_default_shell_when_there_is_one_is_a_file_that_exists() {
    match host::default_shell() {
        Ok(shell) => {
            assert!(shell.is_absolute(), "{}", shell.display());
            assert!(shell.is_file(), "{} does not exist", shell.display());
        }
        // Windows always names its command interpreter; a Unix process may
        // be started without SHELL, and then there is no answer to give.
        Err(error) if cfg!(windows) => panic!("{error}"),
        Err(error) => {
            assert_eq!(error.kind(), ErrorKind::NotFound, "{error}");
            assert!(std::env::var_os("SHELL").is_none());
        }
    }
}

#[test]
fn the_tailscale_location_is_absolute_where_the_system_has_one() {
    match host::tailscale_cli() {
        Ok(cli) => assert!(cli.is_absolute(), "{}", cli.display()),
        Err(error) if cfg!(target_os = "linux") => {
            assert_eq!(error.kind(), ErrorKind::Unsupported, "{error}")
        }
        Err(error) => panic!("{error}"),
    }
}

#[test]
fn the_machine_names_itself_the_same_way_twice() {
    let name = host::name().unwrap();
    assert!(!name.is_empty());
    assert_eq!(host::name().unwrap(), name);
    let id = host::machine_id().unwrap();
    assert!(!id.is_empty());
    assert!(!id.chars().any(char::is_whitespace), "{id:?}");
    assert_eq!(host::machine_id().unwrap(), id);
}

/// Where the system keeps an item it trashed, found and removed again so the
/// test leaves the account's Trash as it found it.
fn take_back_from_the_trash(name: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let kept = host::home_dir().unwrap().join(".Trash").join(name);
        let found = kept.symlink_metadata().is_ok();
        if found {
            fs::remove_file(&kept).unwrap();
        }
        found
    }
    #[cfg(not(target_os = "macos"))]
    {
        let items: Vec<_> = trash::os_limited::list()
            .unwrap()
            .into_iter()
            .filter(|item| item.name == std::ffi::OsStr::new(name))
            .collect();
        let found = !items.is_empty();
        trash::os_limited::purge_all(items).unwrap();
        found
    }
}

#[test]
fn a_trashed_file_leaves_its_folder_for_the_trash() {
    let folder = tempfile::tempdir().unwrap();
    let name = format!("hide-platform-trash-{}.txt", std::process::id());
    let file: PathBuf = folder.path().join(&name);
    fs::write(&file, b"to be trashed").unwrap();
    host::trash(&file).unwrap();
    assert!(file.symlink_metadata().is_err(), "the file is still there");
    assert!(
        take_back_from_the_trash(&name),
        "{name} is not in the system's Trash"
    );
}

#[test]
fn trashing_a_missing_file_fails() {
    let folder = tempfile::tempdir().unwrap();
    assert!(host::trash(&folder.path().join("missing")).is_err());
}
