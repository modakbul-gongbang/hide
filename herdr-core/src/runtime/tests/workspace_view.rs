use super::*;
use crate::workspace_views::Tool;

// PRD S6: a shell with separate Agent and View areas keeps each Workspace's
// document beside its terminals, its layout and tools, and brings them back
// after a restart; a shell without View areas keeps its
// document-in-place-of-terminal rule (Risks: older wire).

/// A new folder for a views file, and that file's path in it; whoever keeps
/// the folder keeps the file.
pub(super) fn views_path(name: &str) -> (tempfile::TempDir, PathBuf) {
    let folder = scratch_dir(&format!("hide-workspace-views-{name}-"));
    let path = folder.path().join("workspace-views.json");
    (folder, path)
}

/// `with_views` on a new views file whose folder the runtime keeps.
pub(super) fn with_new_views(runtime: Runtime, name: &str) -> Runtime {
    let (folder, path) = views_path(name);
    let mut runtime = with_views(runtime, &path);
    runtime.test_dirs.push(folder);
    runtime
}

/// A daemon runtime whose local checkout roots are open.
pub(super) fn with_views(mut runtime: Runtime, path: &Path) -> Runtime {
    let roots = runtime
        .catalog_workspaces()
        .filter(|workspace| workspace.device_id == crate::node::TEST_NODE)
        .flat_map(|workspace| workspace.checkouts.iter())
        .filter_map(|checkout| {
            let path = PathBuf::from(&checkout.path);
            std::fs::File::open(&path).ok().map(|file| (path, file))
        })
        .collect();
    runtime.set_file_roots(crate::files::FileRoots::from_opened(roots));
    with_views_only(runtime, path)
}

/// The daemon's own order: the views file is open and the first snapshot is
/// read before any root is pinned.
pub(super) fn with_views_only(mut runtime: Runtime, path: &Path) -> Runtime {
    let panel = (
        runtime.snapshot.ui_state.right_panel_visible,
        runtime.snapshot.ui_state.right_panel_section,
    );
    runtime.workspace_views = Some(WorkspaceViewStore::open(path.to_path_buf(), panel).0);
    runtime.sync_workspace_view();
    runtime
}

fn open(runtime: &mut Runtime, checkout_id: &str, path: &Path) {
    assert!(runtime.dispatch_json(&explorer_event(
        "file_open",
        serde_json::json!({
            "path": path.to_string_lossy(),
            "workspace_id": "workspace:order",
            "checkout_id": checkout_id,
            "preview": false
        }),
    )));
}

pub(super) fn layout(runtime: &mut Runtime, payload: serde_json::Value) {
    runtime.dispatch_json(&explorer_event("workspace_view", payload));
    assert_eq!(runtime.snapshot.status.last_error, None);
}

/// Whether the front Workspace's File Views and Tools columns are on.
fn columns(runtime: &Runtime) -> (bool, bool) {
    let view = runtime
        .snapshot
        .workspace_view
        .as_ref()
        .expect("a front Workspace");
    (view.views, view.tools)
}

fn only_display(runtime: &Runtime) -> (serde_json::Value, String) {
    let view = runtime.snapshot.workspace_view.as_ref().unwrap();
    let workspace = serde_json::json!({"device_id": view.device_id, "path": view.path});
    let display = match &view.layout.root {
        crate::model::ViewNodeSnapshot::Area(area) => area.active.clone(),
        _ => None,
    }
    .expect("one display");
    (workspace, display)
}

