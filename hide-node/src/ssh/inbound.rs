//! A node that dialed this core (PRD core-host-node-remote-core D-04, D-10):
//! its link arrives on the local stream the core machine's attach role hands
//! over, and from there it is a device link like any other. The same
//! [`RemoteHost`] carries its calls, its pane proofs and streams, its
//! terminals and its Herdr, so the credentials it vouches for are bound to
//! this link's identity exactly as a dialed device's are, and end with it.
//!
//! The core never dials such a node: [`InboundTransport::establish`] hands
//! over the link that is already up, once; a link that ended is replaced
//! only by the node dialing again. Its Herdr is reached through the link
//! (`link_herdr`), never through a socket on this machine.

use std::sync::{Arc, Mutex};

use hide_node_link::call_as;
use hide_node_link::device::{
    CapabilityReport, DeviceTransport, EstablishError, Established, HostConsent, HostIdentity,
    RemoteHostIdentity, RemoteStage, SnapshotCheck, Upload,
};
use hide_node_link::protocol::{Call, Hello, PROTOCOL_VERSION};
use serde_json::json;

use super::{HELLO_TIMEOUT, PaneHook, RemoteHost, TerminalHook, lock_recover};

/// Who the node said it is when it dialed, checked against its Hello.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundNode {
    /// Its node id: its machine identity, which its Hello must repeat.
    pub node: String,
    /// The name its row shows: the machine's host name.
    pub label: String,
    /// Its own Herdr socket, as the node spells it; the node's pane and
    /// terminal services attach there, on its own machine.
    pub herdr_socket: String,
}

/// Where the link's end goes: the core's handler, once the core took the
/// link, or held until it does.
#[derive(Default)]
struct CloseSlot {
    state: Mutex<CloseState>,
}

#[derive(Default)]
enum CloseState {
    #[default]
    Waiting,
    Heard(Box<dyn FnOnce(String) + Send + 'static>),
    Ended(String),
    Told,
}

impl CloseSlot {
    fn ended(&self, reason: String) {
        let mut state = lock_recover(&self.state);
        match std::mem::replace(&mut *state, CloseState::Told) {
            CloseState::Heard(on_close) => {
                drop(state);
                on_close(reason);
            }
            CloseState::Waiting => *state = CloseState::Ended(reason),
            told => *state = told,
        }
    }

    fn hear(&self, on_close: Box<dyn FnOnce(String) + Send + 'static>) {
        let mut state = lock_recover(&self.state);
        match std::mem::replace(&mut *state, CloseState::Told) {
            CloseState::Ended(reason) => {
                drop(state);
                on_close(reason);
            }
            CloseState::Waiting => *state = CloseState::Heard(on_close),
            told => *state = told,
        }
    }
}

/// A node that dialed this core, as the core reaches it.
pub struct InboundTransport {
    link: RemoteHost,
    node: InboundNode,
    established: Mutex<Option<Established>>,
    closed: Arc<CloseSlot>,
}

impl InboundTransport {
    /// The link, for what the shell binds to its identity (the screen relay's
    /// grant) and closes with it.
    pub fn link(&self) -> &RemoteHost {
        &self.link
    }

    pub fn node(&self) -> &InboundNode {
        &self.node
    }
}

/// Starts the link of `node` over `stream`: Hello, then its pane and
/// terminal services for its own Herdr. Refused when the node speaks
/// another protocol or its Hello names another machine than it claimed.
/// Blocking; run it off the runtime lock.
pub fn establish(
    node: InboundNode,
    stream: hide_platform::ipc::LocalStream,
    panes: Option<PaneHook>,
    terminals: Option<TerminalHook>,
) -> Result<Arc<InboundTransport>, String> {
    let closed = Arc::new(CloseSlot::default());
    let on_close = {
        let closed = Arc::clone(&closed);
        Box::new(move |reason: String| closed.ended(reason))
    };
    let target = format!("inbound:{}", node.node);
    let link = super::over_local_stream(&target, stream, panes.clone(), on_close)
        .map_err(|error| format!("the node's link could not start: {error}"))?;
    let refuse = |reason: String| {
        crate::diagnostic!(json!({
            "component": "remote_host",
            "kind": "inbound.refused",
            "target": target,
            "reason": reason,
        }));
        link.close(&reason);
        reason
    };
    let hello: Hello = call_as(&link, Call::Hello, HELLO_TIMEOUT)
        .map_err(|error| refuse(format!("the node did not answer Hello: {error}")))?;
    if hello.protocol != PROTOCOL_VERSION {
        return Err(refuse(format!(
            "the node speaks protocol {}, this core needs {PROTOCOL_VERSION}",
            hello.protocol
        )));
    }
    match hello.machine_identity.clone().into_result() {
        Ok(machine) if machine == node.node => {}
        Ok(_) => {
            return Err(refuse(
                "the node's Hello names another machine than it dialed as".to_owned(),
            ));
        }
        Err(reason) => {
            return Err(refuse(format!(
                "the node's machine identity is unavailable: {reason}"
            )));
        }
    }
    let readers = hello.readers();
    for reason in readers.diagnostics() {
        crate::diagnostic!(json!({
            "component": "remote_host",
            "kind": "host.reader_advertisement_refused",
            "target": target,
            "reason": reason,
        }));
    }
    link.take_readers(hello.protocol, readers).map_err(refuse)?;
    if panes.is_some() {
        super::start_panes_at(Ok(node.herdr_socket.clone()), &link);
    }
    let terminals = match terminals {
        Some(hook) => super::start_terminals_at(Ok(node.herdr_socket.clone()), &link, hook),
        None => Err("this process takes no device terminals".to_owned()),
    };
    crate::diagnostic!(json!({
        "component": "remote_host",
        "kind": "inbound.ready",
        "target": target,
        "link": link.identity(),
        "terminals": terminals.is_ok(),
    }));
    let established = Established {
        host: Arc::new(link.clone()),
        identity: HostIdentity {
            user: String::new(),
            hostname: node.label.clone(),
            port: 0,
            host_key_sha256: String::new(),
        },
        hello,
        installed: false,
        helper_path: String::new(),
        upload: Upload::default(),
        terminals,
    };
    Ok(Arc::new(InboundTransport {
        link,
        node,
        established: Mutex::new(Some(established)),
        closed,
    }))
}

impl DeviceTransport for InboundTransport {
    fn herdr_api_connector(&self) -> Arc<dyn hide_herdr_client::ApiConnector> {
        Arc::new(self.link.herdr_connector())
    }

