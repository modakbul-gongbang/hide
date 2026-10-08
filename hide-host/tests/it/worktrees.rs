//! The worktree checks a repository's host answers for this machine and for
//! a device alike (`Call::BranchCheck`, `Call::Directory`), and the removal
//! that sets a worktree's folder aside before Git drops its registration.

use std::path::Path;
use std::process::Command;

use hide_host::ErrorCode;
use hide_host::protocol::Call;
use hide_host::serve::handle;
use hide_host::worktrees::{ConfirmedRemoval, ignored_repositories};

fn removal(root: &Path, linked: &Path) -> ConfirmedRemoval {
    ConfirmedRemoval {
        repository_root: root.to_string_lossy().into_owned(),
        checkout_path: linked.to_string_lossy().into_owned(),
        expected_head_sha: Some(output(linked, &["rev-parse", "HEAD"]).trim().to_owned()),
        expected_branch: Some("linked".into()),
        protected_base_branch: Some("main".into()),
        delete_branch: None,
        force_delete_branch: false,
        discard_changes: false,
        expected_ignored_repositories: Vec::new(),
    }
}

#[test]
fn a_locked_worktree_is_named_with_its_reason_and_manual_unlock_action_before_removal() {
    let (_fixture, root, linked) = linked_worktree(0);
    let reason = "Release review\n보관 $(never-execute)";
    git(
        &root,
        &[
            "worktree",
            "lock",
            "--reason",
            reason,
            linked.to_str().unwrap(),
        ],
    );
    let facts = hide_host::worktrees::read(&root, &Default::default(), None).unwrap();
    assert_eq!(facts.worktrees[1].lock_reason.as_deref(), Some(reason));
    let mut request = removal(&root, &linked);
    request.discard_changes = true;
    let refused = handle(Call::WorktreeRemovalCheck {
        removal: request.clone(),
    })
    .unwrap_err();
    assert!(refused.message.contains("Worktree linked is locked"));
    assert!(refused.message.contains(reason));
    assert!(refused.message.contains("git worktree unlock"));
    assert!(refused.message.contains("panes are kept"));
    let removed: hide_host::worktrees::RemovalOutcome =
        serde_json::from_value(handle(Call::WorktreeRemove { removal: request }).unwrap()).unwrap();
    assert!(!removed.removed);
    assert!(linked.exists());
    assert!(trash_entries(&root).is_empty());
    assert!(hide_host::worktrees::registered(&root).unwrap()[1].locked);
}

#[test]
fn discard_names_every_ignored_repository_including_worktrees_separate_gitdirs_and_bare_repositories()
 {
    let (fixture, root, linked) = linked_worktree(0);
    for name in [
        "target/vendor/alpha",
        "target/vendor/alpha/embedded",
        "target/deep/beta",
    ] {
        let repo = linked.join(name);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
    }
    let source = repository();
    git(
        source.path(),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.join("target/linked").to_str().unwrap(),
        ],
    );
    let separate = linked.join("target/separate");
    std::fs::create_dir_all(&separate).unwrap();
    git(
        &separate,
        &[
            "init",
            "-q",
            "--separate-git-dir",
            fixture.path().join("separate.git").to_str().unwrap(),
        ],
    );
    let bare = linked.join("target/bare.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare"]);
    #[cfg(unix)]
    std::os::unix::fs::symlink(source.path(), linked.join("target/linked-alias")).unwrap();
    let expected = vec![
        "target/bare.git",
        "target/deep/beta",
        "target/linked",
        "target/separate",
        "target/vendor/alpha",
        "target/vendor/alpha/embedded",
    ];
    assert_eq!(ignored_repositories(&linked).unwrap(), expected);
    let mut request = removal(&root, &linked);
    request.discard_changes = true;
    let refused = handle(Call::WorktreeRemovalCheck {
        removal: request.clone(),
    })
    .unwrap_err();
    assert!(refused.message.contains("repository list changed"));
    assert!(linked.exists());
    request.expected_ignored_repositories = expected.into_iter().map(str::to_owned).collect();
    assert_eq!(
        handle(Call::WorktreeRemovalCheck {
            removal: request.clone()
        })
        .unwrap(),
        serde_json::Value::Null
    );
    // The check is non-mutating. A newly created ignored repository invalidates consent.
    assert!(linked.exists());
    let added = linked.join("target/late");
    std::fs::create_dir_all(&added).unwrap();
    git(&added, &["init", "-q"]);
    let outcome: hide_host::worktrees::RemovalOutcome =
        serde_json::from_value(handle(Call::WorktreeRemove { removal: request }).unwrap()).unwrap();
    assert!(!outcome.removed);
    assert!(linked.exists());
    assert!(trash_entries(&root).is_empty());
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.invalid"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", "seed"]);
    directory
}

