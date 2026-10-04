use super::*;
use crate::workspace_control::{Action, ActionPreparation, Edge, Query};

fn action_id(suffix: &str) -> String {
    format!("{}-{suffix}", unix_milliseconds())
}

fn caller_fixture() -> (Runtime, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("fixture directory");
    let mut runtime = runtime();
    runtime.snapshot.status.herdr.state = "connected".to_owned();
    runtime.workspace_views = Some(
        WorkspaceViewStore::open(
            dir.path().join("views.json"),
            (
                runtime.snapshot.ui_state.right_panel_visible,
                runtime.snapshot.ui_state.right_panel_section,
            ),
        )
        .0,
    );
    runtime.snapshot.navigator.workspaces = vec![
        workspace(
            "workspace-a",
            "A",
            "/checkouts/a",
            vec![checkout(
                "workspace-a",
                "checkout-a",
                "/checkouts/a",
                Some(pane("pane-a", "/checkouts/a")),
            )],
        ),
        workspace(
            "workspace-b",
            "B",
            "/checkouts/b",
            vec![checkout(
                "workspace-b",
                "checkout-b",
                "/checkouts/b",
                Some(pane("pane-b", "/checkouts/b")),
            )],
        ),
    ];
    (runtime, dir)
}

#[test]
fn caller_query_reads_only_its_live_checkout_and_untouched_views_are_empty() {
    let (mut runtime, _dir) = caller_fixture();
    let before = runtime.snapshot.navigator.focused_workspace_id.clone();
    let info = runtime
        .workspace_control_query("local", "pane-b", Query::Info)
        .unwrap();
    assert_eq!(info.context.device_id, "local");
    assert_eq!(info.context.workspace_id, "workspace-b");
    assert_eq!(info.context.checkout_id, "checkout-b");
    assert_eq!(info.context.checkout_path, "/checkouts/b");
    assert!(info.views.is_none());
    assert_eq!(
        runtime
            .workspace_control_query("local", "pane-b", Query::ViewList)
            .unwrap()
            .views,
        Some(Vec::new())
    );

    let layout = &mut runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .views
        .entry("local", "/checkouts/b")
        .layout;
    let display = layout.new_display(
        "/checkouts/b/보고서.md",
        crate::view_layout::DisplayKind::File,
        None,
        false,
    );
    layout.insert("a1", display, 1).unwrap();
    let views = runtime
        .workspace_control_query("local", "pane-b", Query::ViewList)
        .unwrap()
        .views
        .unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].target, "/checkouts/b/보고서.md");
    assert_eq!(views[0].kind, "file");
    assert!(views[0].selected);
    assert!(views[0].active_area);
    assert_eq!(runtime.snapshot.navigator.focused_workspace_id, before);
}

#[test]
fn caller_query_refuses_stale_or_ambiguous_pane_without_using_the_front_workspace() {
    let (mut runtime, _dir) = caller_fixture();
    assert_eq!(
        runtime
            .workspace_control_query("local", "closed-pane", Query::Info)
            .unwrap_err()
            .reason,
        "pane_not_connected"
    );
    runtime.snapshot.navigator.workspaces[1].checkouts[0].tabs[0].panes[0].id = "pane-a".to_owned();
    assert_eq!(
        runtime
            .workspace_control_query("local", "pane-a", Query::Info)
            .unwrap_err()
            .reason,
        "ambiguous_pane"
    );
    runtime.snapshot.navigator.workspaces[1].checkouts[0].tabs[0].panes[0].id = "pane-b".to_owned();
    runtime.workspace_views = None;
    assert_eq!(
        runtime
            .workspace_control_query("local", "pane-b", Query::Info)
            .unwrap_err()
            .reason,
        "views_unavailable"
    );

    runtime.snapshot.status.herdr.state = "disconnected".to_owned();
    assert_eq!(
        runtime
            .workspace_control_query("local", "pane-b", Query::Info)
            .unwrap_err()
            .reason,
        "pane_not_connected"
    );
}

