use super::*;
use crate::workspace_views::PanelState;

// PRD S6: a shell with separate Agent and View areas keeps each Workspace's
// document beside its terminals, its layout and tools, and brings them back
// after a restart; the Swift shell keeps its document-in-place-of-terminal
// rule (Risks: Swift wire).

pub(super) fn views_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hide-workspace-views-{name}-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("state directory");
    dir.join("workspace-views.json")
}

/// A daemon runtime whose local checkout roots are open.
pub(super) fn with_views(mut runtime: Runtime, path: &Path) -> Runtime {
    let roots = runtime
        .catalog_workspaces()
        .filter(|workspace| workspace.device_id == workspace::LOCAL_DEVICE_ID)
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

fn panel(runtime: &Runtime) -> (PanelState, bool) {
    let view = runtime
        .snapshot
        .workspace_view
        .as_ref()
        .expect("a front Workspace");
    (view.panel, view.pinned)
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

/// D-08, B11, B12, issue 170: an explicit file open opens a closed side
/// panel, an agent chosen from elsewhere closes one that floats over the
/// agents, and a status update moves nothing.
#[test]
fn explicit_opens_bring_the_hidden_area_back_and_status_changes_do_not() {
    let (runtime, checkout_id, directory) = strip_checkout("area-intent");
    let mut runtime = with_views(runtime, &views_path("area-intent"));
    with_tabs(&mut runtime, &directory);
    assert_eq!(
        panel(&runtime),
        (PanelState::Closed, false),
        "a new Workspace has no View to show yet"
    );

    open(&mut runtime, &checkout_id, &directory.join("notes.md"));
    assert_eq!(panel(&runtime), (PanelState::Open, false));

    layout(&mut runtime, serde_json::json!({"panel": "expanded"}));
    // B22: expanding the panel makes no split.
    assert!(matches!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .expect("a front Workspace")
            .layout
            .root,
        crate::model::ViewNodeSnapshot::Area(_)
    ));
    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &tabs,
        &tabs,
        "w-order:t2",
    )));
    assert_eq!(
        panel(&runtime),
        (PanelState::Expanded, false),
        "a session update is not a request"
    );
    runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2"));
    assert_eq!(panel(&runtime), (PanelState::Closed, false));
}

/// Issue 170: the side panel closes on an agent chosen from elsewhere and
/// opens again on request, closing no view; a choice among the agents beside
/// it leaves it up; a pinned panel sits beside the agents, so an agent choice
/// only brings an expanded one back to its width; and the file keeps the
/// panel, its pin and its width.
#[test]
fn the_side_panel_closes_and_opens_without_closing_a_view() {
    let (runtime, checkout_id, directory) = strip_checkout("views-over");
    let state = views_path("views-over");
    let mut runtime = with_views(runtime, &state);
    with_tabs(&mut runtime, &directory);
    open(&mut runtime, &checkout_id, &directory.join("notes.md"));
    assert_eq!(panel(&runtime), (PanelState::Open, false));

    // A session update is not a request: the panel stays up.
    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &tabs,
        &tabs,
        "w-order:t2",
    )));
    assert_eq!(panel(&runtime).0, PanelState::Open);
    // A tab chosen where it shows, beside the panel, leaves it up.
    let mut in_place: serde_json::Value =
        serde_json::from_slice(&focus_tab_event(&checkout_id, "w-order:t2")).unwrap();
    in_place["payload"]["in_place"] = serde_json::json!(true);
    runtime.dispatch_json(&serde_json::to_vec(&in_place).unwrap());
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(
        panel(&runtime).0,
        PanelState::Open,
        "a choice among the visible agents"
    );
    // One chosen from elsewhere closes it.
    runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2"));
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(panel(&runtime), (PanelState::Closed, false));
    assert_eq!(
        active_label(&runtime).as_deref(),
        Some("notes.md"),
        "closing the panel closes no view"
    );

    layout(
        &mut runtime,
        serde_json::json!({"panel": "open", "pinned": true, "views_over_share": 0.45}),
    );
    assert_eq!(panel(&runtime), (PanelState::Open, true));
    let (saved, _) = crate::workspace_views::load(&state, 0);
    assert!(
        saved
            .workspaces
            .iter()
            .any(|view| view.panel == PanelState::Open
                && view.pinned
                && view.views_over_share == 0.45),
        "the file keeps the panel open, pinned, at its width"
    );
    runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2"));
    assert_eq!(
        panel(&runtime),
        (PanelState::Open, true),
        "a pinned panel covers no agent"
    );
    layout(&mut runtime, serde_json::json!({"panel": "expanded"}));
    runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2"));
    assert_eq!(
        panel(&runtime),
        (PanelState::Open, true),
        "an expanded pinned panel goes back to its width"
    );
    layout(&mut runtime, serde_json::json!({"views_over_share": 3.0}));
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .views_over_share,
        crate::workspace_views::MAX_VIEWS_OVER_SHARE
    );

    // Closing the last view leaves the panel where it is: it narrows to the
    // Explorer, or says nothing is open.
    layout(&mut runtime, serde_json::json!({"pinned": false}));
    let view = runtime.snapshot.workspace_view.as_ref().unwrap();
    let workspace = serde_json::json!({"device_id": view.device_id, "path": view.path});
    let display = match &view.layout.root {
        crate::model::ViewNodeSnapshot::Area(area) => area.active.clone(),
        _ => None,
    }
    .expect("one display");
    runtime.dispatch_json(&explorer_event(
        "view_layout",
        serde_json::json!({"workspace": workspace, "action": "close", "display_id": display}),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
    let view = runtime.snapshot.workspace_view.as_ref().unwrap();
    assert_eq!(
        (view.layout.display_count, view.panel),
        (0, PanelState::Open)
    );
}

/// Issue 170: a tool shown while the panel is closed opens the panel with
/// it, a reveal too, and a payload that names the panel keeps its word.
#[test]
fn a_tool_shown_while_the_panel_is_closed_opens_the_panel() {
    let (runtime, _checkout_id, directory) = strip_checkout("tool-opens");
    let mut runtime = with_views(runtime, &views_path("tool-opens"));
    layout(&mut runtime, serde_json::json!({"explorer": false}));
    assert_eq!(panel(&runtime).0, PanelState::Closed);
    assert!(!runtime.snapshot.ui_state.right_panel_visible);

    layout(&mut runtime, serde_json::json!({"changes": true}));
    assert_eq!(panel(&runtime).0, PanelState::Open);
    assert!(runtime.snapshot.ui_state.right_panel_visible);

    layout(&mut runtime, serde_json::json!({"panel": "closed"}));
    assert!(
        !runtime.snapshot.ui_state.right_panel_visible,
        "a closed panel's tools read nothing"
    );
    layout(
        &mut runtime,
        serde_json::json!({"reveal": directory.join("notes.md").to_string_lossy()}),
    );
    assert_eq!(panel(&runtime).0, PanelState::Open);

    layout(
        &mut runtime,
        serde_json::json!({"panel": "closed", "explorer": true}),
    );
    assert_eq!(panel(&runtime).0, PanelState::Closed);
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
        serde_json::json!({"panel": "sideways"}),
    ));
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("workspace_view.unknown_panel")
    );
}

