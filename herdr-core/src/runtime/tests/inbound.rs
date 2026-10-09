//! A node that dials this core (PRD core-host-node-remote-core D-04, D-10):
//! which links the core takes, how the node is registered, and what the
//! operator's device actions do to it.

use super::*;
use crate::remote::{CapabilityReport, DeviceTransport, EstablishError, Established};
use hide_node_link::device::{HostConsent, SnapshotCheck};
use std::sync::mpsc;

const NODE: &str = "inbound-node-1";

/// A node's link whose establishment waits until the test lets it go, and
/// then fails, so the core's phase for it can be read at each step.
struct HeldLink {
    release: Mutex<Option<mpsc::Receiver<()>>>,
}

impl HeldLink {
    fn new() -> (Arc<Self>, mpsc::Sender<()>) {
        let (release, wait) = mpsc::channel();
        (
            Arc::new(Self {
                release: Mutex::new(Some(wait)),
            }),
            release,
        )
    }
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

    fn capability_test(&self, operation_id: &str, _check: SnapshotCheck<'_>) -> CapabilityReport {
        CapabilityReport::new(
            operation_id,
            crate::remote::RemoteHostIdentity {
                host_id: NODE.to_owned(),
                alias: String::new(),
                hostname: NODE.to_owned(),
                port: 0,
            },
        )
    }

    fn establish(
        &self,
        _consent: &HostConsent,
        _retirement_projects: &[String],
        _on_close: Box<dyn FnOnce(String) + Send + 'static>,
    ) -> Result<Established, EstablishError> {
        if let Some(wait) = self.release.lock().unwrap().take() {
            let _ = wait.recv_timeout(Duration::from_secs(10));
        }
        Err(EstablishError::Helper("the test's link ended".to_owned()))
    }

    fn stage_attachments(
        &self,
        _request_id: &str,
        _files: &[hide_node_link::attachments::AttachmentFile],
        _cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<String>, String> {
        Err("not staged".to_owned())
    }

    fn remove_attachments(
        &self,
        _request_id: &str,
        _files: &[hide_node_link::attachments::AttachmentFile],
    ) {
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

#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn wait(shared: &Arc<Mutex<Runtime>>, what: &str, ready: impl Fn(&Runtime) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready(&shared.lock().unwrap()) {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The first link of a node registers it as a device the core never
/// dials: its row is a device row named by its host name, its registration
/// says it dials in and is kept, and it takes no consent.
#[test]
fn a_node_s_first_link_registers_it_as_a_device_that_dials_in() {
    let shared = shared_runtime();
    let (link, release) = HeldLink::new();
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
    assert!(registration.inbound);
    assert_eq!(registration.label, "MacBook");
    assert_eq!(registration.ssh_alias, None);
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
    let (link, _release) = HeldLink::new();
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
    let (link, _release) = HeldLink::new();
    assert_eq!(
        shared
            .lock()
            .unwrap()
            .accept_inbound_node("studio", "studio", link),
        Err("dialed_device".to_owned())
    );

    let (first, release_first) = HeldLink::new();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", first)
        .expect("the first link");
    wait(&shared, "the first link connecting", |runtime| {
        runtime.host_snapshot(NODE).state == "connecting"
    });
    let (second, _release) = HeldLink::new();
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
        .filter(|registration| registration.inbound)
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
    let (link, release) = HeldLink::new();
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
    let (next, release) = HeldLink::new();
    shared
        .lock()
        .unwrap()
        .accept_inbound_node(NODE, "MacBook", next)
        .expect("the next link is taken");
    wait(&shared, "the next link connecting", |runtime| {
        runtime.host_snapshot(NODE).state == "connecting"
    });
    drop(release);
}
