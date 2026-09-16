use super::*;

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
    let spaces = Runtime::session_spaces(&payload, "");
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
    let spaces = Runtime::session_spaces(&payload(), "");
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
            root_path: "/tmp/hide".to_owned(),
            status: GithubStatusSnapshot {
                available: true,
                last_success_at_unix_ms: Some(1_000),
                ..GithubStatusSnapshot::default()
            },
            pull_requests: vec![pull_request.clone()],
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
    assert_eq!(project.pull_requests, vec![pull_request]);
    assert!(project.status.stale);
    assert!(!project.status.available);
    assert_eq!(project.status.last_success_at_unix_ms, Some(1_000));
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

#[test]
fn removing_registration_in_use_preserves_it_and_explains_recovery() {
    let mut runtime = runtime();
    let path = std::env::temp_dir().join(format!("hide-registration-busy-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let registration =
        workspace::registration(path.to_str().unwrap(), "Busy project", "local").unwrap();
    runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
    runtime.last_session_spaces = vec![workspace::SessionSpace {
        id: "w1".into(),
        label: "Busy project".into(),
        cwds: vec![registration.path.clone()],
    }];
    runtime.rebuild_catalog();
    runtime.dispatch_json(&serde_json::to_vec(&serde_json::json!({
        "schema_version": 2, "kind": "remove_workspace", "payload": {"workspace_id": registration.id}
    })).unwrap());
    assert_eq!(
        runtime.snapshot.ui_state.workspace_registrations,
        vec![registration]
    );
    assert_eq!(
        runtime.snapshot.status.last_error.as_ref().unwrap().kind,
        "workspace.registration_in_use"
    );
    assert!(path.is_dir(), "Registration removal must not delete files");
    std::fs::remove_dir(path).unwrap();
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

#[test]
fn overview_inspection_does_not_focus_or_repeat_publish() {
    struct CountConnections(std::sync::mpsc::Sender<()>);
    impl hide_herdr_client::ApiConnector for CountConnections {
        fn connect(
            &self,
        ) -> Result<Box<dyn hide_herdr_client::ApiStream>, hide_herdr_client::ApiError> {
            let _ = self.0.send(());
            Err(hide_herdr_client::ApiError::Transport(
                "fixture refused".into(),
            ))
        }
    }
    let mut runtime = live_runtime();
    runtime.ingest_session(Ok(context_payload()));
    let mut target = runtime.snapshot.navigator.workspaces[1].checkouts[0].clone();
    target.workspace_id = runtime.snapshot.navigator.workspaces[0].id.clone();
    runtime.snapshot.navigator.workspaces[0]
        .checkouts
        .push(target.clone());
    let project = runtime.snapshot.navigator.workspaces[0].clone();
    runtime.focus_checkout(&project.id, &project.checkouts[0].id);
    let focused = runtime.snapshot.focused.clone();
    let navigator = runtime.snapshot.navigator.clone();
    let ui = runtime.snapshot.ui_state.clone();
    let (send, receive) = std::sync::mpsc::channel();
    runtime.live.as_mut().unwrap().api_connector = Arc::new(CountConnections(send));
    let event = serde_json::to_vec(&serde_json::json!({
        "schema_version": 2, "kind": "overview_select", "payload": {"checkout_path": target.path}
    }))
    .unwrap();
    runtime.dispatch_json(&event);
    assert_eq!(
        runtime.snapshot.card.inspected_checkout_path,
        Some(target.path)
    );
    assert_eq!(runtime.snapshot.focused, focused);
    assert_eq!(runtime.snapshot.navigator, navigator);
    assert_eq!(runtime.snapshot.ui_state, ui);
    assert!(
        !runtime.dispatch_json(&event),
        "Repeating an inspection has no effects"
    );
    assert!(
        receive
            .recv_timeout(std::time::Duration::from_millis(30))
            .is_err(),
        "Inspection sent a Herdr request"
    );
    let pane = &target.tabs[0].panes[0].id;
    runtime.dispatch_json(&operator_focus_event(pane));
    assert!(
        receive
            .recv_timeout(std::time::Duration::from_secs(1))
            .is_ok(),
        "Explicit Open must notify Herdr"
    );
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
            cwds: vec![checkout_path.to_owned()],
        },
        workspace::SessionSpace {
            id: "w3Z".to_owned(),
            label: "selected".to_owned(),
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
        cwds: vec![checkout_path.to_owned()],
    }];
    let registrations = vec![WorkspaceRegistration {
        id: workspace::workspace_id_for_path(Path::new(checkout_path)),
        label: "Duplicate".to_owned(),
        path: checkout_path.to_owned(),
        device_id: "local".to_owned(),
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
