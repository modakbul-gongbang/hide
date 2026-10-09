//! Machines whose node dials this core (PRD core-host-node-remote-core D-04,
//! D-10): the screen machine of a core that runs elsewhere. The operator's
//! own SSH login opened the link, so the core asks no consent, installs no
//! kit and never dials one; it takes the link the node brought and treats
//! the node as a device from there on, keyed by its node id. A link that
//! ends is replaced only by the node dialing again.

use std::sync::Arc;

use super::*;
use crate::model::DeviceRegistration;
use crate::remote::DeviceTransport;

impl Runtime {
    /// Whether `device_id` is a node that dials this core.
    pub(super) fn is_inbound(&self, device_id: &str) -> bool {
        self.device_registration(device_id)
            .is_some_and(|registration| registration.inbound)
    }

    /// Takes the link of the node `node` dialed in on, registering the node
    /// on its first link. Refused, with the reason the node is told, when
    /// the id is this core's own node, a device this core dials, or a node
    /// whose earlier link is still live.
    pub(crate) fn accept_inbound_node(
        &mut self,
        node: &str,
        label: &str,
        transport: Arc<dyn DeviceTransport>,
    ) -> Result<(), String> {
        if let Some(reason) = self.inbound_refusal(node) {
            crate::diagnostic!(serde_json::json!({
                "component": "remote_host",
                "kind": "inbound.refused",
                "target": node,
                "reason": reason,
            }));
            return Err(reason.to_owned());
        }
        let label = label.trim();
        let label = if label.is_empty() { node } else { label };
        let existing = self
            .snapshot
            .ui_state
            .device_registrations
            .iter_mut()
            .find(|registration| registration.id == node);
        let registration = match existing {
            Some(registration) => {
                if registration.label != label {
                    registration.label = label.to_owned();
                }
                registration.clone()
            }
            None => {
                let registration = DeviceRegistration {
                    id: node.to_owned(),
                    label: label.to_owned(),
                    ssh_alias: None,
                    herdr_socket_path: None,
                    host_consent: None,
                    inbound: true,
                };
                self.snapshot
                    .ui_state
                    .device_registrations
                    .push(registration.clone());
                registration
            }
        };
        self.rebuild_device_rows();
        self.persist_current_ui_state();
        // The coordinator and transports of an earlier link are retired, as
        // an operator's Retry retires a device's, and the operator's device
        // focus stays where it was.
        if self.remote_connections.contains_key(node) {
            let focused_device = self.snapshot.navigator.focused_device_id.clone();
            let persisted_focus = self.snapshot.ui_state.focused_device_id.clone();
            self.disconnect_remote_device(node);
            self.snapshot.navigator.focused_device_id = focused_device;
            self.snapshot.ui_state.focused_device_id = persisted_focus;
        }
        crate::diagnostic!(serde_json::json!({
            "component": "remote_host",
            "kind": "inbound.accepted",
            "target": node,
        }));
        self.inbound_transports.insert(node.to_owned(), transport);
        self.connect_remote_device(&registration);
        self.refresh_device_snapshots();
        Ok(())
    }

    /// Why a link from `node` would be refused, read without changing
    /// anything: its id is not a node id, it is this core's own machine, a
    /// device this core dials, or a node whose earlier link still stands.
    pub(crate) fn inbound_refusal(&self, node: &str) -> Option<&'static str> {
        if crate::node::NodeId::parse(node).is_err() {
            return Some("invalid_node");
        }
        if node == self.node.as_str() {
            return Some("own_node");
        }
        let dialed = self
            .snapshot
            .ui_state
            .device_registrations
            .iter()
            .any(|registration| registration.id == node && !registration.inbound)
            || self
                .device_machine_ids
                .iter()
                .any(|(device, machine)| machine == node && !self.is_inbound(device));
        if dialed {
            return Some("dialed_device");
        }
        let live = match self.device_hosts.get(node).map(|host| &host.phase) {
            Some(hosts::HostPhase::Ready { host, .. }) => host.closed_reason().is_none(),
            Some(hosts::HostPhase::Connecting) => true,
            _ => false,
        };
        live.then_some("already_linked")
    }

    /// The nodes that dialed this core and are connected now, in the order
    /// they were first registered: the machines the operator works at.
    pub(crate) fn linked_nodes(&self) -> Vec<String> {
        self.snapshot
            .ui_state
            .device_registrations
            .iter()
            .filter(|registration| registration.inbound)
            .filter(|registration| {
                self.snapshot.status.remote.iter().any(|status| {
                    status.target_id == registration.id && status.state == "connected"
                })
            })
            .map(|registration| registration.id.clone())
            .collect()
    }

    /// The link a node brought, for its connection to take; `None` until
    /// the node dials.
    pub(super) fn take_inbound_transport(
        &mut self,
        device_id: &str,
    ) -> Option<Arc<dyn DeviceTransport>> {
        self.inbound_transports.remove(device_id)
    }
}

/// The link of a node that dials this core, as its machine's readers ask it:
/// each call takes the device's live link under a brief runtime lock and
/// runs outside it, so a link the node brings again is the one asked next
/// (PRD core-host-node-remote-core B13).
pub(crate) struct InboundLink {
    device_id: String,
    runtime: std::sync::Weak<Mutex<Runtime>>,
}

impl InboundLink {
    pub(crate) fn for_device(
        device_id: &str,
        runtime: std::sync::Weak<Mutex<Runtime>>,
    ) -> Arc<dyn crate::node_access::NodeLink> {
        Arc::new(Self {
            device_id: device_id.to_owned(),
            runtime,
        })
    }

    fn current(&self) -> Result<Arc<dyn crate::node_access::NodeLink>, String> {
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(|| "the runtime has ended".to_owned())?;
        let mut guard = runtime
            .lock()
            .map_err(|_| "the runtime lock is poisoned".to_owned())?;
        guard.node_link(&self.device_id)
    }
}

impl crate::node_access::NodeLink for InboundLink {
    fn call(
        &self,
        call: hide_node_link::protocol::Call,
        timeout: std::time::Duration,
    ) -> Result<crate::node_access::LinkAnswer, crate::node_access::LinkError> {
        self.current()
            .map_err(crate::node_access::LinkError::NotConnected)?
            .call(call, timeout)
    }
}
