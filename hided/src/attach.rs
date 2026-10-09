//! How a node on another machine reaches this core (PRD
//! core-host-node-remote-core D-04, D-07, D-10).
//!
//! The node dials this machine over SSH and runs `hided attach` here. That
//! attach role starts no core: it finds the core already running on this
//! machine through the attach socket the core records in its state folder,
//! and pipes the node's SSH channel to it byte for byte. With no core to
//! find it answers `no_core` and exits.
//!
//! The attach socket sits in a fresh owner-only folder, so only processes
//! of this account reach it; they are inside the core's trust boundary
//! already (they can read its screen token). On it, the core and the node
//! first trade one line each: the core names its node and build, the node
//! names its own and its Herdr socket. A different build, this machine's own
//! node id, a device this core dials, or a node whose earlier link stands is
//! refused with a reason line. Otherwise the core answers with the relay
//! grant for the node's screens and the stream becomes the node's link.
//!
//! The attach role ends when its node's channel ends, when the core's socket
//! closes, or when its node has sent nothing for
//! [`hide_node_link::panes::HEARTBEAT_LIMIT`], so neither a half-open SSH
//! connection nor a stopped core leaves it running (B20).

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hide_node::ssh::RemoteHost;
use hide_node::ssh::host::inbound::{self, InboundNode};
use hide_node::terminal::device::DeviceSink;
use hide_platform::ipc::{LocalListener, LocalStream};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Notify;

use crate::core::CoreHandle;
use crate::state_file::new_token;

/// The longest handshake line either side reads.
const MAX_HANDSHAKE_LINE: u64 = 4096;
/// How long the handshake may take before the attach socket gives up on a
/// node.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Nodes the core takes the handshake of at once; more wait in the
/// listener's backlog.
const MAX_ATTACHING: usize = 4;
/// Grants held at once: one per linked node and a few being handed out.
const MAX_GRANTS: usize = 16;
/// How long a relay waits for a grant handed out moments ago to be bound to
/// its link: the node's Hello and the core's acceptance, with room.
const GRANT_BIND_WAIT: Duration = Duration::from_secs(10);

/// What the attach role answers when no core runs on its machine.
pub const NO_CORE: &str = "no_core";

/// One handshake line, as the attach role, the core and the node write it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    /// From the attach role: no core runs on its machine ([`NO_CORE`]).
    Attach(String),
    /// From the core: who it is.
    Core(CoreHello),
    /// From the node: who it is.
    Node(NodeHello),
    /// From the core: the link is taken, and how the node's screens reach
    /// the core's screen traffic.
    Accepted(Accepted),
    /// From the core: the link is refused, and why.
    Refused(Refusal),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CoreHello {
    pub node: String,
    pub build: String,
    pub protocol: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NodeHello {
    pub node: String,
    pub label: String,
    pub build: String,
    pub herdr_socket: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Accepted {
    /// The core's loopback port on its own machine.
    pub port: u16,
    /// The grant the node's screen relay presents; it ends with the link.
    pub relay_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Refusal {
    pub reason: String,
}

/// Reads one handshake line of at most [`MAX_HANDSHAKE_LINE`] bytes.
pub fn read_line(reader: &mut impl BufRead) -> Result<Line, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_HANDSHAKE_LINE + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| format!("the handshake could not be read: {error}"))?;
    if bytes.is_empty() {
        return Err("the other side closed before the handshake".to_owned());
    }
    if bytes.len() as u64 > MAX_HANDSHAKE_LINE || bytes.last() != Some(&b'\n') {
        return Err("the handshake line is too long".to_owned());
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "the handshake line is not one this build reads ({:?})",
            error.classify()
        )
    })
}

/// Writes one handshake line.
pub fn write_line(writer: &mut impl Write, line: &Line) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(line).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    writer
        .write_all(&bytes)
        .and_then(|()| writer.flush())
        .map_err(|error| format!("the handshake could not be written: {error}"))
}

/// Where the core records its attach socket.
pub fn attach_record(state_dir: &Path) -> PathBuf {
    state_dir.join("node-attach-socket")
}

/// Binds the core's attach socket and records it in `state_dir`.
pub fn bind(state_dir: &Path) -> Result<(LocalListener, PathBuf), String> {
    hide_node::pane_proof::bind_recorded(
        &attach_record(state_dir),
        "hide-attach",
        "a.sock",
        new_token,
    )
}