#[test]
fn attested_device_resolves_colliding_pane_ids_without_crossing_workspaces() {
    let (mut runtime, _dir) = caller_fixture();
    let device = "remote:fixture";
    let mut remote_workspace = workspace(
        "remote-workspace",
        "Remote",
        "/remote/checkout",
        vec![checkout(
            "remote-workspace",
            "remote-checkout",
            "/remote/checkout",
            Some(pane("pane-b", "/remote/checkout")),
        )],
    );
    remote_workspace.device_id = device.to_owned();
    remote_workspace.remote_target_id = Some(device.to_owned());
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: device.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: None,
        session: Some(RemoteSessionSnapshot {
            workspaces: vec![remote_workspace],
            agents: Vec::new(),
            active_tab_ids: Default::default(),
            focused_workspace_id: None,
            focused_checkout_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            pane_layouts: Vec::new(),
            pane_hook_tokens: Default::default(),
        }),
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });

    let local = runtime
        .workspace_control_query("local", "pane-b", Query::Info)
        .unwrap();
    let remote = runtime
        .workspace_control_query(device, "pane-b", Query::Info)
        .unwrap();
    assert_eq!(local.context.workspace_id, "workspace-b");
    assert_eq!(remote.context.workspace_id, "remote-workspace");
    assert_eq!(remote.context.device_id, device);

    runtime.snapshot.status.remote[0].state = "disconnected".to_owned();
    assert_eq!(
        runtime
            .workspace_control_query(device, "pane-b", Query::Info)
            .unwrap_err()
            .reason,
        "pane_not_connected"
    );
    assert_eq!(
        runtime
            .workspace_control_query("local", "pane-b", Query::Info)
            .unwrap()
            .context
            .workspace_id,
        "workspace-b"
    );
}

#[test]
fn background_view_commands_preserve_front_focus_and_retried_split_converges() {
    let (mut runtime, _dir) = caller_fixture();
    let expected = runtime
        .workspace_control_query("local", "pane-b", Query::Info)
        .unwrap()
        .context;
    let other = runtime
        .workspace_control_query("local", "pane-a", Query::Info)
        .unwrap()
        .context;
    let key = ("local", "/checkouts/b");
    let layout = &mut runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .views
        .entry(key.0, key.1)
        .layout;
    for path in ["first.md", "second.md"] {
        let display = layout.new_display(
            &format!("/checkouts/b/{path}"),
            crate::view_layout::DisplayKind::File,
            None,
            false,
        );
        layout.insert("a1", display, 1).unwrap();
    }
    let before = (
        runtime.snapshot.navigator.focused_workspace_id.clone(),
        runtime.snapshot.navigator.focused_checkout_id.clone(),
        runtime.snapshot.ui_state.selected_pane_id.clone(),
    );
    let view_id = runtime
        .view_layout_of(&(key.0.into(), key.1.into()))
        .unwrap()
        .areas()[0]
        .displays[0]
        .id
        .clone();
    let action = Action::Split {
        view_id: view_id.clone(),
        area_id: "a1".into(),
        edge: Edge::Right,
    };
    let retry_id = action_id("retry-split");
    let first = runtime
        .workspace_control_action(
            "local",
            "pane-b",
            &expected,
            &retry_id,
            action.clone(),
            Ok(None),
        )
        .unwrap();
    assert!(first.changed);
    let retry = runtime
        .workspace_control_action("local", "pane-b", &expected, &retry_id, action, Ok(None))
        .unwrap();
    assert_eq!(retry, first);
    assert_eq!(
        runtime
            .view_layout_of(&(key.0.into(), key.1.into()))
            .unwrap()
            .area_count(),
        2
    );
    assert_eq!(
        runtime
            .workspace_control_action(
                "local",
                "pane-b",
                &expected,
                &retry_id,
                Action::Close {
                    view_id: view_id.clone()
                },
                Ok(None),
            )
            .unwrap_err()
            .reason,
        "request_id_reused"
    );
    assert_eq!(
        runtime
            .workspace_control_action(
                "local",
                "pane-a",
                &other,
                &action_id("wrong-workspace"),
                Action::Select {
                    view_id: view_id.clone(),
                    reveal: false,
                },
                Ok(None),
            )
            .unwrap_err()
            .reason,
        "view_layout.unknown_display"
    );
    assert_eq!(
        (
            runtime.snapshot.navigator.focused_workspace_id.clone(),
            runtime.snapshot.navigator.focused_checkout_id.clone(),
            runtime.snapshot.ui_state.selected_pane_id.clone(),
        ),
        before
    );

    runtime.snapshot.navigator.workspaces[1].checkouts[0].tabs[0].panes[0].id =
        "former-pane-b".to_owned();
    runtime.snapshot.navigator.workspaces[0].checkouts[0].tabs[0].panes[0].id = "pane-b".to_owned();
    assert_eq!(
        runtime
            .workspace_control_action(
                "local",
                "pane-b",
                &expected,
                &retry_id,
                Action::Split {
                    view_id,
                    area_id: "a1".into(),
                    edge: Edge::Right,
                },
                Ok(None),
            )
            .unwrap_err()
            .reason,
        "pane_changed"
    );
}

