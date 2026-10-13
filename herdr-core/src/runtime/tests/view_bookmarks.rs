use super::workspace_view::{layout as view_event, with_new_views};
use super::*;
use crate::view_layout::Layout as ViewLayout;
use crate::workspace_control::{Action, Query, checkout_caller_id};

// The core projects one tab-owned presentation; documents stay shared.
// Tests drive public events/actions and inspect the published operator frame.

const TABS: [&str; 3] = ["w-order:t1", "w-order:t2", "w-order:t3"];

#[test]
fn rejected_background_open_preserves_documents_columns_and_save_generation() {
    let (mut runtime, checkout, directory) = setup("background-refusal");
    open(&mut runtime, &checkout, &directory, "a.md");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    open(&mut runtime, &checkout, &directory, "b.md");
    for n in 0..61 {
        control(
            &mut runtime,
            "w-order:t2:p",
            open_page(&format!("https://example.test/{n}"), false),
            &format!("fill-{n}"),
        );
    }
    runtime.sync_workspace_view();
    let before = runtime.workspace_views.as_ref().unwrap().views.clone();
    let frame = runtime.snapshot.workspace_view.clone();
    let editor = serde_json::to_value(&runtime.snapshot.editor).unwrap();
    let generation = runtime.workspace_views.as_ref().unwrap().generation;
    let calls = runtime.workspace_views.as_ref().unwrap().views_calls;
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
        (&store.views, store.generation, store.views_calls),
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
        assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().views);
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
    let before = columns(&runtime);
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
    assert_eq!(columns(&runtime), before);
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
    assert_eq!(columns(&runtime), before);
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

fn columns(runtime: &Runtime) -> crate::model::WorkspaceViewSnapshot {
    let mut view = runtime
        .snapshot
        .workspace_view
        .clone()
        .expect("a published Workspace");
    view.agent_layout = None;
    // A later explicit return to a pending result calls File Views. Compare
    // the restored columns independently of that monotonic call serial.
    view.views_calls = 0;
    view.views_called = 0;
    view
}

// These expectations come from cli-tab-background-views B1-B24. The old
// shared-strip/front-only tests are superseded by whole presentations.
#[test]
fn each_tab_keeps_its_own_list_visibility_and_shared_dirty_buffer() {
    let (mut runtime, checkout, directory) = setup("tab-private-list");
    open(&mut runtime, &checkout, &directory, "a.md");
    runtime.dispatch_json(&explorer_event(
        "file_draft",
        serde_json::json!({"contents_utf8": "unsaved text\n"}),
    ));
    let a = display_id(&runtime, &directory, "a.md");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert_eq!(
        fronts(&runtime, &directory),
        ["a.md"],
        "B2 inherits content"
    );
    assert_ne!(
        display_id(&runtime, &directory, "a.md"),
        a,
        "private display identity"
    );
    open(&mut runtime, &checkout, &directory, "b.md");
    view_event(
        &mut runtime,
        serde_json::json!({"views": false, "tools": true, "views_width": 444}),
    );
    let b = columns(&runtime);
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(
        strip(&runtime, &directory),
        ["a.md"],
        "B7 excludes B's insert"
    );
    assert_eq!(published_front_ids(&runtime), [a]);
    let view = runtime.snapshot.workspace_view.as_ref().unwrap();
    assert!(view.views && view.tools);
    assert_eq!(view.views_width, Some(444));
    assert!(
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.label == "a.md" && tab.dirty)
    );
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert_eq!(columns(&runtime), b);
}

#[test]
fn background_file_beside_preserves_b_on_and_off_and_returns_only_to_a() {
    for visible in [true, false] {
        let (mut runtime, checkout, directory) = setup(if visible {
            "background-on"
        } else {
            "background-off"
        });
        open(&mut runtime, &checkout, &directory, "a.md");
        focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
        open(&mut runtime, &checkout, &directory, "b.md");
        view_event(
            &mut runtime,
            serde_json::json!({"views": visible, "tools": true, "views_width": 444, "tools_width": 333}),
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
            "B5 exact operator presentation"
        );
        assert_eq!(runtime.snapshot.editor.active_tab_id, editor);
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout).as_deref(),
            Some(TABS[1])
        );
        focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
        assert_eq!(fronts(&runtime, &directory), ["a.md", "c.md"]);
        assert!(runtime.snapshot.workspace_view.as_ref().unwrap().views);
        focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
        assert_eq!(
            columns(&runtime),
            {
                let mut before = before.unwrap();
                before.agent_layout = None;
                before.views_calls = 0;
                before.views_called = 0;
                before
            },
            "B7 B stays independent"
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
    assert_eq!(columns(&runtime), {
        let mut before = before.unwrap();
        before.agent_layout = None;
        before.views_calls = 0;
        before.views_called = 0;
        before
    });
}

