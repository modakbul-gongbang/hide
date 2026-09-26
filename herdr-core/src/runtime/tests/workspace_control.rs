use super::*;
use crate::workspace_control::{Action, Edge, Query};

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
        .workspace_control_action("local", "pane-b", &expected, &retry_id, action.clone())
        .unwrap();
    assert!(first.changed);
    let retry = runtime
        .workspace_control_action("local", "pane-b", &expected, &retry_id, action)
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
                }
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
                    view_id: view_id.clone()
                }
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
                action.clone()
            )
            .unwrap()
            .changed
    );
    assert!(
        !runtime
            .workspace_control_action("local", "pane-b", &expected, &action_id("close-2"), action)
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
