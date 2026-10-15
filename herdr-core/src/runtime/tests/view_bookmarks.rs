use super::workspace_view::{layout as view_event, with_new_views};
use super::*;
use crate::view_layout::{Layout as ViewLayout, MAX_VIEW_DISPLAYS};
use crate::workspace_control::{Action, Query, checkout_caller_id};

// The core projects one tab-owned presentation; documents stay shared.
// Tests drive public events/actions and inspect the published operator frame.

const TABS: [&str; 3] = ["w-order:t1", "w-order:t2", "w-order:t3"];

#[test]
fn delegated_local_caller_can_manage_background_views() {
    let (mut runtime, _checkout, directory) = setup("delegated-caller");
    let checkout = runtime
        .snapshot
        .navigator
        .workspaces
        .iter_mut()
        .flat_map(|workspace| &mut workspace.checkouts)
        .find(|checkout| checkout.path == directory.to_string_lossy())
        .unwrap();
    checkout
        .tabs
        .iter_mut()
        .find(|tab| tab.id.as_deref() == Some(TABS[1]))
        .unwrap()
        .delegated = true;
    runtime
        .reconcile_agent_topology(
            &key(&directory),
            &TABS
                .iter()
                .map(|tab| (tab.to_string(), *tab == TABS[1]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    runtime.sync_workspace_view();
    assert!(
        runtime
            .agent_layout_of(&key(&directory))
            .unwrap()
            .tree
            .display(TABS[1])
            .is_none()
    );
    let before = runtime.snapshot.workspace_view.clone();
    let caller = "w-order:t2:p";
    control_file(
        &mut runtime,
        caller,
        &directory,
        "c.md",
        true,
        "delegated-file",
    );
    let page = control(
        &mut runtime,
        caller,
        open_page("https://example.test/delegated", false),
        "delegated-browser",
    );
    control(
        &mut runtime,
        caller,
        Action::Select {
            view_id: page.view_id.clone(),
            reveal: false,
            expected_browser_area: None,
        },
        "delegated-select",
    );
    control(
        &mut runtime,
        caller,
        Action::Close {
            view_id: page.view_id,
            expected_browser_area: None,
        },
        "delegated-close",
    );
    runtime.sync_workspace_view();
    assert_eq!(runtime.snapshot.workspace_view, before);
    assert!(runtime.snapshot.browser_views.is_empty());
    let presentation = &runtime
        .workspace_views
        .as_ref()
        .unwrap()
        .views
        .get(crate::node::TEST_NODE, &directory.to_string_lossy())
        .unwrap()
        .view_presentations[TABS[1]];
    assert!(
        presentation
            .layout
            .displays()
            .any(|display| display.path.ends_with("c.md"))
    );
}

#[test]
fn rejected_background_open_preserves_documents_lane_and_save_generation() {
    let (mut runtime, checkout, directory) = setup("background-refusal");
    open(&mut runtime, &checkout, &directory, "a.md");
    // Nothing can close to make room: A's document holds a draft, B's front
    // is on screen and the rest are files still being read back.
    runtime.dispatch_json(&explorer_event(
        "file_draft",
        serde_json::json!({"contents_utf8": "unsaved text\n"}),
    ));
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    open(&mut runtime, &checkout, &directory, "b.md");
    fill_unread(&mut runtime, &directory, 62);
    runtime.sync_workspace_view();
    assert_eq!(held(&runtime, &directory), MAX_VIEW_DISPLAYS);
    let before = runtime.workspace_views.as_ref().unwrap().views.clone();
    let frame = runtime.snapshot.workspace_view.clone();
    let editor = serde_json::to_value(&runtime.snapshot.editor).unwrap();
    let generation = runtime.workspace_views.as_ref().unwrap().generation;
    let calls = runtime.workspace_views.as_ref().unwrap().lane_calls;
    let caller = "w-order:t1:p";
    let context = runtime
        .workspace_control_query(crate::node::TEST_NODE, caller, Query::Info)
        .unwrap()
        .context;
    let id = format!("{}-refused-file", unix_milliseconds());
    let action = Action::OpenFile {
        path: directory.join("c.md").to_string_lossy().into_owned(),
        beside: true,
        reveal: false,
    };
    let crate::workspace_control::ActionPreparation::Read(source) = runtime
        .workspace_control_prepare_action(crate::node::TEST_NODE, caller, &context, &id, &action)
        .unwrap()
    else {
        panic!("host read")
    };
    let refused = runtime
        .workspace_control_action(
            crate::node::TEST_NODE,
            caller,
            &context,
            &id,
            action.clone(),
            Ok(Some(source.read().unwrap())),
        )
        .unwrap_err();
    assert_eq!(refused.reason, "view_limit");
    assert_eq!(
        runtime
            .workspace_control_action(
                crate::node::TEST_NODE,
                caller,
                &context,
                &id,
                action,
                Ok(None)
            )
            .unwrap_err(),
        refused
    );
    runtime.sync_workspace_view();
    let store = runtime.workspace_views.as_ref().unwrap();
    assert_eq!(
        (&store.views, store.generation, store.lane_calls),
        (&before, generation, calls)
    );
    assert_eq!(runtime.snapshot.workspace_view, frame);
    assert_eq!(
        serde_json::to_value(&runtime.snapshot.editor).unwrap(),
        editor
    );
    assert_eq!(runtime.snapshot.status.last_error, None);
}

#[test]
fn empty_agent_presentations_do_not_consume_view_areas() {
    let (mut runtime, checkout_id, directory) = setup("empty-tabs");
    let tabs = (1..=8).map(|n| format!("w-order:t{n}")).collect::<Vec<_>>();
    let ids = tabs.iter().map(String::as_str).collect::<Vec<_>>();
    for tab in &tabs {
        runtime.ingest_session(Ok(tab_order_payload(
            &directory.to_string_lossy(),
            &ids,
            &ids,
            tab,
        )));
        runtime.sync_workspace_view();
        dispatch_focus_tab(&mut runtime, &checkout_id, tab);
        assert_eq!(runtime.snapshot.workspace_view.as_ref().unwrap().tool, None);
        assert!(strip(&runtime, &directory).is_empty());
        assert!(runtime.snapshot.browser_scopes.len() <= 6);
        assert_eq!(
            runtime
                .workspace_views
                .as_ref()
                .unwrap()
                .views
                .get(&key(&directory).0, &key(&directory).1)
                .unwrap()
                .view_owner
                .as_deref(),
            Some(tab.as_str())
        );
    }
    control_file(
        &mut runtime,
        "w-order:t1:p",
        &directory,
        "a.md",
        false,
        "empty-first",
    );
    assert!(strip(&runtime, &directory).is_empty());
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &ids,
        &ids,
        &tabs[0],
    )));
    runtime.sync_workspace_view();
    assert_eq!(strip(&runtime, &directory), ["a.md"]);
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &ids,
        &ids,
        &tabs[7],
    )));
    runtime.sync_workspace_view();
    assert!(strip(&runtime, &directory).is_empty());
}