/// B19: the daemon opens a checkout's root after the first snapshot names
/// it, so a restore waits for that root instead of reading too early, around
/// the pinned root, or marking every saved file unavailable; the tabs it has not restored yet
/// are not overwritten while it waits.
#[test]
fn a_restore_waits_for_the_daemon_to_open_the_checkout_root() {
    let (runtime, checkout_id, directory) = strip_checkout("waits-for-root");
    let state = views_path("waits-for-root");
    let mut runtime = with_views(runtime, &state);
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
    let state = views_path("unreadable");
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
    let mut runtime = with_views(runtime, &views_path("return-active"));
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
            id: "workspace:other".to_owned(),
            label: "other".to_owned(),
            path: other.to_string_lossy().into_owned(),
            device_id: "local".to_owned(),
            pinned: false,
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

/// B10, AGENTS.md one event per action: revealing a file shows the Explorer
/// and unfolds its folders in the same `workspace_view`, and a path outside
/// the Workspace is refused without showing anything.
#[test]
fn a_reveal_shows_the_explorer_and_unfolds_the_folders_in_one_event() {
    let (runtime, _checkout_id, directory) = strip_checkout("reveal");
    let mut runtime = with_views(runtime, &views_path("reveal"));
    layout(&mut runtime, serde_json::json!({"explorer": false}));
    let root = directory.to_string_lossy().into_owned();

    runtime.dispatch_json(&explorer_event(
        "workspace_view",
        serde_json::json!({"explorer": true, "reveal": "/elsewhere/a.txt"}),
    ));
    assert!(runtime.snapshot.status.last_error.is_some());
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().explorer);
    runtime.dispatch_json(&explorer_event(
        "workspace_view",
        serde_json::json!({"explorer": true, "reveal": format!("{root}/../elsewhere/a.txt")}),
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
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().explorer);

    layout(
        &mut runtime,
        serde_json::json!({"explorer": true, "reveal": format!("{root}/src/deep/a.rs")}),
    );
    assert!(runtime.snapshot.workspace_view.as_ref().unwrap().explorer);
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
            id: "workspace:other".to_owned(),
            label: "other".to_owned(),
            path: other.to_string_lossy().into_owned(),
            device_id: "local".to_owned(),
            pinned: false,
        });
    runtime.rebuild_catalog();
    let checkout = workspace::checkout_id_for_path("workspace:other", &other);
    (other, checkout)
}

fn saved_displays(runtime: &Runtime, path: &Path) -> Vec<String> {
    runtime
        .workspace_views
        .as_ref()
        .and_then(|store| store.views.get("local", &path.to_string_lossy()))
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
    let state = views_path("front-moved");
    let mut runtime = with_views(runtime, &state);
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
    let state = views_path("came-back");
    let mut runtime = with_views(runtime, &state);
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
    let state = views_path("resumed");
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
    let mut runtime = with_views(runtime, &views_path("settings"));
    layout(
        &mut runtime,
        serde_json::json!({"explorer": !saved.0, "changes": false}),
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
    let mut runtime = with_views(runtime, &views_path("device-views"));
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
    store.views.entry("studio", "/srv/app").panel = PanelState::Expanded;
    store.views.entry("local", &directory.to_string_lossy());

    assert!(runtime.dispatch_json(&explorer_event(
        "remove_device",
        serde_json::json!({"device_id": "studio"}),
    )));

    let views = &runtime.workspace_views.as_ref().unwrap().views;
    assert!(views.get("studio", "/srv/app").is_none());
    assert!(views.get("local", &directory.to_string_lossy()).is_some());
}
