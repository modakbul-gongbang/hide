use super::*;
use crate::workspace_views::ViewMode;

// PRD S6: a shell with separate Agent and View areas keeps each Workspace's
// document beside its terminals, its layout and tools, and brings them back
// after a restart; the Swift shell keeps its document-in-place-of-terminal
// rule (Risks: Swift wire).

fn views_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hide-workspace-views-{name}-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("state directory");
    dir.join("workspace-views.json")
}

fn with_views(mut runtime: Runtime, path: &Path) -> Runtime {
    runtime.workspace_views = Some(WorkspaceViewStore::open(path.to_path_buf()).0);
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

fn layout(runtime: &mut Runtime, payload: serde_json::Value) {
    runtime.dispatch_json(&explorer_event("workspace_view", payload));
    assert_eq!(runtime.snapshot.status.last_error, None);
}

fn mode(runtime: &Runtime) -> ViewMode {
    runtime
        .snapshot
        .workspace_view
        .as_ref()
        .expect("a front Workspace")
        .mode
}

fn active_label(runtime: &Runtime) -> Option<String> {
    let active = runtime.snapshot.editor.active_tab_id.as_deref()?;
    runtime
        .snapshot
        .editor
        .tabs
        .iter()
        .find(|tab| tab.id == active)
        .map(|tab| tab.label.clone())
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
/// its View area; the Swift shell's terminal still takes the canvas back.
#[test]
fn a_terminal_tab_choice_keeps_the_workspace_document_only_with_separate_areas() {
    for separate in [true, false] {
        let (runtime, checkout_id, directory) = strip_checkout("keeps-document");
        let mut runtime = if separate {
            with_views(runtime, &views_path("keeps-document"))
        } else {
            runtime
        };
        with_tabs(&mut runtime, &directory);
        open(&mut runtime, &checkout_id, &directory.join("notes.md"));
        assert_eq!(active_label(&runtime).as_deref(), Some("notes.md"));

        runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2"));

        if separate {
            assert_eq!(active_label(&runtime).as_deref(), Some("notes.md"));
            assert!(runtime.snapshot.editor.document.is_some());
        } else {
            assert_eq!(active_label(&runtime), None);
            assert!(runtime.snapshot.workspace_view.is_none());
        }
    }
}

/// D-08, B11, B12: an explicit file open from Agents-only and an explicit
/// agent choice from Views-only bring the other area back; a status update
/// moves nothing.
#[test]
fn explicit_opens_bring_the_hidden_area_back_and_status_changes_do_not() {
    let (runtime, checkout_id, directory) = strip_checkout("area-intent");
    let mut runtime = with_views(runtime, &views_path("area-intent"));
    with_tabs(&mut runtime, &directory);
    assert_eq!(mode(&runtime), ViewMode::Together);

    layout(&mut runtime, serde_json::json!({"mode": "agents"}));
    assert_eq!(mode(&runtime), ViewMode::Agents);
    open(&mut runtime, &checkout_id, &directory.join("notes.md"));
    assert_eq!(mode(&runtime), ViewMode::Together);

    layout(&mut runtime, serde_json::json!({"mode": "views"}));
    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &tabs,
        &tabs,
        "w-order:t2",
    )));
    assert_eq!(
        mode(&runtime),
        ViewMode::Views,
        "a session update is not a request"
    );
    runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2"));
    assert_eq!(mode(&runtime), ViewMode::Together);
}

/// D-05, A5: the tools are the Workspace's, and the one global panel every
/// existing reader gates on follows them.
#[test]
fn workspace_tools_drive_the_panel_the_changes_reader_gates_on() {
    let (runtime, _checkout_id, _directory) = strip_checkout("tools");
    let mut runtime = with_views(runtime, &views_path("tools"));
    layout(
        &mut runtime,
        serde_json::json!({"explorer": false, "changes": true}),
    );
    let view = runtime
        .snapshot
        .workspace_view
        .clone()
        .expect("front Workspace");
    assert!(!view.explorer && view.changes);
    assert!(runtime.snapshot.ui_state.right_panel_visible);
    assert_eq!(
        runtime.snapshot.ui_state.right_panel_section,
        RightPanelSection::Changes
    );

    layout(&mut runtime, serde_json::json!({"explorer": true}));
    assert_eq!(
        runtime.snapshot.ui_state.right_panel_section,
        RightPanelSection::Explorer
    );

    layout(
        &mut runtime,
        serde_json::json!({"explorer": false, "changes": false}),
    );
    assert!(!runtime.snapshot.ui_state.right_panel_visible);

    runtime.dispatch_json(&explorer_event(
        "workspace_view",
        serde_json::json!({"mode": "sideways"}),
    ));
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("workspace_view.unknown_mode")
    );
}

/// B19, B20: after a restart the Workspace comes back with its layout, its
/// tools, its View tabs and its active tab; a file that is gone comes back as
/// a tab that says why, and the others are untouched.
#[test]
fn a_restart_restores_the_workspace_and_marks_a_missing_file_unavailable() {
    let (runtime, checkout_id, directory) = strip_checkout("restart");
    let state = views_path("restart");
    let mut runtime = with_views(runtime, &state);
    std::fs::write(directory.join("gone.md"), "gone\n").expect("fixture");
    open(&mut runtime, &checkout_id, &directory.join("gone.md"));
    open(&mut runtime, &checkout_id, &directory.join("notes.md"));
    layout(
        &mut runtime,
        serde_json::json!({"mode": "views", "changes": true, "agent_share": 0.3}),
    );
    drop(runtime);
    std::fs::remove_file(directory.join("gone.md")).expect("remove fixture");

    let (restarted, checkout_id) = tab_order_runtime(&directory.to_string_lossy());
    let restarted = with_views(restarted, &state);

    let view = restarted
        .snapshot
        .workspace_view
        .clone()
        .expect("front Workspace");
    assert_eq!(view.mode, ViewMode::Views);
    assert!(view.changes);
    assert!((view.agent_share - 0.3).abs() < f32::EPSILON);
    let tabs: Vec<_> = restarted
        .snapshot
        .editor
        .tabs
        .iter()
        .filter(|tab| tab.checkout_id == checkout_id)
        .map(|tab| (tab.label.clone(), tab.unavailable_reason.is_some()))
        .collect();
    assert_eq!(
        tabs,
        vec![("gone.md".to_owned(), true), ("notes.md".to_owned(), false)]
    );
    assert_eq!(active_label(&restarted).as_deref(), Some("notes.md"));
}

/// D-10: a file this build cannot read is left for the operator and the
/// defaults load with a diagnostic, never a screen alert.
#[test]
fn an_unreadable_views_file_is_kept_and_reported_in_the_diagnostic_log() {
    let state = views_path("unreadable");
    std::fs::write(&state, b"{broken").expect("fixture");
    let (store, diagnostic) = WorkspaceViewStore::open(state.clone());
    drop(store);
    assert_eq!(
        diagnostic.map(|(kind, _)| kind),
        Some("workspace_views.unreadable")
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