#[test]
fn close_of_an_already_closed_view_returns_a_no_change_result() {
    let (mut runtime, _dir) = caller_fixture();
    let expected = runtime
        .workspace_control_query("local", "pane-b", Query::Info)
        .unwrap()
        .context;
    let key = ("local", "/checkouts/b");
    let layout = &mut runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .views
        .entry(key.0, key.1)
        .layout;
    let display = layout.new_browser_display("https://example.org", 1);
    let view_id = display.id.clone();
    layout.insert("a1", display, 1).unwrap();
    let action = Action::Close {
        view_id: view_id.clone(),
    };
    assert!(
        runtime
            .workspace_control_action(
                "local",
                "pane-b",
                &expected,
                &action_id("close-1"),
                action.clone(),
                Ok(None),
            )
            .unwrap()
            .changed
    );
    assert!(
        !runtime
            .workspace_control_action(
                "local",
                "pane-b",
                &expected,
                &action_id("close-2"),
                action,
                Ok(None)
            )
            .unwrap()
            .changed
    );
    assert!(
        runtime
            .view_layout_of(&(key.0.into(), key.1.into()))
            .unwrap()
            .displays()
            .next()
            .is_none()
    );
}

#[test]
fn selecting_a_hidden_view_reveals_only_when_requested() {
    let (mut runtime, _dir) = caller_fixture();
    let key = ("local", "/checkouts/b");
    let layout = &mut runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .views
        .entry(key.0, key.1)
        .layout;
    let display = layout.new_browser_display("https://example.org", 1);
    let view_id = display.id.clone();
    layout.insert("a1", display, 1).unwrap();
    let expected = runtime
        .workspace_control_query("local", "pane-b", Query::Info)
        .unwrap()
        .context;
    let before = runtime.snapshot.navigator.focused_workspace_id.clone();
    let called = |runtime: &Runtime| runtime.workspace_views.as_ref().unwrap().views_calls;
    runtime
        .workspace_control_action(
            "local",
            "pane-b",
            &expected,
            &action_id("select-hidden"),
            Action::Select {
                view_id: view_id.clone(),
                reveal: false,
            },
            Ok(None),
        )
        .unwrap();
    assert_eq!(runtime.snapshot.navigator.focused_workspace_id, before);
    assert!(
        !views_of(&runtime, key),
        "a background select leaves File Views off"
    );
    assert_eq!(called(&runtime), 0, "a background select calls no column");
    runtime
        .workspace_control_action(
            "local",
            "pane-b",
            &expected,
            &action_id("select-reveal"),
            Action::Select {
                view_id,
                reveal: true,
            },
            Ok(None),
        )
        .unwrap();
    assert_eq!(
        runtime.snapshot.navigator.focused_workspace_id.as_deref(),
        Some("workspace-b")
    );
    assert!(views_of(&runtime, key), "a reveal turns File Views on");
    // The shell shows a called File Views in a narrow body; a call from the
    // CLI reaches it only as this count rising (PRD three-column-panel D-07).
    assert_eq!(called(&runtime), 1, "a reveal calls File Views");
    let numbered = runtime
        .workspace_views
        .as_ref()
        .unwrap()
        .views_called
        .get(&(key.0.to_owned(), key.1.to_owned()))
        .copied();
    assert_eq!(
        numbered,
        Some(1),
        "the call is numbered for the Workspace it called"
    );
}

