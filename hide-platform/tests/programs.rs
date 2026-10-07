//! The one search for an agent's CLI: the install kit and Hide AI both ask it,
//! so a program is found, or not, the same way for both.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Barrier;
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

/// A stand-in login shell that records each time it is started, one line per
/// start in `starts`.
fn counting_shell(folder: &Path, starts: &Path) -> PathBuf {
    let shell = folder.join("counting-shell");
    fs::write(
        &shell,
        format!(
            "#!/bin/sh\necho started >> \"{}\"\n[ \"$1\" = -ilc ] || exit 64\neval \"$2\"\n",
            starts.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&shell, fs::Permissions::from_mode(0o755)).unwrap();
    shell
}

fn starts(file: &Path) -> usize {
    fs::read_to_string(file).map_or(0, |text| text.lines().count())
}

#[test]
fn callers_that_arrive_cold_together_start_one_shell() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let started = root.path().join("starts");
    let shell = counting_shell(root.path(), &started);
    let callers = 6;
    let together = Barrier::new(callers);

    let answers: Vec<_> = std::thread::scope(|scope| {
        let readers: Vec<_> = (0..callers)
            .map(|_| {
                scope.spawn(|| {
                    together.wait();
                    programs::login_shell_path(&home, Some(&shell), &AtomicBool::new(false))
                })
            })
            .collect();
        readers
            .into_iter()
            .map(|reader| reader.join().unwrap())
            .collect()
    });

    assert!(answers[0].is_some(), "the stand-in shell answers");
    assert!(answers.iter().all(|answer| answer == &answers[0]));
    assert_eq!(starts(&started), 1, "one shell answered every caller");
}

#[test]
fn a_second_home_does_not_make_the_first_ask_again() {
    let root = tempfile::tempdir().unwrap();
    let started = root.path().join("starts");
    let shell = counting_shell(root.path(), &started);
    let stop = AtomicBool::new(false);
    let first = root.path().join("first-home");
    let second = root.path().join("second-home");

    programs::login_shell_path(&first, Some(&shell), &stop).unwrap();
    programs::login_shell_path(&second, Some(&shell), &stop).unwrap();
    assert_eq!(starts(&started), 2, "each home is asked once");
    programs::login_shell_path(&first, Some(&shell), &stop).unwrap();
    programs::login_shell_path(&second, Some(&shell), &stop).unwrap();
    assert_eq!(starts(&started), 2, "both answers are still remembered");
}

#[test]
fn the_path_a_program_runs_with_names_only_absolute_folders() {
    let home = tempfile::tempdir().unwrap();
    let shell_path = std::ffi::OsString::from("relative/bin::/shell/abs:");
    let joined = programs::cli_path_with(home.path(), Some(&shell_path)).unwrap();
    let folders: Vec<PathBuf> = std::env::split_paths(&joined).collect();
    assert!(
        folders.iter().all(|folder| folder.is_absolute()),
        "a relative or empty folder would be searched from the child's working directory: {folders:?}"
    );
    assert!(folders.contains(&PathBuf::from("/shell/abs")));
}

#[test]
fn a_slow_ask_for_one_home_does_not_hold_another_home_behind_it() {
    let root = tempfile::tempdir().unwrap();
    let release = root.path().join("release");
    let entered = root.path().join("entered");
    // The first shell says it was started and then does not answer until the
    // release file exists, so its ask is under way for as long as the test says.
    let slow = root.path().join("slow-shell");
    fs::write(
        &slow,
        format!(
            "#!/bin/sh\necho in > \"{}\"\nwhile [ ! -e \"{}\" ]; do sleep 0.01; done\n[ \"$1\" = -ilc ] || exit 64\neval \"$2\"\n",
            entered.display(),
            release.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&slow, fs::Permissions::from_mode(0o755)).unwrap();
    let quick = counting_shell(root.path(), &root.path().join("quick-starts"));
    let stop = AtomicBool::new(false);

    std::thread::scope(|scope| {
        let waiting = scope
            .spawn(|| programs::login_shell_path(&root.path().join("first"), Some(&slow), &stop));
        while !entered.exists() {
            std::thread::yield_now();
        }
        // The first ask is under way; another home answers without it.
        let other = programs::login_shell_path(&root.path().join("second"), Some(&quick), &stop);
        assert!(
            other.is_some(),
            "the second home was answered while the first was still asking"
        );
        fs::write(&release, "").unwrap();
        assert!(waiting.join().unwrap().is_some());
    });
}