/// The reported case: six tabs each showing a page held the checkout's six
/// View areas, so a seventh tab could open nothing. Each tab's areas are its
/// own; only the 64 views are the checkout's.
#[test]
fn a_seventh_tab_opens_a_page_while_six_tabs_show_pages() {
    let (mut runtime, checkout_id, directory) = setup("areas-per-tab");
    let tabs = (1..=7).map(|n| format!("w-order:t{n}")).collect::<Vec<_>>();
    let ids = tabs.iter().map(String::as_str).collect::<Vec<_>>();
    for (index, tab) in tabs.iter().enumerate() {
        runtime.ingest_session(Ok(tab_order_payload(
            &directory.to_string_lossy(),
            &ids,
            &ids,
            tab,
        )));
        runtime.sync_workspace_view();
        dispatch_focus_tab(&mut runtime, &checkout_id, tab);
        let url = format!("https://example.test/{index}");
        runtime.dispatch_json(&explorer_event(
            "browser_open",
            serde_json::json!({
                "url": url,
                "workspace": {"device_id": crate::node::TEST_NODE, "path": directory.to_string_lossy()},
            }),
        ));
        runtime.sync_workspace_view();
        assert_eq!(runtime.snapshot.status.last_error, None, "{tab} opens");
        assert_eq!(strip(&runtime, &directory), [url]);
    }
    assert_eq!(held(&runtime, &directory), 7);
}

#[test]
fn background_open_preserves_operator_preview_and_layout() {
    let (mut runtime, checkout_id, directory) = setup("preview-isolation");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    focus_tab(&mut runtime, &checkout_id, &directory, TABS[1]);
    runtime.dispatch_json(&explorer_event(
        "file_open",
        serde_json::json!({
            "path": directory.join("b.md"), "workspace_id": "workspace:order",
            "checkout_id": checkout_id, "preview": true,
        }),
    ));
    let before = presentation(&runtime);
    let b = display_id(&runtime, &directory, "b.md");
    assert!(layout(&runtime, &directory).display(&b).unwrap().preview);
    control_file(
        &mut runtime,
        "w-order:t1:p",
        &directory,
        "b.md",
        false,
        "pin-a",
    );
    control_file(
        &mut runtime,
        "w-order:t1:p",
        &directory,
        "c.md",
        true,
        "beside-a",
    );
    assert_eq!(presentation(&runtime), before);
    assert!(layout(&runtime, &directory).display(&b).unwrap().preview);
    focus_tab(&mut runtime, &checkout_id, &directory, TABS[0]);
    assert_eq!(fronts(&runtime, &directory), ["b.md", "c.md"]);
    let c = display_id(&runtime, &directory, "c.md");
    let target = area_id(&runtime, &directory, 0);
    view_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "move", "display_id": c, "area_id": target, "index": 0}),
    );
    focus_tab(&mut runtime, &checkout_id, &directory, TABS[1]);
    assert_eq!(presentation(&runtime), before);
}

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

