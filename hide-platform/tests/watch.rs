//! The contract of `hide_platform::watch`, stated as what a caller observes.
//! The same file runs on macOS, Linux and Windows.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hide_platform::watch::{CAPACITY, Change, Changes, Watcher};

/// How long a change may take to arrive.
const ARRIVES: Duration = Duration::from_secs(2);
/// How long nothing has to arrive before a folder counts as quiet.
const QUIET: Duration = Duration::from_millis(700);

fn watched() -> (tempfile::TempDir, Watcher, Changes) {
    let dir = tempfile::tempdir().unwrap();
    let (mut watcher, changes) = Watcher::new().unwrap();
    watcher.watch(dir.path()).unwrap();
    (dir, watcher, changes)
}

/// Everything that arrives until nothing has for [`QUIET`], so a test starts
/// after the reports its own setup caused.
fn drain(changes: &Changes) -> Vec<Change> {
    let mut seen = Vec::new();
    while let Some(change) = changes.recv_timeout(QUIET) {
        seen.push(change);
    }
    seen
}

/// Waits for a change naming `path`, and returns every change that came.
fn expect_path(changes: &Changes, path: &Path, what: &str) -> Vec<Change> {
    let deadline = Instant::now() + ARRIVES;
    let mut seen = Vec::new();
    while let Some(change) =
        changes.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        let found = matches!(&change, Change::Path { path: changed, .. } if changed == path);
        seen.push(change);
        if found {
            return seen;
        }
    }
    panic!(
        "{what} {} was not reported within {ARRIVES:?}: {seen:?}",
        path.display()
    );
}

fn paths(changes: &[Change]) -> Vec<PathBuf> {
    changes
        .iter()
        .filter_map(|change| match change {
            Change::Path { path, .. } => Some(path.clone()),
            Change::Overflow { .. } => None,
        })
        .collect()
}

#[test]
fn creating_writing_renaming_and_removing_a_file_are_each_reported_under_the_watched_spelling() {
    let (dir, _watcher, changes) = watched();
    let file = dir.path().join("note.txt");
    fs::write(&file, "one").unwrap();
    expect_path(&changes, &file, "a created file");
    drain(&changes);

    fs::write(&file, "two, longer").unwrap();
    expect_path(&changes, &file, "a written file");
    drain(&changes);

    let renamed = dir.path().join("renamed.txt");
    fs::rename(&file, &renamed).unwrap();
    expect_path(&changes, &renamed, "a renamed file");
    drain(&changes);

    fs::remove_file(&renamed).unwrap();
    expect_path(&changes, &renamed, "a removed file");

    // A folder the system knows by another name (macOS keeps temporary
    // folders under /private) is still reported as the caller spelled it.
    for path in paths(&drain(&changes)) {
        assert!(
            path.starts_with(dir.path()),
            "{} is not under {}",
            path.display(),
            dir.path().display()
        );
    }
}

#[test]
fn a_change_deep_under_the_folder_is_reported() {
    let (dir, _watcher, changes) = watched();
    let deep = dir.path().join("a").join("b");
    fs::create_dir_all(&deep).unwrap();
    drain(&changes);
    let file = deep.join("HEAD");
    fs::write(&file, "ref: refs/heads/main\n").unwrap();
    expect_path(&changes, &file, "a file two folders down");
}

#[test]
fn reading_a_file_is_not_a_change() {
    let (dir, _watcher, changes) = watched();
    let file = dir.path().join("index");
    fs::write(&file, "contents that a reader reads").unwrap();
    drain(&changes);
    for _ in 0..20 {
        assert_eq!(fs::read(&file).unwrap(), b"contents that a reader reads");
        fs::metadata(&file).unwrap();
    }
    let reads = drain(&changes);
    assert!(reads.is_empty(), "reading reported {reads:?}");
}

#[test]
fn a_burst_is_bounded_and_ends_in_an_overflow_rather_than_silence() {
    let (dir, _watcher, changes) = watched();
    drain(&changes);
    for index in 0..10_000 {
        fs::write(dir.path().join(format!("f{index}")), b"x").unwrap();
    }
    let burst = drain(&changes);
    assert!(
        burst.len() <= CAPACITY + 1,
        "{} changes for one burst",
        burst.len()
    );
    assert!(
        burst
            .iter()
            .any(|change| matches!(change, Change::Overflow { .. })),
        "10,000 new files and no overflow among {} changes",
        burst.len()
    );
    // The watch survives the overflow: the next change is reported again.
    let after = dir.path().join("after");
    fs::write(&after, b"y").unwrap();
    expect_path(&changes, &after, "a file written after the overflow");
}

#[test]
fn an_unwatched_folder_reports_nothing_more() {
    let (dir, mut watcher, changes) = watched();
    drain(&changes);
    watcher.unwatch(dir.path()).unwrap();
    fs::write(dir.path().join("late"), b"z").unwrap();
    let late = drain(&changes);
    assert!(late.is_empty(), "an unwatched folder reported {late:?}");
}

#[test]
fn only_a_folder_that_exists_can_be_watched() {
    let dir = tempfile::tempdir().unwrap();
    let (mut watcher, _changes) = Watcher::new().unwrap();
    let missing = dir.path().join("missing");
    assert_eq!(
        watcher.watch(&missing).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    let file = dir.path().join("file");
    fs::write(&file, b"").unwrap();
    assert_eq!(
        watcher.watch(&file).unwrap_err().kind(),
        std::io::ErrorKind::NotADirectory
    );
}

#[test]
fn a_filtered_watch_reports_only_what_it_keeps_and_a_burst_it_drops_is_not_an_overflow() {
    let dir = tempfile::tempdir().unwrap();
    let (mut watcher, changes) = Watcher::keeping(|relative| relative.starts_with("refs")).unwrap();
    watcher.watch(dir.path()).unwrap();
    let objects = dir.path().join("objects");
    let refs = dir.path().join("refs");
    fs::create_dir_all(&objects).unwrap();
    fs::create_dir_all(&refs).unwrap();
    drain(&changes);
    // More paths than the queue holds, paced so the system's own queue keeps
    // up: an overflow there is the system's and no filter can prevent it.
    for index in 0..CAPACITY + 500 {
        fs::write(objects.join(format!("o{index}")), b"x").unwrap();
        if index % 100 == 99 {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let dropped = drain(&changes);
    assert!(
        dropped.is_empty(),
        "{} changes outside the filter, first {:?}",
        dropped.len(),
        dropped.first()
    );
    let head = refs.join("main");
    fs::write(&head, b"0123").unwrap();
    expect_path(&changes, &head, "a kept path");
}
