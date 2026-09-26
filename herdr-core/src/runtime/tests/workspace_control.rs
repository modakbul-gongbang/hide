use super::*;
use crate::workspace_control::Query;

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