#[test]
fn a_revealed_browser_keeps_its_page_when_a_tabs_bookmark_is_first_restored() {
    let (mut runtime, checkout_id, directory) = setup("terminal-browser-reveal");
    open(&mut runtime, &checkout_id, &directory, "a.md");
    // A restored Workspace has its saved bookmarks but no runtime observation yet.
    runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .bookmark_seen
        .clear();
    let url = "https://example.test/terminal-report";
    runtime.dispatch_json(&explorer_event(
        "browser_open",
        serde_json::json!({
            "url": url,
            "workspace": {"device_id": crate::node::TEST_NODE, "path": directory.to_string_lossy()},
            "reveal": true,
        }),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), [url]);
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

/// Every view the Workspace holds, in every tab's presentation.
fn held(runtime: &Runtime, directory: &Path) -> usize {
    runtime
        .all_view_layouts(&key(directory))
        .map(ViewLayout::display_count)
        .sum()
}

/// What a hidden tab's presentation holds.
fn hidden_strip(runtime: &Runtime, directory: &Path, tab: &str) -> Vec<String> {
    runtime
        .workspace_views
        .as_ref()
        .unwrap()
        .views
        .get(crate::node::TEST_NODE, &directory.to_string_lossy())
        .unwrap()
        .view_presentations[tab]
        .layout
        .displays()
        .map(|display| name_of(display.url.as_deref().unwrap_or(&display.path)))
        .collect()
}

/// `count` views of files still being read back, behind the projected
/// presentation's front, as a restored Workspace holds them.
fn fill_unread(runtime: &mut Runtime, directory: &Path, count: usize) {
    let entry = runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .views
        .entry(crate::node::TEST_NODE, &directory.to_string_lossy());
    entry.layout.next_id = entry.next_view_identity();
    let area = entry.layout.active_area.clone();
    for index in 0..count {
        let path = directory.join(format!("unread-{index}.md"));
        let shown = entry.layout.new_display(
            &path.to_string_lossy(),
            crate::view_layout::DisplayKind::File,
            None,
            false,
        );
        entry.layout.append(&area, shown).unwrap();
    }
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

/// The published Workspace without its Agent areas and its lane call
/// serial: a later explicit return to a pending result calls the lane, and
/// the restored presentation is compared independently of that monotonic
/// number.
fn presentation(runtime: &Runtime) -> crate::model::WorkspaceViewSnapshot {
    let mut view = runtime
        .snapshot
        .workspace_view
        .clone()
        .expect("a published Workspace");
    view.agent_layout = None;
    view.lane_calls = 0;
    view.lane_called = 0;
    view
}

// These expectations come from cli-tab-background-views B1-B24 and PRD
// right-lane D-11, B15: each Agent tab keeps its View list, layout and
// selection, and the lane (its tool, pin and widths) is the Workspace's.
#[test]
fn each_tab_keeps_its_own_list_and_the_lane_stays_the_workspaces() {
    let (mut runtime, checkout, directory) = setup("tab-private-list");
    open(&mut runtime, &checkout, &directory, "a.md");
    runtime.dispatch_json(&explorer_event(
        "file_draft",
        serde_json::json!({"contents_utf8": "unsaved text\n"}),
    ));
    let a = display_id(&runtime, &directory, "a.md");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert!(
        strip(&runtime, &directory).is_empty(),
        "B2 starts with no Views"
    );
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Some(crate::workspace_views::Tool::Files),
        "the lane stays the Workspace's"
    );
    open(&mut runtime, &checkout, &directory, "a.md");
    assert_ne!(
        display_id(&runtime, &directory, "a.md"),
        a,
        "private display identity"
    );
    assert_eq!(
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .filter(|tab| tab.label == "a.md" && tab.dirty)
            .count(),
        1,
        "one shared dirty buffer"
    );
    open(&mut runtime, &checkout, &directory, "b.md");
    view_event(
        &mut runtime,
        serde_json::json!({"tool": "agent_sessions", "pinned": true, "lane_width": 444}),
    );
    let b = presentation(&runtime);
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(
        strip(&runtime, &directory),
        ["a.md"],
        "B7 excludes B's insert"
    );
    assert_eq!(published_front_ids(&runtime), [a]);
    let view = runtime.snapshot.workspace_view.as_ref().unwrap();
    assert_eq!(
        (view.tool, view.pinned, view.lane_width),
        (
            Some(crate::workspace_views::Tool::AgentSessions),
            true,
            Some(444)
        ),
        "the lane is the Workspace's, so another tab keeps it"
    );
    assert!(
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.label == "a.md" && tab.dirty)
    );
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert_eq!(presentation(&runtime), b);
}

