use super::*;

/// A registered folder below a Git root keeps the live History request and
/// the web's root identity on that folder. The reader's direct scope test is
/// insufficient if the catalog hands the runtime the repository root.
#[test]
fn registered_subfolder_history_stays_scoped_through_runtime_selection() {
    let temporary = tempfile::tempdir().expect("fixture root");
    let repository = temporary.path().canonicalize().unwrap();
    let registered = repository.join("registered");
    std::fs::create_dir(&registered).unwrap();
    let git = |arguments: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(arguments)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    for (name, content) in [
        ("registered/inside.txt", "base inside\n"),
        ("registered/deleted.txt", "deleted content\n"),
        ("registered/rename-old.txt", "rename inside\n"),
        ("outside-source.txt", "outside source content\n"),
        ("outside.txt", "outside base\n"),
    ] {
        std::fs::write(repository.join(name), content).unwrap();
    }
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    git(&["switch", "-q", "-c", "feature"]);
    git(&["mv", "outside-source.txt", "registered/incoming.txt"]);
    std::fs::write(registered.join("inside.txt"), "committed inside\n").unwrap();
    std::fs::write(repository.join("outside.txt"), "committed outside\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "feature"]);
    git(&[
        "mv",
        "registered/rename-old.txt",
        "registered/rename-new.txt",
    ]);
    std::fs::write(registered.join("inside.txt"), "working inside\n").unwrap();
    std::fs::remove_file(registered.join("deleted.txt")).unwrap();
    std::fs::write(repository.join("outside.txt"), "working outside\n").unwrap();

    let registration = workspace::registration(
        registered.to_str().unwrap(),
        "Nested",
        workspace::LOCAL_DEVICE_ID,
    )
    .unwrap();
    let mut runtime = runtime();
    runtime.snapshot.ui_state.workspace_registrations = vec![registration];
    runtime.rebuild_catalog();
    let checkout = &mut runtime.snapshot.navigator.workspaces[0].checkouts[0];
    assert_eq!(checkout.path, repository.to_string_lossy());
    checkout.base_branch = Some("main".to_owned());
    // An occupied Herdr workspace is keyed by its Git root while the
    // registration retains the narrower folder as its actual scope.
    runtime.snapshot.navigator.workspaces[0].path = repository.to_string_lossy().into_owned();
    runtime.sync_changes_root_path();
    runtime.snapshot.ui_state.right_panel_visible = true;
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Changes;
    assert_eq!(
        runtime.snapshot.navigator.changes_root_path.as_deref(),
        registered.to_str()
    );
    let request = runtime.changes_request().expect("local History request");
    assert_eq!(request.root_path, registered);
    assert_eq!(request.checkout_path, repository);
    let mut reader = crate::changes::ChangesReader::new();
    let listed = reader.read_if_due(Some(request)).unwrap();
    assert_eq!(
        listed
            .entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<Vec<_>>(),
        ["deleted.txt", "inside.txt", "rename-new.txt"]
    );
    assert_eq!(
        listed
            .committed
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<Vec<_>>(),
        ["incoming.txt", "inside.txt"]
    );
    assert!(
        listed
            .entries
            .iter()
            .chain(&listed.committed)
            .all(|entry| entry.path.starts_with(registered.to_str().unwrap()))
    );
    assert!(
        listed
            .committed
            .iter()
            .find(|entry| entry.relative_path == "incoming.txt")
            .unwrap()
            .previous_relative_path
            .is_none()
    );
    assert!(runtime.ingest_changes(listed));

    let selected = registered
        .join("incoming.txt")
        .to_string_lossy()
        .into_owned();
    let event = serde_json::to_vec(&serde_json::json!({"schema_version":2,"kind":"changes_select","payload":{"path":selected,"committed":true,"preview":true}})).unwrap();
    assert!(runtime.dispatch_json(&event));
    let selected = reader.read_if_due(runtime.changes_request()).unwrap();
    let diff = selected.diff.expect("in-scope branch diff");
    assert!(diff.text.contains("outside source content"));
    assert!(!diff.text.contains("outside-source.txt"));
    assert!(!diff.text.contains("outside.txt"));

    let deleted = registered
        .join("deleted.txt")
        .to_string_lossy()
        .into_owned();
    let mut request = runtime.changes_request().unwrap();
    request.selected_path = Some(deleted);
    request.selected_committed = false;
    let deleted = reader.read_if_due(Some(request)).unwrap().diff.unwrap();
    assert!(deleted.text.contains("deleted content"));

    // A focus event publishes its new History identity in the same frame.
    // Retain the previous snapshot to model a coordinator read still in flight.
    let local = runtime.snapshot.navigator.workspaces[0].clone();
    let local_id = local.id.clone();
    let local_checkout_id = local.checkouts[0].id.clone();
    let mut remote = local.clone();
    remote.id = "remote-collision".to_owned();
    remote.remote_target_id = Some("remote-device".to_owned());
    remote.checkouts[0].id = "remote-checkout".to_owned();
    remote.checkouts[0].workspace_id = remote.id.clone();
    let mut sibling = local;
    sibling.id = "sibling-local".to_owned();
    sibling.path = repository.to_string_lossy().into_owned();
    sibling.registered = false;
    sibling.checkouts[0].id = "sibling-checkout".to_owned();
    sibling.checkouts[0].workspace_id = sibling.id.clone();
    runtime
        .snapshot
        .navigator
        .workspaces
        .extend([remote, sibling]);
    let focus = |runtime: &mut Runtime, workspace_id: &str, checkout_id: &str| {
        let event = serde_json::to_vec(&serde_json::json!({"schema_version":2,"kind":"focus_checkout","payload":{"workspace_id":workspace_id,"checkout_id":checkout_id}})).unwrap();
        assert!(runtime.dispatch_json(&event));
    };
    focus(&mut runtime, "remote-collision", "remote-checkout");
    assert!(runtime.snapshot.navigator.changes_root_path.is_none());
    assert!(runtime.changes_request().is_none());
    assert_eq!(
        runtime.snapshot.changes.root_path.as_deref(),
        registered.to_str()
    );
    focus(&mut runtime, "sibling-local", "sibling-checkout");
    assert_eq!(
        runtime.snapshot.navigator.changes_root_path.as_deref(),
        repository.to_str()
    );
    assert_eq!(runtime.changes_request().unwrap().root_path, repository);
    focus(&mut runtime, &local_id, &local_checkout_id);
    assert_eq!(
        runtime.snapshot.navigator.changes_root_path.as_deref(),
        registered.to_str()
    );

    // A delayed whole-state write can carry an older focus anchor. It must
    // update checkout, workspace and History identity together.
    let update_focus = |runtime: &mut Runtime, checkout_id: &str| {
        let event = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": [],
                "collapsed_workspace_ids": [],
                "focused_checkout_id": checkout_id,
                "right_panel_visible": true,
                "right_panel_section": "changes"
            }
        }))
        .unwrap();
        assert!(runtime.dispatch_json(&event));
    };
    focus(&mut runtime, "sibling-local", "sibling-checkout");
    update_focus(&mut runtime, &local_checkout_id);
    assert_eq!(
        runtime.snapshot.navigator.focused_workspace_id.as_deref(),
        Some(local_id.as_str())
    );
    assert_eq!(
        runtime.snapshot.navigator.root_path.as_deref(),
        repository.to_str()
    );
    assert_eq!(
        runtime.snapshot.navigator.changes_root_path.as_deref(),
        registered.to_str()
    );
    assert_eq!(runtime.changes_request().unwrap().root_path, registered);
    update_focus(&mut runtime, "remote-checkout");
    assert_eq!(
        runtime.snapshot.navigator.focused_workspace_id.as_deref(),
        Some("remote-collision")
    );
    assert!(runtime.snapshot.navigator.changes_root_path.is_none());
    assert!(runtime.changes_request().is_none());
}

#[test]
fn remote_checkout_with_a_local_path_collision_has_no_history_request() {
    let repository = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .arg("-C")
        .arg(repository.path())
        .args(["init", "-q"])
        .status()
        .unwrap();
    let registration = workspace::registration(
        repository.path().to_str().unwrap(),
        "Remote",
        "remote-device",
    )
    .unwrap();
    let mut runtime = runtime();
    runtime.snapshot.ui_state.workspace_registrations = vec![registration];
    runtime.rebuild_catalog();
    runtime.snapshot.ui_state.right_panel_visible = true;
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Changes;
    assert!(runtime.snapshot.navigator.root_path.is_some());
    assert!(runtime.snapshot.navigator.changes_root_path.is_none());
    assert!(runtime.changes_request().is_none());
}

