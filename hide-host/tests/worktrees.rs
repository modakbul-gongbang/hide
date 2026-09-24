//! The worktree checks a repository's host answers for this machine and for
//! a device alike (`Call::BranchCheck`, `Call::Directory`).

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
