//! The screen side of the terminal path (PRD core-host-node-terminal D-11,
//! D-16, D-22): every pane's output, from this machine's node or a
//! device's, reaches the `/ws` clients from here without passing the core.
//!
//! Each chunk is rendered once, as the JSON object a `terminal` frame
//! carries, and shared: a pane's recent chunks (at most
//! [`RETAINED_CHUNKS`] and [`RETAINED_BYTES`]) and every client's unsent
//! queue hold the same allocation, so a pane's retention stays under
//! `RETAINED_BYTES + MAX_UNSENT_OUTPUT_BYTES` and its latest full frame
//! however many clients are connected. One sequence numbers every chunk of every pane; a client's
//! cursor is the last one it was sent, so one number resumes every pane.
//!
//! A client that leaves a pane's output unsent past
//! [`MAX_UNSENT_OUTPUT_BYTES`] loses that pane's backlog and gets nothing
//! more of it until a full frame, which it asks the pane's node for. A full
//! frame replaces whatever of its pane waits for a client, since it draws
//! over it, and is never counted against the cap, so a pane whose full
//! frame alone passes the cap still draws. A
//! client that resumes from a cursor the retained chunks no longer reach,
//! for a pane, is treated the same way for that pane. Either way the pane is
//! drawn whole, never left blank or drawn from a broken stream, and the
//! redraw is counted in the diagnostic log.
//!
//! A chunk past [`MAX_CHUNK_BYTES`] is not drawn at all: its pane waits for
//! a full frame that fits, asked for when the long chunk was not full
//! itself, so a pane whose every frame is that long cannot loop.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use base64::Engine as _;
use hide_node::terminal::OutputSink;
use hide_node_link::terminal::{MAX_UNSENT_OUTPUT_BYTES, RETAINED_BYTES, RETAINED_CHUNKS};
use serde_json::json;

/// Chunks one `terminal` frame carries at most, and the text after which it
/// takes no more, so a client catching up gets its backlog in frames the
/// socket can interleave with the rest.
const FRAME_CHUNKS: usize = 256;
const FRAME_TEXT_BYTES: usize = 4 * 1024 * 1024;
/// The longest output chunk the hub draws, in bytes before encoding; with
/// a pane's ring and unsent output this bounds what one pane holds.
const MAX_CHUNK_BYTES: usize = 4 * 1024 * 1024;
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
    /// The chunk draws the whole screen.
    full: bool,
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
    /// Each pane's bytes in `queue` after its full frame, if one waits.
    unsent: HashMap<Arc<str>, usize>,
    /// Panes this client gets nothing of until their next full frame.
    awaiting_full: HashSet<Arc<str>>,
    /// Panes whose redraw this client asks the node for on its next turn.
    redraw: HashSet<Arc<str>>,
    /// The last sequence decided for this client, sent or withheld.
    examined: u64,
    /// The client's cursor is to be told even with no chunk to carry it: it
    /// started from a whole snapshot and has none it can resume from.
    announce: bool,
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

/// Where a client starts in the hub's output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resume {
    /// The client drew every pane again from a whole snapshot and asks each
    /// view's full frame itself; it is told its cursor at once.
    Fresh,
    /// The client kept its terminals and was sent everything up to this
    /// cursor.
    After(u64),
    /// The client kept its terminals but names no cursor: every pane is
    /// drawn again from a full frame.
    Redraw,
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

    /// Adds a client, starting where `resume` says.
    pub fn connect(self: &Arc<Self>, resume: Resume) -> HubClient {
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
            announce: resume == Resume::Fresh,
            wake: Arc::clone(&wake),
        };
        match resume {
            Resume::Fresh => {}
            Resume::After(cursor) => resume_from(&mut state, &mut client, cursor),
            Resume::Redraw => {
                for pane in state.panes.keys() {
                    await_full(&mut client, pane, "resume_without_cursor");
                }
            }
        }
        if client.announce || !client.queue.is_empty() || !client.redraw.is_empty() {
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
        // The same rule as live output: a full frame replaces what of its
        // pane waits and is never counted.
        let unsent = client.unsent.entry(Arc::clone(&chunk.pane)).or_default();
        if chunk.full {
            *unsent = 0;
            client.queue.retain(|queued| queued.pane != chunk.pane);
        } else {
            *unsent += chunk.size();
        }
        client.queue.push_back(chunk);
    }
}