fn close_only_display(runtime: &mut Runtime) {
    let (workspace, display) = only_display(runtime);
    runtime.dispatch_json(&explorer_event(
        "view_layout",
        serde_json::json!({"workspace": workspace, "action": "close", "display_id": display}),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
}

pub(super) fn active_label(runtime: &Runtime) -> Option<String> {
    let active = runtime.snapshot.editor.active_tab_id.as_deref()?;
    runtime
        .snapshot
        .editor
        .tabs
        .iter()
        .find(|tab| tab.id == active)
        .map(|tab| tab.label.clone())
}

#[test]
fn browser_inventory_revokes_removed_and_disconnected_catalog_checkouts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().to_string_lossy().into_owned();
    let mut runtime = runtime();
    runtime.snapshot.status.herdr.state = "connected".into();
    let project = workspace(
        "inventory-project",
        "Inventory",
        &path,
        vec![checkout(
            "inventory-project",
            "inventory-checkout",
            &path,
            None,
        )],
    );
    runtime.snapshot.navigator.workspaces = vec![project.clone()];
    runtime = with_views_only(runtime, &directory.path().join("views.json"));
    let store = runtime.workspace_views.as_mut().unwrap();
    let layout = &mut store.views.entry(crate::node::TEST_NODE, &path).layout;
    let browser = layout.new_browser_display("https://example.test/inventory", 1);
    let id = browser.id.clone();
    layout.insert("a1", browser, 1).unwrap();
    store.generation += 1;
    runtime.sync_workspace_view();
    assert_eq!(runtime.snapshot.browser_views.len(), 1);
    assert_eq!(runtime.snapshot.browser_views[0].view_id, id);
    assert_eq!(runtime.snapshot.browser_views[0].area_id, "a1");
    assert_eq!(runtime.snapshot.browser_scopes.len(), 1);
    assert_eq!(runtime.snapshot.browser_scopes[0].device_id, crate::node::TEST_NODE);
    assert_eq!(runtime.snapshot.browser_scopes[0].path, path);
    assert_eq!(runtime.snapshot.browser_scopes[0].area_id, "a1");
    let original_incarnation = runtime.snapshot.browser_scopes[0].incarnation;
    let published = runtime.snapshot.browser_views_revision;
    runtime.sync_workspace_view();
    assert_eq!(
        runtime.snapshot.browser_views_revision, published,
        "unchanged scope does not publish again"
    );
    assert_eq!(
        runtime.snapshot.browser_scopes[0].incarnation,
        original_incarnation
    );

    runtime.snapshot.navigator.workspaces.clear();
    runtime.sync_workspace_view();
    assert!(
        runtime.snapshot.browser_views.is_empty(),
        "removed catalog cannot retain a page"
    );
    assert_ne!(runtime.snapshot.browser_views_revision, published);
    assert!(runtime.snapshot.browser_scopes.is_empty());
    assert!(
        runtime
            .workspace_views
            .as_ref()
            .unwrap()
            .views
            .get(crate::node::TEST_NODE, &path)
            .unwrap()
            .layout
            .display(&id)
            .is_some(),
        "saved layout remains restorable"
    );

    runtime.snapshot.navigator.workspaces = vec![project];
    runtime.snapshot.status.herdr.state = "disconnected".into();
    runtime.sync_workspace_view();
    assert!(
        runtime.snapshot.browser_views.is_empty(),
        "offline saved layout has no page authority"
    );
    assert!(runtime.snapshot.browser_scopes.is_empty());
    runtime.snapshot.status.herdr.state = "connected".into();
    runtime.sync_workspace_view();
    assert_eq!(
        runtime.snapshot.browser_views.len(),
        1,
        "reconnected checkout gets fresh inventory"
    );
    assert_eq!(runtime.snapshot.browser_scopes.len(), 1);
    let regranted_incarnation = runtime.snapshot.browser_scopes[0].incarnation;
    assert_ne!(
        regranted_incarnation, original_incarnation,
        "revoke/regrant cannot preserve a capability even if no intervening frame was sent"
    );

    let store = runtime.workspace_views.as_mut().unwrap();
    store
        .views
        .entry(crate::node::TEST_NODE, &path)
        .layout
        .remove(&id)
        .unwrap();
    store.generation += 1;
    runtime.sync_workspace_view();
    assert!(runtime.snapshot.browser_views.is_empty());
    assert_eq!(
        runtime.snapshot.browser_scopes.len(),
        1,
        "closing the final page preserves an authorized empty area"
    );
    assert_eq!(
        runtime.snapshot.browser_scopes[0].incarnation, regranted_incarnation,
        "page closure does not retire the still-authorized area"
    );
}

#[test]
fn browser_authority_regrant_survives_coalesced_session_updates() {
    let (runtime, _, directory) = strip_checkout("browser-authority-regrant");
    let path = directory.to_string_lossy().into_owned();
    let mut runtime = with_views(runtime, &directory.join("views.json"));
    with_tabs(&mut runtime, &directory);
    let store = runtime.workspace_views.as_mut().unwrap();
    let layout = &mut store.views.entry(crate::node::TEST_NODE, &path).layout;
    let browser = layout.new_browser_display("https://example.test/scoped", 1);
    layout.insert("a1", browser, 1).unwrap();
    store.generation += 1;
    let initial = runtime.snapshot_delta_payload(0, 0);
    assert_eq!(runtime.snapshot.browser_views.len(), 1);
    let initial_incarnation = runtime.snapshot.browser_scopes[0].incarnation;

    assert!(runtime.ingest_session(Err(SessionFetchError::Unreachable(
        "fixture connection lost".into()
    ))));
    assert!(runtime.snapshot.browser_scopes.is_empty());
    // No snapshot is read between the failure and recovery, as with two
    // worker updates under one latched notification.
    with_tabs(&mut runtime, &directory);
    let recovered = runtime.snapshot_delta_payload(initial.revision, 0);
    assert_ne!(
        runtime.snapshot.browser_scopes[0].incarnation, initial_incarnation,
        "a native client must observe a new grant even without an absent frame"
    );
    let recovered_incarnation = runtime.snapshot.browser_scopes[0].incarnation;
    let tabs = ["w-order:t1", "w-order:t2"];
    assert!(
        !runtime.ingest_session(Ok(tab_order_payload(
            &directory.to_string_lossy(),
            &tabs,
            &tabs,
            "w-order:t1"
        ))),
        "an unchanged session must not publish another change"
    );
    runtime.snapshot_delta_payload(recovered.revision, 0);
    assert_eq!(
        runtime.snapshot.browser_scopes[0].incarnation, recovered_incarnation,
        "unchanged session updates preserve authority"
    );

    assert!(runtime.ingest_session(Err(SessionFetchError::Unreachable(
        "fixture connection lost again".into()
    ))));
    let revoked = runtime.snapshot_delta_payload(recovered.revision, 0);
    let wire: serde_json::Value =
        serde_json::from_slice(&serialize_snapshot_delta(&revoked).unwrap()).unwrap();
    assert_eq!(wire["rest"]["browser_scopes"], serde_json::json!([]));
    assert_eq!(wire["rest"]["browser_views"], serde_json::json!([]));
}

#[test]
fn browser_authority_records_remote_session_and_catalog_revocations_before_reads() {
    let directory = tempfile::tempdir().unwrap();
    let path = "/browser-authority-remote-fixture";
    let mut runtime = with_views_only(runtime(), &directory.path().join("views.json"));
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: "mini".into(),
        state: "connecting".into(),
        message: None,
        herdr_version: None,
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    let mut project = workspace(
        "remote-workspace",
        "Remote fixture",
        path,
        vec![checkout("remote-workspace", "remote-checkout", path, None)],
    );
    project.device_id = "mini".into();
    project.remote_target_id = Some("mini".into());
    let raw = RemoteSessionSnapshot {
        workspaces: vec![project],
        agents: Vec::new(),
        active_tab_ids: Default::default(),
        focused_workspace_id: None,
        focused_checkout_id: None,
        focused_tab_id: None,
        focused_pane_id: None,
        pane_layouts: Vec::new(),
        pane_hook_tokens: Default::default(),
    };
    let store = runtime.workspace_views.as_mut().unwrap();
    store.views.entry("mini", path);
    store.generation += 1;
    assert!(runtime.ingest_remote_session("mini", Ok(raw.clone())));
    let initial = runtime.snapshot_delta_payload(0, 0);
    assert_eq!(runtime.snapshot.browser_scopes.len(), 1);
    let initial_incarnation = runtime.snapshot.browser_scopes[0].incarnation;

    assert!(runtime.ingest_remote_session(
        "mini",
        Err(SessionFetchError::Unreachable("fixture device lost".into()))
    ));
    assert!(runtime.snapshot.browser_scopes.is_empty());
    assert!(runtime.ingest_remote_session("mini", Ok(raw.clone())));
    let recovered = runtime.snapshot_delta_payload(initial.revision, 0);
    let recovered_incarnation = runtime.snapshot.browser_scopes[0].incarnation;
    assert_ne!(recovered_incarnation, initial_incarnation);

    runtime
        .device_raw_sessions
        .get_mut("mini")
        .unwrap()
        .workspaces
        .clear();
    assert!(runtime.refresh_device_catalog("mini"));
    assert!(runtime.snapshot.browser_scopes.is_empty());
    runtime.device_raw_sessions.insert("mini".into(), raw);
    assert!(runtime.refresh_device_catalog("mini"));
    runtime.snapshot_delta_payload(recovered.revision, 0);
    assert_ne!(
        runtime.snapshot.browser_scopes[0].incarnation, recovered_incarnation,
        "a catalog loss/regrant also retires the old grant before a read"
    );
}

fn with_tabs(runtime: &mut Runtime, directory: &Path) {
    let tabs = ["w-order:t1", "w-order:t2"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &tabs,
        &tabs,
        "w-order:t1"
    ))));
}