/// The catalog before the worktree reader has answered.
/// Reconciling a session with a precomputed catalog runs no git at all.
///
/// Placing each tab into a checkout used to resolve the pane directory's
/// repository root with `git rev-parse` while the runtime lock was held,
/// once per tab per publish. The root index arrives with the catalog
/// instead, so the lock is never held across a subprocess.
#[test]
fn reconciling_with_a_precomputed_catalog_runs_no_git() {
    let base = workspace::temp_base_outside_any_repository();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let roots: Vec<std::path::PathBuf> = (0..3)
        .map(|index| base.join(format!("hide-no-git-reconcile-{stamp}-{index}")))
        .collect();
    for root in &roots {
        std::fs::create_dir_all(root).expect("fixture directory");
    }
    let cwds: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    let tabs: Vec<serde_json::Value> = (0..3)
        .map(|index| serde_json::json!({"workspace_id": "w1", "tab_id": format!("w1:t{index}"), "label": ""}))
        .collect();
    let panes: Vec<serde_json::Value> = (0..3)
        .map(|index| serde_json::json!({"pane_id": format!("w1:p{index}"), "cwd": cwds[index]}))
        .collect();
    let layouts: Vec<serde_json::Value> = (0..3)
        .map(|index| serde_json::json!({
            "workspace_id": "w1",
            "tab_id": format!("w1:t{index}"),
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": format!("w1:p{index}"),
            "panes": [{"pane_id": format!("w1:p{index}"), "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
            "splits": []
        }))
        .collect();
    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "workspaces": [{"workspace_id": "w1", "label": "three"}],
        "panes": panes,
        "tabs": tabs,
        "layouts": layouts,
    }))
    .expect("three-tab payload");
    let spaces = Runtime::session_spaces(&payload);
    let catalog = session_sync::PrecomputedCatalog {
        registrations: Vec::new(),
        workspaces: workspace::build_catalog(&[], &spaces, &no_worktrees()),
        roots: workspace::root_index(&spaces),
    };
    let mut runtime = runtime();

    let before = workspace::git_calls_on_this_thread();
    assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
    let after = workspace::git_calls_on_this_thread();

    assert_eq!(
        after - before,
        0,
        "the reconcile ran git under the runtime lock"
    );
    let placed: usize = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .map(|checkout| checkout.tabs.len())
        .sum();
    assert_eq!(placed, 3, "every tab landed in a checkout without git");
    assert!(
        !runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "catalog.root_unresolved"),
        "the root index carried every pane directory"
    );
    for root in &roots {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// A precomputed catalog whose registrations went stale between the
/// coordinator's read and the reconcile is not rebuilt under the lock: the
/// last accepted catalog stands, and a diagnostic says so.
#[test]
fn a_stale_precomputed_catalog_keeps_the_last_accepted_one() {
    let base = workspace::temp_base_outside_any_repository();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = base.join(format!("hide-stale-catalog-{stamp}"));
    std::fs::create_dir_all(&root).expect("fixture directory");
    let cwd = root.to_string_lossy().into_owned();
    let payload = || -> SessionSnapshotPayload {
        serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [{"workspace_id": "w1", "label": "one"}],
            "panes": [{"pane_id": "w1:p1", "cwd": cwd}],
            "tabs": [{"workspace_id": "w1", "tab_id": "w1:t1", "label": ""}],
            "layouts": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w1:p1",
                "panes": [{"pane_id": "w1:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }]
        }))
        .expect("one-tab payload")
    };
    let spaces = Runtime::session_spaces(&payload());
    let fresh = session_sync::PrecomputedCatalog {
        registrations: Vec::new(),
        workspaces: workspace::build_catalog(&[], &spaces, &no_worktrees()),
        roots: workspace::root_index(&spaces),
    };
    let mut runtime = runtime();
    assert!(runtime.ingest_session_with_catalog(Ok(payload()), Some(fresh)));
    let accepted = runtime.snapshot().navigator.workspaces.clone();
    assert!(!accepted.is_empty(), "the first catalog was accepted");

    let stale = session_sync::PrecomputedCatalog {
        registrations: vec![WorkspaceRegistration {
            id: "workspace:stale".to_owned(),
            label: "stale".to_owned(),
            path: cwd.clone(),
            device_id: workspace::LOCAL_DEVICE_ID.to_owned(),
            pinned: false,
        }],
        workspaces: Vec::new(),
        roots: workspace::RootIndex::new(),
    };
    let before = workspace::git_calls_on_this_thread();
    runtime.ingest_session_with_catalog(Ok(payload()), Some(stale));
    let after = workspace::git_calls_on_this_thread();

    assert_eq!(
        after - before,
        0,
        "a stale catalog was rebuilt under the runtime lock"
    );
    assert_eq!(
        runtime.snapshot().navigator.workspaces,
        accepted,
        "the last accepted catalog stands until the next publish"
    );
    assert!(
        runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "catalog.precomputed_stale"),
        "the stale catalog was reported"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn worktree_rows_sort_main_then_open_then_commit_time() {
    use crate::model::{ProjectWorktreesSnapshot, WorktreeCatalogSnapshot, WorktreeSnapshot};

    let mut runtime = runtime();
    let mut project = workspace(
        "workspace-1",
        "hide",
        "/repo",
        vec![
            checkout("workspace-1", "main", "/repo", None),
            checkout(
                "workspace-1",
                "open",
                "/repo.worktrees/open",
                Some(pane("w1:p1", "/repo.worktrees/open")),
            ),
        ],
    );
    project.is_git = true;
    runtime.snapshot.navigator.workspaces = vec![project];
    runtime.snapshot.navigator.focused_checkout_id = Some("main".to_owned());
    let row = |path: &str, branch: &str, is_main: bool, committed_at: u64| WorktreeSnapshot {
        path: path.to_owned(),
        branch: Some(branch.to_owned()),
        is_main,
        last_commit_unix_seconds: Some(committed_at),
        ..WorktreeSnapshot::default()
    };

    runtime.ingest_worktrees(WorktreeCatalogSnapshot {
        projects: vec![ProjectWorktreesSnapshot {
            root_path: "/repo".to_owned(),
            worktrees: vec![
                row("/repo.worktrees/old", "old", false, 20),
                row("/repo.worktrees/recent", "recent", false, 30),
                row("/repo.worktrees/open", "open", false, 10),
                row("/repo", "main", true, 1),
            ],
            ..ProjectWorktreesSnapshot::default()
        }],
    });

    let ordered = runtime
        .snapshot()
        .git_worktrees
        .as_ref()
        .expect("focused local project")
        .worktrees
        .iter()
        .map(|worktree| worktree.branch.as_deref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ordered, ["main", "open", "recent", "old"]);
    assert_eq!(
        runtime
            .snapshot()
            .git_worktrees
            .as_ref()
            .unwrap()
            .worktrees
            .iter()
            .filter(|worktree| worktree.is_main)
            .count(),
        1,
        "bare registrations never become rows"
    );
}

#[test]
fn sidebar_github_request_is_scoped_idempotent_and_does_not_move_focus() {
    let mut runtime = runtime();
    let mut project = workspace(
        "workspace-1",
        "hide",
        "/tmp/hide",
        vec![settled_worktree(crate::model::PullRequestBadge::Open)],
    );
    project.is_git = true;
    let mut other = workspace("workspace-2", "other", "/tmp/other", Vec::new());
    other.is_git = true;
    runtime.snapshot.navigator.workspaces = vec![project, other];
    let focus = runtime.snapshot.focused.clone();
    let event = |refresh| {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "github_request",
            "payload": {"workspace_id":"workspace-1", "refresh":refresh}
        }))
        .unwrap()
    };
    assert!(runtime.dispatch_json(&event(false)));
    assert!(!runtime.dispatch_json(&event(false)));
    assert_eq!(runtime.snapshot.focused, focus);
    assert_eq!(runtime.github_request().projects.len(), 1);
    assert_eq!(
        runtime.github_request().projects[0].root,
        PathBuf::from("/tmp/hide")
    );
    assert_eq!(runtime.github_request().projects[0].generation, 0);
    assert!(
        runtime.snapshot.navigator.workspaces[0].checkouts[0]
            .github
            .loading
    );
    assert!(runtime.dispatch_json(&event(true)));
    assert_eq!(runtime.github_request().projects[0].generation, 1);
}

/// Overview reads its focused project; explicit sidebar requests remain
/// independent of the retired right-panel Git tab.
#[test]
fn pull_requests_are_scoped_to_overview_and_explicit_sidebar_requests() {
    let mut runtime = runtime();
    let mut hide = workspace(
        "workspace-1",
        "hide",
        "/tmp/hide",
        vec![settled_worktree(crate::model::PullRequestBadge::Open)],
    );
    hide.is_git = true;
    runtime.snapshot.navigator.focused_checkout_id = Some(hide.checkouts[0].id.clone());
    let mut other = workspace("workspace-2", "other", "/tmp/other", Vec::new());
    other.is_git = true;
    runtime.snapshot.navigator.workspaces = vec![hide, other];
    let generations = |runtime: &Runtime| -> Vec<(String, u64)> {
        runtime
            .github_request()
            .projects
            .into_iter()
            .map(|project| {
                (
                    project.root.to_string_lossy().into_owned(),
                    project.generation,
                )
            })
            .collect()
    };
    assert_eq!(
        generations(&runtime),
        vec![("/tmp/hide".to_owned(), 0)],
        "Overview reads only its project"
    );
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Explorer;
    assert!(generations(&runtime).is_empty());
    assert!(!runtime.projected_card().github.loading);
    runtime.snapshot.ui_state.right_panel_visible = true;
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Overview;
    assert!(runtime.projected_card().github.loading);
    assert_eq!(generations(&runtime), vec![("/tmp/hide".to_owned(), 0)]);
    runtime.refresh_pull_requests("/tmp/hide");
    assert_eq!(generations(&runtime), vec![("/tmp/hide".to_owned(), 1)]);
    runtime.refresh_card();
    assert!(runtime.snapshot.card.github.loading);
    let leave_git = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ui_state_update",
        "payload": {
            "expanded_paths": [],
            "right_panel_section": "explorer",
            "focused_checkout_id": runtime.snapshot.navigator.focused_checkout_id,
        }
    }))
    .expect("leave Git event");
    assert!(runtime.dispatch_json(&leave_git));
    assert!(!runtime.snapshot.card.github.loading);
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Overview;
    runtime.snapshot.ui_state.right_panel_visible = false;
    assert!(!runtime.projected_card().github.loading);
}

/// The card consumes the same deletion gate as the sidebar and Git list.
/// It must not derive an older, card-only rule from pull-request state.
#[test]
fn the_card_projects_the_worktrees_shared_deletion_gate() {
    use crate::model::PullRequestBadge;
    let mut runtime = runtime();
    let mut checkout = settled_worktree(PullRequestBadge::Open);
    let gate = checkout.worktree.as_mut().expect("worktree");
    gate.deletion_gate.blocked_reason =
        Some("Commit or discard uncommitted changes first".to_owned());
    gate.deletion_gate.warnings = vec!["not pushed".to_owned()];
    gate.deletion_gate.button_label = "Close 2 panes and delete".to_owned();

    assert_eq!(
        card_for(&mut runtime, checkout).deletion_gate,
        Some(crate::model::WorktreeDeletionGateSnapshot {
            blocked_reason: Some("Commit or discard uncommitted changes first".to_owned()),
            warnings: vec!["not pushed".to_owned()],
            button_label: "Close 2 panes and delete".to_owned(),
            can_delete_branch: true,
        })
    );
}