#[test]
fn background_file_beside_preserves_b_open_or_closed_and_returns_only_to_a() {
    for tool in [serde_json::json!("files"), serde_json::Value::Null] {
        let (mut runtime, checkout, directory) = setup(if tool.is_null() {
            "background-closed"
        } else {
            "background-open"
        });
        open(&mut runtime, &checkout, &directory, "a.md");
        focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
        open(&mut runtime, &checkout, &directory, "b.md");
        view_event(
            &mut runtime,
            serde_json::json!({"tool": tool, "pinned": true, "lane_width": 444, "tree_width": 333}),
        );
        let before = runtime.snapshot.workspace_view.clone();
        let editor = runtime.snapshot.editor.active_tab_id.clone();
        control_file(
            &mut runtime,
            "w-order:t1:p",
            &directory,
            "c.md",
            true,
            "background-beside",
        );
        runtime.sync_workspace_view();
        assert_eq!(
            runtime.snapshot.workspace_view, before,
            "B5 exact operator presentation and lane"
        );
        assert_eq!(runtime.snapshot.editor.active_tab_id, editor);
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout).as_deref(),
            Some(TABS[1])
        );
        focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
        assert_eq!(fronts(&runtime, &directory), ["a.md", "c.md"]);
        assert_eq!(
            runtime.snapshot.workspace_view.as_ref().unwrap().tool,
            Some(crate::workspace_views::Tool::Files),
            "choosing A shows its background result in the files lane"
        );
        focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
        assert_eq!(
            presentation(&runtime).layout,
            before.unwrap().layout,
            "B7 B's View list stays independent"
        );
    }
}

#[test]
fn hidden_same_url_has_its_own_identity_load_and_retained_scope() {
    let (mut runtime, checkout, directory) = setup("background-browser");
    let url = "https://example.test/same";
    control(
        &mut runtime,
        "w-order:t1:p",
        open_page(url, false),
        "first-browser",
    );
    runtime.sync_workspace_view();
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    control(
        &mut runtime,
        "w-order:t2:p",
        open_page(url, false),
        "operator-browser",
    );
    runtime.sync_workspace_view();
    let before = runtime.snapshot.workspace_view.clone();
    let browser_b = layout(&runtime, &directory)
        .displays()
        .next()
        .unwrap()
        .clone();
    let scopes_b = runtime.snapshot.browser_scopes.clone();
    control(
        &mut runtime,
        "w-order:t1:p",
        open_page(url, false),
        "background-browser",
    );
    runtime.sync_workspace_view();
    assert_eq!(runtime.snapshot.workspace_view, before);
    assert_eq!(
        layout(&runtime, &directory).display(&browser_b.id),
        Some(&browser_b)
    );
    for scope in scopes_b {
        assert!(runtime.snapshot.browser_scopes.contains(&scope));
    }
    let a = runtime
        .workspace_control_query(crate::node::TEST_NODE, "w-order:t1:p", Query::ViewList)
        .unwrap()
        .views
        .unwrap();
    assert_eq!(a.len(), 1);
    assert_ne!(a[0].view_id, browser_b.id);
    assert!(
        runtime
            .browser_route_source(
                crate::node::TEST_NODE,
                &directory.to_string_lossy(),
                &a[0].view_id,
                a[0].page.as_ref().unwrap().load
            )
            .is_some()
    );
    assert_eq!(a[0].page.as_ref().unwrap().state, "pending");
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(published_front_ids(&runtime), [a[0].view_id.clone()]);
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert_eq!(presentation(&runtime), {
        let mut before = before.unwrap();
        before.agent_layout = None;
        before.lane_calls = 0;
        before.lane_called = 0;
        // Choosing A's background result opened the Workspace's one lane.
        before.tool = Some(crate::workspace_views::Tool::Files);
        before
    });
}

