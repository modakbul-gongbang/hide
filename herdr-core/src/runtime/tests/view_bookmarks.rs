use super::workspace_view::{layout as view_event, with_new_views};
use super::*;
use crate::view_layout::Layout as ViewLayout;
use crate::workspace_control::{Action, Query, checkout_caller_id};

// PRD tab-view-bookmark: the View list, layout and panel are the Workspace's;
// each Agent tab only remembers, per View area, the display that was in front
// while it was the active tab, and gets it back when it is shown again. Every
// test drives the events the shell sends and reads the tree the runtime holds
// after the frame that event produced.

const TABS: [&str; 3] = ["w-order:t1", "w-order:t2", "w-order:t3"];

/// A checkout of a real directory with a live Herdr context, so a tab choice
/// leaves for Herdr and waits for its answer as it does in the daemon.
fn live_checkout(name: &str) -> (Runtime, String, PathBuf) {
    let folder = scratch_dir(&format!("hide-bookmark-{name}-"));
    let directory = folder.path().canonicalize().expect("a real checkout path");
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&directory)
            .status()
            .expect("git init runs")
            .success()
    );
    let (mut runtime, checkout_id) = live_tab_order_runtime(&directory.to_string_lossy());
    runtime.test_dirs.push(folder);
    (runtime, checkout_id, directory)
}

fn setup(name: &str) -> (Runtime, String, PathBuf) {
    let (runtime, checkout_id, directory) = live_checkout(name);
    let mut runtime = with_new_views(runtime, name);
    runtime.snapshot.status.herdr.state = "connected".to_owned();
    for file in ["a.md", "b.md", "c.md", "d.md"] {
        std::fs::write(directory.join(file), format!("{file}\n")).expect("fixture");
    }
    herdr_focuses(&mut runtime, &directory, "w-order:t1");
    (runtime, checkout_id, directory)
}

/// A session snapshot of the three tabs with Herdr focused on `active`.
fn herdr_focuses(runtime: &mut Runtime, directory: &Path, active: &str) {
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &TABS,
        &TABS,
        active,
    )));
    runtime.sync_workspace_view();
}

fn key(directory: &Path) -> (String, String) {
    (
        crate::node::TEST_NODE.to_owned(),
        directory.to_string_lossy().into_owned(),
    )
}

fn layout<'a>(runtime: &'a Runtime, directory: &Path) -> &'a ViewLayout {
    runtime
        .view_layout_of(&key(directory))
        .expect("the Workspace has a layout")
}

