//! The Git changes contract (PRD S5.5 B19-B21): the answer describes the
//! opened checkout and only the registered folder under it, a failed or
//! impossible read is an error rather than a clean tree, and a folder or
//! checkout swapped after it was opened is never read in its place.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;

use hide_host::git::{ChangesQuery, FileStatus, MAX_DIFF_BYTES, changes};
use hide_host::protocol::{Call, RootRef};
use hide_host::{ErrorCode, Root};

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(path: &Path) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["config", "user.name", "Fixture"]);
    git(path, &["config", "user.email", "fixture@example.invalid"]);
}

fn query(scope: &str, selected: Option<&str>, committed: bool, base: Option<&str>) -> ChangesQuery {
    ChangesQuery {
        scope: scope.to_owned(),
        selected: selected.map(str::to_owned),
        committed,
        base: base.map(str::to_owned),
    }
}

fn read(root: &Root, query: &ChangesQuery) -> hide_host::HostResult<hide_host::git::Changes> {
    changes(root, Path::new(&query.scope), query)
}

#[test]
fn a_registered_subfolder_keeps_its_history_without_exposing_siblings() {
    let temporary = tempfile::tempdir().unwrap();
    let checkout = temporary.path().join("checkout");
    repository(&checkout);
    let registered = checkout.join("registered");
    fs::create_dir(&registered).unwrap();
    for (path, content) in [
        ("registered/inside.txt", "inside base\n"),
        ("registered/delete.txt", "delete me\n"),
        ("registered/rename-old.txt", "rename within\n"),
        ("registered/outgoing.txt", "move out\n"),
        ("registered/work-out.txt", "working outbound only\n"),
        ("outside.txt", "outside base\n"),
        ("outside-source.txt", "move in\n"),
        ("work-in.txt", "working inbound only\n"),
    ] {
        fs::write(checkout.join(path), content).unwrap();
    }
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-q", "-m", "base"]);
    git(&checkout, &["checkout", "-q", "-b", "feature"]);
    git(
        &checkout,
        &[
            "mv",
            "registered/rename-old.txt",
            "registered/rename-new.txt",
        ],
    );
    git(
        &checkout,
        &["mv", "outside-source.txt", "registered/incoming.txt"],
    );
    git(
        &checkout,
        &["mv", "registered/outgoing.txt", "outside-outgoing.txt"],
    );
    fs::write(registered.join("inside.txt"), "inside committed\n").unwrap();
    fs::write(checkout.join("outside.txt"), "outside committed\n").unwrap();
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-q", "-m", "feature"]);
    git(&checkout, &["mv", "work-in.txt", "registered/work-in.txt"]);
    git(
        &checkout,
        &["mv", "registered/work-out.txt", "work-out.txt"],
    );
    fs::write(registered.join("inside.txt"), "inside working\n").unwrap();
    fs::remove_file(registered.join("delete.txt")).unwrap();
    fs::write(registered.join("new.txt"), "untracked inside\n").unwrap();
    fs::write(checkout.join("outside-new.txt"), "untracked outside\n").unwrap();

    let root = Root::open(&checkout).unwrap();
    let listed = read(
        &root,
        &query("registered", Some("inside.txt"), false, Some("main")),
    )
    .unwrap();
    let paths = |entries: &[hide_host::git::ChangedFile]| {
        entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.status))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        paths(&listed.entries),
        [
            ("delete.txt".to_owned(), FileStatus::Deleted),
            ("inside.txt".to_owned(), FileStatus::Modified),
            ("new.txt".to_owned(), FileStatus::Untracked),
            ("work-in.txt".to_owned(), FileStatus::Added),
            ("work-out.txt".to_owned(), FileStatus::Deleted),
        ]
    );
    let diff = listed.diff.unwrap();
    assert!(diff.text.contains("inside working"));
    assert!(!diff.text.contains("outside"));
    assert_eq!(listed.base.as_deref(), Some("main"));
    assert_eq!(
        paths(listed.committed.as_deref().unwrap()),
        [
            ("incoming.txt".to_owned(), FileStatus::Added),
            ("inside.txt".to_owned(), FileStatus::Modified),
            ("outgoing.txt".to_owned(), FileStatus::Deleted),
            ("rename-new.txt".to_owned(), FileStatus::Renamed),
        ]
    );
    for name in ["incoming.txt", "outgoing.txt", "rename-new.txt"] {
        let diff = read(&root, &query("registered", Some(name), true, Some("main")))
            .unwrap()
            .diff
            .unwrap();
        assert!(!diff.text.is_empty(), "{name} has a diff");
        assert!(!diff.text.contains("outside-source.txt"), "{name}");
        assert!(!diff.text.contains("outside-outgoing.txt"), "{name}");
    }
    let untracked = read(&root, &query("registered", Some("new.txt"), false, None)).unwrap();
    assert!(untracked.diff.unwrap().text.contains("untracked inside"));
    // No base and no `origin/HEAD`: the committed group is absent, not empty.
    assert_eq!(untracked.committed, None);
    assert_eq!(untracked.base, None);
    assert!(
        read(&root, &query("registered", None, false, Some("gone")))
            .unwrap()
            .committed
            .is_none()
    );
}

