use super::workspace_view::{active_label, layout, second_checkout, views_path, with_views};
use super::*;
use crate::model::{ViewDisplaySnapshot, ViewDisplayState, ViewLayoutSnapshot, ViewNodeSnapshot};

// PRD S7: a Workspace's View areas hold displays of documents; where an open
// lands, what the operator's layout actions do, how a display follows its
// document, and what comes back after a restart. Each test reads the tree the
// snapshot publishes.

fn open(runtime: &mut Runtime, checkout_id: &str, path: &Path, preview: bool, beside: bool) {
    runtime.dispatch_json(&explorer_event(
        "file_open",
        serde_json::json!({
            "path": path.to_string_lossy(),
            "workspace_id": "workspace:order",
            "checkout_id": checkout_id,
            "preview": preview,
            "beside": beside,
        }),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
}

/// Sends a `view_layout` action as the web does, named for the Workspace of
/// the frame it was taken on.
fn act(runtime: &mut Runtime, payload: serde_json::Value) -> bool {
    runtime.sync_workspace_view();
    let view = runtime
        .snapshot
        .workspace_view
        .as_ref()
        .expect("a front Workspace");
    let workspace = serde_json::json!({"device_id": view.device_id, "path": view.path});
    act_on(runtime, workspace, payload)
}

fn act_on(
    runtime: &mut Runtime,
    workspace: serde_json::Value,
    mut payload: serde_json::Value,
) -> bool {
    payload["workspace"] = workspace;
    runtime.dispatch_json(&explorer_event("view_layout", payload))
}

fn tree(runtime: &mut Runtime) -> ViewLayoutSnapshot {
    runtime.sync_workspace_view();
    runtime
        .snapshot
        .workspace_view
        .clone()
        .expect("a front Workspace")
        .layout
}

/// Every area in tree order with its displays.
fn areas(layout: &ViewLayoutSnapshot) -> Vec<(String, Option<String>, Vec<ViewDisplaySnapshot>)> {
    let mut found = Vec::new();
    let mut nodes = vec![&layout.root];
    while let Some(node) = nodes.pop() {
        match node {
            ViewNodeSnapshot::Area(area) => {
                found.push((area.id.clone(), area.active.clone(), area.displays.clone()))
            }
            ViewNodeSnapshot::Split(split) => nodes.extend([&*split.second, &*split.first]),
        }
    }
    found
}

/// Each area's display labels, a preview marked with `*`.
fn labels(runtime: &mut Runtime) -> Vec<Vec<String>> {
    areas(&tree(runtime))
        .into_iter()
        .map(|(_, _, displays)| {
            displays
                .iter()
                .map(|display| {
                    format!(
                        "{}{}",
                        display.label,
                        if display.preview { "*" } else { "" }
                    )
                })
                .collect()
        })
        .collect()
}

/// The id of the display labelled `label` in area `index`.
fn display(runtime: &mut Runtime, index: usize, label: &str) -> String {
    areas(&tree(runtime))[index]
        .2
        .iter()
        .find(|display| display.label == label)
        .unwrap_or_else(|| panic!("{label} in area {index}"))
        .id
        .clone()
}

fn area_id(runtime: &mut Runtime, index: usize) -> String {
    areas(&tree(runtime))[index].0.clone()
}

fn split(runtime: &mut Runtime, display_id: &str, area_id: &str, request_id: &str) -> bool {
    act(
        runtime,
        serde_json::json!({
            "action": "split", "display_id": display_id, "area_id": area_id,
            "edge": "right", "request_id": request_id,
        }),
    )
}

fn draft(runtime: &mut Runtime, tab_id: &str, contents: &str) {
    runtime.dispatch_json(&explorer_event(
        "file_draft",
        serde_json::json!({"tab_id": tab_id, "contents_utf8": contents}),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
}

fn tab_of(runtime: &Runtime, label: &str) -> String {
    runtime
        .snapshot
        .editor
        .tabs
        .iter()
        .find(|tab| tab.label == label)
        .unwrap_or_else(|| panic!("{label} is open"))
        .id
        .clone()
}

fn files(directory: &Path, names: &[&str]) {
    for name in names {
        std::fs::write(directory.join(name), format!("{name}\n")).expect("fixture");
    }
}

fn views_runtime(name: &str) -> (Runtime, String, PathBuf) {
    let (runtime, checkout_id, directory) = strip_checkout(name);
    let runtime = with_views(runtime, &views_path(name));
    (runtime, checkout_id, directory)
}

/// B1: an open lands in the area the operator used last.
#[test]
fn an_open_lands_in_the_view_area_used_last() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-last-used");
    files(&directory, &["a.md", "b.md", "c.md", "d.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("a.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("b.md"),
        false,
        false,
    );
    let (b, first) = (display(&mut runtime, 0, "b.md"), area_id(&mut runtime, 0));
    assert!(split(&mut runtime, &b, &first, "split-b"));

    let a = display(&mut runtime, 0, "a.md");
    act(
        &mut runtime,
        serde_json::json!({"action": "focus", "display_id": a}),
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("c.md"),
        false,
        false,
    );
    act(
        &mut runtime,
        serde_json::json!({"action": "focus", "display_id": b}),
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("d.md"),
        false,
        false,
    );

    assert_eq!(
        labels(&mut runtime),
        vec![vec!["a.md", "c.md"], vec!["b.md", "d.md"]]
    );
    let layout = tree(&mut runtime);
    assert_eq!(layout.active_area, area_id(&mut runtime, 1));
    assert_eq!(active_label(&runtime).as_deref(), Some("d.md"));
}

/// B2, D-02: a preview open takes the area's preview display in place; the
/// document it showed goes, with no Recent Closed entry, only when no other
/// display shows it.
#[test]
fn a_preview_open_takes_the_preview_display_and_retires_an_undisplayed_document() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-preview");
    files(&directory, &["x.md", "y.md", "z.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("notes.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("x.md"),
        true,
        false,
    );
    let slot = display(&mut runtime, 0, "x.md");
    // x also shows in a new area to the right.
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("x.md"),
        false,
        true,
    );
    act(
        &mut runtime,
        serde_json::json!({"action": "focus", "display_id": slot}),
    );

    open(
        &mut runtime,
        &checkout_id,
        &directory.join("y.md"),
        true,
        false,
    );
    assert_eq!(
        labels(&mut runtime),
        vec![vec!["notes.md", "y.md*"], vec!["x.md"]]
    );
    assert_eq!(
        display(&mut runtime, 0, "y.md"),
        slot,
        "retargeted in place"
    );
    assert!(
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.label == "x.md"),
        "x still shows to the right"
    );

    open(
        &mut runtime,
        &checkout_id,
        &directory.join("z.md"),
        true,
        false,
    );
    assert_eq!(display(&mut runtime, 0, "z.md"), slot);
    assert!(
        !runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.label == "y.md"),
        "y showed nowhere else, so it went"
    );
    assert_eq!(runtime.snapshot.recent_closed.count, 0);
}

/// B3: a second single click on a file already shown focuses the display
/// that showed it last, wherever that is, and adds nothing.
#[test]
fn a_second_single_click_focuses_the_display_that_showed_the_file_last() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-refocus");
    files(&directory, &["x.md", "w.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("x.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("w.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("x.md"),
        false,
        true,
    );
    let beside = display(&mut runtime, 1, "x.md");
    let w = display(&mut runtime, 0, "w.md");
    act(
        &mut runtime,
        serde_json::json!({"action": "focus", "display_id": w}),
    );

    open(
        &mut runtime,
        &checkout_id,
        &directory.join("x.md"),
        true,
        false,
    );

    let layout = tree(&mut runtime);
    assert_eq!(layout.display_count, 3);
    let right = &areas(&layout)[1];
    assert_eq!(layout.active_area, right.0);
    assert_eq!(right.1.as_deref(), Some(beside.as_str()));
}

/// B4: Open to the side uses the area next to the one in use, and makes a
/// new area to the right only when there is none.
#[test]
fn open_to_the_side_uses_the_neighbour_or_a_new_area_to_the_right() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-beside");
    files(&directory, &["a.md", "b.md", "c.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("a.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("b.md"),
        false,
        true,
    );
    let layout = tree(&mut runtime);
    let ViewNodeSnapshot::Split(split) = &layout.root else {
        panic!("a new area beside");
    };
    assert_eq!(split.axis, crate::view_layout::SplitAxis::Row);

    let a = display(&mut runtime, 0, "a.md");
    act(
        &mut runtime,
        serde_json::json!({"action": "focus", "display_id": a}),
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("c.md"),
        false,
        true,
    );

    assert_eq!(
        labels(&mut runtime),
        vec![vec!["a.md"], vec!["b.md", "c.md"]]
    );
    assert_eq!(active_label(&runtime).as_deref(), Some("c.md"));
}

/// B18, B19, contract 2: a split past the area cap and an open past the
/// display cap are refused with a reason, and the tree stays as it was.
#[test]
fn a_split_or_an_open_past_the_caps_is_refused_and_changes_nothing() {
    use crate::view_layout::{DisplayKind, Edge, Layout, MAX_VIEW_DISPLAYS};
    let (mut runtime, checkout_id, directory) = views_runtime("view-caps");
    let root = directory.to_string_lossy().into_owned();
    // Six areas, three splits deep, holding the display cap between them.
    let mut full = Layout::default();
    for index in 0..MAX_VIEW_DISPLAYS - 5 {
        let shown = full.new_display(
            &format!("{root}/f{index}.md"),
            DisplayKind::File,
            None,
            false,
        );
        full.insert("a1", shown, 1).unwrap();
    }
    let beside = |layout: &mut Layout, area: &str, edge: Edge, name: &str| {
        let shown = layout.new_display(&format!("{root}/{name}"), DisplayKind::File, None, false);
        layout.split_new(area, edge, shown, 1).unwrap()
    };
    let right = beside(&mut full, "a1", Edge::Right, "r.md");
    let below = beside(&mut full, "a1", Edge::Down, "d.md");
    beside(&mut full, &right, Edge::Down, "rd.md");
    beside(&mut full, "a1", Edge::Right, "ar.md");
    beside(&mut full, &below, Edge::Right, "dr.md");
    runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .views
        .entry("local", &root)
        .layout = full;
    let before = tree(&mut runtime);
    assert_eq!(before.display_count, MAX_VIEW_DISPLAYS);
    let tabs = runtime.snapshot.editor.tabs.clone();

    let first = areas(&before)[0].2[0].id.clone();
    split(&mut runtime, &first, "a1", "past-the-cap");
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("view_layout.limit")
    );
    assert_eq!(tree(&mut runtime), before);

    runtime.dispatch_json(&explorer_event(
        "file_open",
        serde_json::json!({
            "path": directory.join("notes.md"), "workspace_id": "workspace:order",
            "checkout_id": checkout_id, "preview": false,
        }),
    ));
    let refused = runtime.snapshot.status.last_error.clone().unwrap();
    assert_eq!(refused.kind, "view_layout.display_limit");
    assert_eq!(
        refused.message,
        "This Workspace has 64 views open. Close a view to open another."
    );
    assert_eq!(tree(&mut runtime), before);
    assert_eq!(runtime.snapshot.editor.tabs, tabs, "nothing was read");
}

/// `count` views of files that are not open, in the front Workspace's one
/// area, as a restored Workspace holds them before they are read.
fn fill(runtime: &mut Runtime, directory: &Path, count: usize) {
    use crate::view_layout::{DisplayKind, Layout};
    let mut layout = Layout::default();
    for index in 0..count {
        let path = directory.join(format!("f{index}.md"));
        let shown = layout.new_display(&path.to_string_lossy(), DisplayKind::File, None, false);
        layout.insert("a1", shown, 1).unwrap();
    }
    runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .views
        .entry("local", &directory.to_string_lossy())
        .layout = layout;
}

/// Review U1, B19: at 64 views nothing adds a 65th. Reopen Closed, a created
/// file and a revealed file are refused with the reason before anything is
/// read or made, and the closed file stays reopenable.
#[test]
fn at_the_display_cap_a_reopen_a_created_file_or_a_reveal_is_refused() {
    use crate::view_layout::MAX_VIEW_DISPLAYS;
    let (mut runtime, checkout_id, directory) = views_runtime("view-cap-origins");
    files(&directory, &["closed.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("closed.md"),
        false,
        false,
    );
    let closed = tab_of(&runtime, "closed.md");
    runtime.dispatch_json(&explorer_event(
        "file_close",
        serde_json::json!({"tab_id": closed}),
    ));
    assert_eq!(runtime.snapshot.recent_closed.count, 1);
    fill(&mut runtime, &directory, MAX_VIEW_DISPLAYS);
    let before = tree(&mut runtime);
    assert_eq!(before.display_count, MAX_VIEW_DISPLAYS);
    let tabs = runtime.snapshot.editor.tabs.clone();

    for (kind, payload) in [
        ("reopen_closed", serde_json::json!({})),
        (
            "file_create",
            serde_json::json!({"root": directory, "parent": directory, "name": "new.md"}),
        ),
        (
            "reveal_path",
            serde_json::json!({
                "path": directory.join("notes.md"), "workspace_id": "workspace:order",
                "checkout_id": checkout_id, "is_directory": false,
            }),
        ),
    ] {
        runtime.dispatch_json(&explorer_event(kind, payload));
        let error = runtime
            .snapshot
            .status
            .last_error
            .clone()
            .unwrap_or_else(|| panic!("{kind} at the cap is refused"));
        assert_eq!(error.kind, "view_layout.display_limit", "{kind}");
    }
    assert_eq!(tree(&mut runtime), before);
    assert_eq!(runtime.snapshot.editor.tabs, tabs, "nothing was read");
    assert!(!directory.join("new.md").exists(), "nothing was made");
    assert_eq!(
        runtime.snapshot.recent_closed.count, 1,
        "the closed file stays reopenable"
    );
}

/// Review U1: a file that lands after the tree filled, here a Reopen Closed
/// admitted at 63 views, is refused with the reason as it lands rather than
/// making a 65th view; nothing is opened and the file stays reopenable.
#[test]
fn a_reopen_that_lands_after_the_views_filled_is_refused_and_stays_reopenable() {
    use crate::view_layout::{DisplayKind, MAX_VIEW_DISPLAYS};
    let (runtime, checkout_id, directory) = views_runtime("view-cap-landing");
    let shared = Arc::new(Mutex::new(runtime));
    shared
        .lock()
        .unwrap()
        .install_worker_context(Arc::downgrade(&shared), crate::ffi::ChangeNotifier::noop());
    let wait = |what: &str, ready: &dyn Fn(&Runtime) -> bool| {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !ready(&shared.lock().unwrap()) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    open(
        &mut shared.lock().unwrap(),
        &checkout_id,
        &directory.join("notes.md"),
        false,
        false,
    );
    wait("the file to be read", &|runtime| {
        !runtime.editor_documents.is_empty()
    });
    {
        let mut runtime = shared.lock().unwrap();
        let notes = tab_of(&runtime, "notes.md");
        runtime.dispatch_json(&explorer_event(
            "file_close",
            serde_json::json!({"tab_id": notes}),
        ));
        fill(&mut runtime, &directory, MAX_VIEW_DISPLAYS - 1);
        runtime.dispatch_json(&explorer_event("reopen_closed", serde_json::json!({})));
        assert_eq!(runtime.snapshot.status.last_error, None);
        assert!(runtime.snapshot.recent_closed.restoring);
        // Another view takes the last place while the file is read.
        let layout = &mut runtime
            .workspace_views
            .as_mut()
            .unwrap()
            .views
            .entry("local", &directory.to_string_lossy())
            .layout;
        let last = layout.new_display(
            &directory.join("last.md").to_string_lossy(),
            DisplayKind::File,
            None,
            false,
        );
        layout.insert("a1", last, 2).unwrap();
    }
    wait("the reopen to land", &|runtime| {
        !runtime.snapshot.recent_closed.restoring
    });
    let mut runtime = shared.lock().unwrap();
    let refused = runtime.snapshot.status.last_error.clone().unwrap();
    assert_eq!(refused.kind, "view_layout.display_limit");
    assert_eq!(tree(&mut runtime).display_count, MAX_VIEW_DISPLAYS);
    assert!(
        runtime.snapshot.editor.tabs.is_empty(),
        "nothing was opened"
    );
    assert_eq!(runtime.snapshot.recent_closed.count, 1, "still reopenable");
    assert!(
        runtime.snapshot.recent_closed.notices[0]
            .message
            .contains(&refused.message)
    );
}

/// Review U1: the reconcile holds the cap too. A document of the Workspace
/// that no view shows while it has 64, which no action makes now but a
/// future path might, waits off screen, is reported once in the diagnostic
/// log, and shows once a view is closed.
#[test]
fn a_document_the_full_views_cannot_hold_shows_once_a_view_is_closed() {
    use crate::view_layout::MAX_VIEW_DISPLAYS;
    let (mut runtime, checkout_id, directory) = views_runtime("view-cap-reconcile");
    fill(&mut runtime, &directory, MAX_VIEW_DISPLAYS);
    let path = directory.join("notes.md").to_string_lossy().into_owned();
    runtime.insert_diff_tab("workspace:order", &checkout_id, &path, false, false);
    let shown = |runtime: &mut Runtime| {
        labels(runtime)
            .concat()
            .contains(&"notes.md (working diff)".to_owned())
    };
    let reported = |runtime: &Runtime| {
        runtime
            .snapshot
            .status
            .diagnostics
            .iter()
            .filter(|logged| logged.kind == "view_layout.display_limit")
            .count()
    };

    let full = tree(&mut runtime);
    assert_eq!(full.display_count, MAX_VIEW_DISPLAYS);
    assert!(!shown(&mut runtime));
    layout(&mut runtime, serde_json::json!({"changes": true}));
    assert_eq!(reported(&runtime), 1, "reported once, not on every pass");

    let first = areas(&full)[0].2[0].id.clone();
    assert!(act(
        &mut runtime,
        serde_json::json!({"action": "close", "display_id": first}),
    ));
    assert!(shown(&mut runtime));
    assert_eq!(tree(&mut runtime).display_count, MAX_VIEW_DISPLAYS);
}

/// Engineering principle 11: a split sent twice splits once.
#[test]
fn a_repeated_split_request_splits_once() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-split-twice");
    files(&directory, &["a.md", "b.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("a.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("b.md"),
        false,
        false,
    );
    let (b, first) = (display(&mut runtime, 0, "b.md"), area_id(&mut runtime, 0));
    assert!(split(&mut runtime, &b, &first, "split-1"));
    let once = tree(&mut runtime);

    assert!(!split(&mut runtime, &b, &first, "split-1"));
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(tree(&mut runtime), once);
    assert_eq!(labels(&mut runtime), vec![vec!["a.md"], vec!["b.md"]]);
}

/// B9: moving an area's last display away closes the area, and its
/// neighbour takes the space.
#[test]
fn moving_an_areas_last_display_away_collapses_the_area() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-move");
    files(&directory, &["a.md", "b.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("a.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("b.md"),
        false,
        false,
    );
    let (b, first) = (display(&mut runtime, 0, "b.md"), area_id(&mut runtime, 0));
    split(&mut runtime, &b, &first, "split-b");

    assert!(act(
        &mut runtime,
        serde_json::json!({"action": "move", "display_id": b, "area_id": first, "index": 0}),
    ));

    let layout = tree(&mut runtime);
    assert!(matches!(layout.root, ViewNodeSnapshot::Area(_)));
    assert_eq!(labels(&mut runtime), vec![vec!["b.md", "a.md"]]);
    assert_eq!(active_label(&runtime).as_deref(), Some("b.md"));
}

/// Contract 4.1, review F1: an action names the Workspace of the frame it
/// was taken on. Ids like `d2` repeat across Workspaces, so a close or move
/// that arrives after the front moved on changes neither tree and closes no
/// document, least of all the other Workspace's dirty one; the same action
/// for the Workspace in front applies.
#[test]
fn an_action_for_a_workspace_no_longer_in_front_changes_nothing() {
    let (mut runtime, checkout_id, directory) = strip_checkout("view-stale-workspace");
    let (other, other_checkout) = second_checkout(&mut runtime, &directory);
    let mut runtime = with_views(runtime, &views_path("view-stale-workspace"));
    files(&other, &["b.md", "c.md"]);
    let identity = |runtime: &mut Runtime| {
        runtime.sync_workspace_view();
        let view = runtime.snapshot.workspace_view.as_ref().unwrap();
        serde_json::json!({"device_id": view.device_id, "path": view.path})
    };
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("notes.md"),
        false,
        false,
    );
    let notes = tab_of(&runtime, "notes.md");
    draft(&mut runtime, &notes, "draft\n");
    let (first, dirty_display) = (identity(&mut runtime), display(&mut runtime, 0, "notes.md"));
    runtime.dispatch_json(&explorer_event(
        "focus_checkout",
        serde_json::json!({"workspace_id": "workspace:other", "checkout_id": other_checkout}),
    ));
    for name in ["b.md", "c.md"] {
        runtime.dispatch_json(&explorer_event(
            "file_open",
            serde_json::json!({
                "path": other.join(name).to_string_lossy(),
                "workspace_id": "workspace:other",
                "checkout_id": other_checkout,
                "preview": false,
            }),
        ));
    }
    let front = identity(&mut runtime);
    let shown = display(&mut runtime, 0, "b.md");
    assert_eq!(shown, dirty_display, "both Workspaces hold the same id");
    let area = area_id(&mut runtime, 0);
    let open_tabs = |runtime: &Runtime| {
        let mut tabs: Vec<_> = runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .map(|tab| (tab.label.clone(), tab.dirty))
            .collect();
        tabs.sort();
        tabs
    };
    let before = open_tabs(&runtime);

    let moved =
        serde_json::json!({"action": "move", "display_id": shown, "area_id": area, "index": 1});
    let closed = serde_json::json!({"action": "close", "display_id": shown});
    for action in [&moved, &closed] {
        assert!(act_on(&mut runtime, first.clone(), action.clone()));
        assert_eq!(runtime.snapshot.status.last_error, None);
        assert_eq!(labels(&mut runtime), vec![vec!["b.md", "c.md"]]);
        assert_eq!(open_tabs(&runtime), before);
        let logged = runtime.snapshot.status.diagnostics.last().unwrap();
        assert_eq!(logged.kind, "view_layout.stale_workspace");
        assert!(
            logged
                .message
                .contains(&directory.to_string_lossy().into_owned())
                && logged
                    .message
                    .contains(&other.to_string_lossy().into_owned()),
            "{}",
            logged.message
        );
    }
    assert_eq!(
        before,
        vec![
            ("b.md".to_owned(), false),
            ("c.md".to_owned(), false),
            ("notes.md".to_owned(), true)
        ]
    );

    act_on(&mut runtime, front.clone(), moved);
    assert_eq!(labels(&mut runtime), vec![vec!["c.md", "b.md"]]);
    act_on(&mut runtime, front, closed);
    assert_eq!(labels(&mut runtime), vec![vec!["c.md"]]);
    runtime.dispatch_json(&explorer_event(
        "focus_checkout",
        serde_json::json!({"workspace_id": "workspace:order", "checkout_id": checkout_id}),
    ));
    assert_eq!(labels(&mut runtime), vec![vec!["notes.md"]]);
    assert!(
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.id == notes && tab.dirty)
    );
}

/// B10: closing one of two displays of a document closes only that display;
/// the document and its unsaved draft stay.
#[test]
fn closing_one_of_two_displays_keeps_the_document_and_its_draft() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-close-one");
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("notes.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("notes.md"),
        false,
        true,
    );
    let tab_id = tab_of(&runtime, "notes.md");
    draft(&mut runtime, &tab_id, "draft\n");

    let beside = display(&mut runtime, 1, "notes.md");
    assert!(act(
        &mut runtime,
        serde_json::json!({"action": "close", "display_id": beside}),
    ));

    assert_eq!(labels(&mut runtime), vec![vec!["notes.md"]]);
    let tab = runtime
        .snapshot
        .editor
        .tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .expect("the document stays open");
    assert!(tab.dirty);
    let documents = runtime.snapshot_delta_payload(0, 0).documents.unwrap();
    assert_eq!(
        documents.changed[0].1.contents_utf8.as_deref(),
        Some("draft\n")
    );
    // A close of the display already gone is a converged no-op.
    assert!(!act(
        &mut runtime,
        serde_json::json!({"action": "close", "display_id": beside}),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
}

/// B10, S5.5 save-then-close: closing a dirty document's last display saves
/// it first; a refused save keeps the display and the draft, and a save that
/// lands closes both.
#[test]
fn closing_the_last_display_of_a_dirty_document_saves_it_first() {
    let (runtime, checkout_id, directory) = views_runtime("view-close-last");
    let path = directory.join("notes.md");
    let shared = Arc::new(Mutex::new(runtime));
    shared
        .lock()
        .unwrap()
        .install_worker_context(Arc::downgrade(&shared), crate::ffi::ChangeNotifier::noop());
    let wait = |what: &str, ready: &dyn Fn(&Runtime) -> bool| {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !ready(&shared.lock().unwrap()) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    open(
        &mut shared.lock().unwrap(),
        &checkout_id,
        &path,
        false,
        false,
    );
    wait("the file to be read", &|runtime| {
        !runtime.editor_documents.is_empty()
    });
    let tab_id = tab_of(&shared.lock().unwrap(), "notes.md");
    draft(&mut shared.lock().unwrap(), &tab_id, "draft\n");
    std::fs::write(&path, "changed elsewhere\n").expect("fixture");
    let close = |runtime: &mut Runtime| {
        let shown = display(runtime, 0, "notes.md");
        act(
            runtime,
            serde_json::json!({
                "action": "close", "display_id": shown,
                "pending_save": {"tab_id": tab_id, "path": path, "contents_utf8": "draft\n"},
            }),
        )
    };

    close(&mut shared.lock().unwrap());
    wait("the save to be refused", &|runtime| {
        runtime.editor_documents[&tab_id].conflict.is_some()
    });
    {
        let mut runtime = shared.lock().unwrap();
        assert_eq!(labels(&mut runtime), vec![vec!["notes.md"]]);
        assert!(
            runtime
                .snapshot
                .editor
                .tabs
                .iter()
                .any(|tab| tab.id == tab_id && tab.dirty)
        );
        runtime.dispatch_json(&explorer_event(
            "file_conflict",
            serde_json::json!({"tab_id": tab_id, "action": "keep_editing"}),
        ));
        close(&mut runtime);
    }
    wait("the document to close", &|runtime| {
        runtime.snapshot.editor.tabs.is_empty()
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "draft\n");
    let mut runtime = shared.lock().unwrap();
    assert_eq!(tree(&mut runtime).display_count, 0);
}

/// B5: a draft in one display keeps every display of that document open.
#[test]
fn a_draft_in_one_display_keeps_every_display_of_its_document_open() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-draft-pins");
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("notes.md"),
        true,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("notes.md"),
        false,
        true,
    );
    assert_eq!(
        labels(&mut runtime),
        vec![vec!["notes.md*"], vec!["notes.md"]]
    );

    let tab_id = tab_of(&runtime, "notes.md");
    draft(&mut runtime, &tab_id, "edited\n");

    assert_eq!(
        labels(&mut runtime),
        vec![vec!["notes.md"], vec!["notes.md"]]
    );
    assert!(!runtime.snapshot.editor.tabs[0].preview);
}

/// B5, contract 4.2: a save keeps every display of its document open while
/// it runs and after it lands, though no draft came before it.
#[test]
fn a_save_without_a_draft_keeps_every_display_of_its_document_open() {
    let (runtime, checkout_id, directory) = views_runtime("view-save-pins");
    let path = directory.join("notes.md");
    let shared = Arc::new(Mutex::new(runtime));
    shared
        .lock()
        .unwrap()
        .install_worker_context(Arc::downgrade(&shared), crate::ffi::ChangeNotifier::noop());
    let wait = |what: &str, ready: &dyn Fn(&Runtime) -> bool| {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !ready(&shared.lock().unwrap()) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    open(
        &mut shared.lock().unwrap(),
        &checkout_id,
        &path,
        true,
        false,
    );
    wait("the file to be read", &|runtime| {
        !runtime.editor_documents.is_empty()
    });
    let tab_id = {
        let mut runtime = shared.lock().unwrap();
        open(&mut runtime, &checkout_id, &path, false, true);
        assert_eq!(
            labels(&mut runtime),
            vec![vec!["notes.md*"], vec!["notes.md"]]
        );
        let tab_id = tab_of(&runtime, "notes.md");
        runtime.dispatch_json(&explorer_event(
            "file_save",
            serde_json::json!({"tab_id": tab_id, "path": path, "contents_utf8": "notes\n"}),
        ));
        assert_eq!(runtime.snapshot.status.last_error, None);
        assert_eq!(
            labels(&mut runtime),
            vec![vec!["notes.md"], vec!["notes.md"]],
            "a document being saved is never a preview"
        );
        tab_id
    };
    wait("the save to land", &|runtime| {
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.id == tab_id && !tab.dirty)
    });
    let mut runtime = shared.lock().unwrap();
    assert_eq!(
        labels(&mut runtime),
        vec![vec!["notes.md"], vec!["notes.md"]]
    );
    assert!(!runtime.snapshot.editor.tabs[0].preview);
}

/// Contract 4.2: a draft or a conflict choice names its document. One that
/// names a document no longer open is refused with the reason and never
/// lands in the document in use, where a stray draft would overwrite the
/// operator's edit and a stray reload would discard it.
#[test]
fn a_draft_or_conflict_choice_for_a_closed_document_is_refused_and_changes_nothing() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-closed-draft");
    files(&directory, &["closed.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("closed.md"),
        false,
        false,
    );
    let closed = tab_of(&runtime, "closed.md");
    assert!(runtime.dispatch_json(&explorer_event(
        "file_close",
        serde_json::json!({"tab_id": closed}),
    )));
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("notes.md"),
        false,
        false,
    );
    let notes = tab_of(&runtime, "notes.md");
    draft(&mut runtime, &notes, "draft\n");
    let in_use = |runtime: &mut Runtime| {
        let documents = runtime
            .snapshot_delta_payload(0, 0)
            .documents
            .expect("a fresh reader gets the section");
        let (_, document) = documents
            .changed
            .iter()
            .find(|(tab_id, _)| *tab_id == notes)
            .expect("the document in use is on screen");
        (
            labels(runtime),
            document.contents_utf8.clone(),
            document.dirty,
        )
    };
    let before = in_use(&mut runtime);
    assert_eq!(
        before,
        (
            vec![vec!["notes.md".to_owned()]],
            Some("draft\n".to_owned()),
            true
        )
    );

    for (kind, payload, refusal) in [
        (
            "file_draft",
            serde_json::json!({"tab_id": closed, "contents_utf8": "stray\n"}),
            "file.draft_rejected",
        ),
        (
            "file_conflict",
            serde_json::json!({"tab_id": closed, "action": "reload"}),
            "file.conflict_without_tab",
        ),
    ] {
        runtime.dispatch_json(&explorer_event(kind, payload));
        let error = runtime
            .snapshot
            .status
            .last_error
            .clone()
            .unwrap_or_else(|| panic!("{kind} naming a closed document is refused"));
        assert_eq!(error.kind, refusal);
        assert!(error.message.contains(&closed), "{}", error.message);
        assert_eq!(in_use(&mut runtime), before, "{kind} changed nothing");
    }
}

/// S5.5 B9: a renamed file keeps its display, which shows the new name.
#[test]
fn a_renamed_file_keeps_its_display() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-rename");
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("notes.md"),
        false,
        false,
    );
    let before = areas(&tree(&mut runtime))[0].2[0].clone();

    assert!(runtime.dispatch_json(&explorer_event(
        "path_rename",
        serde_json::json!({
            "root": directory.to_string_lossy(),
            "path": directory.join("notes.md").to_string_lossy(),
            "name": "renamed.md",
        }),
    )));

    let after = areas(&tree(&mut runtime))[0].2[0].clone();
    assert_eq!(
        (after.id, after.tab_id, after.label.as_str(), after.state),
        (
            before.id,
            before.tab_id,
            "renamed.md",
            ViewDisplayState::Open
        )
    );
    assert_eq!(after.path, directory.join("renamed.md").to_string_lossy());
}