/// A file's name, or a page's whole address.
fn name_of(target: &str) -> String {
    if target.contains("://") {
        return target.to_owned();
    }
    Path::new(target).file_name().map_or_else(
        || target.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// What each View area shows in front, in tree order.
fn fronts(runtime: &Runtime, directory: &Path) -> Vec<String> {
    layout(runtime, directory)
        .areas()
        .iter()
        .map(|area| {
            area.active
                .as_ref()
                .and_then(|id| area.displays.iter().find(|display| &display.id == id))
                .map(|display| name_of(display.url.as_deref().unwrap_or(&display.path)))
                .unwrap_or_default()
        })
        .collect()
}

/// The front display id of each area as the published snapshot carries it,
/// in tree order: what the shell draws, not what the store holds.
fn published_front_ids(runtime: &Runtime) -> Vec<String> {
    fn walk(node: &crate::model::ViewNodeSnapshot, out: &mut Vec<String>) {
        match node {
            crate::model::ViewNodeSnapshot::Area(area) => {
                out.push(area.active.clone().unwrap_or_default());
            }
            crate::model::ViewNodeSnapshot::Split(split) => {
                walk(&split.first, out);
                walk(&split.second, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &runtime
            .snapshot
            .workspace_view
            .as_ref()
            .expect("a Workspace view is published")
            .layout
            .root,
        &mut out,
    );
    out
}

/// Every display of every area, in tree order: the strip.
fn strip(runtime: &Runtime, directory: &Path) -> Vec<String> {
    layout(runtime, directory)
        .displays()
        .map(|display| name_of(display.url.as_deref().unwrap_or(&display.path)))
        .collect()
}

fn display_id(runtime: &Runtime, directory: &Path, name: &str) -> String {
    layout(runtime, directory)
        .displays()
        .find(|display| name_of(display.url.as_deref().unwrap_or(&display.path)) == name)
        .unwrap_or_else(|| panic!("{name} is open"))
        .id
        .clone()
}

fn area_id(runtime: &Runtime, directory: &Path, index: usize) -> String {
    layout(runtime, directory).areas()[index].id.clone()
}

fn open(runtime: &mut Runtime, checkout_id: &str, directory: &Path, name: &str) {
    runtime.dispatch_json(&explorer_event(
        "file_open",
        serde_json::json!({
            "path": directory.join(name).to_string_lossy(),
            "workspace_id": "workspace:order",
            "checkout_id": checkout_id,
            "preview": false,
        }),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
}

/// A `view_layout` action as the web sends it, named for the front Workspace.
fn view_act(runtime: &mut Runtime, directory: &Path, mut payload: serde_json::Value) {
    payload["workspace"] = serde_json::json!({"device_id": crate::node::TEST_NODE, "path": directory.to_string_lossy()});
    runtime.dispatch_json(&explorer_event("view_layout", payload));
    assert_eq!(runtime.snapshot.status.last_error, None);
}

fn agent_act(runtime: &mut Runtime, directory: &Path, mut payload: serde_json::Value) {
    payload["workspace"] = serde_json::json!({"device_id": crate::node::TEST_NODE, "path": directory.to_string_lossy()});
    runtime.dispatch_json(&explorer_event("agent_layout", payload));
}

/// The operator's tab choice, then Herdr's answer that it took the focus.
fn focus_tab(runtime: &mut Runtime, checkout_id: &str, directory: &Path, tab_id: &str) {
    dispatch_focus_tab(runtime, checkout_id, tab_id);
    herdr_focuses(runtime, directory, tab_id);
}

fn dispatch_focus_tab(runtime: &mut Runtime, checkout_id: &str, tab_id: &str) {
    runtime.dispatch_json(&explorer_event(
        "focus_tab",
        serde_json::json!({
            "workspace_id": "workspace:order",
            "checkout_id": checkout_id,
            "tab_id": tab_id,
        }),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
}

fn bookmark(runtime: &Runtime, directory: &Path, tab: &str) -> Vec<(String, String)> {
    runtime
        .workspace_views
        .as_ref()
        .and_then(|store| {
            store
                .views
                .get(crate::node::TEST_NODE, &directory.to_string_lossy())
        })
        .and_then(|view| view.view_bookmarks.of(tab))
        .map(|areas| {
            areas
                .iter()
                .map(|(area, display)| (area.clone(), display.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// B1, B2, B3, B6, B22: each tab gets back the View it left in front, the
/// tab and the panel change in one frame, the strip is one list, and a tab
/// with no bookmark leaves the panel alone.
#[test]
fn a_tab_shown_again_gets_back_the_view_it_had_in_front() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-switch");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    assert_eq!(fronts(&runtime, &directory), ["a.md"], "no bookmark yet");
    open(&mut runtime, &checkout_id, &directory, "b.md");
    assert_eq!(fronts(&runtime, &directory), ["b.md"]);

    dispatch_focus_tab(&mut runtime, &checkout_id, "w-order:t1");
    // B2: the same frame that shows the tab shows its View, before Herdr
    // has answered.
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t1")
    );
    assert_eq!(fronts(&runtime, &directory), ["a.md"]);
    assert_eq!(
        published_front_ids(&runtime),
        [display_id(&runtime, &directory, "a.md")],
        "the snapshot that shows the tab shows its View"
    );
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .layout
            .display_count,
        2
    );
    assert_eq!(strip(&runtime, &directory), ["a.md", "b.md"]);
    herdr_focuses(&mut runtime, &directory, "w-order:t1");
    assert_eq!(fronts(&runtime, &directory), ["a.md"]);

    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    assert_eq!(fronts(&runtime, &directory), ["b.md"]);
    assert_eq!(strip(&runtime, &directory), ["a.md", "b.md"], "B3");

    // B22: the View that went behind stays in the strip and picking it is
    // the tab's new bookmark.
    let a = display_id(&runtime, &directory, "a.md");
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "focus", "display_id": a}),
    );
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    assert_eq!(fronts(&runtime, &directory), ["a.md"]);
}

/// B4, B5: a bookmark is per View area, an area a tab never remembered keeps
/// what it shows, and a View that closed leaves its area as it is.
#[test]
fn each_view_area_restores_alone_and_a_closed_view_leaves_its_area() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-areas");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    open(&mut runtime, &checkout_id, &directory, "b.md");
    let (b, first) = (
        display_id(&runtime, &directory, "b.md"),
        area_id(&runtime, &directory, 0),
    );
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "split", "display_id": b, "area_id": first,
            "edge": "right", "request_id": "split-1"}),
    );
    assert_eq!(fronts(&runtime, &directory), ["a.md", "b.md"]);
    assert_eq!(bookmark(&runtime, &directory, "w-order:t1").len(), 2);

    // Tab 2 puts c.md in the right area and d.md in the left one.
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    open(&mut runtime, &checkout_id, &directory, "c.md");
    let left = area_id(&runtime, &directory, 0);
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "focus_area", "area_id": left}),
    );
    open(&mut runtime, &checkout_id, &directory, "d.md");
    assert_eq!(fronts(&runtime, &directory), ["d.md", "c.md"]);

    dispatch_focus_tab(&mut runtime, &checkout_id, "w-order:t1");
    assert_eq!(
        published_front_ids(&runtime),
        [
            display_id(&runtime, &directory, "a.md"),
            display_id(&runtime, &directory, "b.md")
        ],
        "both areas' fronts leave in the snapshot of the switch"
    );
    herdr_focuses(&mut runtime, &directory, "w-order:t1");
    assert_eq!(fronts(&runtime, &directory), ["a.md", "b.md"]);
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    assert_eq!(fronts(&runtime, &directory), ["d.md", "c.md"]);

    // Tab 3 only ever changed the right area: the left one stays.
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t3");
    let right = area_id(&runtime, &directory, 1);
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "focus_area", "area_id": right}),
    );
    let b = display_id(&runtime, &directory, "b.md");
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "focus", "display_id": b}),
    );
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    assert_eq!(fronts(&runtime, &directory), ["d.md", "c.md"]);
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t3");
    assert_eq!(
        fronts(&runtime, &directory),
        ["d.md", "b.md"],
        "left area kept"
    );

    // B5: tab 1 remembers b.md; it closes while tab 3 is shown.
    let b = display_id(&runtime, &directory, "b.md");
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "close", "display_id": b}),
    );
    assert_eq!(strip(&runtime, &directory), ["a.md", "d.md", "c.md"]);
    let before = fronts(&runtime, &directory);
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    assert_eq!(
        fronts(&runtime, &directory)[1],
        before[1],
        "the right area keeps the front the close left it"
    );
    assert_eq!(fronts(&runtime, &directory)[0], "a.md");
}

