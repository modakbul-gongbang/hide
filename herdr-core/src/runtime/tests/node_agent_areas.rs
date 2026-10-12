//! A node that dialed in arranges its Workspaces' tabs in Agent areas as
//! this machine does, since the core arranged them as its own before it moved
//! off that machine (PRD core-host-node-move D-29); a device this core dials
//! keeps the one tab its Herdr has in front.

use super::device_catalog::{
    RecordingHerdr, connected_status, dispatch, herdr_workspace, next_request, session, tree,
};
use super::*;
use crate::device_catalog;

const NODE: &str = "macbook-node";

/// `origin`'s device with one Workspace of tabs `t1` and `t2` in front, its
/// Herdr's keyboard on `t1`, and its requests recorded.
fn device_in_front(
    origin: crate::model::LinkOrigin,
    path: &str,
) -> (
    Runtime,
    Arc<Mutex<Vec<serde_json::Value>>>,
    tempfile::TempDir,
) {
    let mut runtime = runtime();
    runtime
        .snapshot
        .ui_state
        .device_registrations
        .push(crate::model::DeviceRegistration {
            id: NODE.to_owned(),
            label: "MacBook".to_owned(),
            origin,
            herdr_socket_path: None,
            host_consent: None,
        });
    runtime.snapshot.status.remote.push(connected_status(NODE));
    let views = tempfile::tempdir().unwrap();
    runtime.workspace_views =
        Some(WorkspaceViewStore::open(views.path().join("views.json"), Default::default()).0);
    let requests = Arc::new(Mutex::new(Vec::new()));
    runtime.install_remote_control(RemoteControlContext::new(
        NODE,
        Arc::new(RecordingHerdr {
            requests: requests.clone(),
        }),
        Weak::new(),
        ChangeNotifier::noop(),
    ));
    runtime.snapshot.navigator.focused_device_id = Some(NODE.to_owned());
    runtime.ingest_remote_session(NODE, Ok(focused_on(path, &["t1", "t2"], "t1")));
    runtime.sync_workspace_view();
    (runtime, requests, views)
}

/// The node's Herdr reporting tabs `tabs` at `path` with its keyboard on
/// `focused`.
fn focused_on(path: &str, tabs: &[&str], focused: &str) -> RemoteSessionSnapshot {
    let tabs = tabs.iter().map(|tab| (*tab, path)).collect::<Vec<_>>();
    let mut raw = session(vec![herdr_workspace(NODE, "w1", path, &tabs)]);
    raw.focused_workspace_id = Some(format!("remote:{NODE}:workspace:w1"));
    raw.focused_checkout_id = Some(format!("remote:{NODE}:checkout:w1"));
    raw.focused_tab_id = Some(tab(focused));
    raw
}

fn tab(id: &str) -> String {
    format!("remote:{NODE}:tab:{id}")
}

fn split(runtime: &mut Runtime, path: &str, tab_id: &str) {
    dispatch(
        runtime,
        "agent_layout",
        serde_json::json!({
            "workspace": {"device_id": NODE, "path": path},
            "action": "split", "tab_id": tab(tab_id), "area_id": "a1", "edge": "right",
            "request_id": "split-1",
        }),
    );
}

fn shown(runtime: &Runtime) -> Vec<String> {
    let layout = runtime
        .snapshot()
        .workspace_view
        .as_ref()
        .expect("the front Workspace's view")
        .agent_layout
        .clone()
        .expect("the node's Agent areas");
    let mut areas = Vec::new();
    collect(&layout.root, &layout.canvases, &mut areas);
    areas
}

fn collect(
    node: &crate::split_tree::Node<crate::agent_layout::Tab>,
    canvases: &std::collections::BTreeMap<String, String>,
    shown: &mut Vec<String>,
) {
    match node {
        crate::split_tree::Node::Area(area) => {
            if let Some(tab) = canvases.get(&area.id).or(area.active.as_ref()) {
                shown.push(tab.clone());
            }
        }
        crate::split_tree::Node::Split(split) => {
            collect(&split.first, canvases, shown);
            collect(&split.second, canvases, shown);
        }
    }
}

