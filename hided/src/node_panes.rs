//! A device's panes on the node link (PRD core-host-node B18, B19). The
//! device's node proves each caller with its own kernel and sends the proof
//! up its link; this daemon issues the credential, bound to that node and
//! that link, and answers down. A device pane's `hide` command arrives on the
//! same link as a byte stream, which this daemon serves exactly as a
//! `/ws` connection from a pane of this machine, except that only a
//! credential the same node vouched for over the same link is accepted.
//! Closing or losing the link revokes everything it vouched for at once.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use hide_node::ssh::{PaneEvents, RemoteHost};
use hide_node_link::panes::{
    MAX_CHUNK, MAX_PENDING_CHUNKS, MAX_STREAMS, NodeEvent, PaneIdentity, ProofAnswer,
};
use hide_node_link::protocol::Call;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::pane_auth::{self, RemoteGrant};
use crate::server::AppState;

/// How long one call down the link may take: an answer to a proof, or a
/// chunk of a command's bytes.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Proofs worked on at once across every device; a node may hold at most
/// sixteen callers, and a burst beyond this is refused rather than queued.
const MAX_PROOFS: usize = 32;

/// The daemon's half of every device link's pane traffic. Created empty with
/// the core, so its links can deliver from the start, and given the server's
/// state once it exists ([`NodePanes::serve`]); a proof before that is
/// refused as unavailable.
pub struct NodePanes {
    state: OnceLock<AppState>,
    runtime: tokio::runtime::Handle,
    streams: Mutex<HashMap<StreamKey, mpsc::Sender<Vec<u8>>>>,
    proofs: AtomicUsize,
}

/// One stream of one link: the same id on another link is another stream.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct StreamKey {
    node: String,
    link: usize,
    stream: u64,
}

fn link_key(link: &RemoteHost) -> usize {
    link.identity()
}

impl NodePanes {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        Self {
            state: OnceLock::new(),
            runtime,
            streams: Mutex::new(HashMap::new()),
            proofs: AtomicUsize::new(0),
        }
    }

    /// The server's state: from now on, proofs are answered and streams
    /// served.
    pub fn serve(&self, state: AppState) {
        let _ = self.state.set(state);
    }

    fn proof(
        self: &Arc<Self>,
        node: &str,
        link: &RemoteHost,
        request: u64,
        pane_id: String,
        identity: PaneIdentity,
        one_shot: bool,
    ) {
        if self.proofs.fetch_add(1, Ordering::AcqRel) >= MAX_PROOFS {
            self.proofs.fetch_sub(1, Ordering::AcqRel);
            record_refusal(node, Some(&pane_id), "bridge_busy", "core");
            answer_proof(link, request, refused("bridge_busy"));
            return;
        }
        let panes = Arc::clone(self);
        let node = node.to_owned();
        let link = link.clone();
        let spawned = std::thread::Builder::new()
            .name("hided-node-proof".to_owned())
            .spawn(move || {
                let named = pane_id.clone();
                let answer = match panes.state.get() {
                    Some(state) => issue(state, &node, &link, pane_id, &identity, one_shot),
                    None => refused("hide_unavailable"),
                };
                if let ProofAnswer::Refused { reason } = &answer {
                    record_refusal(&node, Some(&named), reason, "core");
                }
                answer_proof(&link, request, answer);
                panes.proofs.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            self.proofs.fetch_sub(1, Ordering::AcqRel);
        }
    }

    fn open_stream(&self, node: &str, link: &RemoteHost, stream: u64) {
        let key = StreamKey {
            node: node.to_owned(),
            link: link_key(link),
            stream,
        };
        let Some(state) = self.state.get().cloned() else {
            record_refusal(node, None, "hide_unavailable", "core");
            close_stream(link, stream);
            return;
        };
        let (sender, receiver) = mpsc::channel(MAX_PENDING_CHUNKS);
        {
            let mut streams = lock(&self.streams);
            let open = streams
                .keys()
                .filter(|open| open.node == key.node && open.link == key.link)
                .count();
            if open >= MAX_STREAMS || streams.contains_key(&key) {
                drop(streams);
                let reason = if open >= MAX_STREAMS {
                    "streams_full"
                } else {
                    "stream_reused"
                };
                record_refusal(node, None, reason, "core");
                close_stream(link, stream);
                return;
            }
            streams.insert(key.clone(), sender);
        }
        let node = node.to_owned();
        let link = link.clone();
        self.runtime
            .spawn(serve_stream(state, node, link, stream, receiver));
    }

    fn stream_data(&self, node: &str, link: &RemoteHost, stream: u64, data: &str) {
        let key = StreamKey {
            node: node.to_owned(),
            link: link_key(link),
            stream,
        };
        let decoded = (data.len() <= MAX_CHUNK.div_ceil(3) * 4)
            .then(|| base64::engine::general_purpose::STANDARD.decode(data).ok())
            .flatten();
        let mut streams = lock(&self.streams);
        let Some(sender) = streams.get(&key) else {
            return;
        };
        let delivered = decoded.is_some_and(|bytes| sender.try_send(bytes).is_ok());
        if !delivered {
            // A chunk the stream cannot hold ends it; the command reads the
            // end and reports its daemon unavailable.
            streams.remove(&key);
            drop(streams);
            herdr_core::diagnostic!(json!({
                "component": "node_panes",
                "kind": "stream.overflow",
                "node": node,
                "stream": stream,
            }));
            close_stream(link, stream);
        }
    }

    fn stream_closed(&self, node: &str, link: &RemoteHost, stream: u64) {
        lock(&self.streams).remove(&StreamKey {
            node: node.to_owned(),
            link: link_key(link),
            stream,
        });
    }
}

