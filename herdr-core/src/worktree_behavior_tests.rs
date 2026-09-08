use super::*;

struct Repository(PathBuf);
impl Repository {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hide-worktree-policy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-b", "main"]).unwrap();
        git(&root, &["config", "user.name", "Fixture"]).unwrap();
        git(&root, &["config", "user.email", "fixture@example.invalid"]).unwrap();
        git(&root, &["config", "commit.gpgsign", "false"]).unwrap();
        git(&root, &["commit", "--allow-empty", "-m", "initial"]).unwrap();
        git(&root, &["update-ref", "refs/remotes/origin/main", "HEAD"]).unwrap();
        git(
            &root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        )
        .unwrap();
        Self(root)
    }
    fn linked(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        git(
            &self.0,
            &["worktree", "add", "-b", name, path.to_str().unwrap()],
        )
        .unwrap();
        path
    }
    fn read(&self, base: Option<&str>) -> ProjectWorktreesSnapshot {
        read_project(
            &self.0,
            self.0.to_string_lossy().into_owned(),
            &BTreeMap::new(),
            base,
        )
    }
}

#[test]
fn base_branch_default_selection() {
    let repo = Repository::new();
    git(&repo.0, &["branch", "release"]).unwrap();
    let project = repo.read(None);
    assert_eq!(project.base_branch.as_deref(), Some("main"));
    assert_eq!(project.base_source, "origin_head");
    assert_eq!(project.branches, ["main", "release"]);
}

#[test]
fn base_branch_unknown_allows_creation() {
    let repo = Repository::new();
    git(
        &repo.0,
        &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
    )
    .unwrap();
    let project = repo.read(None);
    assert_eq!(project.base_branch, None);
    assert_eq!(project.base_source, "unknown");
    assert!(project.worktrees.iter().any(|row| row.branch.is_some()));
}