/// D-04, B8: choosing another Agent tab leaves the Workspace's document in
/// its View area; without View areas the terminal still takes the canvas back.
#[test]
fn a_terminal_tab_choice_keeps_the_workspace_document_only_with_separate_areas() {
    for separate in [true, false] {
        let (runtime, checkout_id, directory) = strip_checkout("keeps-document");
        let mut runtime = if separate {
            with_new_views(runtime, "keeps-document")
        } else {
            runtime
        };
        with_tabs(&mut runtime, &directory);
        open(&mut runtime, &checkout_id, &directory.join("notes.md"));
        assert_eq!(active_label(&runtime).as_deref(), Some("notes.md"));

        runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2"));

        if separate {
            assert_eq!(active_label(&runtime).as_deref(), Some("notes.md"));
            let shown = runtime
                .snapshot_delta_payload(0, 0)
                .documents
                .map(|documents| documents.visible)
                .unwrap_or_default();
            assert_eq!(shown, vec![runtime.snapshot.editor.tabs[0].id.clone()]);
        } else {
            assert_eq!(active_label(&runtime), None);
            assert!(runtime.snapshot.workspace_view.is_none());
        }
    }
}

/// PRD three-column-panel B2, B4, B22, D-17: a new Workspace shows Agent
/// Views alone; an explicit file open turns File Views on and the file lands
/// in it; a session update, and an agent or tab chosen from anywhere, change
/// no column.
#[test]
fn a_file_open_turns_file_views_on_and_an_agent_choice_changes_no_column() {
    let (runtime, checkout_id, directory) = strip_checkout("area-intent");
    let mut runtime = with_new_views(runtime, "area-intent");
    with_tabs(&mut runtime, &directory);
    assert_eq!(columns(&runtime), (false, false), "Agent Views alone");
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Tool::Explorer
    );

    open(&mut runtime, &checkout_id, &directory.join("notes.md"));
    assert_eq!(columns(&runtime), (true, false));
    assert_eq!(active_label(&runtime).as_deref(), Some("notes.md"));

    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &tabs,
        &tabs,
        "w-order:t2",
    )));
    assert_eq!(
        columns(&runtime),
        (true, false),
        "a session update is not a request"
    );
    runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2"));
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(
        columns(&runtime),
        (true, false),
        "an agent choice moves no column"
    );
    layout(&mut runtime, serde_json::json!({"tools": true}));
    runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t1"));
    assert_eq!(columns(&runtime), (true, true));
}