fn views_of(runtime: &Runtime, key: (&str, &str)) -> bool {
    runtime
        .workspace_views
        .as_ref()
        .unwrap()
        .views
        .workspaces
        .iter()
        .find(|view| view.is(key.0, key.1))
        .map(|view| view.views)
        .unwrap()
}

#[test]
fn file_open_reads_outside_the_runtime_then_places_once_in_the_callers_workspace() {
    let (mut runtime, dir) = caller_fixture();
    let root = dir.path().to_string_lossy().into_owned();
    runtime.snapshot.navigator.workspaces[1].path = root.clone();
    runtime.snapshot.navigator.workspaces[1].checkouts[0].path = root.clone();
    let path = dir.path().join("보고서.md");
    std::fs::write(&path, "hello\n").unwrap();
    let expected = runtime
        .workspace_control_query("local", "pane-b", Query::Info)
        .unwrap()
        .context;
    let before = runtime.snapshot.navigator.focused_workspace_id.clone();
    let action = Action::OpenFile {
        path: path.to_string_lossy().into_owned(),
        beside: false,
        reveal: false,
    };
    let id = action_id("file-open");
    let ActionPreparation::Read(source) = runtime
        .workspace_control_prepare_action("local", "pane-b", &expected, &id, &action)
        .unwrap()
    else {
        panic!("file needs a host read")
    };
    let material = source.read().unwrap();
    let first = runtime
        .workspace_control_action(
            "local",
            "pane-b",
            &expected,
            &id,
            action.clone(),
            Ok(Some(material)),
        )
        .unwrap();
    assert!(first.changed);
    assert_eq!(first.context.checkout_path, root);
    assert_eq!(runtime.snapshot.navigator.focused_workspace_id, before);
    let views = runtime
        .workspace_control_query("local", "pane-b", Query::ViewList)
        .unwrap()
        .views
        .unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].view_id, first.view_id);
    assert_eq!(views[0].target, path.to_string_lossy());
    std::fs::remove_file(&path).unwrap();
    let ActionPreparation::Cached(Ok(retried)) = runtime
        .workspace_control_prepare_action("local", "pane-b", &expected, &id, &action)
        .unwrap()
    else {
        panic!("retry should return its recorded result without rereading")
    };
    assert_eq!(retried, first);

    let reveal = Action::OpenFile {
        path: path.to_string_lossy().into_owned(),
        beside: false,
        reveal: true,
    };
    let reveal_id = action_id("reveal-open-file");
    let ActionPreparation::Read(source) = runtime
        .workspace_control_prepare_action("local", "pane-b", &expected, &reveal_id, &reveal)
        .unwrap()
    else {
        panic!("a new intent checks its source")
    };
    let material = source.read().unwrap();
    runtime
        .workspace_control_action(
            "local",
            "pane-b",
            &expected,
            &reveal_id,
            reveal,
            Ok(Some(material)),
        )
        .unwrap();
    assert_eq!(
        runtime.snapshot.navigator.focused_workspace_id.as_deref(),
        Some("workspace-b")
    );
}