fn check(root: &Path, branch: &str) -> Result<serde_json::Value, hide_host::HostError> {
    handle(Call::BranchCheck {
        path: root.to_string_lossy().into_owned(),
        branch: branch.to_owned(),
    })
}

#[test]
fn a_new_branch_is_checked_by_gits_own_rules_before_anything_is_created() {
    let repository = repository();
    let root = repository.path();
    git(root, &["branch", "existing"]);

    assert!(check(root, "feature/new").is_ok());
    let invalid = check(root, "bad..name").unwrap_err();
    assert_eq!(invalid.code, ErrorCode::InvalidRequest);
    assert_eq!(
        check(root, "-x").unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    let existing = check(root, "existing").unwrap_err();
    assert_eq!(existing.code, ErrorCode::AlreadyExists);
    assert_eq!(
        existing.message,
        "fatal: a branch named 'existing' already exists"
    );
    // The checked-out branch passes: Herdr answers Git's worktree refusal.
    assert!(check(root, "main").is_ok());
    // Asking created nothing.
    let branches = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["branch", "--format=%(refname:short)"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&branches.stdout),
        "existing\nmain\n"
    );
    assert_eq!(
        check(Path::new("relative"), "x").unwrap_err().code,
        ErrorCode::InvalidPath
    );
}

#[test]
fn a_directory_answers_its_real_path_and_nothing_else_answers_one() {
    let repository = repository();
    let root = repository.path();
    std::fs::write(root.join("file"), "x").unwrap();
    let real = std::fs::canonicalize(root).unwrap();
    let directory = |path: &Path| {
        handle(Call::Directory {
            path: path.to_string_lossy().into_owned(),
        })
        .unwrap()
    };
    assert_eq!(directory(root), serde_json::json!(real.to_string_lossy()));
    assert_eq!(directory(&root.join("file")), serde_json::Value::Null);
    assert_eq!(directory(&root.join("gone")), serde_json::Value::Null);
}

fn output(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A repository with a linked worktree at `linked`, its build folder ignored
/// and filled with `files` files.
fn linked_worktree(files: usize) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(directory.path()).unwrap();
    let root = base.join("repo");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "-q", "-m", "seed"]);
    let linked = base.join("linked");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    for index in 0..files {
        let folder = linked
            .join("target/debug/deps")
            .join(format!("{}", index % 50));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("{index}.o")), [0u8; 512]).unwrap();
    }
    (directory, root, linked)
}