/// PRD three-column-panel D-05, B11, B12, B13, B21: each column turns on and
/// off by its own result value, the same value twice lands once, turning one
/// off keeps the other and the View tree, and the widths are stored per
/// Workspace within the core's bound.
#[test]
fn each_column_toggles_by_its_own_result_value_and_keeps_the_other() {
    let (runtime, checkout_id, directory) = strip_checkout("columns");
    let (_views, state) = views_path("columns");
    let mut runtime = with_views(runtime, &state);
    with_tabs(&mut runtime, &directory);
    open(&mut runtime, &checkout_id, &directory.join("notes.md"));
    layout(&mut runtime, serde_json::json!({"tools": true}));
    assert_eq!(columns(&runtime), (true, true));

    layout(&mut runtime, serde_json::json!({"views": false}));
    assert_eq!(columns(&runtime), (false, true), "Tools stays");
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .layout
            .display_count,
        1,
        "turning File Views off closes no view"
    );
    // The same result sent again changes nothing and reports no change.
    assert!(!runtime.dispatch_json(&explorer_event(
        "workspace_view",
        serde_json::json!({"views": false})
    )));
    assert_eq!(columns(&runtime), (false, true));
    layout(&mut runtime, serde_json::json!({"views": true}));
    assert_eq!(columns(&runtime), (true, true));
    assert_eq!(active_label(&runtime).as_deref(), Some("notes.md"));
    layout(&mut runtime, serde_json::json!({"tools": false}));
    assert_eq!(columns(&runtime), (true, false), "File Views stays");

    layout(
        &mut runtime,
        serde_json::json!({"views_width": 700, "tools_width": 300}),
    );
    layout(&mut runtime, serde_json::json!({"tools_width": 99_999}));
    let view = runtime.snapshot.workspace_view.as_ref().unwrap();
    assert_eq!(
        (view.views_width, view.tools_width),
        (Some(700), Some(crate::workspace_views::MAX_COLUMN_WIDTH))
    );
    let (saved, _) = crate::workspace_views::load(&state, 0);
    let saved = saved.get(&view.device_id, &view.path).unwrap();
    assert_eq!(
        (saved.views, saved.tools, saved.views_width),
        (true, false, Some(700))
    );
    let written = std::fs::read_to_string(&state).unwrap();
    for retired in ["\"panel\"", "pinned", "views_over_share", "covered"] {
        assert!(!written.contains(retired), "{retired} is not written");
    }
}

/// PRD three-column-panel D-10, B16: File Views turned on with no view opens
/// one New tab page in the same event, and closing that untouched tab turns
/// File Views off again.
#[test]
fn file_views_turned_on_with_no_view_opens_a_new_tab_page() {
    let (runtime, _checkout_id, _directory) = strip_checkout("views-empty");
    let mut runtime = with_new_views(runtime, "views-empty");
    layout(&mut runtime, serde_json::json!({"views": true}));
    let view = runtime.snapshot.workspace_view.as_ref().unwrap();
    assert!(view.views);
    assert_eq!(view.layout.display_count, 1);
    assert!(
        runtime.new_tab_visible(),
        "the one view is the New tab page"
    );
    // The same request again opens no second page.
    layout(&mut runtime, serde_json::json!({"views": true}));
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .layout
            .display_count,
        1
    );

    close_only_display(&mut runtime);
    assert_eq!(columns(&runtime), (false, false));
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .layout
            .display_count,
        0
    );
}