/// B14-B17, contract 5: a restart brings back the tree, its ratio, each
/// area's order, preview and active display, the area in use, the mode and
/// the tools; a file that is gone comes back as an unavailable display that
/// Retry reads again once the file is back.
#[test]
fn a_restart_restores_the_view_tree_and_marks_a_missing_file_unavailable() {
    let (runtime, checkout_id, directory) = strip_checkout("view-restart");
    let state = views_path("view-restart");
    let mut runtime = with_views(runtime, &state);
    files(&directory, &["a.md", "b.md", "gone.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("a.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("b.md"),
        true,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("gone.md"),
        false,
        true,
    );
    let ViewNodeSnapshot::Split(divider) = tree(&mut runtime).root else {
        panic!("two areas");
    };
    act(
        &mut runtime,
        serde_json::json!({"action": "resize", "split_id": divider.id, "ratio": 0.3}),
    );
    let a = display(&mut runtime, 0, "a.md");
    act(
        &mut runtime,
        serde_json::json!({"action": "focus", "display_id": a}),
    );
    layout(
        &mut runtime,
        serde_json::json!({"mode": "views", "changes": true}),
    );
    let before = tree(&mut runtime);
    drop(runtime);
    std::fs::remove_file(directory.join("gone.md")).expect("remove fixture");

    let (restarted, _) = tab_order_runtime(&directory.to_string_lossy());
    let mut restarted = with_views(restarted, &state);
    let after = tree(&mut restarted);

    let shape = |layout: &ViewLayoutSnapshot| {
        let ViewNodeSnapshot::Split(split) = &layout.root else {
            panic!("two areas");
        };
        let displays = areas(layout)
            .into_iter()
            .map(|(id, active, displays)| {
                let shown: Vec<_> = displays
                    .iter()
                    .map(|display| (display.id.clone(), display.label.clone(), display.preview))
                    .collect();
                (id, active, shown)
            })
            .collect::<Vec<_>>();
        (
            split.axis,
            split.ratio,
            layout.active_area.clone(),
            displays,
        )
    };
    assert_eq!(shape(&after), shape(&before));
    assert!(
        (divider.ratio - 0.5).abs() < f32::EPSILON && (shape(&after).1 - 0.3).abs() < f32::EPSILON
    );
    let states = |layout: &ViewLayoutSnapshot| {
        areas(layout)
            .into_iter()
            .flat_map(|(_, _, displays)| displays)
            .map(|display| (display.label, display.state))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        states(&after),
        vec![
            ("a.md".to_owned(), ViewDisplayState::Open),
            ("b.md".to_owned(), ViewDisplayState::Open),
            ("gone.md".to_owned(), ViewDisplayState::Unavailable),
        ]
    );
    let view = restarted.snapshot.workspace_view.clone().unwrap();
    assert_eq!(
        (view.mode, view.changes),
        (crate::workspace_views::ViewMode::Views, true)
    );
    assert_eq!(active_label(&restarted).as_deref(), Some("a.md"));

    std::fs::write(directory.join("gone.md"), "back\n").expect("fixture");
    let gone = display(&mut restarted, 1, "gone.md");
    assert!(act(
        &mut restarted,
        serde_json::json!({"action": "retry", "display_id": gone}),
    ));
    assert_eq!(
        states(&tree(&mut restarted))[2],
        ("gone.md".to_owned(), ViewDisplayState::Open)
    );
}

/// Contract 1: an S6 file's View tabs come back as one area, in their saved
/// order, with the active tab and the preview, and the next save writes the
/// new schema.
#[test]
fn a_schema_1_views_file_migrates_into_one_area() {
    let (runtime, _checkout_id, directory) = strip_checkout("view-v1");
    let state = views_path("view-v1");
    std::fs::write(directory.join("b.md"), "b\n").expect("fixture");
    let file = |name: &str, preview: bool| serde_json::json!({"path": directory.join(name), "kind": "file", "preview": preview});
    let v1 = serde_json::json!({"schema_version": 1, "workspaces": [{
        "device_id": "local", "path": directory, "mode": "together",
        "tabs": [file("b.md", false), file("notes.md", true)],
        "active": file("b.md", false),
    }]});
    std::fs::write(&state, serde_json::to_vec(&v1).unwrap()).expect("fixture");
    let (_, diagnostics) = WorkspaceViewStore::open(state.clone(), Default::default());
    assert_eq!(
        diagnostics
            .iter()
            .map(|(kind, _)| *kind)
            .collect::<Vec<_>>(),
        vec!["workspace_views.migrated"]
    );

    let mut runtime = with_views(runtime, &state);
    assert_eq!(labels(&mut runtime), vec![vec!["b.md", "notes.md*"]]);
    assert_eq!(active_label(&runtime).as_deref(), Some("b.md"));

    let notes = display(&mut runtime, 0, "notes.md");
    act(
        &mut runtime,
        serde_json::json!({"action": "focus", "display_id": notes}),
    );
    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    assert_eq!(written["schema_version"], 2);
}

/// Contract 3.1, B19: the documents on screen ride their own section, each
/// with its own revision: a fresh reader gets all of them, an unchanged one
/// is not sent again, and a keystroke re-sends its own document and not the
/// editor.
#[test]
fn a_keystroke_resends_only_its_own_document() {
    let (mut runtime, checkout_id, directory) = views_runtime("view-documents");
    files(&directory, &["b.md"]);
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("notes.md"),
        false,
        false,
    );
    open(
        &mut runtime,
        &checkout_id,
        &directory.join("b.md"),
        false,
        true,
    );
    let (notes, b) = (tab_of(&runtime, "notes.md"), tab_of(&runtime, "b.md"));
    let changed = |documents: &crate::model::DocumentsDelta| {
        documents
            .changed
            .iter()
            .map(|(tab_id, _)| tab_id.clone())
            .collect::<Vec<_>>()
    };

    let fresh = runtime.snapshot_delta_payload(0, 0);
    let documents = fresh.documents.expect("a fresh reader gets the section");
    assert_eq!(documents.visible, vec![notes.clone(), b.clone()]);
    assert_eq!(changed(&documents), vec![notes.clone(), b.clone()]);
    let cursor = fresh.revision;
    assert!(
        runtime
            .snapshot_delta_payload(cursor, 0)
            .documents
            .is_none()
    );

    draft(&mut runtime, &notes, "one\n");
    let first = runtime.snapshot_delta_payload(cursor, 0);
    assert_eq!(
        changed(first.documents.as_ref().unwrap()),
        vec![notes.clone()]
    );
    draft(&mut runtime, &notes, "two\n");
    let second = runtime.snapshot_delta_payload(first.revision, 0);
    let documents = second.documents.expect("the edited document");
    assert_eq!(documents.visible, vec![notes.clone(), b]);
    assert_eq!(changed(&documents), vec![notes]);
    assert_eq!(
        documents.changed[0].1.contents_utf8.as_deref(),
        Some("two\n")
    );
    assert!(second.editor.is_none(), "the editor section is not re-sent");
}