#[test]
fn a_folder_that_is_not_a_repository_or_not_its_top_level_is_refused() {
    let temporary = tempfile::tempdir().unwrap();
    let plain = temporary.path().join("plain");
    fs::create_dir(&plain).unwrap();
    // A temporary directory can itself sit inside a repository; one with its
    // own `.git` file pointing nowhere is never one.
    fs::write(plain.join(".git"), "gitdir: /nonexistent\n").unwrap();
    let refused = read(&Root::open(&plain).unwrap(), &query("", None, false, None)).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unsupported);

    let checkout = temporary.path().join("checkout");
    repository(&checkout);
    fs::create_dir(checkout.join("sub")).unwrap();
    let below = read(
        &Root::open(&checkout.join("sub")).unwrap(),
        &query("", None, false, None),
    )
    .unwrap_err();
    assert_eq!(
        below.message,
        "This History scope belongs to another checkout"
    );
}

#[test]
fn a_registered_folder_replaced_by_a_link_to_a_sibling_answers_nothing() {
    let temporary = tempfile::tempdir().unwrap();
    let checkout = temporary.path().join("checkout");
    repository(&checkout);
    fs::create_dir(checkout.join("registered")).unwrap();
    fs::create_dir(checkout.join("sibling")).unwrap();
    fs::write(checkout.join("registered/inside.txt"), "INSIDE\n").unwrap();
    fs::write(checkout.join("sibling/inside.txt"), "OUTSIDE_SENTINEL\n").unwrap();
    let root = Root::open(&checkout).unwrap();
    let selected = query("registered", Some("inside.txt"), false, None);
    assert!(
        read(&root, &selected)
            .unwrap()
            .diff
            .unwrap()
            .text
            .contains("INSIDE")
    );

    fs::rename(checkout.join("registered"), checkout.join("moved")).unwrap();
    symlink("sibling", checkout.join("registered")).unwrap();
    let refused = read(&root, &selected).unwrap_err();
    assert!(!format!("{refused:?}").contains("OUTSIDE_SENTINEL"));
}

#[test]
fn a_checkout_replaced_after_it_was_opened_is_never_read() {
    let temporary = tempfile::tempdir().unwrap();
    let checkout = temporary.path().join("checkout");
    let replacement = temporary.path().join("replacement");
    repository(&checkout);
    repository(&replacement);
    fs::write(checkout.join("inside.txt"), "INSIDE\n").unwrap();
    fs::write(replacement.join("secret.txt"), "OUTSIDE_SENTINEL\n").unwrap();
    let opened = Root::open(&checkout).unwrap();
    let identity = opened.identity();

    fs::rename(&checkout, temporary.path().join("moved")).unwrap();
    symlink(&replacement, &checkout).unwrap();
    // Git runs in the handle opened before the swap, which now sits at
    // another path, so the read is refused and the replacement is not read.
    let moved = read(&opened, &query("", None, false, None)).unwrap_err();
    assert_eq!(moved.code, ErrorCode::Conflict);
    assert!(!format!("{moved:?}").contains("secret.txt"));
    // A request naming the identity it was opened with is refused.
    let refused = hide_host::serve::handle(Call::Changes {
        root: RootRef {
            path: checkout.to_string_lossy().into_owned(),
            identity,
        },
        scope: String::new(),
        selected: None,
        committed: false,
        base: None,
    })
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::RootReplaced);
}

#[test]
fn a_large_untracked_patch_is_cut_within_the_wire_budget() {
    let temporary = tempfile::tempdir().unwrap();
    let checkout = temporary.path().join("checkout");
    repository(&checkout);
    fs::write(checkout.join("large.txt"), "한글".repeat(100_000)).unwrap();
    let diff = read(
        &Root::open(&checkout).unwrap(),
        &query("", Some("large.txt"), false, None),
    )
    .unwrap()
    .diff
    .unwrap();
    assert!(diff.text.len() <= MAX_DIFF_BYTES);
    assert!(diff.notice.unwrap().contains("truncated"));
}
