//! A node that dials this core (PRD core-host-node-remote-core D-04, D-10):
//! which links the core takes, how the node is registered, and what the
//! operator's device actions do to it.

use super::device_catalog::{herdr_workspace, session};
use super::*;
use crate::remote::{Arrived, DeviceTransport};
use std::fs;
use std::sync::mpsc;

const NODE: &str = "inbound-node-1";

/// The transport of a node that dialed in, reaching no Herdr.
struct HeldLink;

/// A node's link that stands until the test lets it go, and then ends, so
/// the core's phase for it can be read at each step.
fn held_link() -> (Arrived, mpsc::Sender<()>) {
    held_link_to(super::device_kit::KitDevice::answering(Err(
        "not asked".to_owned()
    )))
}

/// [`held_link`] whose node answers as `host` does.
fn held_link_to(host: Arc<dyn crate::node_access::NodeLink>) -> (Arrived, mpsc::Sender<()>) {
    let (release, wait) = mpsc::channel::<()>();
    let arrived = Arrived {
        transport: Arc::new(HeldLink),
        node: super::device_kit::node_ready(host, None),
        hear_close: Box::new(move |on_close| {
            let _ = wait.recv_timeout(Duration::from_secs(10));
            on_close("the test's link ended".to_owned());
        }),
    };
    (arrived, release)
}

impl DeviceTransport for HeldLink {
    fn herdr_api_connector(&self) -> Arc<dyn hide_herdr_client::ApiConnector> {
        Arc::new(hide_herdr_client::LocalSocketConnector::new(
            "/tmp/herdr-core-inbound-never.sock",
        ))
    }

    fn cached_herdr_version(&self) -> Option<String> {
        None
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
}

fn shared_runtime() -> SharedRuntime {
    let shared = SharedRuntime::new(runtime());
    shared
        .lock()
        .unwrap()
        .install_worker_context(shared.weak(), ChangeNotifier::noop());
    shared
}

/// The first link of a node registers it as a device the core never
/// dials: its row is a device row named by its host name, its registration
/// says it dials in and is kept, and it takes no consent.
#[test]
fn a_node_s_first_link_registers_it_as_a_device_that_dials_in() {
    let shared = shared_runtime();
    let (link, release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link is taken");
    let runtime = shared.lock().unwrap();
    let registration = runtime
        .snapshot
        .ui_state
        .device_registrations
        .iter()
        .find(|registration| registration.id == NODE)
        .expect("the node is registered");
    assert_eq!(registration.origin, crate::model::LinkOrigin::Inbound);
    assert_eq!(registration.label, "MacBook");
    assert_eq!(registration.host_consent, None);
    let row = runtime
        .snapshot()
        .navigator
        .devices
        .iter()
        .find(|device| device.id == NODE)
        .expect("a device row");
    assert_eq!(row.kind, "remote");
    assert_eq!(runtime.host_snapshot(NODE).consent, "granted");
    drop(runtime);
    drop(release);
}

/// The core's row names the core's machine, which a window on a node shows
/// in place of "This Mac", and only a node that dials in says so; a device
/// the core dials does not (PRD core-host-node-move B2).
#[test]
fn the_core_row_names_its_machine_and_only_a_node_row_dials_in() {
    let shared = shared_runtime();
    shared.lock().unwrap().machine_name = Some("Mac mini".to_owned());
    let event = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "register_device",
        "payload": {"id": "studio", "label": "studio", "ssh_alias": "studio-host"},
    });
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
    let (link, release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link is taken");
    let runtime = shared.lock().unwrap();
    let own = runtime.node.as_str().to_owned();
    let rows = runtime
        .snapshot()
        .navigator
        .devices
        .iter()
        .map(|device| {
            (
                device.id.clone(),
                device.machine_name.clone(),
                device.dials_in,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (own, Some("Mac mini".to_owned()), false),
            ("studio".to_owned(), None, false),
            (NODE.to_owned(), None, true),
        ]
    );
    drop(runtime);
    drop(release);
}

/// A node is refused when it names this core's own machine, a device this
/// core dials, or a node whose earlier link still stands (D-10, amendment
/// 2), and a refusal registers nothing.
#[test]
fn a_link_is_refused_for_this_machine_a_dialed_device_or_a_live_node() {
    let shared = shared_runtime();
    let own = shared.lock().unwrap().node.as_str().to_owned();
    let (link, _release) = held_link();
    assert_eq!(
        shared
            .lock()
            .unwrap()
            .accept_inbound_node(&own, "this", link),
        Err("own_node".to_owned())
    );

    let event = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "register_device",
        "payload": {"id": "studio", "label": "studio", "ssh_alias": "studio-host"},
    });
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
    let (link, _release) = held_link();
    assert_eq!(
        shared
            .lock()
            .unwrap()
            .accept_inbound_node("studio", "studio", link),
        Err("dialed_device".to_owned())
    );