/// The size shown must be this checkout's. Until the measurement for the
/// selected path arrives, the card says it is measuring rather than
/// showing the previous checkout's number.
#[test]
fn the_card_says_it_is_measuring_until_this_checkouts_size_arrives() {
    use crate::model::PullRequestBadge;
    let mut runtime = runtime();
    let card = card_for(&mut runtime, settled_worktree(PullRequestBadge::Merged));
    assert!(card.disk_measuring, "nothing has been measured yet");

    runtime.disk_usage = vec![crate::model::DiskUsageSnapshot {
        path: Some("/tmp/hide/somewhere-else".to_owned()),
        total_bytes: Some(4096),
        ..crate::model::DiskUsageSnapshot::default()
    }];
    let card = card_for(&mut runtime, settled_worktree(PullRequestBadge::Merged));
    assert!(
        card.disk_measuring,
        "another checkout's size is not this one's"
    );

    runtime.disk_usage = vec![crate::model::DiskUsageSnapshot {
        path: Some("/tmp/hide/feature".to_owned()),
        total_bytes: Some(4096),
        ..crate::model::DiskUsageSnapshot::default()
    }];
    let card = card_for(&mut runtime, settled_worktree(PullRequestBadge::Merged));
    assert!(!card.disk_measuring);
    assert_eq!(card.disk.total_bytes, Some(4096));
}

/// A failed lookup must not erase the pull requests it failed to replace:
/// the card shows the previous ones with how old they are, which is a
/// different thing from showing none.
#[test]
fn a_failed_lookup_keeps_the_pull_requests_it_could_not_refresh() {
    use crate::model::{
        GithubProjectSnapshot, GithubSnapshot, GithubStatusSnapshot, PullRequestBadge,
        PullRequestSnapshot,
    };
    let mut runtime = runtime();
    let pull_request = PullRequestSnapshot {
        closing_issues: Default::default(),
        title: "Fixture pull request".into(),
        checks: crate::model::PullRequestChecks::Unknown,
        number: 7,
        head_branch: "feature".to_owned(),
        base_branch: "main".to_owned(),
        url: "https://example.invalid/pull/7".to_owned(),
        badge: PullRequestBadge::Open,
        review: None,
        is_draft: false,
        merged_at_unix_ms: None,
        updated_at_unix_ms: None,
    };
    runtime.ingest_github(GithubSnapshot {
        projects: vec![GithubProjectSnapshot {
            issues: Default::default(),
            root_path: "/tmp/hide".to_owned(),
            status: GithubStatusSnapshot {
                available: true,
                last_success_at_unix_ms: Some(1_000),
                ..GithubStatusSnapshot::default()
            },
            pull_requests: vec![pull_request.clone()],
            pull_requests_read: true,
            issues_read: true,
        }],
    });

    runtime.ingest_github(GithubSnapshot {
        projects: vec![GithubProjectSnapshot {
            root_path: "/tmp/hide".to_owned(),
            status: GithubStatusSnapshot {
                available: true,
                stale: true,
                unavailable_reason: Some("gh pr list: network unreachable".to_owned()),
                ..GithubStatusSnapshot::default()
            },
            ..GithubProjectSnapshot::default()
        }],
    });

    let project = runtime
        .github
        .project("/tmp/hide")
        .expect("the project survives");
    assert_eq!(project.pull_requests, vec![pull_request.clone()]);
    assert!(project.status.stale);
    assert_eq!(project.status.last_success_at_unix_ms, Some(1_000));
    runtime.ingest_github(GithubSnapshot {
        projects: vec![GithubProjectSnapshot {
            root_path: "/tmp/hide".to_owned(),
            status: GithubStatusSnapshot {
                available: false,
                unavailable_reason: Some("gh is not logged in".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        }],
    });
    let project = runtime.github.project("/tmp/hide").unwrap();
    assert_eq!(project.pull_requests, vec![pull_request.clone()]);
    assert!(project.status.stale);
    assert!(!project.status.available);
    assert_eq!(project.status.last_success_at_unix_ms, Some(1_000));
    let mut fresh_pr = pull_request;
    fresh_pr.number = 8;
    runtime.ingest_github(GithubSnapshot {
        projects: vec![GithubProjectSnapshot {
            root_path: "/tmp/hide".into(),
            pull_requests: vec![fresh_pr.clone()],
            pull_requests_read: true,
            status: GithubStatusSnapshot {
                available: true,
                stale: true,
                unavailable_reason: Some("issue-only read failure".into()),
                ..Default::default()
            },
            ..Default::default()
        }],
    });
    assert_eq!(
        runtime.github.project("/tmp/hide").unwrap().pull_requests,
        vec![fresh_pr]
    );
}

#[test]
fn workspace_creation_failures_retire_inflight_and_keep_partial_registration_visible() {
    let mut runtime = runtime();
    let missing_path = "/tmp/hide-workspace-missing";
    runtime
        .workspace_creations_in_flight
        .insert(missing_path.to_owned());

    assert!(runtime.ingest_workspace_creation(
        missing_path,
        Err("Workspace path does not exist".to_owned()),
        4,
    ));
    assert!(!runtime.workspace_creations_in_flight.contains(missing_path));
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("workspace.create_failed")
    );

    let partial_path = "/tmp/hide-workspace-partial";
    let registration = WorkspaceRegistration {
        id: "workspace:partial".to_owned(),
        label: "Partial".to_owned(),
        path: partial_path.to_owned(),
        device_id: workspace::LOCAL_DEVICE_ID.to_owned(),
        pinned: false,
    };
    runtime
        .workspace_creations_in_flight
        .insert(partial_path.to_owned());

    assert!(
        runtime.ingest_workspace_creation(
            partial_path,
            Ok(live::WorkspaceCreationOutcome {
                registration: registration.clone(),
                base_registrations: Vec::new(),
                registrations: vec![registration.clone()],
                workspaces: Vec::new(),
                session: serde_json::from_value(serde_json::json!({
                    "agents": [],
                    "layouts": [],
                }))
                .expect("empty session payload"),
                created_pane_id: None,
                git_init_error: Some("git init failed explicitly".to_owned()),
            }),
            7,
        )
    );
    assert!(!runtime.workspace_creations_in_flight.contains(partial_path));
    assert_eq!(
        runtime.snapshot.ui_state.workspace_registrations,
        [registration]
    );
    let error = runtime
        .snapshot
        .status
        .last_error
        .as_ref()
        .expect("partial failure stays visible");
    assert_eq!(error.kind, "workspace.git_init_failed");
    assert!(error.message.contains("registered"));
    assert!(error.message.contains("git init failed explicitly"));
}

/// A registered project with panes, as `Remove project…` sees it: the
/// session that `context_payload` describes, with its Alpha workspace
/// registered so the row is a registration rather than a temporary folder.
fn registered_context_runtime() -> (Runtime, WorkspaceRegistration) {
    let mut runtime = live_runtime();
    let registration =
        workspace::registration("/tmp/hide-context-alpha", "Alpha", "local").unwrap();
    runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
    runtime.ingest_session(Ok(context_payload()));
    (runtime, registration)
}

fn remove_event(workspace_id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "remove_workspace",
        "payload": {"workspace_id": workspace_id}
    }))
    .unwrap()
}

/// B11, B12, B14. A project with panes is not refused: the confirmation
/// counts come from the snapshot, the registration survives until Herdr
/// confirms the closes, a timeout keeps it with the reason visible, and a
/// repeat while the close runs starts nothing. Only the confirmation removes
/// the row, and files are never touched.
#[test]
fn removing_a_registration_with_panes_closes_them_and_removes_only_on_confirmation() {
    let (mut runtime, registration) = registered_context_runtime();
    let alpha = runtime
        .snapshot
        .navigator
        .workspaces
        .iter()
        .find(|workspace| workspace.id == registration.id)
        .expect("the registered project row");
    assert_eq!(alpha.removal.pane_count, 1);
    assert_eq!(alpha.removal.running_agent_count, 1);

    assert!(runtime.dispatch_json(&remove_event(&registration.id)));
    assert_eq!(
        runtime.snapshot.ui_state.workspace_registrations,
        vec![registration.clone()],
        "the registration waits for Herdr's confirmation"
    );
    assert!(runtime.snapshot.status.last_error.is_none());
    assert!(
        !runtime.dispatch_json(&remove_event(&registration.id)),
        "a repeat while the close is in flight is a no-op"
    );

    assert!(runtime.ingest_workspace_close_result(
        &registration.id,
        Err("Timed out waiting for Herdr to confirm closed panes: w1:p1".to_owned())
    ));
    assert_eq!(
        runtime.snapshot.ui_state.workspace_registrations,
        vec![registration.clone()],
        "a timeout leaves the project registered"
    );
    let error = runtime.snapshot.status.last_error.clone().unwrap();
    assert_eq!(error.kind, "workspace.remove_failed");
    assert!(error.message.contains("Timed out"));
    assert!(
        !runtime.ingest_workspace_close_result(&registration.id, Ok(())),
        "a late answer for a request that already failed authorizes nothing"
    );
    assert_eq!(
        runtime.snapshot.ui_state.workspace_registrations,
        vec![registration.clone()]
    );

    // Retry: the same event starts over from the panes that remain.
    runtime.snapshot.status.last_error = None;
    assert!(runtime.dispatch_json(&remove_event(&registration.id)));
    assert!(runtime.ingest_workspace_close_result(&registration.id, Ok(())));
    assert!(runtime.snapshot.ui_state.workspace_registrations.is_empty());
    assert!(
        !runtime
            .snapshot
            .navigator
            .workspaces
            .iter()
            .any(|workspace| workspace.id == registration.id)
    );
    assert!(runtime.snapshot.status.last_error.is_none());
    assert!(
        !runtime.dispatch_json(&remove_event(&registration.id)),
        "the completed removal repeats quietly"
    );
}

/// B12. Removing the project the operator is looking at takes its focus and
/// pane selection with it: the next project comes forward and no sync tick
/// reports the closed pane as "not available for the selected checkout".
#[test]
fn removing_the_focused_project_moves_focus_off_its_closed_panes() {
    let (mut runtime, registration) = registered_context_runtime();
    let alpha_checkout_id = runtime
        .snapshot
        .navigator
        .workspaces
        .iter()
        .find(|workspace| workspace.id == registration.id)
        .and_then(|workspace| workspace.checkouts.first())
        .map(|checkout| checkout.id.clone())
        .expect("alpha checkout");
    runtime.snapshot.ui_state.focused_checkout_id = Some(alpha_checkout_id.clone());
    runtime.snapshot.navigator.focused_checkout_id = Some(alpha_checkout_id.clone());
    runtime.snapshot.ui_state.selected_pane_id = Some("w1:p1".to_owned());
    runtime.snapshot.terminal.pane_id = Some("w1:p1".to_owned());
    runtime.snapshot.focused.pane_id = Some("w1:p1".to_owned());

    assert!(runtime.dispatch_json(&remove_event(&registration.id)));
    assert!(runtime.ingest_workspace_close_result(&registration.id, Ok(())));

    assert!(runtime.snapshot.ui_state.workspace_registrations.is_empty());
    assert_ne!(
        runtime.snapshot.navigator.focused_checkout_id.as_deref(),
        Some(alpha_checkout_id.as_str()),
        "focus does not stay on a checkout no catalog carries"
    );
    assert_ne!(
        runtime.snapshot.ui_state.focused_checkout_id.as_deref(),
        Some(alpha_checkout_id.as_str())
    );
    assert_ne!(runtime.snapshot.terminal.pane_id.as_deref(), Some("w1:p1"));
    assert_ne!(
        runtime.snapshot.ui_state.selected_pane_id.as_deref(),
        Some("w1:p1")
    );
    assert!(runtime.snapshot.status.last_error.is_none());
}