#[test]
fn browser_load_status_tracks_the_current_load_and_retry_does_not_reload() {
    let (mut runtime, _dir) = caller_fixture();
    let expected = runtime
        .workspace_control_query("local", "pane-b", Query::Info)
        .unwrap()
        .context;
    let before = runtime.snapshot.navigator.focused_workspace_id.clone();
    let action = Action::OpenBrowser {
        url: "https://example.test/page".into(),
        reveal: false,
    };
    let request_id = action_id("browser-open");
    let first = runtime
        .workspace_control_action(
            "local",
            "pane-b",
            &expected,
            &request_id,
            action.clone(),
            Ok(None),
        )
        .unwrap();
    assert_eq!(runtime.snapshot.navigator.focused_workspace_id, before);
    let key = ("local".to_owned(), expected.checkout_path.clone());
    let load = runtime
        .view_layout_of(&key)
        .unwrap()
        .display(&first.view_id)
        .unwrap()
        .load;
    assert_eq!(
        runtime
            .workspace_control_action(
                "local",
                "pane-b",
                &expected,
                &request_id,
                action.clone(),
                Ok(None)
            )
            .unwrap(),
        first
    );
    assert_eq!(
        runtime
            .view_layout_of(&key)
            .unwrap()
            .display(&first.view_id)
            .unwrap()
            .load,
        load
    );
    let page = |runtime: &Runtime| {
        runtime
            .workspace_control_query("local", "pane-b", Query::ViewList)
            .unwrap()
            .views
            .unwrap()
            .into_iter()
            .find(|view| view.view_id == first.view_id)
            .unwrap()
            .page
            .unwrap()
    };
    assert_eq!(page(&runtime).state, "pending");
    let report = |runtime: &mut Runtime, load: u64, loading: bool, failure: Option<&str>| {
        let payload = serde_json::from_value(serde_json::json!({
            "workspace":{"device_id":"local","path":expected.checkout_path},
            "display_id":first.view_id,"url":"https://example.test/page","title":"Page",
            "load":load,"loading":loading,"failure":failure,
        }))
        .unwrap();
        runtime.record_browser_state(payload);
    };
    report(&mut runtime, load, true, None);
    assert_eq!(page(&runtime).state, "loading");
    report(&mut runtime, load, false, None);
    assert_eq!(page(&runtime).state, "loaded");
    let gone = serde_json::from_value(serde_json::json!({
        "workspace":{"device_id":"local","path":expected.checkout_path},
        "display_id":first.view_id,"url":"https://example.test/page","title":"Page",
        "load":load,"present":false,
    }))
    .unwrap();
    runtime.record_browser_state(gone);
    assert_eq!(page(&runtime).state, "disconnected");
    runtime
        .workspace_control_action(
            "local",
            "pane-b",
            &expected,
            &action_id("browser-reload"),
            action,
            Ok(None),
        )
        .unwrap();
    let newer = runtime
        .view_layout_of(&key)
        .unwrap()
        .display(&first.view_id)
        .unwrap()
        .load;
    assert!(newer > load);
    assert_eq!(page(&runtime).state, "pending");
    report(&mut runtime, load, false, None);
    assert_eq!(page(&runtime).state, "pending");
    report(&mut runtime, newer, false, Some("Connection refused"));
    assert_eq!(page(&runtime).state, "failed");
    assert_eq!(
        page(&runtime).failure.as_deref(),
        Some("Connection refused")
    );
}

#[test]
fn browser_file_source_is_confined_to_the_calling_checkout() {
    let (mut runtime, dir) = caller_fixture();
    let checkout = dir.path().join("checkout");
    std::fs::create_dir(&checkout).unwrap();
    runtime.snapshot.navigator.workspaces[1].path = checkout.to_string_lossy().into_owned();
    runtime.snapshot.navigator.workspaces[1].checkouts[0].path =
        checkout.to_string_lossy().into_owned();
    let expected = runtime
        .workspace_control_query("local", "pane-b", Query::Info)
        .unwrap()
        .context;
    let outside = dir.path().join("outside.html");
    std::fs::write(&outside, "<title>Outside</title>").unwrap();
    let action = Action::OpenBrowser {
        url: format!("file://{}", outside.display()),
        reveal: false,
    };
    let ActionPreparation::Read(source) = runtime
        .workspace_control_prepare_action(
            "local",
            "pane-b",
            &expected,
            &action_id("outside-html"),
            &action,
        )
        .unwrap()
    else {
        panic!("file URL must read outside the lock")
    };
    assert_eq!(source.read().err().unwrap().reason, "path_outside_checkout");
    let inside = checkout.join("index.html");
    std::fs::write(&inside, "<title>Inside</title>").unwrap();
    let action = Action::OpenBrowser {
        url: format!("file://{}", inside.display()),
        reveal: false,
    };
    let id = action_id("inside-html");
    let ActionPreparation::Read(source) = runtime
        .workspace_control_prepare_action("local", "pane-b", &expected, &id, &action)
        .unwrap()
    else {
        panic!("file URL must read outside the lock")
    };
    let material = source.read().unwrap();
    assert!(
        runtime
            .workspace_control_action(
                "local",
                "pane-b",
                &expected,
                &id,
                action,
                Ok(Some(material))
            )
            .is_ok()
    );
}