/// Issue 170, revised, and PRD three-column-panel B5, B12: Tools holds one
/// tool; choosing one turns Tools on with it, a reveal turns Tools on with
/// the Explorer and leaves File Views as it was, and `reveal_path` (a
/// terminal link) turns File Views on with the file it opens and
/// leaves Tools as it was (B4); a folder, which only the Explorer shows,
/// turns Tools on and leaves File Views as it was.
#[test]
fn a_tool_or_a_reveal_turns_tools_on_and_leaves_file_views_alone() {
    let (runtime, _checkout_id, directory) = strip_checkout("tool-opens");
    let mut runtime = with_new_views(runtime, "tool-opens");
    let tools = |runtime: &Runtime| {
        let view = runtime.snapshot.workspace_view.as_ref().unwrap();
        (view.tool, view.tools)
    };
    assert_eq!(tools(&runtime), (Tool::Explorer, false));
    assert!(!runtime.snapshot.ui_state.right_panel_visible);

    layout(&mut runtime, serde_json::json!({"tool": "changes"}));
    assert_eq!(columns(&runtime), (false, true), "Tools alone is a state");
    assert_eq!(tools(&runtime), (Tool::Changes, true));
    assert!(runtime.snapshot.ui_state.right_panel_visible);
    layout(&mut runtime, serde_json::json!({"tool": "explorer"}));
    assert_eq!(tools(&runtime), (Tool::Explorer, true));

    // Turning Tools off keeps the tool for the next time.
    layout(&mut runtime, serde_json::json!({"tools": false}));
    assert_eq!(tools(&runtime), (Tool::Explorer, false));
    assert!(!runtime.snapshot.ui_state.right_panel_visible);

    layout(
        &mut runtime,
        serde_json::json!({"tool": "changes", "tools": false}),
    );
    assert_eq!(tools(&runtime), (Tool::Changes, false));
    layout(
        &mut runtime,
        serde_json::json!({"reveal": directory.join("notes.md").to_string_lossy()}),
    );
    assert_eq!(columns(&runtime), (false, true), "a reveal opens nothing");
    assert_eq!(tools(&runtime), (Tool::Explorer, true));

    layout(&mut runtime, serde_json::json!({"tools": false}));
    let checkout_id = runtime
        .snapshot
        .navigator
        .focused_checkout_id
        .clone()
        .unwrap();
    runtime.dispatch_json(&explorer_event(
        "reveal_path",
        serde_json::json!({
            "path": directory.join("notes.md"), "workspace_id": runtime.snapshot.navigator.focused_workspace_id,
            "checkout_id": checkout_id, "is_directory": false,
        }),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(
        columns(&runtime),
        (true, false),
        "a linked file leaves Tools off"
    );
    assert_eq!(tools(&runtime), (Tool::Explorer, false));
    assert!(!runtime.snapshot.ui_state.right_panel_visible);

    // A linked folder opens nothing in File Views: it turns Tools on with
    // the Explorer, leaves File Views off and is no File Views call.
    layout(&mut runtime, serde_json::json!({"views": false}));
    let folder = directory.join("docs");
    std::fs::create_dir_all(&folder).unwrap();
    let calls = runtime.workspace_views.as_ref().unwrap().views_calls;
    runtime.dispatch_json(&explorer_event(
        "reveal_path",
        serde_json::json!({
            "path": folder, "workspace_id": runtime.snapshot.navigator.focused_workspace_id,
            "checkout_id": checkout_id, "is_directory": true,
        }),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(
        columns(&runtime),
        (false, true),
        "a linked folder shows in the Explorer alone"
    );
    assert_eq!(tools(&runtime), (Tool::Explorer, true));
    assert_eq!(
        runtime.workspace_views.as_ref().unwrap().views_calls,
        calls,
        "a linked folder is no File Views call"
    );
}

/// D-05, A5: the tool is the Workspace's, and the one global panel every
/// existing reader gates on follows it; an unknown value is refused.
#[test]
fn workspace_tools_drive_the_panel_the_changes_reader_gates_on() {
    let (runtime, _checkout_id, _directory) = strip_checkout("tools");
    let mut runtime = with_new_views(runtime, "tools");
    layout(&mut runtime, serde_json::json!({"tool": "changes"}));
    assert!(runtime.snapshot.ui_state.right_panel_visible);
    assert_eq!(
        runtime.snapshot.ui_state.right_panel_section,
        RightPanelSection::Changes
    );

    layout(&mut runtime, serde_json::json!({"tool": "explorer"}));
    assert_eq!(
        runtime.snapshot.ui_state.right_panel_section,
        RightPanelSection::Explorer
    );

    layout(&mut runtime, serde_json::json!({"tools": false}));
    assert!(!runtime.snapshot.ui_state.right_panel_visible);

    runtime.dispatch_json(&explorer_event(
        "workspace_view",
        serde_json::json!({"tool": "terminal"}),
    ));
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("workspace_view.unknown_tool")
    );
    assert!(!runtime.snapshot.ui_state.right_panel_visible);
}

/// B19: the daemon opens a checkout's root after the first snapshot names
/// it, so a restore waits for that root instead of reading too early, around
/// the pinned root, or marking every saved file unavailable; the tabs it has not restored yet
/// are not overwritten while it waits.
#[test]
fn a_restore_waits_for_the_daemon_to_open_the_checkout_root() {
    let (runtime, checkout_id, directory) = strip_checkout("waits-for-root");
    let (_views, state) = views_path("waits-for-root");
    let mut runtime = with_views(runtime, &state);
    // The test keeps the checkout folder: it restarts on that checkout after
    // the runtime is gone.
    let _checkout = hold_dirs(&mut runtime);
    open(&mut runtime, &checkout_id, &directory.join("notes.md"));
    drop(runtime);

    let (restarted, checkout_id) = tab_order_runtime(&directory.to_string_lossy());
    let mut restarted = with_views_only(restarted, &state);
    let restored = |runtime: &Runtime| {
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .filter(|tab| tab.checkout_id == checkout_id)
            .map(|tab| (tab.label.clone(), tab.unavailable_reason.is_some()))
            .collect::<Vec<_>>()
    };
    assert!(
        restored(&restarted).is_empty(),
        "nothing is read before the root is open"
    );
    restarted.sync_workspace_view();
    let waiting = |runtime: &Runtime| {
        let layout = &runtime.snapshot.workspace_view.as_ref().unwrap().layout;
        let crate::model::ViewNodeSnapshot::Area(area) = &layout.root else {
            panic!("one area");
        };
        area.displays
            .iter()
            .map(|display| (display.label.clone(), display.state, display.reason.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        waiting(&restarted),
        vec![(
            "notes.md".to_owned(),
            crate::model::ViewDisplayState::Waiting,
            Some("Waiting for this checkout to open".to_owned())
        )],
        "the saved display is kept, and says what it waits for"
    );
    assert!(
        !restarted.set_file_roots(crate::files::FileRoots::from_opened(Vec::new())),
        "a root set that does not hold the checkout still waits"
    );

    let root = std::fs::File::open(&directory).expect("open root");
    assert!(
        restarted.set_file_roots(crate::files::FileRoots::from_opened(vec![(
            directory.clone(),
            root
        )]))
    );
    assert_eq!(restored(&restarted), vec![("notes.md".to_owned(), false)]);
    restarted.sync_workspace_view();
    assert_eq!(
        waiting(&restarted),
        vec![(
            "notes.md".to_owned(),
            crate::model::ViewDisplayState::Open,
            None
        )]
    );
    assert!(
        !restarted.set_file_roots(crate::files::FileRoots::from_opened(Vec::new())),
        "a Workspace is restored once per process"
    );
}

/// D-10: a file this build cannot read is left for the operator and the
/// defaults load with a diagnostic, never a screen alert.
#[test]
fn an_unreadable_views_file_is_kept_and_reported_in_the_diagnostic_log() {
    let (_views, state) = views_path("unreadable");
    std::fs::write(&state, b"{broken").expect("fixture");
    let (store, diagnostics) = WorkspaceViewStore::open(state.clone(), Default::default());
    drop(store);
    assert_eq!(
        diagnostics
            .into_iter()
            .map(|(kind, _)| kind)
            .collect::<Vec<_>>(),
        vec!["workspace_views.unreadable"]
    );
    assert!(!state.exists());
    let kept = std::fs::read_dir(state.parent().unwrap())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("workspace-views.json.unreadable-")
        });
    assert!(kept);
}

/// B8: coming back to a Workspace that has no Herdr tab shows the View tab
/// that was active there, not the newest one.
#[test]
fn returning_to_a_workspace_without_agent_tabs_keeps_its_active_view_tab() {
    let (runtime, checkout_id, directory) = strip_checkout("return-active");
    let mut runtime = with_new_views(runtime, "return-active");
    let other = directory.with_file_name(format!(
        "{}-other",
        directory.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&other).expect("second checkout");
    runtime
        .snapshot
        .ui_state
        .workspace_registrations
        .push(WorkspaceRegistration {
            primary_checkout_id: None,
            id: "workspace:other".to_owned(),
            label: "other".to_owned(),
            path: other.to_string_lossy().into_owned(),
            device_id: crate::node::TEST_NODE.to_owned(),
            pinned: false,
            home: false,
        });
    runtime.rebuild_catalog();
    let other_checkout = workspace::checkout_id_for_path("workspace:other", &other);
    std::fs::write(directory.join("later.md"), "later\n").expect("second file");
    open(&mut runtime, &checkout_id, &directory.join("notes.md"));
    open(&mut runtime, &checkout_id, &directory.join("later.md"));
    let notes = runtime
        .snapshot
        .editor
        .tabs
        .iter()
        .find(|tab| tab.label == "notes.md")
        .expect("notes tab")
        .id
        .clone();
    assert!(runtime.dispatch_json(&explorer_event(
        "file_focus",
        serde_json::json!({"tab_id": notes})
    )));
    assert_eq!(active_label(&runtime).as_deref(), Some("notes.md"));

    let focus = |workspace_id: &str, checkout_id: &str| {
        explorer_event(
            "focus_checkout",
            serde_json::json!({"workspace_id": workspace_id, "checkout_id": checkout_id}),
        )
    };
    runtime.dispatch_json(&focus("workspace:other", &other_checkout));
    runtime.dispatch_json(&focus("workspace:order", &checkout_id));

    assert_eq!(active_label(&runtime).as_deref(), Some("notes.md"));
}

/// B10, AGENTS.md one event per action: revealing a file shows the column on the Explorer
/// and unfolds its folders in the same `workspace_view`, and a path outside
/// the Workspace is refused without showing anything.
#[test]
fn a_reveal_shows_the_explorer_and_unfolds_the_folders_in_one_event() {
    let (runtime, _checkout_id, directory) = strip_checkout("reveal");
    let mut runtime = with_new_views(runtime, "reveal");
    layout(&mut runtime, serde_json::json!({"tools": false}));
    let root = directory.to_string_lossy().into_owned();

    runtime.dispatch_json(&explorer_event(
        "workspace_view",
        serde_json::json!({"reveal": "/elsewhere/a.txt"}),
    ));
    assert!(runtime.snapshot.status.last_error.is_some());
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().tools);
    runtime.dispatch_json(&explorer_event(
        "workspace_view",
        serde_json::json!({"reveal": format!("{root}/../elsewhere/a.txt")}),
    ));
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("workspace_view.reveal_outside"),
        "a path that climbs out of the Workspace is refused, not unfolded"
    );
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().tools);

    layout(
        &mut runtime,
        serde_json::json!({"reveal": format!("{root}/src/deep/a.rs")}),
    );
    assert!(runtime.snapshot.workspace_view.as_ref().unwrap().tools);
    let expanded = &runtime.snapshot.ui_state.expanded_paths;
    assert!(expanded.contains(&format!("{root}/src")));
    assert!(expanded.contains(&format!("{root}/src/deep")));
}

/// The daemon's second checkout, registered beside the strip checkout.
pub(super) fn second_checkout(runtime: &mut Runtime, directory: &Path) -> (PathBuf, String) {
    let other = directory.with_file_name(format!(
        "{}-second",
        directory.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&other).expect("second checkout");
    runtime
        .snapshot
        .ui_state
        .workspace_registrations
        .push(WorkspaceRegistration {
            primary_checkout_id: None,
            id: "workspace:other".to_owned(),
            label: "other".to_owned(),
            path: other.to_string_lossy().into_owned(),
            device_id: crate::node::TEST_NODE.to_owned(),
            pinned: false,
            home: false,
        });
    runtime.rebuild_catalog();
    let checkout = workspace::checkout_id_for_path("workspace:other", &other);
    (other, checkout)
}

fn saved_displays(runtime: &Runtime, path: &Path) -> Vec<String> {
    runtime
        .workspace_views
        .as_ref()
        .and_then(|store| store.views.get(crate::node::TEST_NODE, &path.to_string_lossy()))
        .map(|entry| {
            entry
                .layout
                .displays()
                .map(|display| display.path.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// B19: a restore opens into the Workspace in front, so when the front moved
/// after the last sync saw it, nothing is opened until the sync catches up;
/// otherwise one Workspace's saved documents would land in another.
#[test]
fn a_restore_waits_while_the_front_moved_since_the_last_sync() {
    let (runtime, checkout_id, directory) = strip_checkout("front-moved");
    let (_views, state) = views_path("front-moved");
    let mut runtime = with_views(runtime, &state);
    // The test keeps the checkout folder: it restarts on that checkout after
    // the runtime is gone.
    let _checkout = hold_dirs(&mut runtime);
    open(&mut runtime, &checkout_id, &directory.join("notes.md"));
    drop(runtime);

    let (restarted, _) = tab_order_runtime(&directory.to_string_lossy());
    let mut restarted = with_views_only(restarted, &state);
    let (other, other_checkout) = second_checkout(&mut restarted, &directory);
    restarted.snapshot.navigator.focused_workspace_id = Some("workspace:other".to_owned());
    restarted.snapshot.navigator.focused_checkout_id = Some(other_checkout.clone());

    let roots = [directory.clone(), other.clone()]
        .into_iter()
        .map(|path| {
            let file = std::fs::File::open(&path).expect("open root");
            (path, file)
        })
        .collect();
    assert!(
        !restarted.set_file_roots(crate::files::FileRoots::from_opened(roots)),
        "the Workspace the sync saw is no longer in front"
    );
    assert!(
        restarted
            .snapshot
            .editor
            .tabs
            .iter()
            .all(|tab| tab.checkout_id != other_checkout),
        "no saved tab lands in the other Workspace"
    );
    restarted.sync_workspace_view();
    assert_eq!(
        saved_displays(&restarted, &directory),
        vec![directory.join("notes.md").to_string_lossy().into_owned()],
        "the Workspace that did not come back keeps its saved displays"
    );
}

/// B20: a restored tab whose file was missing reads it again when the file
/// is opened after it came back, rather than saying it is unavailable.
#[test]
fn opening_a_file_that_came_back_clears_its_unavailable_tab() {
    let (runtime, checkout_id, directory) = strip_checkout("came-back");
    let (_views, state) = views_path("came-back");
    let mut runtime = with_views(runtime, &state);
    // The test keeps the checkout folder: it restarts on that checkout after
    // the runtime is gone.
    let _checkout = hold_dirs(&mut runtime);
    std::fs::write(directory.join("gone.md"), "gone\n").expect("fixture");
    open(&mut runtime, &checkout_id, &directory.join("gone.md"));
    drop(runtime);
    std::fs::remove_file(directory.join("gone.md")).expect("remove fixture");
    let (restarted, checkout_id) = tab_order_runtime(&directory.to_string_lossy());
    let mut restarted = with_views(restarted, &state);
    let gone = |runtime: &Runtime| {
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .filter(|tab| tab.label == "gone.md")
            .map(|tab| tab.unavailable_reason.is_some())
            .collect::<Vec<_>>()
    };
    assert_eq!(gone(&restarted), vec![true]);

    std::fs::write(directory.join("gone.md"), "back\n").expect("fixture");
    open(&mut restarted, &checkout_id, &directory.join("gone.md"));

    assert_eq!(gone(&restarted), vec![false]);
    assert_eq!(active_label(&restarted).as_deref(), Some("gone.md"));
    let shown = restarted
        .snapshot_delta_payload(0, 0)
        .documents
        .expect("a fresh reader gets the documents on screen");
    assert_eq!(
        shown
            .changed
            .iter()
            .map(|(_, document)| document.contents_utf8.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("back\n")]
    );
}

/// D-11: the app opens on the Workspace the operator last chose, after a
/// reload or a restart; a first run, or a front only Herdr's own focus put
/// there, starts on Main.
#[test]
fn only_the_workspace_the_operator_chose_is_resumed() {
    let (runtime, checkout_id, directory) = strip_checkout("resumed");
    let (_views, state) = views_path("resumed");
    let mut runtime = with_views(runtime, &state);
    let resumed = |runtime: &Runtime| runtime.snapshot.workspace_view.as_ref().map(|v| v.resumed);
    assert_eq!(resumed(&runtime), Some(false), "a first run starts on Main");
    drop(runtime);

    let (restarted, _) = tab_order_runtime(&directory.to_string_lossy());
    runtime = with_views(restarted, &state);
    assert_eq!(
        resumed(&runtime),
        Some(false),
        "a front the operator never chose is not resumed"
    );
    runtime.dispatch_json(&explorer_event(
        "focus_checkout",
        serde_json::json!({"workspace_id": "workspace:order", "checkout_id": checkout_id}),
    ));
    assert_eq!(resumed(&runtime), Some(true), "a reload opens it");
    drop(runtime);

    let (restarted, _) = tab_order_runtime(&directory.to_string_lossy());
    let restarted = with_views(restarted, &state);
    assert_eq!(resumed(&restarted), Some(true), "so does a restart");
}

/// D-10, B20: the tools each Workspace shows drive the one right panel the
/// readers gate on, but the older settings file keeps the panel it had.
#[test]
fn workspace_tools_never_reach_the_older_settings_file() {
    let (runtime, _checkout_id, _directory) = strip_checkout("settings");
    let saved = (
        runtime.snapshot.ui_state.right_panel_visible,
        runtime.snapshot.ui_state.right_panel_section,
    );
    let mut runtime = with_new_views(runtime, "settings");
    layout(
        &mut runtime,
        serde_json::json!({"tool": "explorer", "tools": !saved.0}),
    );
    assert_eq!(runtime.snapshot.ui_state.right_panel_visible, !saved.0);
    let written = runtime.ui_state_to_save();
    assert_eq!(
        (written.right_panel_visible, written.right_panel_section),
        saved
    );
}

/// Removing a device forgets its Workspaces' layouts and View tabs with the
/// rest of Hide's record of it.
#[test]
fn removing_a_device_forgets_its_workspace_views() {
    let (runtime, _checkout_id, directory) = strip_checkout("device-views");
    let mut runtime = with_new_views(runtime, "device-views");
    runtime
        .snapshot
        .ui_state
        .device_registrations
        .push(crate::model::DeviceRegistration {
            id: "studio".to_owned(),
            label: "studio".to_owned(),
            ..Default::default()
        });
    let store = runtime.workspace_views.as_mut().unwrap();
    store.views.entry("studio", "/srv/app").views = true;
    store.views.entry(crate::node::TEST_NODE, &directory.to_string_lossy());
    runtime.apply_area_intent_to(
        &("studio".to_owned(), "/srv/app".to_owned()),
        AreaIntent::Views,
    );

    assert!(runtime.dispatch_json(&explorer_event(
        "remove_device",
        serde_json::json!({"device_id": "studio"}),
    )));

    let views = &runtime.workspace_views.as_ref().unwrap().views;
    assert!(views.get("studio", "/srv/app").is_none());
    assert!(views.get(crate::node::TEST_NODE, &directory.to_string_lossy()).is_some());
    assert!(
        runtime
            .workspace_views
            .as_ref()
            .unwrap()
            .views_called
            .keys()
            .all(|(device, _)| device != "studio")
    );
}

/// Column-call history follows the existing bounded Workspace retention.
#[test]
fn workspace_column_calls_forget_evicted_workspaces() {
    let (runtime, _, _) = strip_checkout("column-calls-cap");
    let mut runtime = with_new_views(runtime, "column-calls-cap");
    for index in 0..=crate::workspace_views::MAX_WORKSPACES {
        let key = ("studio".to_owned(), format!("/srv/workspace-{index}"));
        runtime.apply_area_intent_to(&key, AreaIntent::Views);
    }
    let store = runtime.workspace_views.as_ref().unwrap();
    assert!(store.views_called.len() <= crate::workspace_views::MAX_WORKSPACES);
    assert!(
        store
            .views_called
            .keys()
            .all(|(device, path)| store.views.get(device, path).is_some())
    );
    let last = (
        "studio".to_owned(),
        format!("/srv/workspace-{}", crate::workspace_views::MAX_WORKSPACES),
    );
    assert_eq!(store.views_called.get(&last), Some(&store.views_calls));
}

/// PRD three-column-panel D-10, B17: closing the last view turns File Views
/// off in the same transition and saves it, whatever Tools is, and Tools
/// stays as it was.
#[test]
fn closing_the_last_view_turns_file_views_off_and_keeps_tools() {
    for tools in [true, false] {
        for browser in [true, false] {
            let (runtime, checkout_id, directory) = strip_checkout("last-view-columns");
            let (_views, state) = views_path("last-view-columns");
            let mut runtime = with_views(runtime, &state);
            with_tabs(&mut runtime, &directory);
            if browser {
                runtime.dispatch_json(&explorer_event(
                    "browser_open",
                    serde_json::json!({"url": "about:blank"}),
                ));
            } else {
                open(&mut runtime, &checkout_id, &directory.join("notes.md"));
            }
            layout(&mut runtime, serde_json::json!({"tools": tools}));
            assert_eq!(columns(&runtime), (true, tools));
            close_only_display(&mut runtime);
            let view = runtime.snapshot.workspace_view.as_ref().unwrap();
            assert_eq!(
                (view.layout.display_count, view.views, view.tools),
                (0, false, tools)
            );
            let (saved, _) = crate::workspace_views::load(&state, 0);
            let saved = saved.get(&view.device_id, &view.path).unwrap();
            assert_eq!((saved.views, saved.tools), (false, tools));
        }
    }
}

/// B20: width acknowledgements identify the accepted intent, including a
/// no-op width, without adding UI bookkeeping to the saved Workspace file.
#[test]
fn workspace_width_requests_are_bounded_ephemeral_and_confirm_exact_intents() {
    let (runtime, _checkout_id, directory) = strip_checkout("width-requests");
    let (_views, state) = views_path("width-requests");
    let mut runtime = with_views(runtime, &state);
    // The test keeps the checkout folder: it restarts on that checkout after
    // the runtime is gone.
    let _checkout = hold_dirs(&mut runtime);
    with_tabs(&mut runtime, &directory);
    layout(
        &mut runtime,
        serde_json::json!({"views_width": 672, "width_request_id": "divider:first"}),
    );
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .width_request_id
            .as_deref(),
        Some("divider:first")
    );
    layout(
        &mut runtime,
        serde_json::json!({"views_width": 704, "width_request_id": "divider:second"}),
    );
    let published = runtime.snapshot.workspace_view.as_ref().unwrap();
    assert_eq!(published.views_width, Some(704));
    assert_eq!(
        published.width_request_id.as_deref(),
        Some("divider:second")
    );
    assert!(!runtime.dispatch_json(&explorer_event(
        "workspace_view",
        serde_json::json!({"views_width": 704, "width_request_id": "divider:second"})
    )));
    let saved = std::fs::read_to_string(&state).unwrap();
    assert!(runtime.dispatch_json(&explorer_event(
        "workspace_view",
        serde_json::json!({"views_width": 704, "width_request_id": "divider:third"})
    )));
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .width_request_id
            .as_deref(),
        Some("divider:third")
    );
    assert_eq!(std::fs::read_to_string(&state).unwrap(), saved);
    assert!(!saved.contains("width_request_id"));
    layout(&mut runtime, serde_json::json!({"tools": true}));
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .width_request_id
            .as_deref(),
        Some("divider:third")
    );
    layout(&mut runtime, serde_json::json!({"views_width": 650}));
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .width_request_id,
        None
    );
    for invalid in [String::new(), "x".repeat(129)] {
        assert!(runtime.dispatch_json(&explorer_event(
            "workspace_view",
            serde_json::json!({"views_width": 720, "width_request_id": invalid})
        )));
        assert_eq!(
            runtime.snapshot.status.last_error.as_ref().unwrap().kind,
            "workspace_view.invalid_width_request"
        );
        assert_eq!(
            runtime
                .snapshot
                .workspace_view
                .as_ref()
                .unwrap()
                .views_width,
            Some(650)
        );
    }
    let published = runtime.snapshot.workspace_view.as_ref().unwrap();
    let key = (published.device_id.clone(), published.path.clone());
    drop(runtime);
    let (reloaded, _) = crate::workspace_views::load(&state, 0);
    assert_eq!(reloaded.get(&key.0, &key.1).unwrap().views_width, Some(650));
    std::fs::remove_dir_all(state.parent().unwrap()).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}
