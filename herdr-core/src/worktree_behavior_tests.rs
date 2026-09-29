use super::*;

struct Repository(PathBuf);
impl Repository {
    fn new() -> Self {
        static NEXT_REPOSITORY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hide-worktree-policy-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_REPOSITORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
    let gate = &project
        .worktrees
        .iter()
        .find(|row| row.branch.as_deref() == Some("feature"))
        .unwrap()
        .deletion_gate;
    assert!(gate.blocked_reason.is_none());
    assert!(gate.warnings.contains(&"holds the base branch".to_owned()));
    assert!(!gate.can_delete_branch);
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
    // A squash merge leaves the tip off the base, so its deletion says what
    // `-D` loses rather than calling it merged.
    assert!(row("squash").deletion_gate.can_delete_branch);
    assert!(row("squash").deletion_gate.branch_warning.is_some());
    assert!(row("merged").deletion_gate.branch_warning.is_none());
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
    assert!(!result.worktrees[1].deletion_gate.can_delete_branch);
    let result = repo.read(None);
    assert!(result.worktrees[1].deletion_gate.can_delete_branch);
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
    assert!(parent.deletion_gate.blocked_reason.is_none());
    // The child's folder is untracked in the parent too, so it is counted.
    assert_eq!(
        parent.deletion_gate.discard_label.as_deref(),
        Some("Discard 2 changed files and the worktree inside it")
    );
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

/// Only the main worktree is refused; every other risk is a warning, and a
/// loss the folder would take asks for the discard checkbox.
#[test]
fn deletion_gate_warns_instead_of_blocking_and_reports_pane_consequence() {
    let row = WorktreeSnapshot {
        branch: Some("feature".into()),
        base_branch: Some("main".into()),
        merged: Some(false),
        ahead: 2,
        upstream_state: "gone".into(),
        ..WorktreeSnapshot::default()
    };
    let gate = deletion_gate(&row, false, 2);
    assert_eq!(gate.button_label, "Close 2 panes and delete");
    assert_eq!(
        deletion_gate(&row, false, 1).button_label,
        "Close 1 pane and delete"
    );
    assert_eq!(gate.warnings, ["ahead 2 unmerged", "not pushed"]);
    assert!(gate.blocked_reason.is_none() && gate.discard_label.is_none());
    assert!(gate.can_delete_branch);
    assert_eq!(
        gate.branch_warning.as_deref(),
        Some("2 commits not on main are lost with it")
    );

    let base = deletion_gate(&row, true, 0);
    assert!(base.blocked_reason.is_none() && !base.can_delete_branch);
    assert!(base.branch_warning.is_none());
    assert!(base.warnings.contains(&"holds the base branch".to_owned()));

    let dirty = deletion_gate(
        &WorktreeSnapshot {
            dirty: true,
            changed_file_count: 3,
            ..row.clone()
        },
        false,
        0,
    );
    assert!(dirty.blocked_reason.is_none());
    assert_eq!(dirty.warnings[0], "3 changed files not committed");
    assert_eq!(
        dirty.discard_label.as_deref(),
        Some("Discard 3 changed files")
    );

    let nested = deletion_gate(
        &WorktreeSnapshot {
            nested: true,
            ..row.clone()
        },
        false,
        0,
    );
    assert_eq!(
        nested.discard_label.as_deref(),
        Some("Discard the worktree inside it")
    );

    let unknown = deletion_gate(
        &WorktreeSnapshot {
            unavailable_reason: Some("timed out".into()),
            merged: None,
            ..row.clone()
        },
        false,
        0,
    );
    assert!(
        unknown
            .warnings
            .contains(&"Git status unavailable".to_owned())
    );
    assert_eq!(
        unknown.discard_label.as_deref(),
        Some("Discard any uncommitted changes")
    );
    assert!(unknown.branch_warning.unwrap().contains("could not tell"));

    let merged = deletion_gate(
        &WorktreeSnapshot {
            merged: Some(true),
            ..row.clone()
        },
        false,
        0,
    );
    assert!(merged.branch_warning.is_none());

    assert!(
        deletion_gate(
            &WorktreeSnapshot {
                is_main: true,
                ..row.clone()
            },
            false,
            0
        )
        .blocked_reason
        .is_some()
    );
}

#[test]
fn idle_and_working_tree_edits_do_not_reread_but_manual_refresh_does() {
    let repo = Repository::new();
    std::fs::write(repo.0.join("tracked"), "original").unwrap();
    git(&repo.0, &["add", "tracked"]).unwrap();
    git(&repo.0, &["commit", "-m", "tracked"]).unwrap();
    let mut reader = WorktreeReader::new();
    let mut request = WorktreeRequest {
        projects: vec![WorktreeProjectRequest {
            root_path: repo.0.clone(),
            bases: BTreeMap::new(),
            base_override: None,
            generation: 0,
        }],
        generation: 0,
        removals: 0,
    };
    let wait = |reader: &mut WorktreeReader, request: &WorktreeRequest| {
        let started = std::time::Instant::now();
        loop {
            if let Some(result) = reader.read_if_due(request.clone()) {
                break result.catalog;
            }
            assert!(started.elapsed() < Duration::from_secs(15));
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    wait(&mut reader, &request);
    let listed = std::fs::canonicalize(&repo.0).unwrap();
    let status_before = git_call_count(&listed, "status");
    assert!(reader.read_if_due(request.clone()).is_none());
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        assert!(reader.read_if_due(request.clone()).is_none());
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(git_call_count(&listed, "status"), status_before);
    std::fs::write(repo.0.join("tracked"), "changed contents").unwrap();
    assert!(reader.read_if_due(request.clone()).is_none());
    assert_eq!(git_call_count(&listed, "status"), status_before);
    request.generation += 1;
    assert!(wait(&mut reader, &request).projects[0].worktrees[0].dirty);
    assert_eq!(git_call_count(&listed, "status"), status_before + 1);
}

/// D-09, D-15. The base branch's `behind origin` comes from the same
/// `rev-list` that counts unpushed commits: no upstream is no answer, a gone
/// upstream is no answer, a fetched upstream ahead of the branch is the count
/// of commits the branch is missing, and pushed level is zero, not absent.
#[test]
fn behind_upstream_is_absent_without_an_upstream_and_counts_the_fetched_side() {
    let repo = Repository::new();
    let feature = repo.linked("feature");
    let row = |project: &ProjectWorktreesSnapshot| project.worktrees[1].clone();
    assert_eq!(row(&repo.read(None)).behind_upstream, None);
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
    assert_eq!(row(&repo.read(None)).behind_upstream, Some(0));
    git(&repo.0, &["commit", "--allow-empty", "-m", "fetched one"]).unwrap();
    git(&repo.0, &["commit", "--allow-empty", "-m", "fetched two"]).unwrap();
    git(
        &repo.0,
        &["update-ref", "refs/remotes/origin/feature", "HEAD"],
    )
    .unwrap();
    // git lists the worktree by its real path, which is where the reader runs.
    let listed = std::fs::canonicalize(&feature).unwrap();
    let rev_lists_before = git_call_count(&listed, "rev-list");
    let result = repo.read(None);
    assert_eq!(row(&result).behind_upstream, Some(2));
    assert_eq!(row(&result).upstream_state, "pushed");
    assert_eq!(
        git_call_count(&listed, "rev-list") - rev_lists_before,
        2,
        "One rev-list against the base and one against the upstream; behind rides the second"
    );
    git(
        &repo.0,
        &["update-ref", "-d", "refs/remotes/origin/feature"],
    )
    .unwrap();
    let result = repo.read(None);
    assert_eq!(row(&result).upstream_state, "gone");
    assert_eq!(row(&result).behind_upstream, None);
}

/// D-05. A linked worktree carries the time it was added and the main
/// worktree carries none, so the Overview can keep the primary first and the
/// rest in the order they were created.
#[test]
fn linked_worktrees_carry_their_creation_time_and_the_main_worktree_none() {
    let repo = Repository::new();
    repo.linked("older");
    std::thread::sleep(Duration::from_millis(20));
    repo.linked("newer");
    let project = repo.read(None);
    let by_branch = |branch: &str| {
        project
            .worktrees
            .iter()
            .find(|row| row.branch.as_deref() == Some(branch))
            .unwrap()
            .clone()
    };
    assert!(by_branch("main").is_main);
    assert_eq!(by_branch("main").created_at_unix_ms, None);
    let older = by_branch("older").created_at_unix_ms.expect("older time");
    let newer = by_branch("newer").created_at_unix_ms.expect("newer time");
    assert!(older <= newer, "{older} should not be after {newer}");
}

/// A commit in one registered repository re-reads that repository alone: the
/// other project's `git status` does not run again. Before the per-project
/// key, every project's status ran on any project's change.
#[test]
fn a_commit_in_one_project_does_not_rerun_status_in_another() {
    let changing = Repository::new();
    let quiet = Repository::new();
    let mut reader = WorktreeReader::new();
    let request = WorktreeRequest {
        projects: [&changing, &quiet]
            .into_iter()
            .map(|repo| WorktreeProjectRequest {
                root_path: repo.0.clone(),
                bases: BTreeMap::new(),
                base_override: None,
                generation: 0,
            })
            .collect(),
        generation: 0,
        removals: 0,
    };
    let wait = |reader: &mut WorktreeReader, phase: &str| {
        let started = std::time::Instant::now();
        loop {
            if let Some(result) = reader.read_if_due(request.clone()) {
                break result.catalog;
            }
            assert!(started.elapsed() < Duration::from_secs(15), "{phase}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let catalog = wait(&mut reader, "initial catalog");
    assert_eq!(catalog.projects.len(), 2);
    // Status runs in the path git lists, which is the canonical one.
    let listed = |catalog: &WorktreeCatalogSnapshot, index: usize| {
        PathBuf::from(&catalog.projects[index].worktrees[0].path)
    };
    let (changing_path, quiet_path) = (listed(&catalog, 0), listed(&catalog, 1));
    let changing_before = git_call_count(&changing_path, "status");
    let quiet_before = git_call_count(&quiet_path, "status");
    assert!(changing_before > 0 && quiet_before > 0);
    git(&changing.0, &["commit", "--allow-empty", "-m", "moved"]).unwrap();
    assert_eq!(wait(&mut reader, "commit refresh").projects.len(), 2);
    assert_eq!(
        git_call_count(&changing_path, "status") - changing_before,
        1
    );
    assert_eq!(git_call_count(&quiet_path, "status") - quiet_before, 0);
}

/// Git's own ref writes wake the reader without an Overview refresh event.
/// A pull's many writes settle as one catalog read for the affected project.
#[test]
fn moved_remote_ref_refreshes_one_project_once_after_a_burst() {
    let changing = Repository::new();
    let quiet = Repository::new();
    git(
        &changing.0,
        &["remote", "add", "origin", "https://example.invalid/repo"],
    )
    .unwrap();
    git(
        &changing.0,
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    )
    .unwrap();
    git(
        &changing.0,
        &["branch", "--set-upstream-to=origin/main", "main"],
    )
    .unwrap();
    let mut reader = WorktreeReader::new();
    let request = WorktreeRequest {
        projects: [&changing, &quiet]
            .into_iter()
            .map(|repo| WorktreeProjectRequest {
                root_path: repo.0.clone(),
                bases: BTreeMap::new(),
                base_override: None,
                generation: 0,
            })
            .collect(),
        generation: 0,
        removals: 0,
    };
    let wait = |reader: &mut WorktreeReader| {
        let started = std::time::Instant::now();
        loop {
            if let Some(result) = reader.read_if_due(request.clone()) {
                break result.catalog;
            }
            assert!(
                started.elapsed() < Duration::from_secs(15),
                "Git watch did not refresh"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let initial = wait(&mut reader);
    let main = PathBuf::from(&initial.projects[0].worktrees[0].path);
    let quiet_path = PathBuf::from(&initial.projects[1].worktrees[0].path);
    let before = git_call_count(&main, "status");
    let quiet_before = git_call_count(&quiet_path, "status");
    git(&changing.0, &["branch", "future"]).unwrap();
    git(&changing.0, &["switch", "future"]).unwrap();
    git(
        &changing.0,
        &["commit", "--allow-empty", "-m", "remote future"],
    )
    .unwrap();
    git(&changing.0, &["switch", "main"]).unwrap();
    git(
        &changing.0,
        &["update-ref", "refs/remotes/origin/main", "future"],
    )
    .unwrap();
    let updated = wait(&mut reader);
    assert_eq!(updated.projects[0].worktrees[0].behind_upstream, Some(1));
    assert_eq!(git_call_count(&main, "status") - before, 1);
    assert_eq!(git_call_count(&quiet_path, "status") - quiet_before, 0);
}

#[test]
fn a_watch_change_during_a_read_keeps_the_old_answer_stale() {
    let repo = Repository::new();
    let mut reader = WorktreeReader::new();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = std::sync::Mutex::new(release_rx);
    reader.inner =
        BackgroundRead::on_change(Duration::ZERO, move |observation: &ObservedRequest| {
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            (observation.clone(), Vec::new())
        });
    let request = WorktreeRequest {
        projects: vec![WorktreeProjectRequest {
            root_path: repo.0.clone(),
            bases: BTreeMap::new(),
            base_override: None,
            generation: 0,
        }],
        generation: 0,
        removals: 0,
    };
    assert!(reader.read_if_due(request.clone()).is_none());
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    reader.git_generations.insert(repo.0.clone(), 1);
    release_tx.send(()).unwrap();
    let wait = |reader: &mut WorktreeReader| {
        let started = std::time::Instant::now();
        loop {
            if let Some(answer) = reader.read_if_due(request.clone()) {
                break answer;
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    assert!(!wait(&mut reader).observations_current);
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    release_tx.send(()).unwrap();
    assert!(wait(&mut reader).observations_current);
}

/// A git invocation is bounded: one that outruns the deadline is killed and
/// reported, and one whose output outgrows the pipe buffer still completes.
#[test]
fn git_output_is_bounded_in_time_and_drained_past_the_pipe_buffer() {
    let mut slow = std::process::Command::new("sleep");
    slow.arg("30");
    let started = std::time::Instant::now();
    assert!(
        output_within(&mut slow, Duration::from_millis(200))
            .unwrap()
            .is_none()
    );
    assert!(started.elapsed() < Duration::from_secs(5));

    let mut wide = std::process::Command::new("sh");
    wide.args(["-c", "yes | head -c 300000"]);
    let output = output_within(&mut wide, Duration::from_secs(15))
        .unwrap()
        .expect("finished within the deadline");
    assert_eq!(output.stdout.len(), 300_000);
}