/// A split on a node's Workspace shows both tabs, asks the node's Herdr for
/// the tab the new area holds, and attaches the panes of both; the node's
/// keyboard moving back to the first tab moves the area in use with it.
#[test]
fn a_node_that_dialed_in_splits_its_tabs_into_agent_areas_and_its_herdr_shows_the_active_one() {
    let t = tree();
    let (mut runtime, requests, _views) =
        device_in_front(crate::model::LinkOrigin::Inbound, &t.main);

    split(&mut runtime, &t.main, "t2");
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(shown(&runtime), [tab("t1"), tab("t2")]);
    let request = next_request(&requests, 0);
    assert_eq!(request["method"], "tab.focus");
    assert_eq!(
        request["params"]["tab_id"], "t2",
        "the id the node's Herdr knows"
    );
    for pane in ["t1", "t2"] {
        assert!(
            runtime
                .terminal_attach_requested
                .contains(&format!("remote:{NODE}:pane:{pane}")),
            "the pane of {pane}, shown in its own area, attaches"
        );
    }

    runtime.ingest_remote_session(NODE, Ok(focused_on(&t.main, &["t1", "t2"], "t2")));
    runtime.sync_workspace_view();
    assert_eq!(shown(&runtime), [tab("t1"), tab("t2")]);
    runtime.ingest_remote_session(NODE, Ok(focused_on(&t.main, &["t1", "t2"], "t1")));
    runtime.sync_workspace_view();
    let layout = runtime
        .workspace_views
        .as_ref()
        .unwrap()
        .views
        .get(NODE, &t.main)
        .unwrap()
        .agent_layout
        .clone();
    assert_eq!(layout.active(), Some(tab("t1").as_str()));
    assert_eq!(
        shown(&runtime),
        [tab("t1"), tab("t2")],
        "no area lost its tab"
    );
}

/// A tab the node's Herdr opens joins the area in use without replacing
/// what it shows, and a tab it closes leaves its area, collapsing it.
#[test]
fn a_node_s_own_tabs_join_and_leave_its_agent_areas() {
    let t = tree();
    let (mut runtime, _requests, _views) =
        device_in_front(crate::model::LinkOrigin::Inbound, &t.main);
    split(&mut runtime, &t.main, "t2");
    runtime.ingest_remote_session(NODE, Ok(focused_on(&t.main, &["t1", "t2", "t3"], "t2")));
    runtime.sync_workspace_view();
    assert_eq!(shown(&runtime), [tab("t1"), tab("t2")]);

    runtime.ingest_remote_session(NODE, Ok(focused_on(&t.main, &["t1", "t3"], "t1")));
    runtime.sync_workspace_view();
    assert_eq!(shown(&runtime), [tab("t1")], "the emptied area closed");
}

/// A device this core dials keeps the one tab its Herdr has in front: no
/// Agent areas reach the window and a split is refused with the reason.
#[test]
fn a_dialed_device_shows_one_tab_and_refuses_agent_groups() {
    let t = tree();
    let (mut runtime, requests, _views) = device_in_front(
        crate::model::LinkOrigin::Dialed {
            ssh_alias: NODE.to_owned(),
        },
        &t.main,
    );
    assert_eq!(
        runtime
            .snapshot()
            .workspace_view
            .as_ref()
            .map(|view| view.agent_layout.is_none()),
        Some(true)
    );
    split(&mut runtime, &t.main, "t2");
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("agent_layout.unsupported")
    );
    assert!(requests.lock().unwrap().is_empty());
    let checkout = device_catalog::checkout_id(NODE, &t.main);
    assert!(
        runtime
            .workspace_views
            .as_ref()
            .unwrap()
            .views
            .get(NODE, &t.main)
            .is_none_or(|view| view.agent_layout.tree.display_count() == 0),
        "nothing arranged for {checkout}"
    );
}