/// A registration id is keyed by its path, so adding the folder back while
/// its removal is still closing panes would name the entry the retire is
/// about to delete. Each side refuses the other with a visible reason, and a
/// creation that lands anyway cancels the removal rather than losing the
/// project it just opened a pane in.
#[test]
fn adding_a_folder_while_its_removal_closes_panes_is_refused_and_a_landed_add_cancels_it() {
    let (mut runtime, registration) = registered_context_runtime();
    assert!(runtime.dispatch_json(&remove_event(&registration.id)));
    assert!(
        runtime
            .workspace_removals_in_flight
            .contains(&registration.id)
    );

    let create = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "create_workspace",
        "payload": {"path": registration.path, "label": "Alpha again", "initialize_git": false}
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&create));
    let error = runtime.snapshot.status.last_error.clone().unwrap();
    assert_eq!(error.kind, "workspace.remove_in_flight");
    assert!(
        !runtime
            .workspace_creations_in_flight
            .contains(&registration.path),
        "the refused add starts no worker"
    );

    // The back-stop: a creation for the same id that got past the front
    // door (another spelling of the path) lands while the close is pending.
    runtime.snapshot.status.last_error = None;
    runtime
        .workspace_creations_in_flight
        .insert("/tmp/./hide-context-alpha".to_owned());
    assert!(runtime.ingest_workspace_creation(
        "/tmp/./hide-context-alpha",
        Ok(live::WorkspaceCreationOutcome {
            registration: registration.clone(),
            base_registrations: vec![registration.clone()],
            registrations: vec![registration.clone()],
            workspaces: Vec::new(),
            session: context_payload(),
            created_pane_id: Some("w1:p1".to_owned()),
            git_init_error: None,
        }),
        3,
    ));
    let error = runtime.snapshot.status.last_error.clone().unwrap();
    assert_eq!(error.kind, "workspace.remove_cancelled");
    assert!(
        !runtime
            .workspace_removals_in_flight
            .contains(&registration.id)
    );
    assert!(
        !runtime.ingest_workspace_close_result(&registration.id, Ok(())),
        "the close's late answer authorizes nothing"
    );
    assert_eq!(
        runtime.snapshot.ui_state.workspace_registrations,
        vec![registration.clone()],
        "the project stays registered"
    );
}

/// The mirror: removing a project whose creation is still opening its first
/// pane would be undone when that creation lands, so it is refused.
#[test]
fn removing_a_project_whose_creation_is_in_flight_is_refused() {
    let (mut runtime, registration) = registered_context_runtime();
    runtime
        .workspace_creations_in_flight
        .insert(registration.path.clone());

    assert!(runtime.dispatch_json(&remove_event(&registration.id)));
    let error = runtime.snapshot.status.last_error.clone().unwrap();
    assert_eq!(error.kind, "workspace.create_in_flight");
    assert!(
        !runtime
            .workspace_removals_in_flight
            .contains(&registration.id)
    );
    assert_eq!(
        runtime.snapshot.ui_state.workspace_registrations,
        vec![registration]
    );
}

/// B14. Without a live Herdr connection the panes cannot be closed, so the
/// registration stays and the banner says why instead of a half-removed row.
#[test]
fn removing_a_registration_with_panes_needs_a_live_connection() {
    let mut runtime = runtime();
    let registration =
        workspace::registration("/tmp/hide-context-alpha", "Alpha", "local").unwrap();
    runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
    runtime.ingest_session(Ok(context_payload()));

    assert!(runtime.dispatch_json(&remove_event(&registration.id)));

    assert_eq!(
        runtime.snapshot.ui_state.workspace_registrations,
        vec![registration]
    );
    let error = runtime.snapshot.status.last_error.clone().unwrap();
    assert_eq!(error.kind, "workspace.remove_failed");
    assert!(error.message.contains("live Herdr connection"));
}

/// B2, B4, B5, B10. Pinning moves the registered row ahead of its device's
/// activity order, carries the flag on both the row and the registration
/// that persists it, and unpinning puts it back. The same value again
/// changes nothing; an id with no registration is refused, not ignored.
#[test]
fn pinning_a_registration_reorders_the_row_and_persists_the_flag() {
    let mut runtime = runtime();
    let alpha = workspace::registration("/tmp/hide-context-alpha", "Alpha", "local").unwrap();
    let zeta = workspace::registration("/tmp/hide-context-zeta", "Zeta", "local").unwrap();
    runtime.snapshot.ui_state.workspace_registrations = vec![alpha.clone(), zeta.clone()];
    runtime.ingest_session(Ok(context_payload()));
    let ids = |runtime: &Runtime| {
        runtime
            .snapshot
            .navigator
            .workspaces
            .iter()
            .map(|workspace| workspace.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&runtime), [zeta.id.clone(), alpha.id.clone()]);
    let pin = |workspace_id: &str, pinned: bool| {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "workspace_pin_set",
            "payload": {"workspace_id": workspace_id, "pinned": pinned}
        }))
        .unwrap()
    };

    assert!(runtime.dispatch_json(&pin(&alpha.id, true)));
    assert_eq!(ids(&runtime), [alpha.id.clone(), zeta.id.clone()]);
    assert!(runtime.snapshot.navigator.workspaces[0].pinned);
    assert!(!runtime.snapshot.navigator.workspaces[1].pinned);
    assert_eq!(
        runtime.snapshot.ui_state.workspace_registrations[0],
        WorkspaceRegistration {
            pinned: true,
            ..alpha.clone()
        }
    );
    let saved = std::fs::read_to_string(&runtime.state_path).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved["workspace_registrations"][0]["pinned"], true);
    assert_eq!(saved["workspace_registrations"][1]["pinned"], false);

    assert!(
        !runtime.dispatch_json(&pin(&alpha.id, true)),
        "the same value is a no-op"
    );
    assert!(runtime.snapshot.status.last_error.is_none());

    // The next session publish carries the pin through the catalog too.
    runtime.ingest_session(Ok(context_payload()));
    assert_eq!(ids(&runtime), [alpha.id.clone(), zeta.id.clone()]);
    assert!(runtime.snapshot.navigator.workspaces[0].pinned);

    assert!(runtime.dispatch_json(&pin(&alpha.id, false)));
    assert_eq!(ids(&runtime), [zeta.id.clone(), alpha.id.clone()]);
    assert!(!runtime.snapshot.navigator.workspaces[1].pinned);
    assert!(!runtime.snapshot.ui_state.workspace_registrations[0].pinned);

    assert!(runtime.dispatch_json(&pin("workspace:missing", true)));
    assert_eq!(
        runtime.snapshot.status.last_error.as_ref().unwrap().kind,
        "workspace.pin_unregistered"
    );
    assert!(
        runtime
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .all(|registration| !registration.pinned)
    );
}

#[test]
fn removing_registration_converges_without_git_or_repeat_publication() {
    let mut runtime = runtime();
    let path = std::env::temp_dir().join(format!("hide-registration-empty-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let registration =
        workspace::registration(path.to_str().unwrap(), "Empty project", "local").unwrap();
    runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
    runtime.rebuild_catalog();
    let event = serde_json::to_vec(&serde_json::json!({
        "schema_version": 2, "kind": "remove_workspace", "payload": {"workspace_id": registration.id}
    })).unwrap();
    let git_before = workspace::git_calls_on_this_thread();
    assert!(runtime.dispatch_json(&event));
    assert!(runtime.snapshot.ui_state.workspace_registrations.is_empty());
    assert!(runtime.snapshot.navigator.workspaces.is_empty());
    assert_eq!(workspace::git_calls_on_this_thread(), git_before);
    assert!(
        !runtime.dispatch_json(&event),
        "The same target state is already reached"
    );
    assert!(runtime.snapshot.status.last_error.is_none());
    assert!(path.is_dir());
    std::fs::remove_dir(path).unwrap();
}

#[test]
fn cleanup_review_without_live_state_is_visible_and_never_deletes() {
    let mut runtime = runtime();
    runtime.ingest_session(Ok(context_payload()));
    let project = runtime.snapshot.navigator.workspaces[0].clone();
    runtime.focus_checkout(&project.id, &project.checkouts[0].id);
    let root = runtime.focused_local_checkout().unwrap().0.path.clone();
    runtime.ingest_worktrees(crate::model::WorktreeCatalogSnapshot {
        projects: vec![crate::model::ProjectWorktreesSnapshot {
            root_path: root,
            ..Default::default()
        }],
    });
    runtime.dispatch_json(br#"{"schema_version":2,"kind":"cleanup_review","payload":{}}"#);
    let snapshot = serde_json::to_value(runtime.snapshot()).unwrap();
    assert_eq!(snapshot["git_worktrees"]["cleanup"]["phase"], "failed");
    assert!(
        snapshot["git_worktrees"]["cleanup"]["message"]
            .as_str()
            .unwrap()
            .contains("live Herdr connection")
    );
    runtime.dispatch_json(br#"{"schema_version":2,"kind":"cleanup_dismiss","payload":{}}"#);
    assert!(
        serde_json::to_value(runtime.snapshot()).unwrap()["git_worktrees"]["cleanup"].is_null()
    );
}

/// B11, D-12. `Open in History` on another checkout's header is one event:
/// the checkout comes forward and the panel is on History afterwards, and
/// the same event on the focused checkout only moves the section. A path
/// the navigator does not list moves nothing and records the error.
#[test]
fn overview_open_section_focuses_the_checkout_and_switches_the_panel_in_one_event() {
    let mut runtime = runtime();
    runtime.ingest_session(Ok(context_payload()));
    let alpha = runtime.snapshot.navigator.workspaces[1].clone();
    let zeta = runtime.snapshot.navigator.workspaces[0].clone();
    runtime.focus_checkout(&zeta.id, &zeta.checkouts[0].id);
    runtime.snapshot.ui_state.right_panel_visible = false;
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Overview;
    let event = |path: &str, section: &str| {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "overview_open_section",
            "payload": {"checkout_path": path, "section": section}
        }))
        .unwrap()
    };
    assert!(runtime.dispatch_json(&event(&alpha.checkouts[0].path, "changes")));
    assert_eq!(
        runtime.snapshot.navigator.focused_checkout_id.as_deref(),
        Some(alpha.checkouts[0].id.as_str())
    );
    assert!(runtime.snapshot.ui_state.right_panel_visible);
    assert_eq!(
        runtime.snapshot.ui_state.right_panel_section,
        RightPanelSection::Changes
    );
    assert!(runtime.snapshot.status.last_error.is_none());

    let focused = runtime.snapshot.focused.clone();
    let navigator = runtime.snapshot.navigator.clone();
    let ui = runtime.snapshot.ui_state.clone();
    assert!(runtime.dispatch_json(&event("/tmp/hide-context-nowhere", "changes")));
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("overview.unknown_checkout")
    );
    assert_eq!(runtime.snapshot.focused, focused);
    assert_eq!(runtime.snapshot.navigator, navigator);
    assert_eq!(runtime.snapshot.ui_state, ui);
}