/// B7, B8, D-02, D-03, D-04: Agent tabs shown side by side do not swap the
/// panel when focus moves between them, and what the operator picks next is
/// the focused tab's bookmark.
#[test]
fn moving_focus_between_side_by_side_agent_tabs_leaves_the_panel_alone() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-side-by-side");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    agent_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "split", "tab_id": "w-order:t2", "area_id": "a1",
            "edge": "right", "request_id": "agent-split"}),
    );
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t2")
    );
    // Tab 2 was not shown before the split, so its (empty) bookmark left
    // a.md in front; b.md is picked for it.
    open(&mut runtime, &checkout_id, &directory, "b.md");
    assert_eq!(bookmark(&runtime, &directory, "w-order:t2").len(), 1);

    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t1")
    );
    assert_eq!(fronts(&runtime, &directory), ["b.md"], "B7: no swap");
    // B8: a pane of the tab already in focus moves nothing either.
    runtime.dispatch_json(&operator_focus_event("w-order:t1:p"));
    assert_eq!(fronts(&runtime, &directory), ["b.md"]);

    // The next View picked belongs to the tab that has focus now.
    open(&mut runtime, &checkout_id, &directory, "c.md");
    assert_eq!(fronts(&runtime, &directory), ["c.md"]);
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    assert_eq!(fronts(&runtime, &directory), ["c.md"], "still no swap");
    assert_eq!(
        bookmark(&runtime, &directory, "w-order:t1")[0].1,
        display_id(&runtime, &directory, "c.md")
    );
    assert_eq!(
        bookmark(&runtime, &directory, "w-order:t2")[0].1,
        display_id(&runtime, &directory, "b.md")
    );
}