/// The screen relay grants this core handed to linked nodes, each bound to
/// the link it was handed out on and ending with it (D-10).
#[derive(Default)]
pub struct RelayGrants {
    grants: Mutex<HashMap<String, Grant>>,
}

struct Grant {
    node: String,
    /// The link the grant belongs to, once the link is up.
    link: Option<RemoteHost>,
}

impl RelayGrants {
    /// A new grant for `node`, not yet bound to its link; `None` at the cap.
    fn issue(&self, node: &str) -> Option<String> {
        let mut grants = lock(&self.grants);
        grants.retain(|_, grant| {
            grant
                .link
                .as_ref()
                .is_none_or(|link| link.closed_reason().is_none())
        });
        if grants.len() >= MAX_GRANTS {
            return None;
        }
        let token = new_token();
        grants.insert(
            token.clone(),
            Grant {
                node: node.to_owned(),
                link: None,
            },
        );
        Some(token)
    }

    fn bind(&self, token: &str, link: RemoteHost) {
        if let Some(grant) = lock(&self.grants).get_mut(token) {
            grant.link = Some(link);
        }
    }

    fn revoke(&self, token: &str) {
        lock(&self.grants).remove(token);
    }

    /// The node and link `token` was granted on, waiting up to
    /// [`GRANT_BIND_WAIT`] for a grant handed out moments ago to be bound:
    /// the node's screens may ask before the core finished taking its link.
    pub async fn admit(&self, token: &str) -> Option<(String, RemoteHost)> {
        let deadline = tokio::time::Instant::now() + GRANT_BIND_WAIT;
        loop {
            {
                let grants = lock(&self.grants);
                let grant = grants.get(token)?;
                if grant.link.is_some() {
                    drop(grants);
                    return self.valid(token);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The node and link `token` was granted on, while that link lives.
    pub fn valid(&self, token: &str) -> Option<(String, RemoteHost)> {
        let grants = lock(&self.grants);
        let grant = grants.get(token)?;
        let link = grant.link.as_ref()?;
        link.closed_reason()
            .is_none()
            .then(|| (grant.node.clone(), link.clone()))
    }
}

/// What the core's attach socket needs to take a node's link.
pub struct AttachService {
    pub core: Arc<CoreHandle>,
    /// This core's build, which a node must match (D-23); `None` reads it
    /// from this executable when the first node attaches.
    pub build: Option<String>,
    /// The core's loopback port, handed to a linked node's screen relay.
    pub port: u16,
    pub panes: hide_node::ssh::PaneEventsSlot,
    pub terminals: Arc<dyn DeviceSink>,
    pub grants: Arc<RelayGrants>,
}

/// Takes nodes' links on `listener` until `shutdown`.
pub async fn serve(listener: LocalListener, service: Arc<AttachService>, shutdown: Arc<Notify>) {
    // Closed however this future ends, a drop included, so the accept
    // thread never outlives it.
    let closing = CloseOnDrop {
        closer: listener.closer(),
        closed: Arc::new(AtomicBool::new(false)),
    };
    let (accepted, mut arrivals) = tokio::sync::mpsc::channel(MAX_ATTACHING);
    let accepting = {
        let closed = Arc::clone(&closing.closed);
        std::thread::Builder::new()
            .name("node-attach-accept".to_owned())
            .spawn(move || {
                loop {
                    match listener.accept() {
                        Ok(stream) => {
                            if accepted.blocking_send(Ok(stream)).is_err() {
                                return;
                            }
                        }
                        Err(error)
                            if error.kind() == std::io::ErrorKind::ConnectionAborted
                                && closed.load(Ordering::SeqCst) =>
                        {
                            return;
                        }
                        Err(error) => {
                            let _ = accepted.blocking_send(Err(error));
                            return;
                        }
                    }
                }
            })
    };
    if let Err(error) = accepting {
        attach_stopped(&error.to_string());
        return;
    }
    let limit = Arc::new(tokio::sync::Semaphore::new(MAX_ATTACHING));
    loop {
        let arrival = tokio::select! {
            arrival = arrivals.recv() => arrival,
            _ = shutdown.notified() => break,
        };
        let stream = match arrival {
            Some(Ok(stream)) => stream,
            Some(Err(error)) => {
                attach_stopped(&error.to_string());
                break;
            }
            None => break,
        };
        let Ok(permit) = Arc::clone(&limit).try_acquire_owned() else {
            herdr_core::diagnostic!(json!({
                "component": "node_link",
                "kind": "attach.busy",
                "cap": MAX_ATTACHING,
            }));
            continue;
        };
        let service = Arc::clone(&service);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            take_link(stream, &service);
        });
    }
}

struct CloseOnDrop {
    closer: hide_platform::ipc::ListenerCloser,
    /// Set before the close, so the accept thread tells its own stop from an
    /// accept that failed.
    closed: Arc<AtomicBool>,
}

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        self.closer.close();
    }
}