#[test]
fn diff_open_requires_a_real_working_tree_change() {
    let (mut runtime, dir) = caller_fixture();
    let root = dir.path().to_string_lossy().into_owned();
    runtime.snapshot.navigator.workspaces[1].path = root.clone();
    runtime.snapshot.navigator.workspaces[1].checkouts[0].path = root;
    let path = dir.path().join("memo.md");
    std::fs::write(&path, "baseline\n").unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&["add", "memo.md"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "-qm",
        "baseline",
    ]);
    let expected = runtime
        .workspace_control_query("local", "pane-b", Query::Info)
        .unwrap()
        .context;
    let action = Action::OpenDiff {
        path: path.to_string_lossy().into_owned(),
        beside: false,
        reveal: false,
    };
    let clean_id = action_id("clean");
    let ActionPreparation::Read(source) = runtime
        .workspace_control_prepare_action("local", "pane-b", &expected, &clean_id, &action)
        .unwrap()
    else {
        panic!("diff needs a Git read")
    };
    let Err(refusal) = source.read() else {
        panic!("unchanged file must have no diff")
    };
    assert_eq!(refusal.reason, "diff_unchanged");
    assert_eq!(
        runtime
            .workspace_control_action(
                "local",
                "pane-b",
                &expected,
                &clean_id,
                action.clone(),
                Err(refusal),
            )
            .unwrap_err()
            .reason,
        "diff_unchanged"
    );
    std::fs::write(&path, "changed\n").unwrap();
    let ActionPreparation::Cached(Err(retried)) = runtime
        .workspace_control_prepare_action("local", "pane-b", &expected, &clean_id, &action)
        .unwrap()
    else {
        panic!("the failed intent must keep its refusal")
    };
    assert_eq!(retried.reason, "diff_unchanged");
    let id = action_id("changed");
    let ActionPreparation::Read(source) = runtime
        .workspace_control_prepare_action("local", "pane-b", &expected, &id, &action)
        .unwrap()
    else {
        panic!("diff needs a Git read")
    };
    let material = source.read().unwrap();
    let result = runtime
        .workspace_control_action(
            "local",
            "pane-b",
            &expected,
            &id,
            action,
            Ok(Some(material)),
        )
        .unwrap();
    let views = runtime
        .workspace_control_query("local", "pane-b", Query::ViewList)
        .unwrap()
        .views
        .unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].view_id, result.view_id);
    assert_eq!(views[0].kind, "diff");
}