/// B13, D-11. `New agent here` is one task operation in the slot the
/// worktree sheet uses: the provider rides on it for the shell to start,
/// a provider Hide has no launcher for is refused before anything is
/// requested, and without a live Herdr the operation fails in place rather
/// than hanging in `working`.
#[test]
fn agent_start_in_checkout_reports_through_the_task_operation_slot() {
    let mut runtime = runtime();
    runtime.ingest_session(Ok(context_payload()));
    let checkout = runtime.snapshot.navigator.workspaces[0].checkouts[0].clone();
    let event = |path: &str, provider: &str| {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "agent_start_in_checkout",
            "payload": {"checkout_path": path, "provider": provider}
        }))
        .unwrap()
    };
    assert!(runtime.dispatch_json(&event(&checkout.path, "gemini")));
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("agent_start.unknown_provider")
    );
    assert!(runtime.snapshot.task_operation.is_none());

    assert!(runtime.dispatch_json(&event(&checkout.path, "claude")));
    let operation = runtime.snapshot.task_operation.clone().expect("operation");
    assert_eq!(operation.kind, "agent_start");
    assert_eq!(operation.agent_kind.as_deref(), Some("claude"));
    assert_eq!(operation.phase, "failed");
    assert!(
        operation
            .message
            .as_deref()
            .is_some_and(|message| message.contains("live Herdr connection"))
    );
    assert_eq!(operation.pane_id, None);
}

#[test]
fn projects_follow_authoritative_activity_and_identical_snapshots_settle() {
    let mut runtime = runtime();
    let mut payload = context_payload();
    runtime.ingest_session(Ok(payload.clone()));
    assert!(
        runtime.snapshot.navigator.workspaces[0]
            .path
            .ends_with("/hide-context-zeta")
    );
    runtime.ingest_session(Ok(payload.clone()));
    let before = runtime.snapshot_delta_payload(0, 0);
    let revision = before.revision;
    for _ in 0..5 {
        runtime.ingest_session(Ok(payload.clone()));
        let delta = runtime.snapshot_delta_payload(revision, 0);
        assert!(
            delta.rest.is_none(),
            "Unchanged activity must not resend the navigator"
        );
    }
    payload.agents[0]
        .tokens
        .insert("activity".into(), serde_json::json!("1788873000000"));
    runtime.ingest_session(Ok(payload));
    assert!(
        runtime.snapshot.navigator.workspaces[0]
            .path
            .ends_with("/hide-context-alpha")
    );
}

#[test]
fn branch_migration_receipt_preserves_core_focus() {
    let mut runtime = runtime();
    runtime.snapshot.terminal.pane_id = Some("existing:pane".into());
    runtime.snapshot.focused.surface = Surface::Terminal;
    runtime.snapshot.focused.pane_id = Some("existing:pane".into());
    runtime.snapshot.ui_state.selected_pane_id = Some("existing:pane".into());
    let id = runtime
        .begin_task_operation(
            "branch_migrate",
            Some("/fixture/repo".into()),
            Some("feature".into()),
            Some("main".into()),
            None,
        )
        .unwrap();

    assert!(runtime.ingest_task_operation_result(
        id,
        Ok(live::WorktreeTaskOutcome {
            path: "/fixture/worktree".into(),
            pane_id: "new:pane".into(),
            purpose_error: None,
            unconfirmed_purpose_token: None,
        })
    ));
    assert_eq!(
        runtime.snapshot.terminal.pane_id.as_deref(),
        Some("existing:pane")
    );
    assert_eq!(
        runtime.snapshot.focused.pane_id.as_deref(),
        Some("existing:pane")
    );
    assert_eq!(
        runtime.snapshot.ui_state.selected_pane_id.as_deref(),
        Some("existing:pane")
    );
}

/// B16 and B19. A created checkout starts collapsed, and a purpose mirror
/// failure is diagnostic detail on an otherwise ready creation receipt.
#[test]
fn created_worktree_starts_collapsed_and_keeps_purpose_failure_non_blocking() {
    let mut runtime = runtime();
    let state_path = runtime.state_path.clone();
    let path = "/fixture/repo.worktrees/topic";
    let mut created = checkout("workspace-fixture", "checkout-created", path, None);
    created.purpose = Some(crate::model::CheckoutPurposeSnapshot {
        text: "Unconfirmed creation purpose".to_owned(),
        origin: crate::model::CheckoutPurposeOrigin::Token,
    });
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "workspace-fixture",
        "Fixture",
        "/fixture/repo",
        vec![
            checkout("workspace-fixture", "checkout-main", "/fixture/repo", None),
            created,
        ],
    )];
    let id = runtime
        .begin_task_operation(
            "worktree_create",
            Some("/fixture/repo".to_owned()),
            Some("topic".to_owned()),
            Some("main".to_owned()),
            None,
        )
        .expect("creation operation");
    runtime.begin_created_purpose_write(path, "Unconfirmed creation purpose");
    assert_eq!(
        runtime
            .unconfirmed_created_purpose_values()
            .get(&workspace::normalized_for_comparison(Path::new(path)))
            .map(String::as_str),
        Some("Unconfirmed creation purpose"),
        "the mirror sees the suppression before the token write can publish"
    );
    assert!(runtime.ingest_task_operation_result(
        id,
        Ok(live::WorktreeTaskOutcome {
            path: path.to_owned(),
            pane_id: "w-created:p1".to_owned(),
            purpose_error: Some("injected purpose mirror failure".to_owned()),
            unconfirmed_purpose_token: Some("Unconfirmed creation purpose".to_owned()),
        })
    ));

    assert!(
        runtime.snapshot.navigator.workspaces[0].checkouts[1]
            .purpose
            .is_none(),
        "a token whose compensating clear failed is hidden behind the row fallback"
    );
    assert_eq!(
        runtime
            .unconfirmed_created_purposes
            .get(&workspace::normalized_for_comparison(Path::new(path)))
            .map(String::as_str),
        Some("Unconfirmed creation purpose")
    );
    assert!(runtime.created_purpose_writes_in_flight.is_empty());

    let created_id = workspace::checkout_id_for_path("workspace-fixture", Path::new(path));
    assert!(
        runtime
            .snapshot
            .ui_state
            .collapsed_checkout_ids
            .contains(&created_id),
        "the created row starts collapsed before the catalog refresh arrives"
    );
    let operation = runtime.snapshot.task_operation.as_ref().unwrap();
    assert_eq!(operation.phase, "ready");
    assert!(operation.message.is_none());
    assert!(
        runtime
            .snapshot
            .status
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.kind == "checkout_purpose.create_failed"
                    && diagnostic
                        .message
                        .contains("injected purpose mirror failure")
            })
    );
    let _ = std::fs::remove_file(state_path);
}

/// B18. Unicode-scalar validation matches the Swift sheet and fails inside
/// the caller-visible task operation instead of publishing a detached alert.
#[test]
fn invalid_purpose_fails_the_sheet_operation_with_the_shared_scalar_limit() {
    let mut runtime = runtime();
    let mut local = workspace(
        "workspace-purpose",
        "Purpose",
        "/fixture/purpose",
        vec![checkout(
            "workspace-purpose",
            "checkout-purpose",
            "/fixture/purpose",
            None,
        )],
    );
    local.checkouts[0].branch = Some("topic".to_owned());
    runtime.snapshot.navigator.workspaces = vec![local];

    assert!(runtime.set_checkout_purpose(SetCheckoutPurposePayload {
        checkout_id: "checkout-purpose".to_owned(),
        text: "a".repeat(81),
    }));

    let operation = runtime.snapshot.task_operation.as_ref().expect("operation");
    assert_eq!(operation.kind, "checkout_purpose");
    assert_eq!(operation.phase, "failed");
    assert_eq!(
        operation.message.as_deref(),
        Some("Purpose must be one line of 80 characters or fewer")
    );
    assert!(
        runtime
            .snapshot
            .status
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.kind == "checkout_purpose.invalid" })
    );
}