/// PRD right-lane B17: a first background view leaves the operator's closed
/// lane closed, and choosing its tab shows it in the files lane.
#[test]
fn first_background_view_leaves_the_closed_lane_closed() {
    let (mut runtime, checkout, directory) = setup("background-first");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    let before = runtime.snapshot.workspace_view.clone();
    control(
        &mut runtime,
        "w-order:t1:p",
        open_page("https://example.test/first", false),
        "hidden-first",
    );
    runtime.sync_workspace_view();
    assert_eq!(runtime.snapshot.workspace_view, before);
    assert_eq!(runtime.snapshot.workspace_view.as_ref().unwrap().tool, None);
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(fronts(&runtime, &directory), ["https://example.test/first"]);
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Some(crate::workspace_views::Tool::Files)
    );
}

/// PRD right-lane B17: from the front tab, `hide file open` and `hide view
/// select` call the lane as a click does, and a file opened while the tree
/// showed alone opens beside it.
#[test]
fn a_front_tab_open_calls_the_lane_and_keeps_a_lone_tree() {
    let (mut runtime, checkout, directory) = setup("front-open");
    open(&mut runtime, &checkout, &directory, "a.md");
    let a = display_id(&runtime, &directory, "a.md");
    view_event(&mut runtime, serde_json::json!({"tool": null}));
    let calls = runtime.snapshot.workspace_view.as_ref().unwrap().lane_calls;
    let selected = control(
        &mut runtime,
        "w-order:t1:p",
        Action::Select {
            view_id: a,
            reveal: false,
            expected_browser_area: None,
        },
        "front-select",
    );
    assert!(
        selected.changed,
        "selecting the view already in front still announces the lane call"
    );
    runtime.sync_workspace_view();
    let view = runtime.snapshot.workspace_view.as_ref().unwrap();
    assert_eq!(view.tool, Some(crate::workspace_views::Tool::Files));
    assert!(view.lane_called > calls, "a put-away lane comes forward");

    // A tab shown for the first time has no view, so the tree fills it.
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().tree);
    control_file(
        &mut runtime,
        "w-order:t2:p",
        &directory,
        "b.md",
        false,
        "beside-tree",
    );
    runtime.sync_workspace_view();
    assert_eq!(strip(&runtime, &directory), ["b.md"]);
    assert!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tree,
        "the tree stays beside the opened file"
    );
}

#[test]
fn reveal_selects_the_actual_caller_and_checkout_calls_use_the_operator() {
    let (mut runtime, checkout, directory) = setup("caller-reveal");
    open(&mut runtime, &checkout, &directory, "a.md");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    control(
        &mut runtime,
        "w-order:t1:p",
        open_page("https://example.test/reveal", true),
        "reveal-a",
    );
    runtime.sync_workspace_view();
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some(TABS[0])
    );
    assert_eq!(
        fronts(&runtime, &directory),
        ["https://example.test/reveal"]
    );
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Some(crate::workspace_views::Tool::Files)
    );
    herdr_focuses(&mut runtime, &directory, TABS[0]);
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert!(
        strip(&runtime, &directory).is_empty(),
        "B never opened a.md"
    );
    let caller = checkout_caller_id("fixture", &directory.to_string_lossy());
    control(
        &mut runtime,
        &caller,
        open_page("https://example.test/checkout", false),
        "checkout-open",
    );
    runtime.sync_workspace_view();
    assert_eq!(
        fronts(&runtime, &directory),
        ["https://example.test/checkout"]
    );
}

#[test]
fn background_select_uses_as_query_and_keeps_bs_layout() {
    let (mut runtime, checkout, directory) = setup("background-select");
    open(&mut runtime, &checkout, &directory, "a.md");
    open(&mut runtime, &checkout, &directory, "b.md");
    let a = display_id(&runtime, &directory, "a.md");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    let before = runtime.snapshot.workspace_view.clone();
    let action = Action::Select {
        view_id: a.clone(),
        reveal: false,
        expected_browser_area: None,
    };
    control(&mut runtime, "w-order:t1:p", action, "select-a");
    runtime.sync_workspace_view();
    assert_eq!(runtime.snapshot.workspace_view, before);
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(published_front_ids(&runtime), [a]);
}