fn trash_entries(root: &Path) -> Vec<std::path::PathBuf> {
    match std::fs::read_dir(root.join(".git").join(hide_host::worktrees::TRASH)) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn wait_for_empty_trash(root: &Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !trash_entries(root).is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "trash still holds {:?}",
            trash_entries(root)
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Waits until a sweep's thread has deleted `entry`.
#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn wait_until_deleted(entry: &Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::fs::symlink_metadata(entry).is_ok() {
        assert!(
            std::time::Instant::now() < deadline,
            "{} stayed",
            entry.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// A build folder is not Git's to delete in place: the removal answers once
/// the folder is gone from its path and Git no longer registers it, and the
/// files themselves are deleted from the repository's trash afterwards.
#[test]
fn a_worktree_with_a_build_folder_is_removed_by_setting_the_folder_aside() {
    let (_directory, root, linked) = linked_worktree(5_000);

    let message = hide_host::worktrees::remove_worktree(&root, &linked, false).unwrap();

    assert!(message.contains("removed"), "{message}");
    assert!(!linked.exists());
    assert!(!output(&root, &["worktree", "list", "--porcelain"]).contains("linked"));
    assert!(output(&root, &["branch", "--list", "linked"]).contains("linked"));
    wait_for_empty_trash(&root);
}

/// Git still refuses what it would refuse in place, and a refused removal
/// leaves the folder where it was with nothing in the trash.
#[test]
fn a_refused_removal_leaves_the_folder_in_place() {
    let (_directory, root, linked) = linked_worktree(10);
    std::fs::write(linked.join("draft.txt"), "work").unwrap();

    let refused = hide_host::worktrees::remove_worktree(&root, &linked, false).unwrap_err();
    assert!(refused.contains("modified or untracked"), "{refused}");
    assert!(linked.join("draft.txt").exists());
    assert!(trash_entries(&root).is_empty());

    git(&root, &["worktree", "lock", linked.to_str().unwrap()]);
    let locked = hide_host::worktrees::remove_worktree(&root, &linked, true).unwrap_err();
    assert!(locked.contains("locked"), "{locked}");
    assert!(linked.join("draft.txt").exists());
    assert!(output(&root, &["worktree", "list", "--porcelain"]).contains("linked"));
    assert!(trash_entries(&root).is_empty());

    git(&root, &["worktree", "unlock", linked.to_str().unwrap()]);
    hide_host::worktrees::remove_worktree(&root, &linked, true).unwrap();
    assert!(!linked.exists());
    wait_for_empty_trash(&root);
}

/// A process that stopped between moving the folder aside and Git dropping
/// the registration leaves an entry Git still registers: a sweep keeps it,
/// and retrying the removal drops the registration and then deletes it, once.
#[test]
#[allow(clippy::disallowed_methods)] // a window in which the kept entry must not be deleted: no state reports an event that has not happened
fn an_entry_left_before_git_dropped_the_registration_is_kept_until_a_retry_removes_it() {
    let (_directory, root, linked) = linked_worktree(10);
    let trash = root.join(".git").join(hide_host::worktrees::TRASH);
    std::fs::create_dir_all(&trash).unwrap();
    let entry = trash.join("1-1-linked");
    std::fs::rename(&linked, &entry).unwrap();

    // The sweep decides what to delete before it returns and deletes each
    // entry on its own thread, so the window covers only that deletion.
    hide_host::worktrees::sweep_trash(&trash);
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(entry.join("target").exists());
    assert!(output(&root, &["worktree", "list", "--porcelain"]).contains("linked"));

    hide_host::worktrees::remove_worktree(&root, &linked, false).unwrap();
    assert!(!output(&root, &["worktree", "list", "--porcelain"]).contains("linked"));
    wait_for_empty_trash(&root);
    // Nothing is left to delete a second time.
    hide_host::worktrees::sweep_trash(&trash);
    assert!(trash_entries(&root).is_empty());
}

/// A sweep deletes only what a removal named, and only from a real trash
/// folder: a stray entry keeps its files, and a trash that is a link leads
/// nowhere.
#[test]
#[allow(clippy::disallowed_methods)] // a window in which a linked trash must not be followed: no state reports an event that has not happened
fn a_sweep_keeps_names_no_removal_made_and_never_follows_a_linked_trash() {
    let (directory, root, _linked) = linked_worktree(10);
    let common = root.join(".git");
    let trash = common.join(hide_host::worktrees::TRASH);
    std::fs::create_dir_all(&trash).unwrap();
    for stray in ["notes", "12-abc-x", "-1-x", "1-2-"] {
        std::fs::create_dir_all(trash.join(stray)).unwrap();
        std::fs::write(trash.join(stray).join("keep"), "x").unwrap();
    }
    let ours = trash.join("1-2-gone");
    std::fs::create_dir_all(&ours).unwrap();
    hide_host::worktrees::sweep_trash(&trash);
    wait_until_deleted(&ours);
    for stray in ["notes", "12-abc-x", "-1-x", "1-2-"] {
        assert!(trash.join(stray).join("keep").exists(), "{stray} was swept");
    }

    std::fs::remove_dir_all(&trash).unwrap();
    let elsewhere = directory.path().join("elsewhere");
    let victim = elsewhere.join("1-2-victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("keep"), "x").unwrap();
    hide_platform::fs::link::create_link(&elsewhere, &trash).unwrap();
    // The sweep decides what to delete before it returns, so the window
    // covers only a deletion it would have started.
    hide_host::worktrees::sweep_trash(&trash);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(victim.join("keep").exists(), "a linked trash was followed");
}

/// The wait for the trash is for the entries a run put there: a folder that
/// another process left behind is swept but never held against the run.
#[test]
fn draining_waits_only_for_the_entries_it_was_given() {
    let (_directory, root, _linked) = linked_worktree(10);
    let common = root.join(".git");
    let trash = common.join(hide_host::worktrees::TRASH);
    std::fs::create_dir_all(&trash).unwrap();
    let ours = trash.join("3-4-mine");
    std::fs::create_dir_all(ours.join("target")).unwrap();
    let theirs = trash.join("notes");
    std::fs::create_dir_all(&theirs).unwrap();
    let mine = std::collections::BTreeSet::from([ours.clone()]);
    let left =
        hide_host::worktrees::drain_trash(&common, &mine, std::time::Duration::from_secs(30));
    assert_eq!(left, 0);
    assert!(!ours.exists());
    assert!(theirs.exists(), "an entry nobody named stays");
}