#[test]
fn first_background_view_does_not_open_bs_empty_column() {
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
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().views);
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(fronts(&runtime, &directory), ["https://example.test/first"]);
    assert!(runtime.snapshot.workspace_view.as_ref().unwrap().views);
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
    assert!(runtime.snapshot.workspace_view.as_ref().unwrap().views);
    herdr_focuses(&mut runtime, &directory, TABS[0]);
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert_eq!(strip(&runtime, &directory), ["a.md"]);
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
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().views);
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

#[test]
fn presentation_restart_keeps_background_results_and_visibility() {
    let (mut runtime, checkout, directory) = setup("presentation-restart");
    open(&mut runtime, &checkout, &directory, "a.md");
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    view_event(&mut runtime, serde_json::json!({"views": false}));
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
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().views);
    focus_tab(&mut runtime, &checkout, &directory, TABS[0]);
    assert_eq!(fronts(&runtime, &directory), ["a.md", "c.md"]);
    assert!(runtime.snapshot.workspace_view.as_ref().unwrap().views);
    focus_tab(&mut runtime, &checkout, &directory, TABS[1]);
    assert_eq!(strip(&runtime, &directory), ["a.md"]);
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().views);
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
    open(&mut runtime, &checkout, &directory, "b.md");
    let b = columns(&runtime);
    runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &TABS[1..],
        &TABS[1..],
        TABS[1],
    )));
    runtime.sync_workspace_view();
    assert_eq!(columns(&runtime), b);
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
    assert!(!stored.view_visibility.contains_key(TABS[0]));
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
        let before = columns(&runtime);
        runtime.ingest_session(Ok(tab_order_payload(
            &directory.to_string_lossy(),
            &TABS[1..],
            &TABS[1..],
            TABS[1],
        )));
        runtime.sync_workspace_view();
        assert_eq!(
            columns(&runtime),
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
fn repeated_hidden_select_is_a_noop_and_cap_refusal_keeps_b() {
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
    for index in 0..61 {
        let action = open_page(&format!("https://example.test/{index}"), false);
        control(&mut runtime, caller, action, &format!("fill-{index}"));
    }
    runtime.sync_workspace_view();
    let context = runtime
        .workspace_control_query(crate::node::TEST_NODE, caller, Query::Info)
        .unwrap()
        .context;
    let stored = runtime.workspace_views.as_ref().unwrap().views.clone();
    let refused = runtime
        .workspace_control_action(
            crate::node::TEST_NODE,
            caller,
            &context,
            &format!("{}-over-cap", unix_milliseconds()),
            open_page("https://example.test/too-many", false),
            Ok(None),
        )
        .unwrap_err();
    assert_eq!(refused.reason, "view_layout.display_limit");
    runtime.sync_workspace_view();
    assert_eq!(runtime.snapshot.workspace_view, before);
    assert_eq!(runtime.workspace_views.as_ref().unwrap().views, stored);
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
    entry
        .view_visibility
        .insert("remote-tab-1".to_owned(), true);
    entry
        .view_visibility
        .insert("remote-tab-2".to_owned(), false);
    let front = |runtime: &Runtime| {
        runtime
            .view_layout_of(&(device.to_owned(), "/srv/remote".to_owned()))
            .unwrap()
            .areas()[0]
            .active
            .clone()
    };
    runtime.sync_workspace_view();
    assert!(runtime.snapshot.workspace_view.as_ref().unwrap().views);
    assert_eq!(
        front(&runtime),
        Some(ids[0].clone()),
        "the visible tab's own"
    );

    let session = runtime.snapshot.status.remote[0].session.as_mut().unwrap();
    session.workspaces[0].checkouts[0].active_tab_id = Some("remote-tab-2".to_owned());
    runtime.sync_workspace_view();
    assert_ne!(front(&runtime), Some(ids[1].clone()));
    assert!(
        runtime
            .view_layout_of(&(device.to_owned(), "/srv/remote".to_owned()))
            .unwrap()
            .displays()
            .any(|display| display.path.ends_with("y.md")
                && Some(&display.id) == front(&runtime).as_ref())
    );
    assert!(!runtime.snapshot.workspace_view.as_ref().unwrap().views);
    let session = runtime.snapshot.status.remote[0].session.as_mut().unwrap();
    session.workspaces[0].checkouts[0].active_tab_id = Some("remote-tab-1".to_owned());
    runtime.sync_workspace_view();
    assert_eq!(front(&runtime), Some(ids[0].clone()));
    assert!(runtime.snapshot.workspace_view.as_ref().unwrap().views);
}