    let (first, release_first) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", first)
        .expect("the first link");
    wait(&shared, "the first link ready", |runtime| {
        runtime.host_snapshot(NODE).state == "ready"
    });
    let (second, _release) = held_link();
    assert_eq!(
        shared
            .lock()
            .unwrap()
            .accept_inbound_node(NODE, "MacBook", second),
        Err("already_linked".to_owned())
    );
    let registrations = shared
        .lock()
        .unwrap()
        .snapshot
        .ui_state
        .device_registrations
        .iter()
        .filter(|registration| registration.origin == crate::model::LinkOrigin::Inbound)
        .count();
    assert_eq!(registrations, 1);
    drop(release_first);
}

/// A node's ended link is not dialed again by the core: no retry is
/// scheduled, Retry and consent are refused with a reason, and the node's
/// next link is taken.
#[test]
fn the_core_never_redials_a_node_and_takes_its_next_link() {
    let shared = shared_runtime();
    let (link, release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link");
    drop(release);
    wait(&shared, "the link ending", |runtime| {
        runtime.host_snapshot(NODE).state == "unavailable"
    });
    {
        let mut runtime = shared.lock().unwrap();
        assert!(!runtime.device_host_retries.contains_key(NODE));
        for kind in ["device_host_retry", "retry_connect", "device_host_consent"] {
            let event = serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "kind": kind,
                "payload": {"device_id": NODE, "target_id": NODE, "allow": true},
            });
            runtime.dispatch_json(&serde_json::to_vec(&event).unwrap());
            let error = runtime
                .snapshot()
                .status
                .last_error
                .clone()
                .expect("a refusal");
            assert!(
                error.kind == "device.host.inbound" || error.kind == "remote.retry_inbound",
                "{kind}: {}",
                error.kind
            );
        }
        assert!(!runtime.device_host_retries.contains_key(NODE));
        assert!(runtime.node_link(NODE).is_err());
        assert!(!runtime.device_host_connecting(NODE));
    }
    let (next, release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", next)
        .expect("the next link is taken");
    wait(&shared, "the next link ready", |runtime| {
        runtime.host_snapshot(NODE).state == "ready"
    });
    drop(release);
}