    fn cached_herdr_version(&self) -> Option<String> {
        None
    }

    /// A node that dialed in has no connection of the core's to test; its
    /// row reports its link as it is.
    fn capability_test(&self, operation_id: &str, _check: SnapshotCheck<'_>) -> CapabilityReport {
        let mut report = CapabilityReport::new(
            operation_id,
            RemoteHostIdentity {
                host_id: format!("inbound:{}", self.node.node),
                alias: String::new(),
                hostname: self.node.label.clone(),
                port: 0,
            },
        );
        report.fail(
            RemoteStage::Ssh,
            "This machine connects to the core itself; it is tested from its own side",
            false,
            false,
        );
        report
    }

    fn establish(
        &self,
        _consent: &HostConsent,
        _retirement_projects: &[String],
        on_close: Box<dyn FnOnce(String) + Send + 'static>,
    ) -> Result<Established, EstablishError> {
        let established = lock_recover(&self.established).take();
        match established {
            Some(established) => {
                self.closed.hear(on_close);
                Ok(established)
            }
            None => Err(EstablishError::Helper(
                "This machine connects to the core itself; it reconnects when it can reach the core"
                    .to_owned(),
            )),
        }
    }

    /// Files reach a pane of this machine from its own screen (D-06); a
    /// window on another machine does not stage them here.
    fn stage_attachments(
        &self,
        _request_id: &str,
        _files: &[hide_node_link::attachments::AttachmentFile],
        _cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<String>, String> {
        Err("A file reaches a pane of this machine from a window on this machine only".to_owned())
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

impl Drop for InboundTransport {
    fn drop(&mut self) {
        self.link.close("the core let go of the node");
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::time::Duration;

    use hide_node_link::protocol::{MachineIdentity, Outcome, Request, Response};

    use super::*;

    /// A node that dialed in and answers its Hello as `protocol`, naming
    /// `machine`; answers what the core read from it.
    fn node_answering(
        protocol: u32,
        machine: &str,
    ) -> (
        hide_platform::ipc::LocalStream,
        std::thread::JoinHandle<bool>,
    ) {
        let (core, node) = hide_platform::ipc::LocalStream::pair().unwrap();
        let machine = machine.to_owned();
        let answering = std::thread::spawn(move || {
            node.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(node.duplicate());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: Request = serde_json::from_str(&line).unwrap();
            assert_eq!(request.call, Call::Hello);
            let hello = Hello {
                protocol,
                version: "test".to_owned(),
                os: "macos".to_owned(),
                arch: "aarch64".to_owned(),
                home: None,
                machine_identity: MachineIdentity::Available { id: machine },
                reader_features: None,
            };
            let response = Response {
                id: request.id,
                outcome: Outcome::Ok(serde_json::to_value(hello).unwrap()),
            };
            let mut writer = node;
            writeln!(writer, "{}", serde_json::to_string(&response).unwrap()).unwrap();
            // The core closes a link it refused: the node reads its end.
            let mut rest = Vec::new();
            reader.read_to_end(&mut rest).is_ok()
        });
        (core, answering)
    }

    fn inbound(node: &str) -> InboundNode {
        InboundNode {
            node: node.to_owned(),
            label: "laptop".to_owned(),
            herdr_socket: "/tmp/herdr.sock".to_owned(),
        }
    }

    /// A node on 27, the protocol of a build without the node's dial (one
    /// from main before NodeLink 28), is refused at its Hello and its link
    /// closed, so it is reinstalled rather than sent calls it does not know.
    /// The number is written out: were the bump lost, both would say 27.
    #[test]
    fn a_node_on_an_older_protocol_is_refused_and_its_link_closed() {
        let (core, node) = node_answering(27, "node-a");
        let refused = establish(inbound("node-a"), core, None, None)
            .err()
            .expect("an older node is refused");
        assert!(refused.contains("protocol 27"), "{refused}");
        assert!(node.join().unwrap(), "the refused link was not closed");
    }

    /// A node whose Hello names another machine than it dialed as is
    /// refused, so a node id cannot be borrowed.
    #[test]
    fn a_node_whose_hello_names_another_machine_is_refused() {
        let (core, node) = node_answering(PROTOCOL_VERSION, "node-b");
        let refused = establish(inbound("node-a"), core, None, None)
            .err()
            .expect("a borrowed node id is refused");
        assert!(refused.contains("another machine"), "{refused}");
        assert!(node.join().unwrap(), "the refused link was not closed");
    }
}