#[test]
fn checkout_caller_resolves_the_longest_registered_checkout_containing_its_cwd() {
    let (mut runtime, _dir) = caller_fixture();
    runtime.snapshot.navigator.workspaces[0]
        .checkouts
        .push(checkout(
            "workspace-a",
            "checkout-a-wt",
            "/checkouts/a/worktrees/wt",
            None,
        ));
    let caller = |key: &str, cwd: &str| crate::workspace_control::checkout_caller_id(key, cwd);

    let nested = runtime
        .workspace_control_query(
            "local",
            &caller("k1", "/checkouts/a/worktrees/wt/src"),
            Query::Info,
        )
        .unwrap();
    assert_eq!(nested.context.checkout_id, "checkout-a-wt");
    assert_eq!(nested.context.checkout_path, "/checkouts/a/worktrees/wt");
    assert_eq!(nested.context.workspace_id, "workspace-a");
    assert!(nested.views.is_none());

    let parent = runtime
        .workspace_control_query("local", &caller("k1", "/checkouts/a/src"), Query::ViewList)
        .unwrap();
    assert_eq!(parent.context.checkout_id, "checkout-a");
    assert_eq!(parent.views, Some(Vec::new()));

    let exact = runtime
        .workspace_control_query("local", &caller("k1", "/checkouts/b"), Query::Info)
        .unwrap();
    assert_eq!(exact.context.checkout_id, "checkout-b");

    for outside in ["/checkouts/ab", "/checkouts", "/elsewhere/checkouts/a"] {
        assert_eq!(
            runtime
                .workspace_control_query("local", &caller("k1", outside), Query::Info)
                .unwrap_err()
                .reason,
            "checkout_not_registered",
            "{outside}"
        );
    }

    runtime.snapshot.status.herdr.state = "disconnected".to_owned();
    assert_eq!(
        runtime
            .workspace_control_query("local", &caller("k1", "/checkouts/a"), Query::Info)
            .unwrap_err()
            .reason,
        "checkout_not_registered"
    );
}

#[test]
fn two_registrations_of_one_path_refuse_the_checkout_caller_instead_of_choosing() {
    let (mut runtime, _dir) = caller_fixture();
    runtime.snapshot.navigator.workspaces[0]
        .checkouts
        .push(checkout(
            "workspace-a",
            "checkout-b-again",
            "/checkouts/b",
            None,
        ));
    assert_eq!(
        runtime
            .workspace_control_query(
                "local",
                &crate::workspace_control::checkout_caller_id("k1", "/checkouts/b/src"),
                Query::Info,
            )
            .unwrap_err()
            .reason,
        "ambiguous_checkout"
    );
    // The pane in that checkout is unaffected.
    assert_eq!(
        runtime
            .workspace_control_query("local", "pane-b", Query::Info)
            .unwrap()
            .context
            .checkout_id,
        "checkout-b"
    );
}

#[test]
fn checkout_callers_in_one_checkout_keep_separate_retry_records() {
    let (mut runtime, _dir) = caller_fixture();
    let first = crate::workspace_control::checkout_caller_id("k1", "/checkouts/b/src");
    let second = crate::workspace_control::checkout_caller_id("k2", "/checkouts/b");
    let expected = runtime
        .workspace_control_query("local", &first, Query::Info)
        .unwrap()
        .context;
    assert_eq!(
        runtime
            .workspace_control_query("local", &second, Query::Info)
            .unwrap()
            .context,
        expected
    );
    let request_id = action_id("shared");
    let opened = runtime
        .workspace_control_action(
            "local",
            &first,
            &expected,
            &request_id,
            Action::OpenBrowser {
                url: "http://localhost:3000".into(),
                reveal: false,
            },
            Ok(None),
        )
        .unwrap();
    assert!(opened.changed);
    assert_eq!(opened.context, expected);
    // The second caller reusing the same request id for another action is not
    // told the id was reused: the record belongs to the first capability.
    let selected = runtime
        .workspace_control_action(
            "local",
            &second,
            &expected,
            &request_id,
            Action::Select {
                view_id: opened.view_id.clone(),
                reveal: false,
            },
            Ok(None),
        )
        .unwrap();
    assert_eq!(selected.view_id, opened.view_id);
    let views = runtime
        .workspace_control_query("local", &second, Query::ViewList)
        .unwrap()
        .views
        .unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].target, "http://localhost:3000");

    // Unregistering the checkout refuses the checkout caller by name, not as a
    // moved pane.
    runtime.snapshot.navigator.workspaces[1].checkouts[0].path = "/checkouts/moved".to_owned();
    assert_eq!(
        runtime
            .workspace_control_action(
                "local",
                &second,
                &expected,
                &action_id("after-move"),
                Action::Close {
                    view_id: opened.view_id
                },
                Ok(None),
            )
            .unwrap_err()
            .reason,
        "checkout_not_registered"
    );
}