/// B5, B12. The two fold events own independent persisted keys and update
/// the snapshot immediately. Repeating each toggle converges back to the
/// default collapsed state without changing project disclosure.
#[test]
fn inactive_fold_events_toggle_project_path_and_device_state_independently() {
    let mut runtime = runtime();
    let path = "/tmp/hide-runtime-inactive";
    let settled = |id: &str, checkout_path: &str, is_worktree: bool| CheckoutSnapshot {
        id: id.to_owned(),
        workspace_id: "workspace-inactive".to_owned(),
        label: id.to_owned(),
        path: checkout_path.to_owned(),
        is_worktree,
        worktree: Some(crate::model::WorktreeSnapshot {
            merged: Some(true),
            ..Default::default()
        }),
        ..CheckoutSnapshot::default()
    };
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "workspace-inactive",
        "Inactive",
        path,
        vec![
            settled("primary", path, false),
            settled("secondary", "/tmp/hide-runtime-inactive-secondary", true),
        ],
    )];
    runtime.refresh_inactive_groups();
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .inactive_checkouts
            .checkout_ids,
        ["secondary"]
    );
    assert_eq!(runtime.snapshot.navigator.inactive_projects.len(), 1);

    let checkout_toggle = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "inactive_checkouts_toggle",
        "payload": { "project_path": path }
    }))
    .unwrap();
    let project_toggle = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "inactive_projects_toggle",
        "payload": { "device_id": "local" }
    }))
    .unwrap();

    assert!(runtime.dispatch_json(&checkout_toggle));
    assert!(
        runtime.snapshot.navigator.workspaces[0]
            .inactive_checkouts
            .expanded
    );
    assert_eq!(
        runtime
            .snapshot
            .ui_state
            .expanded_inactive_checkout_project_paths,
        [path]
    );
    assert!(runtime.dispatch_json(&project_toggle));
    assert!(runtime.snapshot.navigator.inactive_projects[0].expanded);
    assert_eq!(
        runtime
            .snapshot
            .ui_state
            .expanded_inactive_project_device_ids,
        ["local"]
    );

    assert!(runtime.dispatch_json(&checkout_toggle));
    assert!(runtime.dispatch_json(&project_toggle));
    assert!(
        runtime
            .snapshot
            .ui_state
            .expanded_inactive_checkout_project_paths
            .is_empty()
    );
    assert!(
        runtime
            .snapshot
            .ui_state
            .expanded_inactive_project_device_ids
            .is_empty()
    );
}

#[test]
fn a_pane_in_a_second_directory_projects_into_its_own_project() {
    let mut runtime = runtime();
    let repository_path = "/private/tmp/hide-rebrand/herdr-ide";
    let checkout_path = "/private/tmp/hide-rebrand/worktrees/hide-rebrand";
    // Neither path is a git repository here, so each is its own
    // project: a Herdr workspace spanning two directories is two rows,
    // each keyed by its own path. (A real worktree folds into its main
    // repository's project; `workspace::tests` covers that with git.)
    let spaces = vec![workspace::SessionSpace {
        id: "w3M".to_owned(),
        label: "herdr-ide".to_owned(),
        purpose: None,
        cwds: vec![repository_path.to_owned(), checkout_path.to_owned()],
    }];
    let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
    let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
    runtime.snapshot.navigator.workspaces = workspace::build_catalog(&[], &spaces, &no_worktrees());
    runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
    runtime.reset_terminal_projection(None);

    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "workspaces": [{"workspace_id": "w3M", "label": "herdr-ide"}],
        "panes": [{"pane_id": "w3M:p1", "cwd": checkout_path}],
        "tabs": [{"workspace_id": "w3M", "tab_id": "w3M:t1", "label": ""}],
        "layouts": [{
            "workspace_id": "w3M",
            "tab_id": "w3M:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": "w3M:p1",
            "panes": [{"pane_id": "w3M:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
            "splits": []
        }]
    }))
    .expect("second directory pane payload");
    let catalog = session_sync::PrecomputedCatalog {
        registrations: Vec::new(),
        workspaces: workspace::build_catalog(
            &[],
            &spaces,
            &crate::model::WorktreeCatalogSnapshot::default(),
        ),
        roots: workspace::root_index(&spaces),
    };

    assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
    assert_eq!(runtime.snapshot().navigator.workspaces.len(), 2);
    let workspace_snapshot = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .find(|workspace| workspace.id == workspace_id)
        .expect("the second directory's project")
        .clone();
    assert_eq!(workspace_snapshot.label, "hide-rebrand");
    assert_eq!(
        workspace_snapshot.session_workspace_ids,
        vec!["w3M".to_owned()]
    );
    assert_eq!(workspace_snapshot.checkouts.len(), 1);
    let checkout = &workspace_snapshot.checkouts[0];
    assert_eq!(checkout.id, checkout_id);
    assert_eq!(checkout.tabs.len(), 1);
    assert_eq!(checkout.tabs[0].panes[0].id, "w3M:p1");
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w3M:p1")
    );
}

#[test]
fn another_workspace_layout_does_not_steal_the_selected_checkout_projection() {
    let mut runtime = runtime();
    let checkout_path = "/private/tmp/hide-selected-checkout";
    let spaces = vec![
        workspace::SessionSpace {
            id: "w3P".to_owned(),
            label: "other".to_owned(),
            purpose: None,
            cwds: vec![checkout_path.to_owned()],
        },
        workspace::SessionSpace {
            id: "w3Z".to_owned(),
            label: "selected".to_owned(),
            purpose: None,
            cwds: vec![checkout_path.to_owned()],
        },
    ];
    // Two Herdr workspaces in one directory are one project; the selected
    // pane, not the Herdr workspace id, decides which layout is projected.
    let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
    let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
    runtime.snapshot.navigator.workspaces = workspace::build_catalog(&[], &spaces, &no_worktrees());
    runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.ui_state.selected_pane_id = Some("w3Z:p1".to_owned());
    runtime.snapshot.terminal.pane_id = Some("w3Z:p1".to_owned());
    runtime.restore_hint_pending = false;

    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "workspaces": [
            {"workspace_id": "w3P", "label": "other"},
            {"workspace_id": "w3Z", "label": "selected"}
        ],
        "panes": [
            {"pane_id": "w3P:p1", "cwd": checkout_path},
            {"pane_id": "w3Z:p1", "cwd": checkout_path}
        ],
        "tabs": [{"workspace_id": "w3P", "tab_id": "w3P:t1", "label": ""}, {"workspace_id": "w3Z", "tab_id": "w3Z:t1", "label": ""}],
        "layouts": [
            {
                "workspace_id": "w3P",
                "tab_id": "w3P:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w3P:p1",
                "panes": [{"pane_id": "w3P:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            },
            {
                "workspace_id": "w3Z",
                "tab_id": "w3Z:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w3Z:p1",
                "panes": [{"pane_id": "w3Z:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }
        ]
    }))
    .expect("two workspace layout payload");
    let catalog = session_sync::PrecomputedCatalog {
        registrations: Vec::new(),
        workspaces: workspace::build_catalog(
            &[],
            &spaces,
            &crate::model::WorktreeCatalogSnapshot::default(),
        ),
        roots: workspace::RootIndex::new(),
    };
    assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));

    let selected_checkout = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .find(|workspace| workspace.id == workspace_id)
        .and_then(|workspace| {
            workspace
                .checkouts
                .iter()
                .find(|checkout| checkout.id == checkout_id)
        })
        .expect("the selected checkout")
        .clone();
    assert!(selected_checkout.tabs.iter().any(|tab| {
        tab.id.as_deref() == Some("w3Z:t1") && tab.panes.iter().any(|pane| pane.id == "w3Z:p1")
    }));
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w3Z:p1")
    );
    assert_eq!(
        runtime
            .snapshot()
            .active_pane_layout()
            .map(|layout| (layout.workspace_id.as_str(), layout.tab_id.as_str())),
        Some(("w3Z", "w3Z:t1"))
    );
}

#[test]
fn a_registration_herdr_already_has_a_workspace_for_is_listed_once() {
    let checkout_path = "/private/tmp/hide-duplicate-registration";
    let spaces = vec![workspace::SessionSpace {
        id: "w41".to_owned(),
        label: "duplicate".to_owned(),
        purpose: None,
        cwds: vec![checkout_path.to_owned()],
    }];
    let registrations = vec![WorkspaceRegistration {
        id: workspace::workspace_id_for_path(Path::new(checkout_path)),
        label: "Duplicate".to_owned(),
        path: checkout_path.to_owned(),
        device_id: "local".to_owned(),
        pinned: false,
    }];

    let catalog = workspace::build_catalog(&registrations, &spaces, &no_worktrees());

    // The registration is the row's identity; the Herdr workspace is
    // attached to it rather than replacing it.
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].id, registrations[0].id);
    assert_eq!(catalog[0].label, "Duplicate");
    assert!(catalog[0].registered);
    assert_eq!(catalog[0].session_workspace_ids, vec!["w41".to_owned()]);
}

/// The user's report: a project that exists only as a Herdr workspace,
/// one tab, one pane. Closing that pane made Herdr close the workspace,
/// and the project vanished from the sidebar.
#[test]
fn closing_the_last_pane_keeps_an_unregistered_project_listed() {
    let mut runtime = runtime();
    let checkout_path = "/private/tmp/hide-retain-project";
    let spaces = vec![workspace::SessionSpace {
        id: "w5".to_owned(),
        label: "hide main".to_owned(),
        purpose: None,
        cwds: vec![checkout_path.to_owned()],
    }];
    let project_id = workspace::workspace_id_for_path(Path::new(checkout_path));
    let checkout_id = workspace::checkout_id_for_path(&project_id, Path::new(checkout_path));
    let occupied: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "workspaces": [{"workspace_id": "w5", "label": "hide main"}],
        "tabs": [{"workspace_id": "w5", "tab_id": "w5:t1", "label": "1"}],
        "panes": [{"pane_id": "w5:p1", "cwd": checkout_path}],
        "layouts": [{
            "workspace_id": "w5",
            "tab_id": "w5:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": "w5:p1",
            "panes": [{"pane_id": "w5:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
            "splits": []
        }]
    }))
    .expect("occupied payload");
    let catalog = session_sync::PrecomputedCatalog {
        registrations: Vec::new(),
        workspaces: workspace::build_catalog(
            &[],
            &spaces,
            &crate::model::WorktreeCatalogSnapshot::default(),
        ),
        roots: workspace::root_index(&spaces),
    };
    runtime.restore_hint_pending = false;
    assert!(runtime.ingest_session_with_catalog(Ok(occupied), Some(catalog)));
    assert!(runtime.focus_checkout(&project_id, &checkout_id));
    assert!(!runtime.snapshot().navigator.workspaces[0].registered);

    runtime.retain_project_before_last_pane_closes("w5:p1");

    let registrations = &runtime.snapshot().ui_state.workspace_registrations;
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].path, checkout_path);
    assert_eq!(registrations[0].id, project_id);
    // Retaining is idempotent: the close path may run again.
    runtime.retain_project_before_last_pane_closes("w5:p1");
    assert_eq!(runtime.snapshot().ui_state.workspace_registrations.len(), 1);

    // Herdr then drops the workspace with the pane.
    let released: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "workspaces": [],
        "tabs": [],
        "panes": [],
        "layouts": []
    }))
    .expect("released payload");
    assert!(runtime.ingest_session(Ok(released)));

    let navigator = &runtime.snapshot().navigator;
    assert_eq!(navigator.workspaces.len(), 1);
    assert_eq!(navigator.workspaces[0].id, project_id);
    assert!(navigator.workspaces[0].registered);
    assert!(navigator.workspaces[0].session_workspace_ids.is_empty());
    assert_eq!(navigator.workspaces[0].checkouts[0].id, checkout_id);
    assert!(navigator.workspaces[0].checkouts[0].tabs.is_empty());
    // The selection survives, so the shell shows this checkout's empty
    // state with its start control rather than "no workspace".
    assert_eq!(
        navigator.focused_checkout_id.as_deref(),
        Some(checkout_id.as_str())
    );
    assert_eq!(navigator.root_path.as_deref(), Some(checkout_path));
}

