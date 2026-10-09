//! The core's side of a linked node's screens (PRD core-host-node-remote-core
//! D-05, D-06, D-10, D-20): `/relay`, reached only through the node's own SSH
//! connection and only with the grant the core handed that node's link.
//!
//! A node's screen connects to the node's hided, which opens one relay per
//! screen (`mode=screen`) for the core traffic, and one per node
//! (`mode=terminals`) for the output of every pane that is not the node's
//! own. The node's own panes never come back over it: the node draws them
//! from its own terminals, and this core sees their output only while a
//! screen of its own looks at them.
//!
//! The terminals relay is fed by [`ScreenOutputs`], which hands every
//! pane's output to the hub and to each relay's [`RelayTap`]. A tap holds at
//! most [`TAP_CAP_BYTES`] for its node; past it the backlog is dropped, each
//! pane it held is drawn again from a full frame, and nothing of that pane
//! goes out before the full frame does, so the node's hub never holds a
//! broken stream (B17).

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use hide_node::ssh::RemoteHost;
use hide_node::terminal::OutputSink;
use hide_node_link::terminal::{
    TerminalDown, TerminalLine, TerminalNode, TerminalOutput, TerminalUp, decode_base64,
    device_pane_prefix, encode_base64,
};
use serde_json::json;

use crate::terminal_hub::TerminalHub;

/// The most output one node's terminals relay holds unsent.
pub const TAP_CAP_BYTES: usize = 4 * 1024 * 1024;
/// Output items one relay message carries at most.
const MESSAGE_ITEMS: usize = 256;
/// The text after which a relay message takes no more items.
const MESSAGE_BYTES: usize = 1024 * 1024;
/// How often a relay looks whether its node's link still stands.
const LINK_CHECK: Duration = Duration::from_secs(1);
/// The longest line a node's terminals relay may send: a paste goes up as
/// one key, as large as a screen of this machine may send one.
const MAX_DOWN_LINE: usize = 16 * 1024 * 1024;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Every pane's output on this machine, as the screens here and the
/// terminals relays of linked nodes read it.
pub struct ScreenOutputs {
    hub: Arc<TerminalHub>,
    /// Which screen each pane's size follows; a pane leaves it when it is
    /// forgotten here.
    pane_sizes: Arc<crate::pane_sizes::PaneSizes>,
    taps: Mutex<Vec<Arc<RelayTap>>>,
    /// How many taps there are, read without the lock: with none, output
    /// costs one load beyond the hub.
    tapped: AtomicUsize,
}

impl ScreenOutputs {
    pub fn new(hub: Arc<TerminalHub>) -> Arc<Self> {
        Arc::new(Self {
            hub,
            pane_sizes: Arc::default(),
            taps: Mutex::default(),
            tapped: AtomicUsize::new(0),
        })
    }

    /// Adds a node's tap, ending the one its node had: a node keeps one
    /// terminals relay, so a second replaces the first rather than double
    /// what the core sends it.
    pub fn pane_sizes(&self) -> Arc<crate::pane_sizes::PaneSizes> {
        Arc::clone(&self.pane_sizes)
    }

    fn add(&self, tap: Arc<RelayTap>) {
        let mut taps = lock(&self.taps);
        taps.retain(|held| {
            if held.node == tap.node {
                held.close();
                held.wake.notify_one();
                false
            } else {
                true
            }
        });
        taps.push(tap);
        self.tapped.store(taps.len(), Ordering::SeqCst);
    }

    fn remove(&self, tap: &Arc<RelayTap>) {
        let mut taps = lock(&self.taps);
        taps.retain(|held| !Arc::ptr_eq(held, tap));
        self.tapped.store(taps.len(), Ordering::SeqCst);
    }

    fn taps(&self) -> Vec<Arc<RelayTap>> {
        if self.tapped.load(Ordering::SeqCst) == 0 {
            return Vec::new();
        }
        lock(&self.taps).clone()
    }
}

impl OutputSink for ScreenOutputs {
    fn output(&self, pane: &str, bytes: &[u8], full: bool) {
        self.hub.output(pane, bytes, full);
        for tap in self.taps() {
            tap.output(pane, bytes, full);
        }
    }

    fn forget(&self, pane: &str) {
        self.hub.forget(pane);
        self.pane_sizes.forget(pane);
        for tap in self.taps() {
            tap.forget(pane);
        }
    }
}