/// B9, B10, D-21: a tab that Herdr's own focus makes visible is restored like
/// one the operator chose, and a sidebar pick of an agent counts too.
#[test]
fn a_tab_shown_by_herdrs_own_focus_or_a_pane_pick_is_restored_too() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-followed");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    herdr_focuses(&mut runtime, &directory, "w-order:t2");
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t2")
    );
    open(&mut runtime, &checkout_id, &directory, "b.md");

    herdr_focuses(&mut runtime, &directory, "w-order:t1");
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t1")
    );
    assert_eq!(fronts(&runtime, &directory), ["a.md"], "B10");

    runtime.dispatch_json(&operator_focus_event("w-order:t2:p"));
    assert_eq!(fronts(&runtime, &directory), ["b.md"], "B9: a pane pick");
}

/// B11, D-12: a restore moves neither the area in use nor the panel, and it
/// applies while the panel is closed, so reopening it shows the bookmark.
#[test]
fn a_restore_keeps_the_area_in_use_and_the_panel_state() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-quiet");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    open(&mut runtime, &checkout_id, &directory, "b.md");
    let (b, first) = (
        display_id(&runtime, &directory, "b.md"),
        area_id(&runtime, &directory, 0),
    );
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "split", "display_id": b, "area_id": first,
            "edge": "right", "request_id": "split-quiet"}),
    );
    // Tab 1 remembers a.md | b.md, with the right area in use.
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    open(&mut runtime, &checkout_id, &directory, "c.md");
    let left = area_id(&runtime, &directory, 0);
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "focus_area", "area_id": left.clone()}),
    );
    open(&mut runtime, &checkout_id, &directory, "d.md");
    let right = area_id(&runtime, &directory, 1);
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "focus_area", "area_id": right.clone()}),
    );
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    assert_eq!(fronts(&runtime, &directory), ["a.md", "b.md"]);
    assert_eq!(layout(&runtime, &directory).active_area, right);
    assert!(runtime.snapshot.workspace_view.as_ref().unwrap().views);

    // With File Views off the bookmark still applies, so turning it on shows
    // the tab's front.
    view_event(&mut runtime, serde_json::json!({"views": false}));
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().views);
    assert_eq!(fronts(&runtime, &directory), ["d.md", "c.md"]);
    assert_eq!(layout(&runtime, &directory).active_area, right);
}