#[test]
fn hidden_references_protect_dirty_document_when_bs_copy_closes() {
    let (mut runtime, checkout, directory) = setup("hidden-close");
    open(&mut runtime, &checkout, &directory, "a.md");
    runtime.dispatch_json(&explorer_event(
        "file_draft",
        serde_json::json!({"contents_utf8": "shared draft\n"}),
    ));
    let a = display_id(&runtime, &directory, "a.md");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    open(&mut runtime, &checkout, &directory, "a.md");
    let b = display_id(&runtime, &directory, "a.md");
    control(
        &mut runtime,
        "w-order:t2:p",
        Action::Close {
            view_id: b,
            expected_browser_area: None,
        },
        "close-b",
    );
    runtime.sync_workspace_view();
    assert!(strip(&runtime, &directory).is_empty());
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Some(crate::workspace_views::Tool::Files),
        "closing B's last view keeps the lane"
    );
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(published_front_ids(&runtime), std::slice::from_ref(&a));
    let expected = runtime
        .workspace_control_query(crate::node::TEST_NODE, "w-order:t1:p", Query::Info)
        .unwrap()
        .context;
    let refused = runtime
        .workspace_control_action(
            crate::node::TEST_NODE,
            "w-order:t1:p",
            &expected,
            &format!("{}-last-dirty", unix_milliseconds()),
            Action::Close {
                view_id: a,
                expected_browser_area: None,
            },
            Ok(None),
        )
        .unwrap_err();
    assert_eq!(refused.reason, "unsaved_document");
}

#[test]
fn async_completion_is_bound_to_the_original_tab_and_failure_is_atomic() {
    let (mut runtime, checkout, directory) = setup("async-caller");
    open(&mut runtime, &checkout, &directory, "a.md");
    let caller = "w-order:t1:p";
    let context = runtime
        .workspace_control_query(crate::node::TEST_NODE, caller, Query::Info)
        .unwrap()
        .context;
    let action = Action::OpenFile {
        path: directory.join("c.md").to_string_lossy().into_owned(),
        beside: true,
        reveal: false,
    };
    let id = format!("{}-async", unix_milliseconds());
    let crate::workspace_control::ActionPreparation::Read(source) = runtime
        .workspace_control_prepare_action(crate::node::TEST_NODE, caller, &context, &id, &action)
        .unwrap()
    else {
        panic!("host read")
    };
    let material = source.read().unwrap();
    focus_tab(&mut runtime, &checkout, &directory, TABS[2]);
    let before = runtime.snapshot.workspace_view.clone();
    runtime
        .workspace_control_action(
            crate::node::TEST_NODE,
            caller,
            &context,
            &id,
            action.clone(),
            Ok(Some(material)),
        )
        .unwrap();
    runtime.sync_workspace_view();
    assert_eq!(runtime.snapshot.workspace_view, before);
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(fronts(&runtime, &directory), ["a.md", "c.md"]);
    // A native pane moved within the same checkout is no longer the caller
    // captured at prepare, even though its checkout capability still matches.
    let id = format!("{}-moved", unix_milliseconds());
    runtime
        .workspace_control_prepare_action(crate::node::TEST_NODE, caller, &context, &id, &action)
        .unwrap();
    let mut payload = tab_order_payload(&directory.to_string_lossy(), &TABS, &TABS, TABS[2]);
    payload.layouts[0].panes[0].pane_id = "w-order:moved:p".to_owned();
    payload.layouts[0].focused_pane_id = "w-order:moved:p".to_owned();
    payload.layouts[1].panes[0].pane_id = caller.to_owned();
    payload.layouts[1].focused_pane_id = caller.to_owned();
    runtime.ingest_session(Ok(payload));
    runtime.sync_workspace_view();
    let before = runtime.snapshot.workspace_view.clone();
    assert_eq!(
        runtime
            .workspace_control_action(
                crate::node::TEST_NODE,
                caller,
                &context,
                &id,
                action,
                Ok(None)
            )
            .unwrap_err()
            .reason,
        "pane_changed"
    );
    runtime.sync_workspace_view();
    assert_eq!(runtime.snapshot.workspace_view, before);
}

/// PRD right-lane B15, B22: a restart keeps each tab's background result and
/// the Workspace's lane; the lane stays as it is whichever tab is chosen.
#[test]
fn presentation_restart_keeps_background_results_and_the_lane() {
    let (mut runtime, checkout, directory) = setup("presentation-restart");
    open(&mut runtime, &checkout, &directory, "a.md");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    open(&mut runtime, &checkout, &directory, "a.md");
    view_event(&mut runtime, serde_json::json!({"tool": null}));
    control_file(
        &mut runtime,
        "w-order:t1:p",
        &directory,
        "c.md",
        true,
        "restart-background",
    );
    let path = runtime.workspace_views.as_ref().unwrap().path.clone();
    runtime.workspace_views =
        Some(WorkspaceViewStore::open(path, (false, RightPanelSection::default())).0);
    runtime.sync_workspace_view();
    assert_eq!(fronts(&runtime, &directory), ["a.md"]);
    assert_eq!(runtime.snapshot.workspace_view.as_ref().unwrap().tool, None);
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(fronts(&runtime, &directory), ["a.md", "c.md"]);
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Some(crate::workspace_views::Tool::Files)
    );
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert_eq!(strip(&runtime, &directory), ["a.md"]);
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Some(crate::workspace_views::Tool::Files),
        "the lane is the Workspace's"
    );
}