enum TapItem {
    Output(TerminalOutput),
    /// The pane is gone; the node forgets what it kept of it.
    Forget(String),
}

impl TapItem {
    fn pane(&self) -> &str {
        match self {
            Self::Output(output) => &output.pane,
            Self::Forget(pane) => pane,
        }
    }

    /// What it holds of the tap's byte bound.
    fn size(&self) -> usize {
        match self {
            Self::Output(output) => output.data.len(),
            Self::Forget(pane) => pane.len(),
        }
    }
}

#[derive(Default)]
struct TapState {
    items: VecDeque<TapItem>,
    bytes: usize,
    /// Panes whose next item must be a full frame: the relay just started,
    /// or the pane's backlog was dropped.
    awaiting_full: HashSet<String>,
    /// Every pane awaits a full frame until it has sent one: the relay's
    /// node holds nothing of any pane yet.
    started_whole: HashSet<String>,
    /// Panes whose redraw this tap asked for and the relay has not sent on.
    redraws: Vec<String>,
    dropped: u64,
    closed: bool,
}

/// One linked node's view of this machine's pane output: every pane except
/// the node's own.
pub struct RelayTap {
    node: String,
    own_prefix: String,
    state: Mutex<TapState>,
    wake: tokio::sync::Notify,
}

impl RelayTap {
    fn new(node: &str) -> Arc<Self> {
        Arc::new(Self {
            node: node.to_owned(),
            own_prefix: device_pane_prefix(node),
            state: Mutex::default(),
            wake: tokio::sync::Notify::new(),
        })
    }

    fn output(&self, pane: &str, bytes: &[u8], full: bool) {
        if pane.starts_with(&self.own_prefix) {
            return;
        }
        // Encoded before the tap's lock, which every pane's output takes.
        let data = encode_base64(bytes);
        let mut state = lock(&self.state);
        if state.closed {
            return;
        }
        if !state.started_whole.contains(pane) {
            if !full {
                // The node holds nothing of the pane: ask a full frame once.
                if state.awaiting_full.insert(pane.to_owned()) {
                    state.redraws.push(pane.to_owned());
                    drop(state);
                    self.wake.notify_one();
                }
                return;
            }
            state.started_whole.insert(pane.to_owned());
        }
        if full {
            state.awaiting_full.remove(pane);
            // A full frame draws over whatever of the pane still waits.
            let mut freed = 0;
            state.items.retain(|item| match item {
                TapItem::Output(output) if output.pane == pane => {
                    freed += output.data.len();
                    false
                }
                _ => true,
            });
            state.bytes -= freed;
        } else if state.awaiting_full.contains(pane) {
            return;
        }
        let output = TerminalOutput {
            pane: pane.to_owned(),
            data,
            full,
        };
        let size = output.data.len();
        if state.bytes + size > TAP_CAP_BYTES && !full {
            self.overflow(&mut state);
            state.awaiting_full.insert(pane.to_owned());
            if !state.redraws.iter().any(|asked| asked == pane) {
                state.redraws.push(pane.to_owned());
            }
            drop(state);
            self.wake.notify_one();
            return;
        }
        state.bytes += size;
        state.items.push_back(TapItem::Output(output));
        drop(state);
        self.wake.notify_one();
    }

    /// The node fell behind: its backlog goes, and each pane in it is drawn
    /// again from a full frame.
    fn overflow(&self, state: &mut TapState) {
        let mut panes = HashSet::new();
        let mut forgotten = VecDeque::new();
        for item in state.items.drain(..) {
            match item {
                TapItem::Output(output) => {
                    panes.insert(output.pane);
                }
                // A pane's end is kept: the node must still forget it.
                forget @ TapItem::Forget(_) => forgotten.push_back(forget),
            }
        }
        state.bytes = forgotten.iter().map(TapItem::size).sum();
        state.items = forgotten;
        state.dropped += 1;
        herdr_core::diagnostic!(json!({
            "component": "node_relay",
            "kind": "relay.terminals_overflow",
            "node": self.node,
            "panes": panes.len(),
            "cap_bytes": TAP_CAP_BYTES,
            "overflows": state.dropped,
        }));
        for pane in panes {
            if state.awaiting_full.insert(pane.clone()) {
                state.redraws.push(pane);
            }
        }
    }