/// B20, D-11: a tab Herdr no longer lists loses its bookmark and the others
/// keep theirs, and an area that collapsed loses its entries.
#[test]
fn a_closed_tab_and_a_collapsed_area_lose_their_bookmark_entries() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-prune");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    open(&mut runtime, &checkout_id, &directory, "b.md");
    let (b, first) = (
        display_id(&runtime, &directory, "b.md"),
        area_id(&runtime, &directory, 0),
    );
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "split", "display_id": b, "area_id": first,
            "edge": "right", "request_id": "split-prune"}),
    );
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    open(&mut runtime, &checkout_id, &directory, "c.md");
    assert_eq!(bookmark(&runtime, &directory, "w-order:t1").len(), 2);
    assert_eq!(bookmark(&runtime, &directory, "w-order:t2").len(), 1);

    // The right area collapses when its last display goes.
    let right = area_id(&runtime, &directory, 1);
    let gone: Vec<String> = layout(&runtime, &directory)
        .area(&right)
        .unwrap()
        .displays
        .iter()
        .map(|display| display.id.clone())
        .collect();
    for display in gone {
        view_act(
            &mut runtime,
            &directory,
            serde_json::json!({"action": "close", "display_id": display}),
        );
    }
    let a = display_id(&runtime, &directory, "a.md");
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "focus", "display_id": a}),
    );
    assert_eq!(layout(&runtime, &directory).area_count(), 1);
    assert!(
        bookmark(&runtime, &directory, "w-order:t1")
            .iter()
            .all(|(area, _)| *area == area_id(&runtime, &directory, 0)),
        "the collapsed area is forgotten: {:?}",
        bookmark(&runtime, &directory, "w-order:t1")
    );

    // Herdr closes tab 2.
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &["w-order:t1", "w-order:t3"],
        &["w-order:t1", "w-order:t3"],
        "w-order:t1",
    )));
    runtime.sync_workspace_view();
    assert!(bookmark(&runtime, &directory, "w-order:t2").is_empty());
    assert!(!bookmark(&runtime, &directory, "w-order:t1").is_empty());
}

/// B18, D-07: the bookmarks are in the file and a runtime that reads it back
/// restores them.
#[test]
fn bookmarks_are_saved_and_a_restarted_runtime_restores_them() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-restart");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    open(&mut runtime, &checkout_id, &directory, "b.md");

    let path = runtime.workspace_views.as_ref().unwrap().path.clone();
    let (saved, _) = crate::workspace_views::load(&path, 1);
    let entry = saved
        .get(crate::node::TEST_NODE, &directory.to_string_lossy())
        .unwrap();
    assert_eq!(entry.view_bookmarks.of("w-order:t1").unwrap().len(), 1);
    assert_eq!(entry.view_bookmarks.of("w-order:t2").unwrap().len(), 1);

    // A new process: the file is read back and nothing was observed yet.
    let panel = (
        runtime.snapshot.ui_state.right_panel_visible,
        runtime.snapshot.ui_state.right_panel_section,
    );
    runtime.workspace_views = Some(WorkspaceViewStore::open(path, panel).0);
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), ["b.md"]);
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    assert_eq!(fronts(&runtime, &directory), ["a.md"]);
}

/// B25: a device Workspace has one visible tab that changes when its Herdr's
/// answer lands, and the restore happens with it.
#[test]
fn a_device_workspace_restores_when_its_visible_tab_changes() {
    let (mut runtime, _checkout_id, _directory) = setup("bookmark-device");
    let device = "remote:fixture";
    let mut project = workspace(
        "remote-workspace",
        "Remote",
        "/srv/remote",
        vec![checkout(
            "remote-workspace",
            "remote-checkout",
            "/srv/remote",
            Some(pane("remote-pane-1", "/srv/remote")),
        )],
    );
    project.device_id = device.to_owned();
    project.remote_target_id = Some(device.to_owned());
    let mut second = project.checkouts[0].tabs[0].clone();
    project.checkouts[0].tabs[0].id = Some("remote-tab-1".to_owned());
    second.id = Some("remote-tab-2".to_owned());
    project.checkouts[0].tabs.push(second);
    project.checkouts[0].active_tab_id = Some("remote-tab-1".to_owned());
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: device.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: None,
        session: Some(RemoteSessionSnapshot {
            workspaces: vec![project],
            agents: Vec::new(),
            active_tab_ids: Default::default(),
            focused_workspace_id: Some("remote-workspace".to_owned()),
            focused_checkout_id: Some("remote-checkout".to_owned()),
            focused_tab_id: Some("remote-tab-1".to_owned()),
            focused_pane_id: None,
            pane_layouts: Vec::new(),
            pane_hook_tokens: Default::default(),
        }),
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    runtime.snapshot.navigator.focused_device_id = Some(device.to_owned());
    let entry = runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .views
        .entry(device, "/srv/remote");
    for name in ["x.md", "y.md"] {
        let display = entry.layout.new_display(
            &format!("/srv/remote/{name}"),
            crate::view_layout::DisplayKind::File,
            None,
            false,
        );
        entry.layout.insert("a1", display, 1).unwrap();
    }
    let ids: Vec<String> = entry.layout.displays().map(|d| d.id.clone()).collect();
    entry.view_bookmarks.record("remote-tab-1", "a1", &ids[0]);
    entry.view_bookmarks.record("remote-tab-2", "a1", &ids[1]);
    let front = |runtime: &Runtime| {
        runtime
            .view_layout_of(&(device.to_owned(), "/srv/remote".to_owned()))
            .unwrap()
            .areas()[0]
            .active
            .clone()
    };
    runtime.sync_workspace_view();
    assert_eq!(
        front(&runtime),
        Some(ids[0].clone()),
        "the visible tab's own"
    );

    let session = runtime.snapshot.status.remote[0].session.as_mut().unwrap();
    session.workspaces[0].checkouts[0].active_tab_id = Some("remote-tab-2".to_owned());
    runtime.sync_workspace_view();
    assert_eq!(front(&runtime), Some(ids[1].clone()));
    let session = runtime.snapshot.status.remote[0].session.as_mut().unwrap();
    session.workspaces[0].checkouts[0].active_tab_id = Some("remote-tab-1".to_owned());
    runtime.sync_workspace_view();
    assert_eq!(front(&runtime), Some(ids[0].clone()));
}