/// [`NodePanes`] as a link's pane events.
pub struct Events(pub Arc<NodePanes>);

impl PaneEvents for Events {
    fn event(&self, node: &str, link: &RemoteHost, event: NodeEvent) {
        let panes = &self.0;
        match event {
            NodeEvent::PaneProof {
                request,
                pane_id,
                identity,
                nonce: _,
                one_shot,
            } => panes.proof(node, link, request, pane_id, identity, one_shot),
            NodeEvent::Revoke { token } => {
                if let Some(state) = panes.state.get() {
                    state.pane_capabilities.revoke_vouched(node, link, &token);
                }
            }
            NodeEvent::StreamOpen { stream } => panes.open_stream(node, link, stream),
            NodeEvent::StreamData { stream, data } => panes.stream_data(node, link, stream, &data),
            NodeEvent::StreamClosed { stream } => panes.stream_closed(node, link, stream),
            NodeEvent::Refused { pane_id, reason } => {
                // The device chooses both; the record keeps a bounded prefix.
                let pane_id = pane_id.as_deref().map(|pane_id| prefix(pane_id, 256));
                record_refusal(node, pane_id, prefix(&reason, 64), "node");
            }
        }
    }

    fn closed(&self, node: &str, link: &RemoteHost) {
        let panes = &self.0;
        if let Some(state) = panes.state.get() {
            state.pane_capabilities.revoke_link(node, link);
        }
        let key = link_key(link);
        lock(&panes.streams).retain(|open, _| !(open.node == node && open.link == key));
    }
}

/// Issues the credential for a pane `node` vouched for over `link`. The
/// pane is resolved within `node`'s own panes, whatever the proof names, so
/// a node can never vouch for a pane of another device.
fn issue(
    state: &AppState,
    node: &str,
    link: &RemoteHost,
    pane_id: String,
    identity: &PaneIdentity,
    one_shot: bool,
) -> ProofAnswer {
    let issued =
        pane_auth::attest_remote(&state.core, node, &pane_id, identity).and_then(|attestation| {
            state.pane_capabilities.issue_remote(
                &attestation,
                RemoteGrant {
                    node: node.to_owned(),
                    link: link.clone(),
                    source_pane_id: pane_id,
                    one_shot,
                },
            )
        });
    match issued {
        // The link may have ended while the credential was made; nothing it
        // vouched for may outlive it.
        Ok((token, _)) if link.closed_reason().is_some() => {
            state.pane_capabilities.revoke_vouched(node, link, &token);
            refused("remote_unavailable")
        }
        Ok((token, issued_new)) => ProofAnswer::Issued { token, issued_new },
        Err(reason) => refused(reason),
    }
}

/// B30: a caller turned away, by the device's node or by this daemon, is a
/// record with the node and the pane it named, never a screen state.
fn record_refusal(node: &str, pane_id: Option<&str>, reason: &str, by: &str) {
    herdr_core::diagnostic!(json!({
        "component": "node_panes",
        "kind": "pane.refused",
        "node": node,
        "pane_id": pane_id,
        "reason": reason,
        "by": by,
    }));
}

fn prefix(text: &str, chars: usize) -> &str {
    text.char_indices()
        .nth(chars)
        .map_or(text, |(end, _)| &text[..end])
}

fn refused(reason: &str) -> ProofAnswer {
    ProofAnswer::Refused {
        reason: reason.to_owned(),
    }
}

fn answer_proof(link: &RemoteHost, request: u64, answer: ProofAnswer) {
    let _ = link.call(Call::PaneProofAnswer { request, answer }, CALL_TIMEOUT);
}

fn close_stream(link: &RemoteHost, stream: u64) {
    let link = link.clone();
    // Called from the link's reader too, which must never wait on the link.
    let _ = std::thread::Builder::new()
        .name("hided-node-stream-close".to_owned())
        .spawn(move || {
            let _ = link.call(Call::StreamClose { stream }, CALL_TIMEOUT);
        });
}

/// Serves one command's stream: its bytes go into an in-process connection
/// the link's `/ws` answers, and that connection's bytes go back down the
/// link, a chunk at a time and in order.
async fn serve_stream(
    state: AppState,
    node: String,
    link: RemoteHost,
    stream: u64,
    mut incoming: mpsc::Receiver<Vec<u8>>,
) {
    let (ours, theirs) = tokio::io::duplex(MAX_CHUNK * 2);
    let server = tokio::spawn(crate::server::serve_link_connection(
        state,
        node,
        link.clone(),
        theirs,
    ));
    let (mut reader, mut writer) = tokio::io::split(ours);
    let up = async {
        while let Some(bytes) = incoming.recv().await {
            if writer.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
    };
    let down_link = link.clone();
    let down = async move {
        let mut buffer = vec![0_u8; MAX_CHUNK];
        loop {
            let read = match reader.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            let data = base64::engine::general_purpose::STANDARD.encode(&buffer[..read]);
            let link = down_link.clone();
            let written = tokio::task::spawn_blocking(move || {
                link.call(Call::StreamWrite { stream, data }, CALL_TIMEOUT)
            })
            .await;
            if !matches!(written, Ok(Ok(_))) {
                break;
            }
        }
    };
    tokio::join!(up, down);
    server.abort();
    let link = link.clone();
    let _ =
        tokio::task::spawn_blocking(move || link.call(Call::StreamClose { stream }, CALL_TIMEOUT))
            .await;
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