/// B16: a doorbell, a warning or a phone reply for an agent on a node goes
/// to that node's Herdr through its link; while the link is down there is
/// no way to it, and none is ever the core's own Herdr instead.
#[test]
fn a_node_s_agents_are_reached_only_through_its_link() {
    let shared = shared_runtime();
    {
        // The core's own Herdr, which a node's agent must never be sent to.
        let mut runtime = shared.lock().unwrap();
        let socket = std::env::temp_dir()
            .join(format!(
                "herdr-core-inbound-own-{}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned();
        runtime.live = Some(crate::live::LiveContext {
            socket_path: socket.clone().into(),
            runtime: std::sync::Weak::new(),
            notifier: crate::handle::ChangeNotifier::noop(),
            api_connector: Arc::new(hide_herdr_client::LocalSocketConnector::new(&socket)),
            node: Arc::new(hide_node::Local::of_process()),
        });
        let own = runtime.node.as_str().to_owned();
        assert!(runtime.delivery_connector(&own).is_some());
    }
    let (link, release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link");
    assert!(
        shared.lock().unwrap().delivery_connector(NODE).is_none(),
        "a link still connecting reaches no Herdr"
    );
    drop(release);
    wait(&shared, "the link ending", |runtime| {
        runtime.host_snapshot(NODE).state == "unavailable"
    });
    let runtime = shared.lock().unwrap();
    assert!(
        runtime.delivery_connector(NODE).is_none(),
        "a node whose link is down is reached through nothing"
    );
}

/// The machines the operator works at are the nodes that dialed this core
/// and are connected now: a node still connecting is not one, and neither
/// is a device this core dials (D-18).
#[test]
fn only_a_connected_node_that_dialed_in_is_a_linked_machine() {
    let shared = shared_runtime();
    let (link, _release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link");
    assert!(shared.lock().unwrap().linked_nodes().is_empty());
    let event = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "register_device",
        "payload": {"id": "studio", "label": "studio", "ssh_alias": "studio-host"},
    });
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
    let mut runtime = shared.lock().unwrap();
    let row = runtime
        .snapshot
        .status
        .remote
        .iter()
        .find(|row| row.target_id == NODE)
        .cloned()
        .expect("the node's status row");
    runtime.snapshot.status.remote.clear();
    for target in [NODE, "studio"] {
        let mut row = row.clone();
        row.target_id = target.to_owned();
        row.state = "connected".to_owned();
        runtime.snapshot.status.remote.push(row);
    }
    assert_eq!(runtime.linked_nodes(), vec![NODE.to_owned()]);
}

/// L11: the nodes that dial this core are capped, so a node whose identity
/// keeps changing cannot grow the device list without end; a node already
/// registered is still taken, and the label a node gives is shown as plain,
/// bounded text.
#[test]
fn inbound_nodes_are_capped_and_their_labels_are_plain_text() {
    let shared = shared_runtime();
    {
        let mut runtime = shared.lock().unwrap();
        for index in 0..crate::runtime::inbound::MAX_INBOUND_NODES {
            runtime
                .snapshot
                .ui_state
                .device_registrations
                .push(crate::model::DeviceRegistration {
                    id: format!("node-{index}"),
                    label: format!("node {index}"),
                    origin: crate::model::LinkOrigin::Inbound,
                    herdr_socket_path: None,
                    host_consent: None,
                });
        }
        assert_eq!(runtime.inbound_refusal("node-new"), Some("nodes_full"));
        assert_eq!(runtime.inbound_refusal("node-3"), None);
    }

    let (link, _release) = held_link();
    let label = format!("\u{1b}[31m{}\n", "x".repeat(200));
    shared
        .lock()
        .unwrap()
        .accept_inbound_node("node-3", &label, link)
        .expect("a registered node's link");
    let runtime = shared.lock().unwrap();
    let shown = &runtime
        .snapshot
        .ui_state
        .device_registrations
        .iter()
        .find(|registration| registration.id == "node-3")
        .unwrap()
        .label;
    assert!(!shown.chars().any(char::is_control), "{shown:?}");
    assert_eq!(shown.chars().count(), 64);
}

/// A node that dials in has no connection of the core's to test: asking
/// for a connection test names where to test it, and no test row starts.
#[test]
fn a_connection_test_of_a_node_that_dials_in_is_refused_with_where_to_test_it() {
    let shared = shared_runtime();
    let (link, release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link is taken");
    wait(&shared, "the link ready", |runtime| {
        runtime.host_snapshot(NODE).state == "ready"
    });
    let event = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "test_device",
        "payload": { "device_id": NODE }
    }))
    .expect("device event");
    let mut runtime = shared.lock().unwrap();
    assert!(runtime.dispatch_json(&event));
    let error = runtime
        .snapshot()
        .status
        .last_error
        .clone()
        .expect("error reported");
    assert_eq!(error.kind, "device.test_inbound");
    let row = runtime
        .snapshot()
        .navigator
        .devices
        .iter()
        .find(|device| device.id == NODE)
        .expect("a device row");
    assert!(row.test.is_none(), "{:?}", row.test);
    drop(runtime);
    drop(release);
}