    fn forget(&self, pane: &str) {
        if pane.starts_with(&self.own_prefix) {
            return;
        }
        let mut state = lock(&self.state);
        if state.closed {
            return;
        }
        let mut freed = 0;
        state.items.retain(|item| {
            if item.pane() == pane {
                freed += item.size();
                false
            } else {
                true
            }
        });
        state.bytes -= freed;
        state.awaiting_full.remove(pane);
        state.redraws.retain(|asked| asked != pane);
        // A pane the node was sent is forgotten there too, so its hub keeps
        // no pane the core no longer has.
        if state.started_whole.remove(pane) {
            state.bytes += pane.len();
            state.items.push_back(TapItem::Forget(pane.to_owned()));
            drop(state);
            self.wake.notify_one();
        }
    }

    /// The next message for the node, and the panes to draw again.
    fn take(&self) -> (Option<String>, Vec<String>) {
        let mut state = lock(&self.state);
        let redraws = std::mem::take(&mut state.redraws);
        if state.items.is_empty() {
            return (None, redraws);
        }
        let mut taken = Vec::new();
        let mut size = 0;
        while taken.len() < MESSAGE_ITEMS && (taken.is_empty() || size < MESSAGE_BYTES) {
            let Some(item) = state.items.pop_front() else {
                break;
            };
            state.bytes -= item.size();
            size += item.size();
            taken.push(item);
        }
        let more = !state.items.is_empty();
        drop(state);
        if more {
            self.wake.notify_one();
        }
        // Serialized outside the tap's lock.
        let mut text = String::new();
        for item in taken {
            let terminal = match item {
                TapItem::Output(output) => TerminalUp::Output(output),
                TapItem::Forget(pane) => TerminalUp::Forget { pane },
            };
            if let Ok(encoded) = serde_json::to_string(&TerminalLine { terminal }) {
                text.push_str(&encoded);
                text.push('\n');
            }
        }
        (Some(text), redraws)
    }

    fn is_closed(&self) -> bool {
        lock(&self.state).closed
    }

    fn close(&self) {
        let mut state = lock(&self.state);
        state.closed = true;
        state.items.clear();
        state.bytes = 0;
    }
}

/// Answers one linked node's screens may wait on at once (D-20): file reads
/// and folder listings of a device's checkout, which run beside the screen
/// and answer later. Past it a new one is refused with `relay_busy`.
pub const MAX_RELAY_REQUESTS: usize = 64;

/// What one linked node's screens wait on this core for. Each answer that
/// runs beside a screen holds a slot until it has answered.
#[derive(Debug, Default)]
pub struct RelayRequests {
    waiting: AtomicUsize,
    refused: AtomicU64,
    /// The node's screens relayed now.
    screens: AtomicUsize,
}

/// The screens one linked node may relay at once: fewer than this core's
/// client cap, so a node's windows never leave the core's own machine
/// without a place for its own.
pub const MAX_RELAY_SCREENS: usize = crate::state_file::MAX_CLIENTS - 2;

/// One relayed screen's place among [`MAX_RELAY_SCREENS`].
#[derive(Debug)]
pub struct RelayScreenSlot(Arc<RelayRequests>);

impl Drop for RelayScreenSlot {
    fn drop(&mut self) {
        self.0.screens.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One waiting answer's place among [`MAX_RELAY_REQUESTS`], given back
/// however the answer ends.
#[derive(Debug)]
pub struct RelaySlot(Arc<RelayRequests>);

impl Drop for RelaySlot {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, Ordering::AcqRel);
    }
}

impl RelayRequests {
    /// A place for one more of `node`'s screens, or `None` at the cap,
    /// which is logged.
    pub fn take_screen(self: &Arc<Self>, node: &str) -> Option<RelayScreenSlot> {
        let taken = self
            .screens
            .try_update(Ordering::AcqRel, Ordering::Acquire, |screens| {
                (screens < MAX_RELAY_SCREENS).then_some(screens + 1)
            })
            .is_ok();
        if taken {
            return Some(RelayScreenSlot(Arc::clone(self)));
        }
        herdr_core::diagnostic!(json!({
            "component": "node_relay",
            "kind": "relay.screens_full",
            "node": node,
            "cap": MAX_RELAY_SCREENS,
        }));
        None
    }