#[test]
fn worktree_creation_blocked_without_branches() {
    let root = std::env::temp_dir().join(format!("hide-unborn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-b", "main"]).unwrap();
    let project = read_project(
        &root,
        root.to_string_lossy().into_owned(),
        &BTreeMap::new(),
        None,
    );
    assert!(
        !project
            .worktrees
            .iter()
            .any(|row| row.branch.is_some() && row.head_sha.is_some())
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn main_worktree_row_label() {
    assert_eq!(
        crate::workspace::checkout_row_label(Some("topic"), Path::new("/repo")),
        "topic"
    );
}

#[test]
fn detached_worktree_row_label() {
    assert_eq!(
        crate::workspace::checkout_row_label(None, Path::new("/worktrees/review")),
        "review"
    );
}

#[test]
fn base_branch_source_excludes_head_fallback() {
    let repo = Repository::new();
    git(
        &repo.0,
        &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
    )
    .unwrap();
    git(&repo.0, &["checkout", "-b", "feature"]).unwrap();
    let project = repo.read(None);
    assert_eq!(project.default_branch, None);
    assert_eq!(project.base_branch, None);
    let row = project.worktrees.iter().find(|row| row.is_main).unwrap();
    assert_eq!(row.base_branch, None);
    assert_eq!(row.merged, None);
    assert_eq!((row.ahead, row.behind), (0, 0));
}

#[test]
fn set_base_branch_from_checkout_row() {
    let repo = Repository::new();
    repo.linked("feature");
    let project = repo.read(Some("feature"));
    assert_eq!(project.base_branch.as_deref(), Some("feature"));
    assert_eq!(project.base_source, "specified");
    assert!(
        project
            .worktrees
            .iter()
            .find(|row| row.branch.as_deref() == Some("feature"))
            .unwrap()
            .deletion_gate
            .blocked_reason
            .is_some()
    );
}
impl Drop for Repository {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn ancestry_does_not_mistake_a_squash_merge_for_a_merged_tip() {
    let repo = Repository::new();
    let merged = repo.linked("merged");
    git(&merged, &["commit", "--allow-empty", "-m", "merged work"]).unwrap();
    git(&repo.0, &["merge", "--ff-only", "merged"]).unwrap();
    let squash = repo.linked("squash");
    std::fs::write(squash.join("change"), "content").unwrap();
    git(&squash, &["add", "change"]).unwrap();
    git(&squash, &["commit", "-m", "squash work"]).unwrap();
    git(&repo.0, &["merge", "--squash", "squash"]).unwrap();
    git(&repo.0, &["commit", "-m", "squashed"]).unwrap();
    let result = repo.read(None);
    let row = |branch: &str| {
        result
            .worktrees
            .iter()
            .find(|w| w.branch.as_deref() == Some(branch))
            .unwrap()
    };
    assert_eq!(row("merged").merged, Some(true));
    assert_eq!(row("squash").merged, Some(false));
    assert!(row("squash").ahead > 0);
    assert!(!row("squash").deletion_gate.can_delete_branch);
    assert!(row("merged").last_commit_unix_seconds.is_some());
    assert!(row("merged").measured_at_unix_ms.is_some());
}

#[test]
fn configured_missing_upstream_is_distinct_from_no_upstream_and_pushed() {
    let repo = Repository::new();
    let feature = repo.linked("feature");
    assert_eq!(repo.read(None).worktrees[1].upstream_state, "no_upstream");
    git(
        &repo.0,
        &["remote", "add", "origin", "https://example.invalid/repo"],
    )
    .unwrap();
    git(
        &repo.0,
        &["update-ref", "refs/remotes/origin/feature", "HEAD"],
    )
    .unwrap();
    git(&feature, &["branch", "--set-upstream-to=origin/feature"]).unwrap();
    assert_eq!(repo.read(None).worktrees[1].upstream_state, "pushed");
    git(&feature, &["commit", "--allow-empty", "-m", "ahead"]).unwrap();
    let result = repo.read(None);
    assert_eq!(result.worktrees[1].upstream_state, "unpushed");
    assert_eq!(result.worktrees[1].unpushed.as_ref().unwrap().count, 1);
    git(
        &repo.0,
        &["update-ref", "-d", "refs/remotes/origin/feature"],
    )
    .unwrap();
    let result = repo.read(None);
    assert_eq!(result.worktrees[1].upstream_state, "gone");
    assert!(
        result.worktrees[1]
            .deletion_gate
            .warnings
            .contains(&"not pushed".to_owned())
    );
}

#[test]
fn base_override_moves_protection_and_absent_override_reports_fallback() {
    let repo = Repository::new();
    repo.linked("feature");
    let result = repo.read(Some("feature"));
    assert_eq!(result.base_branch.as_deref(), Some("feature"));
    assert_eq!(result.base_source, "specified");
    assert!(result.worktrees[1].deletion_gate.blocked_reason.is_some());
    let result = repo.read(None);
    assert!(result.worktrees[1].deletion_gate.blocked_reason.is_none());
    let result = repo.read(Some("deleted"));
    assert_eq!(result.base_branch.as_deref(), Some("main"));
    assert!(
        result
            .base_branch_fallback
            .as_deref()
            .unwrap()
            .contains("deleted")
    );
}

#[test]
fn missing_detached_nested_and_dirty_rows_preserve_their_actual_state() {
    let repo = Repository::new();
    let parent = repo.linked("parent");
    let child = parent.join("child");
    git(
        &repo.0,
        &["worktree", "add", "--detach", child.to_str().unwrap()],
    )
    .unwrap();
    let gone = repo.linked("gone");
    std::fs::remove_dir_all(gone).unwrap();
    std::fs::write(parent.join("dirty"), "work").unwrap();
    let result = repo.read(None);
    let parent = result
        .worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some("parent"))
        .unwrap();
    assert!(parent.nested && parent.dirty);
    assert!(parent.deletion_gate.blocked_reason.is_some());
    let detached = result
        .worktrees
        .iter()
        .find(|w| w.branch.is_none())
        .unwrap();
    assert_eq!(detached.head_sha.as_ref().unwrap().len(), 40);
    let missing = result
        .worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some("gone"))
        .unwrap();
    assert!(missing.missing);
    assert!(!missing.deletion_gate.can_delete_branch);
}

#[test]
fn deletion_gate_blocks_before_warning_and_reports_pane_consequence() {
    let row = WorktreeSnapshot {
        branch: Some("feature".into()),
        merged: Some(false),
        ahead: 2,
        upstream_state: "gone".into(),
        ..WorktreeSnapshot::default()
    };
    let gate = deletion_gate(&row, false, false, 2, 1);
    assert_eq!(gate.button_label, "Close 2 panes and delete");
    assert_eq!(
        gate.warnings,
        ["ahead 2 unmerged", "not pushed", "1 running agents"]
    );
    assert!(gate.blocked_reason.is_none());
    assert!(
        deletion_gate(&row, false, true, 0, 0)
            .blocked_reason
            .is_some()
    );
    assert!(
        deletion_gate(&row, true, false, 0, 0)
            .blocked_reason
            .is_some()
    );
    for blocked in [
        WorktreeSnapshot {
            dirty: true,
            ..row.clone()
        },
        WorktreeSnapshot {
            nested: true,
            ..row.clone()
        },
        WorktreeSnapshot {
            is_main: true,
            ..row.clone()
        },
    ] {
        assert!(
            deletion_gate(&blocked, false, false, 2, 1)
                .blocked_reason
                .is_some()
        );
    }
}

#[test]
fn idle_twenty_seconds_does_not_reread_but_file_edits_and_manual_refresh_do() {
    let repo = Repository::new();
    std::fs::write(repo.0.join("tracked"), "original").unwrap();
    git(&repo.0, &["add", "tracked"]).unwrap();
    git(&repo.0, &["commit", "-m", "tracked"]).unwrap();
    let mut reader = WorktreeReader::new();
    let mut request = WorktreeRequest {
        overview_root: None,
        projects: vec![WorktreeProjectRequest {
            root_path: repo.0.clone(),
            bases: BTreeMap::new(),
            base_override: None,
        }],
        generation: 0,
    };
    let wait = |reader: &mut WorktreeReader, request: &WorktreeRequest| {
        let started = std::time::Instant::now();
        loop {
            if let Some(result) = reader.read_if_due(request.clone()) {
                break result;
            }
            assert!(started.elapsed() < Duration::from_secs(15));
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    wait(&mut reader, &request);
    // First discovery installs the cached tracked-file stat set.
    wait(&mut reader, &request);
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        assert!(reader.read_if_due(request.clone()).is_none());
        std::thread::sleep(Duration::from_millis(100));
    }
    std::fs::write(repo.0.join("tracked"), "changed contents").unwrap();
    assert!(wait(&mut reader, &request).projects[0].worktrees[0].dirty);
    request.generation += 1;
    assert!(wait(&mut reader, &request).projects[0].worktrees[0].dirty);
}

#[test]
fn overview_history_preserves_real_merge_parents_and_reads_all_heads_once() {
    let repo = Repository::new();
    let feature = repo.linked("feature");
    git(&feature, &["-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "Feature"]).unwrap();
    let feature_head = git(&feature, &["rev-parse", "HEAD"]).unwrap().trim().to_owned();
    git(&repo.0, &["-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "Main"]).unwrap();
    let first_parent = git(&repo.0, &["rev-parse", "HEAD"]).unwrap().trim().to_owned();
    git(&repo.0, &["-c", "commit.gpgsign=false", "merge", "--no-ff", "feature", "-m", "Merge"]).unwrap();
    let merge = git(&repo.0, &["rev-parse", "HEAD"]).unwrap().trim().to_owned();
    let mut heads = vec![merge.clone(), feature_head.clone()];
    for _ in 0..24 { heads.push(first_parent.clone()); }
    let before = git_call_count(&repo.0, "log");
    let value = history::read(&repo.0, &heads, Some(&repo.0.join(".git")));
    assert_eq!(git_call_count(&repo.0, "log") - before, 1, "One project history read, independent of worktree count");
    assert!(value.unavailable_reason.is_none());
    assert_eq!(value.commits.iter().find(|c| c.sha == merge).unwrap().parents, [first_parent, feature_head]);
    assert!(value.continuation.is_empty());
    assert!(!value.truncated);
    let invalid = history::read(&repo.0, &["refs/heads/no-such-branch".into()], Some(&repo.0.join(".git")));
    assert!(invalid.unavailable_reason.is_some());
    assert!(invalid.commits.is_empty());
}

#[test]
fn overview_close_and_idle_do_not_run_additional_git_commands() {
    let repo = Repository::new();
    let root = std::fs::canonicalize(&repo.0).unwrap();
    let mut reader = WorktreeReader::new();
    let mut request = WorktreeRequest { overview_root: Some(root.clone()),
        projects: vec![WorktreeProjectRequest { root_path: root.clone(), bases: BTreeMap::new(), base_override: None }], generation: 0 };
    let settle = |reader: &mut WorktreeReader, request: &WorktreeRequest| {
        for _ in 0..1000 {
            if let Some(value) = reader.read_if_due(request.clone()) { return value; }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("Reader did not settle");
    };
    let value = settle(&mut reader, &request);
    assert!(value.projects[0].history.as_ref().is_some_and(|h| !h.commits.is_empty()));
    // Path discovery's existing content generation needs one settling pass.
    for _ in 0..300 { reader.read_if_due(request.clone()); std::thread::sleep(Duration::from_millis(2)); }
    let before = git_call_count(&root, "log");
    request.overview_root = None;
    settle(&mut reader, &request);
    for _ in 0..100 { reader.read_if_due(request.clone()); }
    assert_eq!(git_call_count(&root, "log"), before);
}
