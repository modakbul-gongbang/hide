//! The screen side of the terminal path (PRD core-host-node-terminal D-11,
//! D-16, D-22): every pane's output, from this machine's node or a
//! device's, reaches the `/ws` clients from here without passing the core.
//!
//! Each chunk is rendered once, as the JSON object a `terminal` frame
//! carries, and shared: a pane's recent chunks (at most
//! [`RETAINED_CHUNKS`] and [`RETAINED_BYTES`]) and every client's unsent
//! queue hold the same allocation, so a pane's retention stays under
//! `RETAINED_BYTES + MAX_UNSENT_OUTPUT_BYTES` however many clients are
//! connected. One sequence numbers every chunk of every pane; a client's
//! cursor is the last one it was sent, so one number resumes every pane.
//!
//! A client that leaves a pane's output unsent past
//! [`MAX_UNSENT_OUTPUT_BYTES`] loses that pane's backlog and gets nothing
//! more of it until a full frame, which it asks the pane's node for. A
//! client that resumes from a cursor the retained chunks no longer reach,
//! for a pane, is treated the same way for that pane. Either way the pane is
//! drawn whole, never left blank or drawn from a broken stream, and the
//! redraw is counted in the diagnostic log.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use base64::Engine as _;
use hide_node::terminal::OutputSink;
use hide_node_link::terminal::{MAX_UNSENT_OUTPUT_BYTES, RETAINED_BYTES, RETAINED_CHUNKS};
use serde_json::json;

/// Chunks one `terminal` frame carries at most, so a client catching up
/// gets its backlog in frames the socket can interleave with the rest.
const FRAME_CHUNKS: usize = 256;
/// A redraw asked for and not answered by a full frame is asked again
/// after this long.
const REDRAW_RETRY: Duration = Duration::from_secs(2);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Chunk {
    sequence: u64,
    pane: Arc<str>,
    /// `{"pane_id": …, "bytes_base64": …}`, as a frame carries it.
    json: Box<str>,
}

impl Chunk {
    fn size(&self) -> usize {
        self.json.len()
    }
}

#[derive(Default)]
struct PaneRing {
    chunks: VecDeque<Arc<Chunk>>,
    bytes: usize,
    /// The highest sequence trimmed from this pane's ring: a cursor below
    /// it cannot be resumed from the ring.
    trimmed_through: u64,
    /// A client dropped output of this pane that no full frame has
    /// replaced yet; a client resuming cannot tell whether it was the one.
    unrepaired_drop: bool,
    /// When a redraw was last asked for and not answered yet.
    redraw_asked: Option<Instant>,
}

struct Client {
    queue: VecDeque<Arc<Chunk>>,
    /// Each pane's bytes in `queue`.
    unsent: HashMap<Arc<str>, usize>,
    /// Panes this client gets nothing of until their next full frame.
    awaiting_full: HashSet<Arc<str>>,
    /// Panes whose redraw this client asks the node for on its next turn.
    redraw: HashSet<Arc<str>>,
    /// The last sequence decided for this client, sent or withheld.
    examined: u64,
    wake: Arc<tokio::sync::Notify>,
}

#[derive(Default)]
struct HubState {
    sequence: u64,
    panes: HashMap<Arc<str>, PaneRing>,
    clients: HashMap<u64, Client>,
    next_client: u64,
}

/// Every pane's output on its way to the screens.
#[derive(Default)]
pub struct TerminalHub {
    state: Mutex<HubState>,
}

/// One `terminal` frame for a client: its chunks and the cursor they bring
/// it to.
pub struct TerminalFrame {
    pub text: String,
}

/// A client's place in the hub; dropping it removes the client.
pub struct HubClient {
    hub: Arc<TerminalHub>,
    id: u64,
    wake: Arc<tokio::sync::Notify>,
}

impl TerminalHub {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    /// Adds a client. `resume` is the cursor of a client that kept its
    /// terminals across a reconnect; `None` is a client that draws every
    /// pane from its next full frame (it asks for one per view).
    pub fn connect(self: &Arc<Self>, resume: Option<u64>) -> HubClient {
        let wake = Arc::new(tokio::sync::Notify::new());
        let mut state = lock(&self.state);
        let id = state.next_client;
        state.next_client += 1;
        let mut client = Client {
            queue: VecDeque::new(),
            unsent: HashMap::new(),
            awaiting_full: HashSet::new(),
            redraw: HashSet::new(),
            examined: state.sequence,
            wake: Arc::clone(&wake),
        };
        if let Some(cursor) = resume {
            resume_from(&mut state, &mut client, cursor);
        }
        if !client.queue.is_empty() || !client.redraw.is_empty() {
            wake.notify_one();
        }
        state.clients.insert(id, client);
        drop(state);
        HubClient {
            hub: Arc::clone(self),
            id,
            wake,
        }
    }