#[test]
fn closing_the_last_projected_pane_leaves_an_empty_checkout_without_an_error() {
    let mut runtime = runtime();
    let checkout_path = "/tmp/hide-last-closed-pane";
    let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
    let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
    let registration = WorkspaceRegistration {
        id: workspace_id.clone(),
        label: "Last pane".to_owned(),
        path: checkout_path.to_owned(),
        device_id: "local".to_owned(),
        pinned: false,
    };
    let previous_workspace = workspace(
        &workspace_id,
        "Last pane",
        checkout_path,
        vec![checkout(
            &workspace_id,
            &checkout_id,
            checkout_path,
            Some(pane("w-last:p1", checkout_path)),
        )],
    );
    let current_workspace = workspace(
        &workspace_id,
        "Last pane",
        checkout_path,
        vec![checkout(&workspace_id, &checkout_id, checkout_path, None)],
    );
    runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
    runtime.snapshot.navigator.workspaces = vec![previous_workspace];
    runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id);
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id);
    runtime.snapshot.ui_state.selected_pane_id = Some("w-last:p1".to_owned());
    runtime.snapshot.terminal.pane_id = Some("w-last:p1".to_owned());
    runtime.snapshot.focused.pane_id = Some("w-last:p1".to_owned());
    runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
        workspace_id: "w-last".to_owned(),
        tab_id: "w-last:t1".to_owned(),
        focused_pane_id: "w-last:p1".to_owned(),
        zoomed: false,
        root: PaneLayoutNodeSnapshot::Pane {
            pane_id: "w-last:p1".to_owned(),
        },
    }];
    runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
        pane_id: "w-last:p1".to_owned(),
        closed: false,
        exit_code: None,
        ..TerminalPaneSnapshot::default()
    }];
    runtime.restore_hint_pending = false;

    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "panes": [],
        "layouts": []
    }))
    .expect("empty session payload");

    assert!(runtime.ingest_session_with_catalog(
        Ok(payload),
        Some(session_sync::PrecomputedCatalog {
            registrations: vec![registration],
            workspaces: vec![current_workspace],
            roots: workspace::RootIndex::new(),
        }),
    ));
    assert_eq!(runtime.snapshot().terminal.pane_id, None);
    assert_eq!(runtime.snapshot().focused.pane_id, None);
    assert_eq!(runtime.snapshot().ui_state.selected_pane_id, None);
    assert!(runtime.snapshot().active_pane_layout().is_none());
    assert!(runtime.snapshot().terminal.panes.is_empty());
    assert!(runtime.snapshot().status.last_error.is_none());
}

#[test]
fn a_foreign_stale_projection_is_not_mistaken_for_checkout_pane_retirement() {
    let mut runtime = runtime();
    let checkout_path = "/tmp/hide-foreign-stale-projection";
    let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
    let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
    let registration = WorkspaceRegistration {
        id: workspace_id.clone(),
        label: "Focused checkout".to_owned(),
        path: checkout_path.to_owned(),
        device_id: "local".to_owned(),
        pinned: false,
    };
    let previous_workspace = workspace(
        &workspace_id,
        "Focused checkout",
        checkout_path,
        vec![checkout(
            &workspace_id,
            &checkout_id,
            checkout_path,
            Some(pane("w-focused:p1", checkout_path)),
        )],
    );
    let current_workspace = workspace(
        &workspace_id,
        "Focused checkout",
        checkout_path,
        vec![checkout(
            &workspace_id,
            &checkout_id,
            checkout_path,
            Some(pane("w-focused:p2", checkout_path)),
        )],
    );
    runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
    runtime.snapshot.navigator.workspaces = vec![previous_workspace];
    runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id);
    runtime.snapshot.ui_state.selected_pane_id = Some("w-foreign:p1".to_owned());
    runtime.snapshot.terminal.pane_id = Some("w-foreign:p1".to_owned());
    runtime.snapshot.focused.pane_id = Some("w-foreign:p1".to_owned());
    runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
        workspace_id: "w-foreign".to_owned(),
        tab_id: "w-foreign:t1".to_owned(),
        focused_pane_id: "w-foreign:p1".to_owned(),
        zoomed: false,
        root: PaneLayoutNodeSnapshot::Pane {
            pane_id: "w-foreign:p1".to_owned(),
        },
    }];
    runtime.restore_hint_pending = false;

    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "panes": [{"pane_id": "w-focused:p2", "cwd": checkout_path}],
        "focused_pane_id": "w-focused:p2",
        "tabs": [{"workspace_id": "w-focused", "tab_id": "w-focused:t1", "label": ""}],
        "layouts": [{
            "workspace_id": "w-focused",
            "tab_id": "w-focused:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": "w-focused:p2",
            "panes": [{
                "pane_id": "w-focused:p2",
                "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
            }],
            "splits": []
        }]
    }))
    .expect("focused checkout payload");

    assert!(runtime.ingest_session_with_catalog(
        Ok(payload),
        Some(session_sync::PrecomputedCatalog {
            registrations: vec![registration],
            workspaces: vec![current_workspace],
            roots: workspace::RootIndex::new(),
        }),
    ));
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w-foreign:p1")
    );
    assert!(runtime.snapshot().active_pane_layout().is_none());
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("pane.projection_unavailable")
    );
}

#[test]
fn an_explicit_checkout_waits_without_rendering_stale_projection_when_catalog_is_missing() {
    let mut runtime = runtime();
    let stale_checkout_id = "checkout:selected";
    runtime.snapshot.navigator.focused_checkout_id = Some(stale_checkout_id.to_owned());
    runtime.snapshot.ui_state.focused_checkout_id = Some(stale_checkout_id.to_owned());
    runtime.snapshot.terminal.pane_id = Some("w3P:p1".to_owned());
    runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
        pane_id: "w3P:p1".to_owned(),
        closed: false,
        exit_code: None,
        ..TerminalPaneSnapshot::default()
    }];
    runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
        workspace_id: "w3P".to_owned(),
        tab_id: "w3P:t1".to_owned(),
        focused_pane_id: "w3P:p1".to_owned(),
        zoomed: false,
        root: PaneLayoutNodeSnapshot::Pane {
            pane_id: "w3P:p1".to_owned(),
        },
    }];
    runtime.snapshot.tab = TabSnapshot {
        id: Some("w3P:t1".to_owned()),
        workspace_id: Some("w3P".to_owned()),
        checkout_id: Some(stale_checkout_id.to_owned()),
        label: Some("Old context".to_owned()),
        empty: false,
        delegated: false,
        panes: vec![pane("w3P:p1", "/tmp/old-context")],
    };
    // The user chose this checkout against a running session, so it is an
    // authoritative selection rather than a restore hint.
    runtime.restore_hint_pending = false;

    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "panes": [{"pane_id": "w3P:p1", "cwd": "/tmp/old-context"}],
        "tabs": [{"workspace_id": "w3P", "tab_id": "w3P:t1", "label": ""}],
        "layouts": [{
            "workspace_id": "w3P",
            "tab_id": "w3P:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": "w3P:p1",
            "panes": [{"pane_id": "w3P:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
            "splits": []
        }]
    }))
    .expect("stale projection payload");

    assert!(runtime.ingest_session_with_catalog(
        Ok(payload),
        Some(session_sync::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: Vec::new(),
            roots: workspace::RootIndex::new(),
        }),
    ));
    // The layout stays in the snapshot because it is Herdr's and it
    // describes a tab that exists. What must not happen is drawing it,
    // and that is settled by the tab projection going empty and the
    // projection-unavailable error being raised, both asserted here. The
    // shell has no checkout to look a tab up in, so it has no layout to
    // draw either.
    assert_eq!(runtime.snapshot().pane_layouts.len(), 1);
    assert!(runtime.snapshot().terminal.panes.is_empty());
    assert!(runtime.snapshot().tab.empty);
    assert!(runtime.snapshot().tab.panes.is_empty());
    assert_eq!(runtime.snapshot().tab.checkout_id, None);
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("pane.projection_unavailable")
    );
}

#[test]
fn checkout_path_matching_uses_component_boundaries() {
    let id = NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed);
    let checkout_path = std::env::temp_dir().join(format!(
        "hide-checkout-boundary-{}-{id}",
        std::process::id()
    ));
    let sibling_path = checkout_path.with_file_name(format!(
        "{}-sibling",
        checkout_path
            .file_name()
            .expect("checkout directory name")
            .to_string_lossy()
    ));
    std::fs::create_dir_all(checkout_path.join("src")).expect("checkout fixture");
    std::fs::create_dir_all(sibling_path.join("src")).expect("sibling fixture");
    let checkout = checkout_path.to_string_lossy();
    let child = checkout_path
        .join("src/main.rs")
        .to_string_lossy()
        .into_owned();
    let sibling_child = sibling_path
        .join("src/main.rs")
        .to_string_lossy()
        .into_owned();

    assert!(path_is_within_checkout(&child, &checkout));
    assert!(!path_is_within_checkout(&sibling_child, &checkout));

    let _ = std::fs::remove_dir_all(checkout_path);
    let _ = std::fs::remove_dir_all(sibling_path);
}