// -- Views an agent opens (PRD D-05, D-06) ---------------------------------

fn control(
    runtime: &mut Runtime,
    caller: &str,
    action: Action,
    reveal_suffix: &str,
) -> crate::workspace_control::ActionResult {
    let expected = runtime
        .workspace_control_query(crate::node::TEST_NODE, caller, Query::Info)
        .expect("the caller is connected")
        .context;
    let id = format!("{}-{reveal_suffix}", unix_milliseconds());
    runtime
        .workspace_control_action(
            crate::node::TEST_NODE,
            caller,
            &expected,
            &id,
            action,
            Ok(None),
        )
        .expect("the action applies")
}

/// `hide file open [--beside] <name>` from `caller`, with the host read the
/// daemon does off the lock.
fn control_file(
    runtime: &mut Runtime,
    caller: &str,
    directory: &Path,
    name: &str,
    beside: bool,
    suffix: &str,
) {
    let action = Action::OpenFile {
        path: directory.join(name).to_string_lossy().into_owned(),
        beside,
        reveal: false,
    };
    let expected = runtime
        .workspace_control_query(crate::node::TEST_NODE, caller, Query::Info)
        .unwrap()
        .context;
    let id = format!("{}-{suffix}", unix_milliseconds());
    let crate::workspace_control::ActionPreparation::Read(source) = runtime
        .workspace_control_prepare_action(crate::node::TEST_NODE, caller, &expected, &id, &action)
        .unwrap()
    else {
        panic!("a file needs a host read");
    };
    let material = source.read().unwrap();
    runtime
        .workspace_control_action(
            crate::node::TEST_NODE,
            caller,
            &expected,
            &id,
            action,
            Ok(Some(material)),
        )
        .unwrap();
}

fn open_page(url: &str, reveal: bool) -> Action {
    Action::OpenBrowser {
        url: url.to_owned(),
        reveal,
        area_id: None,
        new_target: false,
    }
}