    /// Bytes each pane keeps for clients now, for the memory bound's test
    /// and the measurement.
    pub fn retained_bytes(&self) -> HashMap<String, usize> {
        let state = lock(&self.state);
        let mut retained = HashMap::<String, HashSet<*const Chunk>>::new();
        let mut sizes = HashMap::<*const Chunk, usize>::new();
        let chunks = state
            .panes
            .values()
            .flat_map(|ring| ring.chunks.iter())
            .chain(
                state
                    .clients
                    .values()
                    .flat_map(|client| client.queue.iter()),
            );
        for chunk in chunks {
            let key = Arc::as_ptr(chunk);
            sizes.insert(key, chunk.size());
            retained
                .entry(chunk.pane.to_string())
                .or_default()
                .insert(key);
        }
        retained
            .into_iter()
            .map(|(pane, chunks)| (pane, chunks.iter().map(|chunk| sizes[chunk]).sum()))
            .collect()
    }
}

/// Queues what a resuming client missed, pane by pane: the chunks after its
/// cursor when the ring still holds them all, else nothing of that pane
/// until a full frame it asks for.
fn resume_from(state: &mut HubState, client: &mut Client, cursor: u64) {
    if cursor > state.sequence {
        // A cursor from another hub (the daemon restarted): every pane it
        // shows is drawn again.
        for pane in state.panes.keys() {
            await_full(client, pane, "resume_unknown_cursor");
        }
        return;
    }
    let mut missed = Vec::new();
    for (pane, ring) in &state.panes {
        if ring.trimmed_through > cursor || ring.unrepaired_drop {
            await_full(client, pane, "resume_gap");
            continue;
        }
        missed.extend(
            ring.chunks
                .iter()
                .filter(|chunk| chunk.sequence > cursor)
                .cloned(),
        );
    }
    missed.sort_by_key(|chunk| chunk.sequence);
    for chunk in missed {
        *client.unsent.entry(Arc::clone(&chunk.pane)).or_default() += chunk.size();
        client.queue.push_back(chunk);
    }
}

fn await_full(client: &mut Client, pane: &Arc<str>, cause: &str) {
    client.awaiting_full.insert(Arc::clone(pane));
    client.redraw.insert(Arc::clone(pane));
    herdr_core::diagnostic!(json!({
        "component": "terminal_hub",
        "kind": "terminal.redraw_wanted",
        "pane_id": pane.as_ref(),
        "cause": cause,
    }));
}

impl OutputSink for TerminalHub {
    fn output(&self, pane: &str, bytes: &[u8], full: bool) {
        let mut state = lock(&self.state);
        state.sequence += 1;
        let sequence = state.sequence;
        let (pane, ring) = match state.panes.get_key_value(pane) {
            Some((key, _)) => {
                let key = Arc::clone(key);
                let ring = state.panes.get_mut(&key).expect("present");
                (key, ring)
            }
            None => {
                let key: Arc<str> = Arc::from(pane);
                (Arc::clone(&key), state.panes.entry(key).or_default())
            }
        };
        let json = json!({
            "pane_id": pane.as_ref(),
            "bytes_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
        })
        .to_string()
        .into_boxed_str();
        let chunk = Arc::new(Chunk {
            sequence,
            pane: Arc::clone(&pane),
            json,
        });
        if full {
            ring.unrepaired_drop = false;
            ring.redraw_asked = None;
        }
        ring.bytes += chunk.size();
        ring.chunks.push_back(Arc::clone(&chunk));
        while ring.chunks.len() > RETAINED_CHUNKS || ring.bytes > RETAINED_BYTES {
            let Some(trimmed) = ring.chunks.pop_front() else {
                break;
            };
            ring.bytes -= trimmed.size();
            ring.trimmed_through = trimmed.sequence;
        }
        let HubState { panes, clients, .. } = &mut *state;
        let ring = panes.get_mut(&pane).expect("present");
        let now = Instant::now();
        // The pane keeps drawing and no full frame has come since a redraw
        // was asked for: a client still waiting asks again.
        let ask_again = !full
            && ring
                .redraw_asked
                .is_some_and(|asked| now.duration_since(asked) >= REDRAW_RETRY);
        for client in clients.values_mut() {
            client.examined = sequence;
            if client.awaiting_full.contains(&pane) {
                if !full {
                    if ask_again && client.redraw.insert(Arc::clone(&pane)) {
                        client.wake.notify_one();
                    }
                    continue;
                }
                client.awaiting_full.remove(&pane);
            }
            let unsent = client.unsent.entry(Arc::clone(&pane)).or_default();
            if *unsent + chunk.size() > MAX_UNSENT_OUTPUT_BYTES {
                let dropped = *unsent;
                *unsent = 0;
                client.queue.retain(|queued| queued.pane != pane);
                ring.unrepaired_drop = true;
                herdr_core::diagnostic!(json!({
                    "component": "terminal_hub",
                    "kind": "terminal.output_overflow",
                    "pane_id": pane.as_ref(),
                    "bytes": dropped + chunk.size(),
                    "cap": MAX_UNSENT_OUTPUT_BYTES,
                }));
                await_full(client, &pane, "overflow");
                client.wake.notify_one();
                continue;
            }
            *unsent += chunk.size();
            client.queue.push_back(Arc::clone(&chunk));
            client.wake.notify_one();
        }
    }