/// A hook's Memory is answered for this machine's panes and a linked node's,
/// never for a device the core dials, which keeps none, and not while a
/// node's link is down (PRD core-host-node-move B14, Q20).
#[test]
fn memory_is_answered_for_this_machine_and_a_linked_node_only() {
    let shared = shared_runtime();
    let own = shared.lock().unwrap().node.as_str().to_owned();
    let scope = |device: &str| {
        shared
            .lock()
            .unwrap()
            .memory_scope(&crate::workspace_control::Context {
                device_id: device.to_owned(),
                workspace_id: "workspace".to_owned(),
                checkout_id: "checkout".to_owned(),
                checkout_path: "/work/project".to_owned(),
            })
            .map(|scope| (scope.node.as_str().to_owned(), scope.checkout_path))
    };
    assert_eq!(scope(&own), Ok((own.clone(), "/work/project".to_owned())));
    assert_eq!(scope(NODE), Err("unregistered"));

    let event = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "register_device",
        "payload": {"id": "studio", "label": "studio", "ssh_alias": "studio-host"},
    });
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
    assert_eq!(scope("studio"), Err("dialed_device"));

    let (link, release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link is taken");
    wait(&shared, "the link ready", |runtime| {
        runtime.host_snapshot(NODE).state == "ready"
    });
    assert_eq!(
        scope(NODE),
        Ok((NODE.to_owned(), "/work/project".to_owned()))
    );
    // The receipts the label reads find are checked against the same store:
    // this machine's and the linked node's, each read through its own link;
    // the dialed device's are dropped.
    let receipt = |session: &str| crate::labels::worker::SightedMemoryReceipt {
        provider: "opencode",
        session_id: session.to_owned(),
        cwd: "/work/project".to_owned(),
        offset: 0,
        text: String::new(),
    };
    let batches = shared.lock().unwrap().receipt_batches(vec![
        (NODE.to_owned(), receipt("on-node")),
        ("studio".to_owned(), receipt("on-studio")),
        (own.clone(), receipt("here")),
        (NODE.to_owned(), receipt("on-node-2")),
    ]);
    let mut batches = batches
        .iter()
        .map(|(node, _, receipts)| {
            (
                node.as_str().to_owned(),
                receipts
                    .iter()
                    .map(|receipt| receipt.session_id.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    batches.sort();
    let mut expected = vec![
        (own.clone(), vec!["here".to_owned()]),
        (
            NODE.to_owned(),
            vec!["on-node".to_owned(), "on-node-2".to_owned()],
        ),
    ];
    expected.sort();
    assert_eq!(batches, expected);

    drop(release);
    wait(&shared, "the link ended", |runtime| {
        runtime.host_snapshot(NODE).state != "ready"
    });
    assert_eq!(scope(NODE), Err("link_down"));
}

/// A linked node's sessions are its own session files, read through its
/// link: the right panel's Sessions for its checkout in front and a named
/// Project's Sessions list what that machine holds, never this machine's,
/// and say the node is not connected once its link is down (PRD
/// core-host-node-move B14).
#[test]
fn a_linked_node_s_sessions_are_read_through_its_link() {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let (node_home, core_home, project) = (
        root.join("node-home"),
        root.join("core-home"),
        root.join("project"),
    );
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&core_home).unwrap();
    let claude = node_home.join(".claude/projects/p");
    fs::create_dir_all(&claude).unwrap();
    let cwd = serde_json::to_string(&project).unwrap();
    fs::write(
        claude.join("on-the-node.jsonl"),
        format!(
            r#"{{"type":"user","cwd":{cwd},"timestamp":"2026-10-11T01:00:00Z","userType":"external","promptId":"p-1","message":{{"role":"user","content":"node request"}}}}"#
        ) + "\n",
    )
    .unwrap();
    let path = project.to_string_lossy().into_owned();

    let shared = shared_runtime();
    shared.lock().unwrap().own_node = Arc::new(hide_node::Local::new(Some(core_home)));
    let (link, release) = held_link_to(Arc::new(hide_node::Local::new(Some(node_home))));
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link is taken");
    wait(&shared, "the link ready", |runtime| {
        runtime.host_snapshot(NODE).state == "ready"
    });
    {
        let mut runtime = shared.lock().unwrap();
        let mut reported = session(vec![herdr_workspace(NODE, "w1", &path, &[("t1", &path)])]);
        reported.focused_workspace_id = Some(format!("remote:{NODE}:workspace:w1"));
        reported.focused_checkout_id = Some(format!("remote:{NODE}:checkout:w1"));
        runtime.ingest_remote_session(NODE, Ok(reported));
        runtime.snapshot.navigator.focused_device_id = Some(NODE.to_owned());
    }
    let refresh = |payload: serde_json::Value| {
        let event = serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "sessions_refresh", "payload": payload,
        });
        shared
            .lock()
            .unwrap()
            .dispatch_json(&serde_json::to_vec(&event).unwrap());
    };
    let ids = |rows: &[crate::model::SessionRowSnapshot]| {
        rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>()
    };

    refresh(serde_json::json!({}));
    wait(&shared, "the panel's sessions read", |runtime| {
        !runtime.snapshot.sessions.loading
    });
    {
        let runtime = shared.lock().unwrap();
        let sessions = &runtime.snapshot.sessions;
        assert_eq!(sessions.unavailable_reason, None);
        assert_eq!(sessions.checkout_path.as_deref(), Some(path.as_str()));
        assert_eq!(ids(&sessions.rows), ["on-the-node"]);
    }

    // The Project as the catalog groups the node's workspaces, which is the
    // id the Projects screen names.
    let project_id = shared
        .lock()
        .unwrap()
        .catalog_workspaces()
        .find(|workspace| workspace.device_id == NODE && workspace.path == path)
        .map(|workspace| workspace.id.clone())
        .expect("the node's Project in the catalog");
    let named = serde_json::json!({"workspace_id": project_id, "device_id": NODE});
    refresh(named.clone());
    wait(&shared, "the named Project's sessions read", |runtime| {
        runtime
            .snapshot
            .project_sessions
            .as_ref()
            .is_some_and(|sessions| !sessions.loading)
    });
    {
        let runtime = shared.lock().unwrap();
        let sessions = runtime.snapshot.project_sessions.as_ref().unwrap();
        assert_eq!(sessions.unavailable_reason, None);
        assert_eq!(sessions.failure, None);
        assert_eq!(ids(&sessions.rows), ["on-the-node"]);
    }

    drop(release);
    wait(&shared, "the link ended", |runtime| {
        runtime.host_snapshot(NODE).state != "ready"
    });
    refresh(named);
    let runtime = shared.lock().unwrap();
    let sessions = runtime.snapshot.project_sessions.as_ref().unwrap();
    assert_eq!(
        sessions.unavailable_reason.as_deref(),
        Some("MacBook is not connected.")
    );
    assert!(sessions.rows.is_empty());
}

/// A Factory runs on this machine or a node that dials it, never a device
/// it dials (PRD core-host-node-move Q17): a create sheet's machine names
/// its node, a caller on the node is admitted as on that machine and its
/// hint is a pane there, and a worker start holds on to the node's Herdr
/// and link, which stop being current once the link ends.
/// After a move every checkout of the machine the core left is a node's
/// (PRD core-host-node-move D-29, arch review open question): one in front
/// carries its node as its target, so the readers of this machine's own
/// checkout in front (the card, the Overview's re-read, the cleanup's
/// current checkout, the GitHub order) do not take it for this machine's.
#[test]
fn a_node_s_checkout_in_front_is_the_node_s_not_this_machine_s() {
    let dir = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(dir.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let shared = shared_runtime();
    let (link, _release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link is taken");
    wait(&shared, "the link ready", |runtime| {
        runtime.host_snapshot(NODE).state == "ready"
    });
    let mut runtime = shared.lock().unwrap();
    let workspace_id = format!("remote:{NODE}:workspace:w1");
    let mut reported = session(vec![herdr_workspace(NODE, "w1", &path, &[("t1", &path)])]);
    reported.focused_workspace_id = Some(workspace_id.clone());
    runtime.ingest_remote_session(NODE, Ok(reported));
    // The node's projects are its session's rows, never this machine's.
    assert!(runtime.snapshot.navigator.workspaces.is_empty());
    let workspace = runtime
        .snapshot
        .status
        .remote
        .iter()
        .filter_map(|status| status.session.as_ref())
        .flat_map(|session| session.workspaces.iter())
        .find(|workspace| workspace.id == workspace_id)
        .expect("the node's project is listed");
    assert_eq!(workspace.remote_target_id.as_deref(), Some(NODE));
    let checkout = workspace.checkouts[0].id.clone();
    runtime.snapshot.navigator.focused_device_id = Some(NODE.to_owned());
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout);
    assert!(runtime.focused_local_checkout().is_none());
}

#[test]
fn a_factory_runs_on_this_machine_or_a_node_that_dials_in_never_a_dialed_device() {
    let shared = shared_runtime();
    let own = shared.lock().unwrap().node.as_str().to_owned();
    let event = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "register_device",
        "payload": {"id": "studio", "label": "studio", "ssh_alias": "studio-host"},
    });
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
    let (link, release) = held_link();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", link)
        .expect("the link is taken");
    wait(&shared, "the link ready", |runtime| {
        runtime.host_snapshot(NODE).state == "ready"
    });
    let path = "/Users/alice/alpha";
    let views = tempfile::tempdir().unwrap();
    {
        let mut runtime = shared.lock().unwrap();
        runtime.ingest_remote_session(
            NODE,
            Ok(session(vec![herdr_workspace(
                NODE,
                "w1",
                path,
                &[("t1", path)],
            )])),
        );
        runtime.workspace_views =
            Some(WorkspaceViewStore::open(views.path().join("views.json"), Default::default()).0);
    }

    let runtime = shared.lock().unwrap();
    let node = |device: &str| {
        runtime
            .factory_node(device)
            .map_err(|refusal| refusal.reason)
    };
    assert_eq!(node(&own), Ok(None));
    assert_eq!(node(NODE), Ok(Some(NODE.to_owned())));
    assert_eq!(node("studio"), Err("factory_device_unsupported".to_owned()));
    assert_eq!(node("gone"), Err("factory_device_unknown".to_owned()));

    let pane = format!("remote:{NODE}:pane:t1");
    let context = runtime
        .workspace_control_query(NODE, &pane, crate::workspace_control::Query::Info)
        .expect("the node's pane is in the catalog")
        .context;
    let caller = runtime
        .factory_caller(NODE, &pane, &context, Some("t9"))
        .expect("a caller on the node");
    assert_eq!(caller.node.as_deref(), Some(NODE));
    assert_eq!(caller.pane.as_deref(), Some(pane.as_str()));
    assert_eq!(caller.cwd.as_deref(), Some(path));
    assert_eq!(
        caller.claimed,
        Some(format!("remote:{NODE}:pane:t9")),
        "a node's hint names a pane on that node"
    );
    assert_eq!(
        runtime
            .factory_caller("studio", "w1:p1", &context, None)
            .err()
            .as_deref(),
        Some("factory_local_only")
    );

    let control = runtime
        .factory_start_control(Some(NODE))
        .expect("the node's Herdr and link");
    assert!(Arc::ptr_eq(
        &control.connector,
        &runtime.delivery_connector(NODE).unwrap()
    ));
    assert!(runtime.factory_start_control_current(&control));
    drop(runtime);
    drop(release);
    wait(&shared, "the link ended", |runtime| {
        runtime.host_snapshot(NODE).state != "ready"
    });
    let runtime = shared.lock().unwrap();
    assert!(!runtime.factory_start_control_current(&control));
    assert!(runtime.factory_start_control(Some(NODE)).is_err());
}