    /// A slot for one more answer to `node`'s screens, or `None` at the
    /// cap, which is logged.
    pub fn take(self: &Arc<Self>, node: &str) -> Option<RelaySlot> {
        let taken = self
            .waiting
            .try_update(Ordering::AcqRel, Ordering::Acquire, |waiting| {
                (waiting < MAX_RELAY_REQUESTS).then_some(waiting + 1)
            })
            .is_ok();
        if taken {
            return Some(RelaySlot(Arc::clone(self)));
        }
        let refused = self.refused.fetch_add(1, Ordering::Relaxed) + 1;
        if refused.is_power_of_two() {
            herdr_core::diagnostic!(json!({
                "component": "node_relay",
                "kind": "relay.requests_full",
                "node": node,
                "cap": MAX_RELAY_REQUESTS,
                "refused": refused,
            }));
        }
        None
    }
}

/// Resolves when `link` has ended.
pub async fn link_ended(link: &RemoteHost) {
    let mut check = tokio::time::interval(LINK_CHECK);
    loop {
        check.tick().await;
        if link.closed_reason().is_some() {
            return;
        }
    }
}

/// Serves one linked node's terminals relay until the node closes it or its
/// link ends: every pane's output but the node's own goes down it, and the
/// node's keys and redraws for those panes come up it.
pub async fn serve_terminals(
    mut socket: WebSocket,
    outputs: Arc<ScreenOutputs>,
    terminals: Arc<dyn TerminalNode>,
    node: String,
    link: RemoteHost,
) {
    let tap = RelayTap::new(&node);
    outputs.add(Arc::clone(&tap));
    herdr_core::diagnostic!(json!({
        "component": "node_relay",
        "kind": "relay.terminals_opened",
        "node": node,
        "link": link.identity(),
    }));
    // Every pane this machine draws starts whole on the node.
    for pane in outputs.hub.panes() {
        if !pane.starts_with(&tap.own_prefix) {
            terminals.redraw(&pane);
        }
    }
    let own_prefix = tap.own_prefix.clone();
    let reason = loop {
        tokio::select! {
            () = tap.wake.notified() => {
                if tap.is_closed() {
                    break "replaced";
                }
                let (text, redraws) = tap.take();
                for pane in redraws {
                    terminals.redraw(&pane);
                }
                if let Some(text) = text
                    && socket.send(Message::Text(text.into())).await.is_err()
                {
                    break "node_closed";
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Err(reason) = take_down(&text, terminals.as_ref(), &own_prefix) {
                        herdr_core::diagnostic!(json!({
                            "component": "node_relay",
                            "kind": "relay.terminals_line_refused",
                            "node": node,
                            "reason": reason,
                        }));
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break "node_closed",
                Some(Ok(_)) => {}
            },
            () = link_ended(&link) => break "link_ended",
        }
    };
    tap.close();
    outputs.remove(&tap);
    let _ = socket.send(Message::Close(None)).await;
    herdr_core::diagnostic!(json!({
        "component": "node_relay",
        "kind": "relay.terminals_closed",
        "node": node,
        "reason": reason,
    }));
}

/// One line a node's screens sent for a pane of this machine: a key, a view
/// or a redraw. A control, and anything for the node's own panes, is not the
/// node's to send.
fn take_down(text: &str, terminals: &dyn TerminalNode, own_prefix: &str) -> Result<(), String> {
    if text.len() > MAX_DOWN_LINE {
        return Err("line_too_long".to_owned());
    }
    let line: TerminalLine<TerminalDown> =
        serde_json::from_str(text).map_err(|error| format!("{:?}", error.classify()))?;
    match line.terminal {
        TerminalDown::Key { target, data, .. } => {
            if let hide_node_link::terminal::KeyTarget::Pane(pane) = &target
                && pane.starts_with(own_prefix)
            {
                return Err("own_pane".to_owned());
            }
            let bytes = decode_base64(&data)?;
            // Stamped here, on the core's clock, as a key from a screen of
            // this machine is.
            terminals.key(target, bytes, crate::server::unix_ms_now());
        }
        // A node's screens send their views with their other events, so the
        // core decides the grid each is drawn at (`pane_sizes`).
        TerminalDown::View { .. } => return Err("view".to_owned()),
        TerminalDown::Redraw { pane } => {
            if pane.starts_with(own_prefix) {
                return Err("own_pane".to_owned());
            }
            terminals.redraw(&pane);
        }
        TerminalDown::Control { .. } => return Err("control".to_owned()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nodes_screens_wait_on_at_most_the_cap_of_answers() {
        let requests = Arc::new(RelayRequests::default());
        let held: Vec<RelaySlot> = (0..MAX_RELAY_REQUESTS)
            .map(|_| requests.take("node").expect("under the cap"))
            .collect();
        assert!(requests.take("node").is_none(), "the next one is refused");
        drop(held);
        assert!(
            requests.take("node").is_some(),
            "an answer gives its slot back"
        );
    }

    /// A node relays at most `MAX_RELAY_SCREENS` screens, fewer than the
    /// core's client cap, so the core's own windows keep a place.
    #[test]
    fn a_node_relays_fewer_screens_than_the_core_takes() {
        let requests = Arc::new(RelayRequests::default());
        let held: Vec<RelayScreenSlot> = (0..MAX_RELAY_SCREENS)
            .map(|_| requests.take_screen("node").expect("a place under the cap"))
            .collect();
        assert!(requests.take_screen("node").is_none());
        drop(held);
        assert!(requests.take_screen("node").is_some());
    }

    fn outputs(tap: &Arc<RelayTap>) -> Vec<TerminalOutput> {
        let (text, _) = tap.take();
        text.unwrap_or_default()
            .lines()
            .map(|line| {
                match serde_json::from_str::<TerminalLine<TerminalUp>>(line)
                    .unwrap()
                    .terminal
                {
                    TerminalUp::Output(output) => output,
                    other => panic!("{other:?}"),
                }
            })
            .collect()
    }

    #[test]
    fn a_tap_skips_the_nodes_own_panes_and_starts_each_pane_whole() {
        let tap = RelayTap::new("mac");
        tap.output("remote:mac:pane:w1:p1", b"own", true);
        tap.output("w1:p1", b"partial", false);
        assert!(outputs(&tap).is_empty());
        let (_, redraws) = tap.take();
        assert!(redraws.is_empty(), "the redraw was asked once already");
        tap.output("w1:p1", b"whole", true);
        tap.output("w1:p1", b"more", false);
        let sent = outputs(&tap);
        assert_eq!(
            sent.iter().map(|output| output.full).collect::<Vec<_>>(),
            [true, false]
        );
        assert!(sent.iter().all(|output| output.pane == "w1:p1"));
    }

    /// A pane the core forgets is forgotten by the node it was sent to, so
    /// the node's hub keeps only panes the core still has; a pane the node
    /// never drew needs no word (R6).
    #[test]
    fn a_pane_the_core_forgets_is_forgotten_on_the_node() {
        let tap = RelayTap::new("mac");
        tap.output("w1:p1", b"whole", true);
        assert_eq!(outputs(&tap).len(), 1);
        tap.output("w1:p1", b"more", false);
        tap.forget("w1:p1");
        tap.forget("w1:p2");
        let (text, _) = tap.take();
        let lines: Vec<TerminalUp> = text
            .unwrap_or_default()
            .lines()
            .map(|line| {
                serde_json::from_str::<TerminalLine<TerminalUp>>(line)
                    .unwrap()
                    .terminal
            })
            .collect();
        assert!(
            matches!(lines.as_slice(), [TerminalUp::Forget { pane }] if pane == "w1:p1"),
            "{lines:?}"
        );
        // The pane coming back starts whole again.
        tap.output("w1:p1", b"partial", false);
        assert!(outputs(&tap).is_empty());
    }

    #[test]
    fn a_tap_past_its_cap_drops_the_backlog_and_waits_for_full_frames() {
        let tap = RelayTap::new("mac");
        tap.output("w1:p1", b"whole", true);
        let chunk = vec![b'x'; 1024 * 1024];
        for _ in 0..4 {
            tap.output("w1:p1", &chunk, false);
        }
        let (text, redraws) = tap.take();
        assert!(text.is_none(), "the backlog was not dropped");
        assert_eq!(redraws, ["w1:p1"]);
        tap.output("w1:p1", b"partial", false);
        assert!(outputs(&tap).is_empty());
        tap.output("w1:p1", b"whole again", true);
        assert_eq!(outputs(&tap).len(), 1);
    }
}