    fn forget(&self, pane: &str) {
        let mut state = lock(&self.state);
        state.panes.remove(pane);
        for client in state.clients.values_mut() {
            client.queue.retain(|queued| queued.pane.as_ref() != pane);
            client.unsent.remove(pane);
            client.awaiting_full.remove(pane);
            client.redraw.remove(pane);
        }
    }
}

impl HubClient {
    /// Resolves when this client has something to send or a redraw to ask
    /// for.
    pub async fn ready(&self) {
        self.wake.notified().await;
    }

    /// The client drew every pane again from scratch (a self-contained
    /// snapshot reset its terminals): nothing queued for it before applies,
    /// and it asks for each view's full frame itself.
    pub fn restart(&self) {
        let mut state = lock(&self.hub.state);
        let sequence = state.sequence;
        if let Some(client) = state.clients.get_mut(&self.id) {
            client.queue.clear();
            client.unsent.clear();
            client.awaiting_full.clear();
            client.redraw.clear();
            client.examined = sequence;
        }
    }

    /// The next frame to send and the panes whose node should draw them
    /// again. Draining stops at [`FRAME_CHUNKS`]; a client with more is
    /// woken again.
    pub fn take(&self) -> (Option<TerminalFrame>, Vec<String>) {
        let mut state = lock(&self.hub.state);
        let now = Instant::now();
        let HubState { panes, clients, .. } = &mut *state;
        let Some(client) = clients.get_mut(&self.id) else {
            return (None, Vec::new());
        };
        // Each pane's redraw is asked for once across clients until it is
        // answered or overdue.
        let mut redraws = Vec::new();
        for pane in client.redraw.drain() {
            let Some(ring) = panes.get_mut(&pane) else {
                continue;
            };
            if ring
                .redraw_asked
                .is_some_and(|asked| now.duration_since(asked) < REDRAW_RETRY)
            {
                continue;
            }
            ring.redraw_asked = Some(now);
            redraws.push(pane.to_string());
        }
        if client.queue.is_empty() {
            return (None, redraws);
        }
        let mut text = String::from(r#"{"type":"terminal","payload":{"chunks":["#);
        let mut last = 0;
        for index in 0..FRAME_CHUNKS {
            let Some(chunk) = client.queue.pop_front() else {
                break;
            };
            if let Some(unsent) = client.unsent.get_mut(&chunk.pane) {
                *unsent = unsent.saturating_sub(chunk.size());
            }
            if index > 0 {
                text.push(',');
            }
            text.push_str(&chunk.json);
            last = chunk.sequence;
        }
        // A drained queue brings the client to everything decided for it,
        // withheld chunks included.
        let cursor = if client.queue.is_empty() {
            client.examined.max(last)
        } else {
            client.wake.notify_one();
            last
        };
        text.push_str(&format!(r#"],"terminal_sequence":{cursor}}}}}"#));
        if !redraws.is_empty() {
            herdr_core::diagnostic!(json!({
                "component": "terminal_hub",
                "kind": "terminal.redraw_requested",
                "panes": redraws.len(),
            }));
        }
        (Some(TerminalFrame { text }), redraws)
    }
}

impl Drop for HubClient {
    fn drop(&mut self) {
        lock(&self.hub.state).clients.remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_chunks(frame: &TerminalFrame) -> (Vec<(String, Vec<u8>)>, u64) {
        let value: serde_json::Value = serde_json::from_str(&frame.text).unwrap();
        assert_eq!(value["type"], "terminal");
        let chunks = value["payload"]["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|chunk| {
                (
                    chunk["pane_id"].as_str().unwrap().to_owned(),
                    base64::engine::general_purpose::STANDARD
                        .decode(chunk["bytes_base64"].as_str().unwrap())
                        .unwrap(),
                )
            })
            .collect();
        (
            chunks,
            value["payload"]["terminal_sequence"].as_u64().unwrap(),
        )
    }

    #[test]
    fn a_client_resumes_every_pane_from_one_cursor() {
        let hub = TerminalHub::new();
        let first = hub.connect(None);
        hub.output("w1:p1", b"a", true);
        hub.output("w1:p2", b"b", true);
        let (frame, _) = first.take();
        let (_, cursor) = frame_chunks(&frame.unwrap());
        assert_eq!(cursor, 2);
        drop(first);
        hub.output("w1:p1", b"c", false);
        hub.output("w1:p2", b"d", false);
        let resumed = hub.connect(Some(cursor));
        let (frame, redraws) = resumed.take();
        assert!(redraws.is_empty());
        let (chunks, cursor) = frame_chunks(&frame.unwrap());
        assert_eq!(
            chunks,
            [
                ("w1:p1".to_owned(), b"c".to_vec()),
                ("w1:p2".to_owned(), b"d".to_vec())
            ]
        );
        assert_eq!(cursor, 4);
    }

    #[test]
    fn a_cursor_the_ring_no_longer_reaches_redraws_that_pane_and_resumes_the_rest() {
        let hub = TerminalHub::new();
        hub.output("w1:p1", b"start", true);
        hub.output("w1:p2", b"quiet", true);
        let cursor = 2;
        for _ in 0..RETAINED_CHUNKS + 1 {
            hub.output("w1:p1", b"x", false);
        }
        hub.output("w1:p2", b"more", false);
        let resumed = hub.connect(Some(cursor));
        let (frame, redraws) = resumed.take();
        assert_eq!(redraws, ["w1:p1"]);
        let (chunks, _) = frame_chunks(&frame.unwrap());
        assert_eq!(chunks, [("w1:p2".to_owned(), b"more".to_vec())]);
        // Nothing of the redrawn pane reaches it before the full frame.
        hub.output("w1:p1", b"partial", false);
        hub.output("w1:p1", b"\x1bcwhole", true);
        let (frame, _) = resumed.take();
        let (chunks, _) = frame_chunks(&frame.unwrap());
        assert_eq!(chunks, [("w1:p1".to_owned(), b"\x1bcwhole".to_vec())]);
    }

    /// D-16, B12: with a client stalled, a flooding pane's retention stays
    /// under 3 MiB and the live client loses nothing.
    #[test]
    fn a_stalled_client_bounds_the_panes_memory_and_costs_the_live_client_nothing() {
        let hub = TerminalHub::new();
        let stalled = hub.connect(None);
        let live = hub.connect(None);
        let chunk = vec![b'z'; 32 * 1024];
        let mut received = 0;
        let mut sent = 0;
        for _ in 0..400 {
            hub.output("w1:p1", &chunk, false);
            sent += 1;
            let (frame, redraws) = live.take();
            assert!(redraws.is_empty(), "the live client never fell behind");
            received += frame_chunks(&frame.unwrap()).0.len();
            let retained = hub.retained_bytes();
            assert!(
                retained["w1:p1"] < 3 * 1024 * 1024,
                "retained {} bytes",
                retained["w1:p1"]
            );
        }
        assert_eq!(received, sent);
        // The stalled client dropped the pane's backlog and asks once for a
        // full frame.
        let (frame, redraws) = stalled.take();
        assert_eq!(redraws, ["w1:p1"]);
        assert!(frame.is_none_or(|frame| frame_chunks(&frame).0.len() < 40));
        hub.output("w1:p1", b"\x1bcwhole", true);
        let (frame, _) = stalled.take();
        let (chunks, _) = frame_chunks(&frame.unwrap());
        assert_eq!(chunks.last().unwrap().1, b"\x1bcwhole");
    }

    #[test]
    fn a_forgotten_pane_leaves_the_ring_and_every_queue() {
        let hub = TerminalHub::new();
        let client = hub.connect(None);
        hub.output("w1:p1", b"a", true);
        hub.forget("w1:p1");
        assert!(client.take().0.is_none());
        assert!(hub.retained_bytes().is_empty());
    }
}