/// A5, A10: the diffs the View areas show are taken in the Changes read,
/// one per area, and none while the Views are hidden.
#[test]
fn the_changes_read_takes_every_diff_on_screen() {
    let (mut runtime, _checkout_id, directory) = views_runtime("view-diffs");
    let changed: Vec<PathBuf> = ["x.md", "y.md"]
        .iter()
        .map(|name| directory.join(name))
        .collect();
    runtime.snapshot.changes.edit().root_path = Some(directory.to_string_lossy().into_owned());
    runtime.snapshot.changes.edit().entries = changed
        .iter()
        .map(|path| crate::model::ChangedFileSnapshot {
            path: path.to_string_lossy().into_owned(),
            relative_path: path.file_name().unwrap().to_string_lossy().into_owned(),
            previous_relative_path: None,
            status: crate::model::ChangedFileStatus::Untracked,
            added_lines: Some(1),
            removed_lines: Some(0),
        })
        .collect();
    for (path, beside) in changed.iter().zip([false, true]) {
        assert!(runtime.dispatch_json(&explorer_event(
            "changes_select",
            serde_json::json!({"path": path, "committed": false, "preview": false, "beside": beside}),
        )));
    }
    assert_eq!(
        labels(&mut runtime),
        vec![vec!["x.md (working diff)"], vec!["y.md (working diff)"]]
    );
    let diffs = |runtime: &mut Runtime| {
        runtime
            .changes_request()
            .map(|request| {
                request
                    .diffs
                    .into_iter()
                    .map(|target| (target.path, target.committed))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    assert_eq!(
        diffs(&mut runtime),
        changed
            .iter()
            .map(|path| (path.to_string_lossy().into_owned(), false))
            .collect::<Vec<_>>()
    );

    layout(&mut runtime, serde_json::json!({"mode": "agents"}));
    assert!(diffs(&mut runtime).is_empty());
}