#[test]
fn authoritative_close_prunes_only_the_closed_tabs_resources() {
    let (mut runtime, checkout, directory) = setup("presentation-prune");
    open(&mut runtime, &checkout, &directory, "a.md");
    control(
        &mut runtime,
        "w-order:t1:p",
        open_page("https://example.test/a", false),
        "browser-a",
    );
    runtime.sync_workspace_view();
    let a = display_id(&runtime, &directory, "https://example.test/a");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    open(&mut runtime, &checkout, &directory, "a.md");
    open(&mut runtime, &checkout, &directory, "b.md");
    let b = presentation(&runtime);
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &TABS[1..],
        &TABS[1..],
        TABS[1],
    )));
    runtime.sync_workspace_view();
    assert_eq!(presentation(&runtime), b);
    assert!(
        !runtime
            .snapshot
            .browser_views
            .iter()
            .any(|view| view.view_id == a)
    );
    assert!(
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.label == "a.md")
    );
    let stored = runtime
        .workspace_views
        .as_ref()
        .unwrap()
        .views
        .get(crate::node::TEST_NODE, &directory.to_string_lossy())
        .unwrap();
    assert!(!stored.view_presentations.contains_key(TABS[0]));
}

#[test]
fn closing_a_hidden_owner_does_not_adopt_its_documents_into_b() {
    for dirty in [false, true] {
        let (mut runtime, checkout, directory) = setup(&format!("owner-close-{dirty}"));
        open(&mut runtime, &checkout, &directory, "a.md");
        focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
        control_file(
            &mut runtime,
            "w-order:t1:p",
            &directory,
            "c.md",
            false,
            "hidden-c",
        );
        if dirty {
            focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
            runtime.dispatch_json(&explorer_event(
                "file_draft",
                serde_json::json!({"contents_utf8": "kept unsaved content\n"}),
            ));
            focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
        }
        let before = presentation(&runtime);
        runtime.ingest_session(Ok(tab_order_payload(
            &directory.to_string_lossy(),
            &TABS[1..],
            &TABS[1..],
            TABS[1],
        )));
        runtime.sync_workspace_view();
        assert_eq!(
            presentation(&runtime),
            before,
            "A's closed presentation does not enter B"
        );
        assert_eq!(
            runtime
                .snapshot
                .editor
                .tabs
                .iter()
                .any(|tab| tab.label == "c.md"),
            dirty,
            "Clean unreferenced documents retire; dirty content remains recoverable"
        );
    }
}

#[test]
fn repeated_hidden_select_is_a_noop_and_a_full_workspace_closes_its_oldest_view() {
    let (mut runtime, checkout, directory) = setup("presentation-budget");
    open(&mut runtime, &checkout, &directory, "a.md");
    let a = display_id(&runtime, &directory, "a.md");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    let action = Action::Select {
        view_id: a,
        reveal: false,
        expected_browser_area: None,
    };
    control(&mut runtime, "w-order:t1:p", action.clone(), "select-first");
    runtime.sync_workspace_view();
    open(&mut runtime, &checkout, &directory, "b.md");
    let generation = runtime.workspace_views.as_ref().unwrap().generation;
    control(&mut runtime, "w-order:t1:p", action, "select-repeat");
    runtime.sync_workspace_view();
    assert_eq!(
        runtime.workspace_views.as_ref().unwrap().generation,
        generation,
        "B24 actual transitions only"
    );
    let before = runtime.snapshot.workspace_view.clone();
    let caller = "w-order:t1:p";
    for index in 0..62 {
        let action = open_page(&format!("https://example.test/{index}"), false);
        control(&mut runtime, caller, action, &format!("fill-{index}"));
    }
    runtime.sync_workspace_view();
    assert_eq!(held(&runtime, &directory), MAX_VIEW_DISPLAYS);
    control(
        &mut runtime,
        caller,
        open_page("https://example.test/too-many", false),
        "over-cap",
    );
    runtime.sync_workspace_view();
    assert_eq!(
        runtime.snapshot.workspace_view, before,
        "B's own Views stay"
    );
    assert_eq!(held(&runtime, &directory), MAX_VIEW_DISPLAYS);
    let hidden = hidden_strip(&runtime, &directory, TABS[0]);
    assert!(
        !hidden.iter().any(|name| name == "a.md"),
        "the view looked at longest ago closed"
    );
    assert!(
        hidden
            .iter()
            .any(|name| name == "https://example.test/too-many")
    );
    assert!(
        !runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.label == "a.md"),
        "its document closed with its last view"
    );
}