fn attach_stopped(message: &str) {
    herdr_core::diagnostic!(json!({
        "component": "node_link",
        "kind": "attach.stopped",
        "message": message,
    }));
}

impl AttachService {
    fn build(&self) -> Result<String, String> {
        static OWN: std::sync::OnceLock<Result<String, String>> = std::sync::OnceLock::new();
        match &self.build {
            Some(build) => Ok(build.clone()),
            None => OWN.get_or_init(crate::build_id::of_current_exe).clone(),
        }
    }
}

/// The handshake with one node and, when it is taken, its link.
fn take_link(stream: LocalStream, service: &AttachService) {
    let started = Instant::now();
    let build = match service.build() {
        Ok(build) => build,
        Err(message) => {
            attach_failed("", &message);
            return;
        }
    };
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let mut writer = stream.duplicate();
    let mut reader = BufReader::new(stream.duplicate());
    let core_node = service.core.node().as_str().to_owned();
    let hello = Line::Core(CoreHello {
        node: core_node.clone(),
        build: build.clone(),
        protocol: hide_node_link::protocol::PROTOCOL_VERSION,
    });
    if let Err(message) = write_line(&mut writer, &hello) {
        attach_failed("", &message);
        return;
    }
    let node = match read_line(&mut reader) {
        Ok(Line::Node(node)) => node,
        Ok(_) => {
            attach_failed("", "the node sent another line than its own");
            return;
        }
        Err(message) => {
            attach_failed("", &message);
            return;
        }
    };
    let refuse = |writer: &mut LocalStream, reason: &str| {
        herdr_core::diagnostic!(json!({
            "component": "node_link",
            "kind": "attach.refused",
            "node": node.node,
            "reason": reason,
            "node_build": node.build,
            "core_build": build,
        }));
        let _ = write_line(
            writer,
            &Line::Refused(Refusal {
                reason: reason.to_owned(),
            }),
        );
    };
    if node.build != build {
        refuse(&mut writer, "other_build");
        return;
    }
    if let Some(reason) = service.core.inbound_refusal(&node.node) {
        refuse(&mut writer, &reason);
        return;
    }
    let Some(relay_token) = service.grants.issue(&node.node) else {
        refuse(&mut writer, "grants_full");
        return;
    };
    let accepted = Line::Accepted(Accepted {
        port: service.port,
        relay_token: relay_token.clone(),
    });
    if let Err(message) = write_line(&mut writer, &accepted) {
        service.grants.revoke(&relay_token);
        attach_failed(&node.node, &message);
        return;
    }
    // From here the stream is the node's link; nothing of the handshake is
    // left unread, since the node writes nothing more until it is asked.
    drop(reader);
    drop(writer);
    let _ = stream.set_read_timeout(None);
    let established = inbound::establish(
        InboundNode {
            node: node.node.clone(),
            label: node.label.clone(),
            herdr_socket: node.herdr_socket.clone(),
        },
        stream,
        Some(hide_node::ssh::host::PaneHook {
            node: node.node.clone(),
            events: Arc::clone(&service.panes),
        }),
        Some(hide_node::ssh::host::TerminalHook {
            node: node.node.clone(),
            sink: Arc::clone(&service.terminals),
        }),
    );
    let transport = match established {
        Ok(transport) => transport,
        Err(message) => {
            service.grants.revoke(&relay_token);
            attach_failed(&node.node, &message);
            return;
        }
    };
    service.grants.bind(&relay_token, transport.link().clone());
    let link = transport.link().identity();
    match service
        .core
        .accept_inbound_node(&node.node, &node.label, transport)
    {
        Ok(()) => herdr_core::diagnostic!(json!({
            "component": "node_link",
            "kind": "attach.linked",
            "node": node.node,
            "link": link,
            "elapsed_ms": started.elapsed().as_millis() as u64,
        })),
        // The transport was dropped with the refusal, which closed the link.
        Err(reason) => {
            service.grants.revoke(&relay_token);
            herdr_core::diagnostic!(json!({
                "component": "node_link",
                "kind": "attach.refused",
                "node": node.node,
                "reason": reason,
            }));
        }
    }
}