/// B2, B7: a projected project id is stable across Herdr sessions, but a
/// reopened tab must target the Herdr workspace that actually owned it.
#[test]
fn close_context_keeps_project_lookup_separate_from_session_workspace() {
    let mut runtime = runtime();
    let mut project = workspace(
        "workspace:project",
        "Fixture",
        "/repo",
        vec![checkout(
            "workspace:project",
            "checkout:main",
            "/repo",
            Some(pane("w7:p2", "/repo")),
        )],
    );
    project.session_workspace_ids = vec!["w7".to_owned()];
    let tab = &mut project.checkouts[0].tabs[0];
    tab.id = Some("w7:t2".to_owned());
    project.checkouts[0].active_tab_id = Some("w7:t2".to_owned());
    runtime.snapshot.navigator.workspaces = vec![project];
    runtime.herdr_workspace_tab_order.insert(
        "w7".to_owned(),
        vec!["w7:t1".to_owned(), "w7:t2".to_owned()],
    );

    let context = runtime
        .close_context(&runtime.snapshot.navigator.workspaces[0].checkouts[0].tabs[0])
        .expect("complete close context");

    assert_eq!(context.workspace_id, "w7");
    assert_eq!(context.workspace_ids_before_close, ["w7"]);
    assert_eq!(context.tab_ids_before_close, ["w7:t1", "w7:t2"]);
    assert_eq!(context.pane_ids_before_close, ["w7:p2"]);
    assert_eq!(context.checkout_id, "checkout:main");
    assert_eq!(context.tab_id, "w7:t2");
    assert_eq!(context.tab_index, 1);
}

#[test]
fn a_session_workspace_shared_by_two_projects_is_not_reused_for_new_tabs() {
    let mut runtime = runtime();
    let mut target = workspace(
        "project:hide",
        "hide",
        "/repo/hide",
        vec![checkout(
            "project:hide",
            "checkout:hide",
            "/repo/hide",
            Some(pane("shared:p1", "/repo/hide")),
        )],
    );
    target.session_workspace_ids = vec!["shared".to_owned()];
    target.checkouts[0].tabs[0].id = Some("shared:t1".to_owned());
    target.checkouts[0].tabs[0].workspace_id = Some("shared".to_owned());
    target.checkouts[0].active_tab_id = Some("shared:t1".to_owned());

    let mut neighbor = workspace(
        "project:task-factory",
        "task-factory",
        "/repo/task-factory",
        vec![checkout(
            "project:task-factory",
            "checkout:task-factory",
            "/repo/task-factory",
            Some(pane("shared:p2", "/repo/task-factory")),
        )],
    );
    neighbor.session_workspace_ids = vec!["shared".to_owned()];
    runtime.snapshot.navigator.workspaces = vec![target, neighbor];
    runtime
        .visible_tab_ids
        .insert("checkout:hide".to_owned(), "shared:t1".to_owned());
    runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
        workspace_id: "shared".to_owned(),
        tab_id: "shared:t1".to_owned(),
        focused_pane_id: "shared:p1".to_owned(),
        zoomed: false,
        root: PaneLayoutNodeSnapshot::Pane {
            pane_id: "shared:p1".to_owned(),
        },
    }];

    assert_eq!(
        runtime.reusable_session_workspace_id("project:hide", "checkout:hide"),
        None,
        "a new tab must create a project-owned Herdr workspace instead of inheriting the neighbor's label"
    );

    runtime.snapshot.navigator.workspaces[1].session_workspace_ids = vec!["neighbor".to_owned()];
    assert_eq!(
        runtime
            .reusable_session_workspace_id("project:hide", "checkout:hide")
            .as_deref(),
        Some("shared"),
        "an exclusive workspace remains reusable"
    );
}

/// The chosen agent starts after the creation is published, and its answer
/// lands on its own axis: a failure keeps the created pane and path, and only
/// a definite failure is offered again.
#[test]
fn a_task_agent_start_reports_apart_from_the_creation_it_follows() {
    let mut runtime = runtime();
    let id = runtime
        .begin_task_operation(
            "agent_start",
            Some("/tmp/hide-agent-task".into()),
            None,
            None,
            Some("claude".into()),
        )
        .unwrap();
    assert!(runtime.ingest_task_operation_result(
        id,
        Ok(live::WorktreeTaskOutcome {
            path: "/tmp/hide-agent-task".into(),
            pane_id: "w1:p9".into(),
            purpose_error: None,
            unconfirmed_purpose_token: None,
        }),
    ));
    let operation = runtime.snapshot().task_operation.clone().unwrap();
    assert_eq!(operation.phase, "ready");
    assert_eq!(operation.agent_phase.as_deref(), Some("starting"));
    assert_eq!(
        runtime.pending_task_agent_start(id),
        Some(("w1:p9".to_owned(), "claude".to_owned()))
    );
    // An acknowledgement while the agent is still starting keeps the slot.
    runtime.acknowledge_task_operation(id);
    assert!(runtime.snapshot().task_operation.is_some());
    // Nor can another task take the slot while the agent's answer is due.
    assert!(
        runtime
            .begin_task_operation("checkout_purpose", None, None, None, None)
            .is_err()
    );

    assert!(runtime.ingest_task_agent_result(
        id,
        live::TaskAgentOutcome::Failed("claude is not installed".into()),
    ));
    let operation = runtime.snapshot().task_operation.clone().unwrap();
    assert_eq!(operation.phase, "ready");
    assert_eq!(operation.pane_id.as_deref(), Some("w1:p9"));
    assert_eq!(operation.agent_phase.as_deref(), Some("failed"));
    assert_eq!(
        operation.agent_message.as_deref(),
        Some("claude is not installed")
    );
    assert_eq!(runtime.pending_task_agent_start(id), None);

    // The pane is not in this runtime's navigator, so the retry says so and
    // starts nothing.
    let retry = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "task_agent_retry",
        "payload": { "id": id }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&retry));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("task_operation.agent_retry_pane_gone")
    );
    assert_eq!(
        runtime
            .snapshot()
            .task_operation
            .as_ref()
            .and_then(|op| op.agent_phase.as_deref()),
        Some("failed")
    );
    runtime.acknowledge_task_operation(id);
    assert!(runtime.snapshot().task_operation.is_none());
}

/// Removal completes only on the core's own worker. A client that claims a
/// completion names an event the core no longer has, so a removal waiting on
/// Git cannot be settled from outside.
#[test]
fn a_client_cannot_claim_a_worktree_removal_finished() {
    let mut runtime = runtime();
    runtime.snapshot.worktree_removal = Some(crate::model::WorktreeRemovalSnapshot {
        id: 7,
        repository_root: "/tmp/hide-removal-repo".into(),
        checkout_path: "/tmp/hide-removal-repo-linked".into(),
        expected_head_sha: None,
        expected_branch: Some("linked".into()),
        protected_base_branch: Some("main".into()),
        branch: Some("linked".into()),
        delete_branch: false,
        phase: "removing".into(),
        message: None,
    });
    let forged = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "worktree_removal_finished",
        "payload": { "id": 7, "removed": true, "message": "done" }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&forged));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("event.unknown_kind")
    );
    assert_eq!(
        runtime
            .snapshot()
            .worktree_removal
            .as_ref()
            .map(|removal| removal.phase.as_str()),
        Some("removing")
    );
    assert!(runtime.ingest_worktree_removal_result(7, Ok("Deleted.".into())));
    assert_eq!(
        runtime
            .snapshot()
            .worktree_removal
            .as_ref()
            .map(|removal| removal.phase.as_str()),
        Some("finished")
    );
    assert!(!runtime.ingest_worktree_removal_result(7, Ok("again".into())));
}

/// Herdr confirms the panes gone before the navigator applies the close, so
/// the pane just closed is still listed when the confirmation lands. Only a
/// pane that was not among those closed stops the removal.
#[test]
fn a_closed_pane_still_listed_does_not_stop_the_removal_but_a_new_one_does() {
    use crate::model::{ProjectWorktreesSnapshot, WorktreeCatalogSnapshot, WorktreeSnapshot};

    let mut runtime = runtime();
    let mut project = workspace(
        "workspace-1",
        "hide",
        "/repo",
        vec![
            checkout("workspace-1", "main", "/repo", None),
            checkout(
                "workspace-1",
                "open",
                "/repo.worktrees/open",
                Some(pane("w1:p1", "/repo.worktrees/open")),
            ),
        ],
    );
    project.is_git = true;
    runtime.snapshot.navigator.workspaces = vec![project];
    runtime.ingest_worktrees(WorktreeCatalogSnapshot {
        projects: vec![ProjectWorktreesSnapshot {
            root_path: "/repo".to_owned(),
            worktrees: vec![
                WorktreeSnapshot {
                    path: "/repo".to_owned(),
                    branch: Some("main".to_owned()),
                    is_main: true,
                    ..WorktreeSnapshot::default()
                },
                WorktreeSnapshot {
                    path: "/repo.worktrees/open".to_owned(),
                    branch: Some("open".to_owned()),
                    head_sha: Some("abc".to_owned()),
                    ..WorktreeSnapshot::default()
                },
            ],
            ..ProjectWorktreesSnapshot::default()
        }],
    });
    let closing = |id: u64| crate::model::WorktreeRemovalSnapshot {
        id,
        repository_root: "/repo".into(),
        checkout_path: "/repo.worktrees/open".into(),
        expected_head_sha: Some("abc".into()),
        expected_branch: Some("open".into()),
        protected_base_branch: Some("main".into()),
        branch: Some("open".into()),
        delete_branch: false,
        phase: "closing".into(),
        message: None,
    };

    runtime.snapshot.worktree_removal = Some(closing(1));
    assert!(runtime.ingest_worktree_close_result(1, &["w1:p1".to_owned()], Ok(())));
    assert_eq!(
        runtime
            .snapshot()
            .worktree_removal
            .as_ref()
            .map(|removal| removal.phase.as_str()),
        Some("removing")
    );
    assert!(runtime.confirmed_worktree_removal(1).is_some());

    runtime.snapshot.worktree_removal = Some(closing(2));
    assert!(runtime.ingest_worktree_close_result(2, &[], Ok(())));
    let removal = runtime.snapshot().worktree_removal.clone().unwrap();
    assert_eq!(removal.phase, "failed");
    assert!(removal.message.unwrap().contains("A pane appeared"));
    assert!(runtime.confirmed_worktree_removal(2).is_none());
}

/// The kind reaches `agent.start`, which runs it in the new pane's shell, so
/// a creation that names anything but a provider Hide starts is refused
/// before any worktree exists.
#[test]
fn a_worktree_creation_naming_an_unknown_agent_is_refused() {
    let mut runtime = runtime();
    let create = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "create_worktree",
        "payload": { "repository_root": "/repo", "branch": "feature", "agent_kind": "/tmp/run.sh" }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&create));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("worktree.create_unknown_agent")
    );
    assert!(runtime.snapshot().task_operation.is_none());
}