/// The reported case: hidden tabs fill the Workspace, and a link the
/// operator clicks still opens beside the agent, in place of the view looked
/// at longest ago.
#[test]
fn a_link_opened_in_a_full_workspace_closes_the_oldest_hidden_view() {
    let (mut runtime, checkout, directory) = setup("link-at-cap");
    open(&mut runtime, &checkout, &directory, "a.md");
    for index in 0..62 {
        control(
            &mut runtime,
            "w-order:t1:p",
            open_page(&format!("https://example.test/{index}"), false),
            &format!("fill-{index}"),
        );
    }
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    open(&mut runtime, &checkout, &directory, "b.md");
    assert_eq!(held(&runtime, &directory), MAX_VIEW_DISPLAYS);
    let url = "https://example.test/clicked";
    runtime.dispatch_json(&explorer_event(
        "browser_open",
        serde_json::json!({
            "url": url,
            "workspace": {"device_id": crate::node::TEST_NODE, "path": directory.to_string_lossy()},
        }),
    ));
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(fronts(&runtime, &directory), [url]);
    assert_eq!(strip(&runtime, &directory), ["b.md", url]);
    assert_eq!(held(&runtime, &directory), MAX_VIEW_DISPLAYS);
    assert!(
        !hidden_strip(&runtime, &directory, TABS[0])
            .iter()
            .any(|name| name == "a.md")
    );
}

/// At the cap with nothing that may close, the operator's open is refused
/// with the View layout's reason, which the shell shows.
#[test]
fn a_link_opened_with_nothing_to_close_is_refused_with_the_view_reason() {
    let (mut runtime, checkout, directory) = setup("link-refused");
    open(&mut runtime, &checkout, &directory, "a.md");
    fill_unread(&mut runtime, &directory, 63);
    runtime.sync_workspace_view();
    let before = runtime.workspace_views.as_ref().unwrap().views.clone();
    runtime.dispatch_json(&explorer_event(
        "browser_open",
        serde_json::json!({
            "url": "https://example.test/refused",
            "workspace": {"device_id": crate::node::TEST_NODE, "path": directory.to_string_lossy()},
        }),
    ));
    let refused = runtime.snapshot.status.last_error.clone().unwrap();
    assert_eq!(refused.kind, "view_layout.display_limit");
    assert_eq!(runtime.workspace_views.as_ref().unwrap().views, before);
}

#[test]
fn ordinary_multi_area_focus_keeps_the_panel_until_explicit_pending_choice() {
    let (mut runtime, checkout, directory) = setup("presentation-multi-area");
    open(&mut runtime, &checkout, &directory, "a.md");
    agent_act(
        &mut runtime,
        &directory,
        serde_json::json!({"action": "split", "tab_id": TABS[1], "area_id": "a1", "edge": "right", "request_id": "agent-split"}),
    );
    open(&mut runtime, &checkout, &directory, "b.md");
    let before = runtime
        .snapshot
        .workspace_view
        .as_ref()
        .unwrap()
        .layout
        .clone();
    runtime.dispatch_json(&operator_focus_event("w-order:t1:p"));
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().layout,
        before
    );
    control_file(
        &mut runtime,
        "w-order:t2:p",
        &directory,
        "c.md",
        false,
        "shown-background",
    );
    runtime.sync_workspace_view();
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().layout,
        before
    );
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert_eq!(fronts(&runtime, &directory), ["c.md"]);
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
            issue_tokens: Default::default(),
            issue_links: Default::default(),
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
    entry.tool = Some(crate::workspace_views::Tool::Files);
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
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Some(crate::workspace_views::Tool::Files)
    );
    assert_eq!(
        front(&runtime),
        Some(ids[0].clone()),
        "the visible tab's own"
    );

    let session = runtime.snapshot.status.remote[0].session.as_mut().unwrap();
    session.workspaces[0].checkouts[0].active_tab_id = Some("remote-tab-2".to_owned());
    runtime.sync_workspace_view();
    assert_eq!(
        runtime
            .view_layout_of(&(device.to_owned(), "/srv/remote".to_owned()))
            .unwrap()
            .display_count(),
        0,
        "another tab starts with no Views"
    );
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Some(crate::workspace_views::Tool::Files),
        "the device tab's change keeps the lane"
    );
    let session = runtime.snapshot.status.remote[0].session.as_mut().unwrap();
    session.workspaces[0].checkouts[0].active_tab_id = Some("remote-tab-1".to_owned());
    runtime.sync_workspace_view();
    assert_eq!(front(&runtime), Some(ids[0].clone()));
    assert_eq!(
        runtime.snapshot.workspace_view.as_ref().unwrap().tool,
        Some(crate::workspace_views::Tool::Files)
    );
}