/// B12, B13, B14: an agent in a tab the operator is not looking at adds its
/// View to the strip and bookmarks its own tab; the operator's screen does
/// not change until that tab is shown, and `--reveal` brings it forward.
#[test]
fn an_agent_in_another_tab_opens_a_view_without_taking_the_screen() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-agent-open");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    open(&mut runtime, &checkout_id, &directory, "b.md");
    let strip_before = strip(&runtime, &directory);

    // Tab 1's agent opens a page while the operator looks at tab 2.
    control(
        &mut runtime,
        "w-order:t1:p",
        open_page("https://example.test/a", false),
        "page-a",
    );
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), ["b.md"], "the screen stays");
    assert_eq!(strip(&runtime, &directory).len(), strip_before.len() + 1);
    assert!(strip(&runtime, &directory).contains(&"https://example.test/a".to_owned()));
    assert_eq!(
        bookmark(&runtime, &directory, "w-order:t2")[0].1,
        display_id(&runtime, &directory, "b.md")
    );
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    assert_eq!(fronts(&runtime, &directory), ["https://example.test/a"]);

    // The same address again makes no second View: it is the bookmark.
    let a = display_id(&runtime, &directory, "a.md");
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "focus", "display_id": a}),
    );
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    let count = strip(&runtime, &directory).len();
    control(
        &mut runtime,
        "w-order:t1:p",
        open_page("https://example.test/a", false),
        "page-a-again",
    );
    runtime.sync_workspace_view();
    assert_eq!(strip(&runtime, &directory).len(), count);
    assert_eq!(fronts(&runtime, &directory), ["b.md"]);
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    assert_eq!(fronts(&runtime, &directory), ["https://example.test/a"]);

    // B13: the tab the operator is on opens as before, and remembers it.
    control(
        &mut runtime,
        "w-order:t1:p",
        open_page("https://example.test/own", false),
        "page-own",
    );
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), ["https://example.test/own"]);
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    assert_eq!(fronts(&runtime, &directory), ["https://example.test/own"]);

    // B14: `--reveal` puts it in front now, for the tab on screen and the
    // tab that asked.
    control(
        &mut runtime,
        "w-order:t2:p",
        open_page("https://example.test/reveal", true),
        "page-reveal",
    );
    runtime.sync_workspace_view();
    assert_eq!(
        fronts(&runtime, &directory),
        ["https://example.test/reveal"]
    );
    let reveal = display_id(&runtime, &directory, "https://example.test/reveal");
    for tab in ["w-order:t1", "w-order:t2"] {
        assert_eq!(bookmark(&runtime, &directory, tab)[0].1, reveal, "{tab}");
    }
}

/// B12: a file an agent opens from another tab goes through the same rule.
#[test]
fn an_agents_file_open_from_another_tab_bookmarks_only_its_tab() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-agent-file");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");

    control_file(
        &mut runtime,
        "w-order:t1:p",
        &directory,
        "c.md",
        false,
        "file",
    );
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), ["a.md"]);
    assert_eq!(strip(&runtime, &directory), ["a.md", "c.md"]);
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    assert_eq!(fronts(&runtime, &directory), ["c.md"]);
}

/// B15, B16, D-20: a caller with no pane acts on the active tab as before,
/// and the first View of a Workspace with none is its front whoever opened it.
#[test]
fn a_checkout_caller_and_the_first_view_of_an_empty_workspace_act_as_before() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-agent-first");
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");

    // B16: nothing is open, so there is no front to keep.
    control(
        &mut runtime,
        "w-order:t1:p",
        open_page("https://example.test/first", false),
        "first",
    );
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), ["https://example.test/first"]);
    let first = display_id(&runtime, &directory, "https://example.test/first");
    assert_eq!(bookmark(&runtime, &directory, "w-order:t1")[0].1, first);
    assert_eq!(bookmark(&runtime, &directory, "w-order:t2")[0].1, first);

    // B15: a checkout caller has no tab, so the active tab is the one asked.
    let caller = checkout_caller_id("abc123", &directory.to_string_lossy());
    control(
        &mut runtime,
        &caller,
        open_page("https://example.test/checkout", false),
        "checkout",
    );
    runtime.sync_workspace_view();
    assert_eq!(
        fronts(&runtime, &directory),
        ["https://example.test/checkout"]
    );
    let second = display_id(&runtime, &directory, "https://example.test/checkout");
    assert_eq!(bookmark(&runtime, &directory, "w-order:t2")[0].1, second);
    assert_eq!(bookmark(&runtime, &directory, "w-order:t1")[0].1, first);
}

