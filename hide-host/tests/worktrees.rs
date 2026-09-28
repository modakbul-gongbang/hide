//! The worktree checks a repository's host answers for this machine and for
//! a device alike (`Call::BranchCheck`, `Call::Directory`), and the removal
//! that sets a worktree's folder aside before Git drops its registration.

use std::path::Path;
use std::process::Command;

use hide_host::ErrorCode;
use hide_host::protocol::Call;
use hide_host::serve::handle;

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
fn an_entry_left_before_git_dropped_the_registration_is_kept_until_a_retry_removes_it() {
    let (_directory, root, linked) = linked_worktree(10);
    let trash = root.join(".git").join(hide_host::worktrees::TRASH);
    std::fs::create_dir_all(&trash).unwrap();
    let entry = trash.join("1-1-linked");
    std::fs::rename(&linked, &entry).unwrap();

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