fn attach_failed(node: &str, message: &str) {
    herdr_core::diagnostic!(json!({
        "component": "node_link",
        "kind": "attach.failed",
        "node": node,
        "message": message,
    }));
}

/// `hided attach [--state-dir <dir>]`: the attach role an SSH connection
/// from a node runs on the core's machine. Its standard input and output are
/// the node's channel.
pub fn run(args: &[OsString]) -> Result<(), String> {
    let state_dir = match args {
        [] => {
            let home = hide_platform::host::home_dir()
                .map_err(|error| format!("the attach role has no home folder: {error}"))?;
            hide_kit::layout::state_dir_from_process(&home)
        }
        [flag, dir] if flag == "--state-dir" => {
            let dir = PathBuf::from(dir);
            if !dir.is_absolute() {
                return Err("--state-dir must be an absolute path".to_owned());
            }
            dir
        }
        _ => return Err("usage: hided attach [--state-dir <absolute dir>]".to_owned()),
    };
    let core = hide_node::pane_proof::recorded_socket_path(&attach_record(&state_dir))
        .ok()
        .and_then(|socket| LocalStream::connect(&socket).ok());
    // A node that is not this account's own login cannot reach the record:
    // the state folder and the socket's folder are owner-only.
    let Some(core) = core else {
        // No core runs here, or it does not answer: the attach role starts
        // none (D-07) and says so.
        let mut stdout = std::io::stdout().lock();
        write_line(&mut stdout, &Line::Attach(NO_CORE.to_owned()))?;
        return Ok(());
    };
    pipe(core)
}

/// Pipes standard input to the core and the core to standard output until
/// either ends or the node falls silent, then exits the process, so no
/// direction is left waiting on the other.
fn pipe(core: LocalStream) -> ! {
    let heard = Arc::new(AtomicU64::new(now_ms()));
    let (done, ended) = std::sync::mpsc::channel::<&'static str>();
    let mut to_core = core.duplicate();
    {
        let heard = Arc::clone(&heard);
        let done = done.clone();
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let mut buffer = vec![0_u8; 64 * 1024];
            let reason = loop {
                match stdin.read(&mut buffer) {
                    Ok(0) => break "node_closed",
                    Ok(read) => {
                        heard.store(now_ms(), Ordering::Relaxed);
                        if to_core.write_all(&buffer[..read]).is_err() {
                            break "core_closed";
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break "node_closed",
                }
            };
            let _ = done.send(reason);
        });
    }
    {
        let done = done.clone();
        let mut from_core = core.duplicate();
        std::thread::spawn(move || {
            let mut stdout = std::io::stdout().lock();
            let mut buffer = vec![0_u8; 64 * 1024];
            let reason = loop {
                match from_core.read(&mut buffer) {
                    Ok(0) => break "core_closed",
                    Ok(read) => {
                        if stdout
                            .write_all(&buffer[..read])
                            .and_then(|()| stdout.flush())
                            .is_err()
                        {
                            break "node_closed";
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break "core_closed",
                }
            };
            let _ = done.send(reason);
        });
    }
    let limit = hide_node_link::panes::HEARTBEAT_LIMIT.as_millis() as u64;
    let reason = loop {
        match ended.recv_timeout(Duration::from_secs(1)) {
            Ok(reason) => break reason,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if now_ms().saturating_sub(heard.load(Ordering::Relaxed)) > limit {
                    break "node_silent";
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break "ended",
        }
    };
    core.shutdown_handle().shutdown();
    eprintln!(
        "{}",
        json!({"component": "node_link", "kind": "attach.ended", "reason": reason})
    );
    std::process::exit(0)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