/// B17, D-06: `hide view select` from a tab that is not on screen bookmarks
/// its own tab, `--reveal` selects at once, and a split applies as it did.
#[test]
fn a_view_select_from_another_tab_waits_for_its_tab_and_reveal_does_not() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-agent-select");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    open(&mut runtime, &checkout_id, &directory, "b.md");
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    let (a, b) = (
        display_id(&runtime, &directory, "a.md"),
        display_id(&runtime, &directory, "b.md"),
    );
    assert_eq!(fronts(&runtime, &directory), ["b.md"]);

    control(
        &mut runtime,
        "w-order:t1:p",
        Action::Select {
            view_id: a.clone(),
            reveal: false,
            expected_browser_area: None,
        },
        "select",
    );
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), ["b.md"]);
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t1");
    assert_eq!(fronts(&runtime, &directory), ["a.md"]);

    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    control(
        &mut runtime,
        "w-order:t1:p",
        Action::Select {
            view_id: b.clone(),
            reveal: true,
            expected_browser_area: None,
        },
        "select-reveal",
    );
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), ["b.md"]);

    // A split changes the shared layout, so it is not held back.
    let first = area_id(&runtime, &directory, 0);
    control(
        &mut runtime,
        "w-order:t1:p",
        Action::Split {
            view_id: a,
            area_id: first,
            edge: crate::workspace_control::Edge::Right,
        },
        "split",
    );
    assert_eq!(layout(&runtime, &directory).area_count(), 2);
}

/// D-05, D-12: with two View areas and the operator's keyboard in the left
/// one, a select or an open to the side from another tab moves neither the
/// fronts nor the area in use, and only the caller's tab remembers it.
#[test]
fn a_parked_select_and_open_beside_keep_the_operators_area_and_fronts() {
    let (mut runtime, checkout_id, directory) = setup("bookmark-parked-areas");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    open(&mut runtime, &checkout_id, &directory, "b.md");
    let (b, first) = (
        display_id(&runtime, &directory, "b.md"),
        area_id(&runtime, &directory, 0),
    );
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "split", "display_id": b, "area_id": first,
            "edge": "right", "request_id": "split-parked"}),
    );
    open(&mut runtime, &checkout_id, &directory, "c.md");
    let (left, right) = (
        area_id(&runtime, &directory, 0),
        area_id(&runtime, &directory, 1),
    );
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "focus_area", "area_id": left}),
    );
    // Tab 2 has no bookmark, so the areas stay as tab 1 left them.
    focus_tab(&mut runtime, &checkout_id, &directory, "w-order:t2");
    assert_eq!(fronts(&runtime, &directory), ["a.md", "c.md"]);
    assert_eq!(layout(&runtime, &directory).active_area, left);
    let before = published_front_ids(&runtime);

    // b.md sits behind c.md in the right area; selecting it there is what
    // moves the area in use when it is not held back.
    let b = display_id(&runtime, &directory, "b.md");
    control(
        &mut runtime,
        "w-order:t1:p",
        Action::Select {
            view_id: b.clone(),
            reveal: false,
            expected_browser_area: None,
        },
        "select-right",
    );
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), ["a.md", "c.md"]);
    assert_eq!(layout(&runtime, &directory).active_area, left);
    assert_eq!(published_front_ids(&runtime), before);
    assert!(bookmark(&runtime, &directory, "w-order:t1").contains(&(right.clone(), b)));

    control_file(
        &mut runtime,
        "w-order:t1:p",
        &directory,
        "d.md",
        true,
        "beside",
    );
    runtime.sync_workspace_view();
    assert_eq!(layout(&runtime, &directory).active_area, left);
    assert_eq!(&fronts(&runtime, &directory)[..2], ["a.md", "c.md"]);
    assert!(strip(&runtime, &directory).contains(&"d.md".to_owned()));
    assert_eq!(
        bookmark(&runtime, &directory, "w-order:t2").len(),
        0,
        "the operator's tab remembers nothing it did not do"
    );
}