/// A chunk too long to draw: no client gets it or anything more of its
/// pane until a full frame that fits. One that was not full asks for that
/// frame; a full one does not ask again.
fn drop_oversized(state: &mut HubState, pane: &str, bytes: usize, full: bool) {
    let pane: Arc<str> = state
        .panes
        .get_key_value(pane)
        .map_or_else(|| Arc::from(pane), |(key, _)| Arc::clone(key));
    let ring = state.panes.entry(Arc::clone(&pane)).or_default();
    ring.unrepaired_drop = true;
    if full {
        ring.redraw_asked = None;
    }
    for client in state.clients.values_mut() {
        client.queue.retain(|queued| queued.pane != pane);
        client.unsent.remove(&pane);
        client.awaiting_full.insert(Arc::clone(&pane));
        if !full && client.redraw.insert(Arc::clone(&pane)) {
            client.wake.notify_one();
        }
    }
    herdr_core::diagnostic!(json!({
        "component": "terminal_hub",
        "kind": "terminal.frame_too_long",
        "pane_id": pane.as_ref(),
        "bytes": bytes,
        "cap": MAX_CHUNK_BYTES,
        "full": full,
    }));
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
        if bytes.len() > MAX_CHUNK_BYTES {
            drop_oversized(&mut state, pane, bytes.len(), full);
            return;
        }
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
            full,
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
            if full {
                *unsent = 0;
                client.queue.retain(|queued| queued.pane != pane);
                client.queue.push_back(Arc::clone(&chunk));
                client.wake.notify_one();
                continue;
            }
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
    /// it asks for each view's full frame itself, and it is told its new
    /// cursor at once.
    pub fn restart(&self) {
        let mut state = lock(&self.hub.state);
        let sequence = state.sequence;
        if let Some(client) = state.clients.get_mut(&self.id) {
            client.queue.clear();
            client.unsent.clear();
            client.awaiting_full.clear();
            client.redraw.clear();
            client.examined = sequence;
            client.announce = true;
            client.wake.notify_one();
        }
    }

    /// The next frame to send and the panes whose node should draw them
    /// again. Draining stops at [`FRAME_CHUNKS`] or [`FRAME_TEXT_BYTES`],
    /// after at least one chunk; a client with more is woken again.
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
        if client.queue.is_empty() && !client.announce {
            return (None, redraws);
        }
        client.announce = false;
        let mut text = String::from(r#"{"type":"terminal","payload":{"chunks":["#);
        let mut last = 0;
        for index in 0..FRAME_CHUNKS {
            if index > 0 && text.len() >= FRAME_TEXT_BYTES {
                break;
            }
            let Some(chunk) = client.queue.pop_front() else {
                break;
            };
            if !chunk.full
                && let Some(unsent) = client.unsent.get_mut(&chunk.pane)
            {
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
        let first = hub.connect(Resume::Fresh);
        hub.output("w1:p1", b"a", true);
        hub.output("w1:p2", b"b", true);
        let (frame, _) = first.take();
        let (_, cursor) = frame_chunks(&frame.unwrap());
        assert_eq!(cursor, 2);
        drop(first);
        hub.output("w1:p1", b"c", false);
        hub.output("w1:p2", b"d", false);
        let resumed = hub.connect(Resume::After(cursor));
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
        let resumed = hub.connect(Resume::After(cursor));
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
        let stalled = hub.connect(Resume::Fresh);
        let live = hub.connect(Resume::Fresh);
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
        let client = hub.connect(Resume::Fresh);
        assert!(client.take().0.is_some(), "the fresh client's cursor");
        hub.output("w1:p1", b"a", true);
        hub.forget("w1:p1");
        assert!(client.take().0.is_none());
        assert!(hub.retained_bytes().is_empty());
    }

    /// A client that started from a whole snapshot learns its cursor before
    /// any output, so a reconnect right after resumes from it.
    #[test]
    fn a_fresh_client_is_told_its_cursor_at_once() {
        let hub = TerminalHub::new();
        hub.output("w1:p1", b"a", true);
        let client = hub.connect(Resume::Fresh);
        let (frame, redraws) = client.take();
        assert!(redraws.is_empty());
        assert_eq!(frame_chunks(&frame.unwrap()), (Vec::new(), 1));
        assert!(client.take().0.is_none());
        hub.output("w1:p1", b"b", false);
        client.restart();
        assert_eq!(frame_chunks(&client.take().0.unwrap()), (Vec::new(), 2));
    }

    /// A client that kept its terminals but names no cursor cannot be
    /// resumed: every pane is drawn again, nothing partial first.
    #[test]
    fn a_client_without_a_cursor_redraws_every_pane() {
        let hub = TerminalHub::new();
        hub.output("w1:p1", b"a", true);
        hub.output("w1:p2", b"b", true);
        let client = hub.connect(Resume::Redraw);
        let (frame, mut redraws) = client.take();
        redraws.sort();
        assert_eq!(redraws, ["w1:p1", "w1:p2"]);
        assert!(frame.is_none());
        hub.output("w1:p1", b"partial", false);
        hub.output("w1:p1", b"\x1bcwhole", true);
        let (chunks, _) = frame_chunks(&client.take().0.unwrap());
        assert_eq!(chunks, [("w1:p1".to_owned(), b"\x1bcwhole".to_vec())]);
    }

    /// A redraw that no full frame answered is asked again once the pane
    /// keeps drawing past the retry interval, and only then.
    #[test]
    fn an_unanswered_redraw_is_asked_again_while_the_pane_keeps_drawing() {
        let hub = TerminalHub::new();
        hub.output("w1:p1", b"a", true);
        let client = hub.connect(Resume::Redraw);
        assert_eq!(client.take().1, ["w1:p1"]);
        hub.output("w1:p1", b"more", false);
        assert!(client.take().1.is_empty(), "asked within the interval");
        lock(&hub.state)
            .panes
            .get_mut("w1:p1")
            .unwrap()
            .redraw_asked = Some(Instant::now() - REDRAW_RETRY);
        hub.output("w1:p1", b"more", false);
        assert_eq!(client.take().1, ["w1:p1"]);
        hub.output("w1:p1", b"\x1bcwhole", true);
        hub.output("w1:p1", b"next", false);
        let (frame, redraws) = client.take();
        assert!(redraws.is_empty());
        assert_eq!(frame_chunks(&frame.unwrap()).0.len(), 2);
    }

    /// A full frame larger than a client's unsent cap reaches the client
    /// whole when nothing of its pane waits, and no redraw is asked for it.
    #[test]
    fn a_full_frame_larger_than_the_cap_reaches_the_client_and_asks_no_redraw() {
        let hub = TerminalHub::new();
        let client = hub.connect(Resume::Fresh);
        assert!(client.take().0.is_some(), "the fresh client's cursor");
        let frame = [b"\x1bc".to_vec(), vec![b'w'; 2 * 1024 * 1024]].concat();
        hub.output("w1:p1", &frame, true);
        let (sent, redraws) = client.take();
        assert!(redraws.is_empty());
        assert_eq!(
            frame_chunks(&sent.unwrap()).0,
            [("w1:p1".to_owned(), frame.clone())]
        );
        hub.output("w1:p1", &frame, true);
        hub.output("w1:p1", b"next", false);
        let (sent, redraws) = client.take();
        assert!(redraws.is_empty(), "nothing was dropped");
        assert_eq!(frame_chunks(&sent.unwrap()).0.len(), 2);
    }

    #[test]
    fn a_chunk_past_the_hub_cap_is_not_drawn_and_its_pane_waits_for_a_frame_that_fits() {
        let hub = TerminalHub::new();
        let client = hub.connect(Resume::Fresh);
        assert!(client.take().0.is_some(), "the fresh client's cursor");
        let long = vec![b'w'; MAX_CHUNK_BYTES + 1];

        hub.output("w1:p1", &long, false);
        hub.output("w1:p1", b"after", false);
        hub.output("w1:p2", b"other", false);
        let (sent, redraws) = client.take();
        assert_eq!(redraws, ["w1:p1"], "a long chunk asks for a full frame");
        assert_eq!(
            frame_chunks(&sent.unwrap()).0,
            [("w1:p2".to_owned(), b"other".to_vec())],
            "nothing of the pane is drawn before it, the other pane goes on"
        );

        hub.output("w1:p1", &long, true);
        hub.output("w1:p1", b"after", false);
        let (sent, redraws) = client.take();
        assert!(redraws.is_empty(), "a long full frame is not asked again");
        assert!(sent.is_none_or(|frame| frame_chunks(&frame).0.is_empty()));

        hub.output("w1:p1", b"screen", true);
        let (sent, _) = client.take();
        assert_eq!(
            frame_chunks(&sent.unwrap()).0,
            [("w1:p1".to_owned(), b"screen".to_vec())]
        );
    }

    #[test]
    fn a_resumed_full_frame_counts_nothing_against_the_cap() {
        let hub = TerminalHub::new();
        let first = hub.connect(Resume::Fresh);
        let (frame, _) = first.take();
        let (_, cursor) = frame_chunks(&frame.unwrap());
        drop(first);
        let screen = vec![b's'; 1200 * 1024];
        hub.output("w1:p1", &screen, true);
        let resumed = hub.connect(Resume::After(cursor));
        let line = vec![b'l'; 600 * 1024];
        hub.output("w1:p1", &line, false);
        let (sent, redraws) = resumed.take();
        assert!(redraws.is_empty(), "the full frame was never unsent output");
        assert_eq!(
            frame_chunks(&sent.unwrap()).0,
            [("w1:p1".to_owned(), screen), ("w1:p1".to_owned(), line)]
        );
    }

    #[test]
    fn a_frame_stops_taking_chunks_past_its_text_budget() {
        let hub = TerminalHub::new();
        let client = hub.connect(Resume::Fresh);
        assert!(client.take().0.is_some(), "the fresh client's cursor");
        let screen = vec![b's'; 1600 * 1024];
        for pane in ["w1:p1", "w1:p2", "w1:p3"] {
            hub.output(pane, &screen, true);
        }
        let counts = std::iter::from_fn(|| client.take().0)
            .map(|frame| frame_chunks(&frame).0.len())
            .collect::<Vec<_>>();
        assert_eq!(counts, [2, 1]);
    }
}
