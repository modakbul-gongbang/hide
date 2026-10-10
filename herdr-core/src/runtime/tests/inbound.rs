//! A node that dials this core (PRD core-host-node-remote-core D-04, D-10):
//! which links the core takes, how the node is registered, and what the
//! operator's device actions do to it.

use super::*;
use crate::remote::{Arrived, DeviceTransport};
use std::sync::mpsc;

const NODE: &str = "inbound-node-1";

/// The transport of a node that dialed in, reaching no Herdr.
struct HeldLink;

/// A node's link that stands until the test lets it go, and then ends, so
/// the core's phase for it can be read at each step.
fn held_link() -> (Arrived, mpsc::Sender<()>) {
    let (release, wait) = mpsc::channel::<()>();
    let arrived = Arrived {
        transport: Arc::new(HeldLink),
        node: super::device_kit::node_ready(
            super::device_kit::KitDevice::answering(Err("not asked".to_owned())),
            None,
        ),
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

    drop(release);
    wait(&shared, "the link ended", |runtime| {
        runtime.host_snapshot(NODE).state != "ready"
    });
    assert_eq!(scope(NODE), Err("link_down"));
}
