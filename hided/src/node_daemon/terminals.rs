//! The node's terminals for its screens: this machine's panes from its
//! own terminals, every other pane through the one terminals relay the node
//! keeps to its core, with keys for those held while it is away.

use super::*;

/// Terminal lines for the core's panes waiting to go up the terminals
/// relay, in bytes; past it a key is refused to its screen.
const HELD_BYTES: usize = 64 * 1024;
/// How long one such line may wait; past it it is dropped and its screen
/// told.
const HELD_FOR: Duration = Duration::from_secs(3);
/// How long a terminals relay that failed waits before it opens again on
/// the same link.
const RELAY_RETRY: Duration = Duration::from_secs(1);

/// This machine's panes, as its hub names them: the core's names for them.
pub(super) struct OwnPanes {
    pub(super) hub: Arc<TerminalHub>,
    pub(super) prefix: String,
}

impl OutputSink for OwnPanes {
    fn output(&self, pane: &str, bytes: &[u8], full: bool) {
        self.hub
            .output(&format!("{}{pane}", self.prefix), bytes, full);
    }

    fn forget(&self, pane: &str) {
        self.hub.forget(&format!("{}{pane}", self.prefix));
    }
}

/// Where a screen's keys and redraws go: this machine's panes to
/// its own terminals for the link's life, every other pane up the
/// terminals relay.
pub struct ScreenTerminals {
    pub(super) own_prefix: String,
    live: watch::Receiver<Option<Arc<LiveLink>>>,
    held: Mutex<HeldLines>,
    /// Wakes the terminals relay's writer when a line is held.
    ready: Notify,
    /// The screen connection whose held key was dropped as too old.
    pub(super) dropped: tokio::sync::broadcast::Sender<u64>,
}

/// Lines for the core's panes not yet written up the terminals relay, in
/// the order the screens sent them.
#[derive(Default)]
struct HeldLines {
    lines: VecDeque<HeldLine>,
    bytes: usize,
}

struct HeldLine {
    text: String,
    connection: u64,
    at: Instant,
}

impl HeldLines {
    /// The next line still in time, telling `dropped` of each that is not.
    fn next(
        &mut self,
        now: Instant,
        dropped: &tokio::sync::broadcast::Sender<u64>,
    ) -> Option<String> {
        while let Some(line) = self.lines.pop_front() {
            self.bytes -= line.text.len();
            if now.duration_since(line.at) <= HELD_FOR {
                return Some(line.text);
            }
            expired(line.connection, dropped);
        }
        None
    }

    /// Drops every line older than [`HELD_FOR`], telling its screen.
    fn expire(&mut self, now: Instant, dropped: &tokio::sync::broadcast::Sender<u64>) {
        while self
            .lines
            .front()
            .is_some_and(|line| now.duration_since(line.at) > HELD_FOR)
        {
            if let Some(line) = self.lines.pop_front() {
                self.bytes -= line.text.len();
                expired(line.connection, dropped);
            }
        }
    }

    /// Drops every line: the link they were typed for ended.
    fn clear(&mut self, dropped: &tokio::sync::broadcast::Sender<u64>) {
        for line in self.lines.drain(..) {
            expired(line.connection, dropped);
        }
        self.bytes = 0;
    }
}

fn expired(connection: u64, dropped: &tokio::sync::broadcast::Sender<u64>) {
    herdr_core::diagnostic!(json!({
        "component": "node_daemon",
        "kind": "terminals.held_dropped",
        "connection": connection,
        "held_ms": HELD_FOR.as_millis() as u64,
    }));
    let _ = dropped.send(connection);
}

impl ScreenTerminals {
    pub(super) fn new(own_prefix: String, live: watch::Receiver<Option<Arc<LiveLink>>>) -> Self {
        Self {
            own_prefix,
            live,
            held: Mutex::new(HeldLines::default()),
            ready: Notify::new(),
            dropped: tokio::sync::broadcast::channel(64).0,
        }
    }

    fn own(&self) -> Option<Arc<LiveLink>> {
        self.live.borrow().clone()
    }

    /// Holds `line` for the terminals relay, behind every line held before
    /// it; refused past [`HELD_BYTES`]. One line is always held, however
    /// large, so a paste goes up as it would on the core's own screen.
    fn up(&self, line: TerminalDown, connection: u64) -> Result<(), String> {
        let text = serde_json::to_string(&TerminalLine { terminal: line })
            .map_err(|error| error.to_string())?;
        let mut held = lock(&self.held);
        if !held.lines.is_empty() && held.bytes + text.len() > HELD_BYTES {
            drop(held);
            herdr_core::diagnostic!(json!({
                "component": "node_daemon",
                "kind": "terminals.held_full",
                "connection": connection,
                "cap": HELD_BYTES,
            }));
            return Err(
                "The core's panes are not taking keys now; this key was not sent".to_owned(),
            );
        }
        held.bytes += text.len();
        held.lines.push_back(HeldLine {
            text,
            connection,
            at: Instant::now(),
        });
        drop(held);
        self.ready.notify_one();
        Ok(())
    }

    /// A key a screen typed: into this machine's pane, or held for the
    /// core's.
    pub(super) fn key(
        &self,
        connection: u64,
        target: KeyTarget,
        bytes: Vec<u8>,
        typed_at_unix_ms: u64,
    ) -> Result<(), String> {
        if let KeyTarget::Pane(pane) = &target
            && let Some(own) = pane.strip_prefix(&self.own_prefix)
        {
            if let Some(link) = self.own() {
                link.terminals
                    .key(KeyTarget::Pane(own.to_owned()), bytes, typed_at_unix_ms);
            }
            return Ok(());
        }
        self.up(
            TerminalDown::Key {
                target,
                data: encode_base64(&bytes),
                typed_at_unix_ms,
            },
            connection,
        )
    }

    /// A screen's view of `pane` needs it drawn whole.
    pub(super) fn redraw(&self, connection: u64, pane: &str) {
        if let Some(own) = pane.strip_prefix(&self.own_prefix) {
            if let Some(link) = self.own() {
                link.terminals.redraw(own);
            }
            return;
        }
        // A redraw refused is asked again by the screen's next view.
        let _ = self.up(
            TerminalDown::Redraw {
                pane: pane.to_owned(),
            },
            connection,
        );
    }
}

/// How often a screen tells the core it still types into one pane.
const INPUT_NOTICE_EVERY: Duration = Duration::from_secs(1);
/// The panes a screen's notices are remembered for; past it they start over.
const INPUT_NOTICE_PANES: usize = 64;

/// The panes one screen told its core it typed into, and when: the core
/// sizes a pane at the grid of the screen that last sent it input
/// (`pane_sizes`), and never sees the keys this daemon takes. A screen that
/// keeps typing says so once a second, so another screen's input in
/// between is overruled within that.
#[derive(Default)]
pub(super) struct InputNotices {
    told: HashMap<String, Instant>,
}

impl InputNotices {
    /// The notice to send for a key into `pane` at `now`, if one is due.
    pub(super) fn notice(&mut self, pane: &str, now: Instant) -> Option<String> {
        if self
            .told
            .get(pane)
            .is_some_and(|told| now.duration_since(*told) < INPUT_NOTICE_EVERY)
        {
            return None;
        }
        if self.told.len() >= INPUT_NOTICE_PANES && !self.told.contains_key(pane) {
            self.told.clear();
        }
        self.told.insert(pane.to_owned(), now);
        Some(
            json!({
                "schema_version": SCHEMA_VERSION,
                "kind": "terminal_input",
                "payload": {"pane_id": pane},
            })
            .to_string(),
        )
    }
}

/// A screen's key, taken here; every other event goes to the core. A key
/// into a pane answers that pane. A view goes to the core too, which decides
/// the grid every screen's view of a pane is drawn at (`pane_sizes`) and
/// sends it to the pane's node.
pub(super) fn take_terminal_key(
    state: &NodeState,
    connection: u64,
    event: &Value,
) -> Result<Option<String>, String> {
    let (target, bytes) = terminal_key(event)?;
    let typed = match &target {
        KeyTarget::Pane(pane) => Some(pane.clone()),
        KeyTarget::Request(_) => None,
    };
    state
        .terminals
        .key(connection, target, bytes, crate::server::unix_ms_now())?;
    Ok(typed)
}

/// Keeps one terminals relay to the core for each live link: the core's
/// panes' output into this daemon's hub, and screens' keys and
/// redraws for those panes up to the core.
pub(super) async fn keep_terminals_relay(
    mut live: watch::Receiver<Option<Arc<LiveLink>>>,
    hub: Arc<TerminalHub>,
    terminals: Arc<ScreenTerminals>,
) {
    loop {
        let link = live.borrow_and_update().clone();
        let Some(link) = link else {
            if live.changed().await.is_err() {
                return;
            }
            continue;
        };
        let ended = terminals_relay(&link, &mut live, &hub, &terminals).await;
        herdr_core::diagnostic!(json!({
            "component": "node_daemon",
            "kind": "terminals.relay_ended",
            "generation": link.generation,
            "reason": ended,
        }));
        let still = live
            .borrow()
            .as_ref()
            .is_some_and(|now| now.generation == link.generation);
        if still {
            tokio::time::sleep(RELAY_RETRY).await;
            lock(&terminals.held).expire(Instant::now(), &terminals.dropped);
        } else {
            // Lines typed for a link that ended are never delivered later.
            lock(&terminals.held).clear(&terminals.dropped);
        }
    }
}

async fn terminals_relay(
    link: &LiveLink,
    live: &mut watch::Receiver<Option<Arc<LiveLink>>>,
    hub: &TerminalHub,
    terminals: &Arc<ScreenTerminals>,
) -> String {
    let upstream = match open_relay(link, "terminals").await {
        Ok(upstream) => upstream,
        Err(message) => return format!("open_failed: {message}"),
    };
    let (mut sink, mut from_core) = upstream.split();
    // Written on a task of its own: a write the core is slow to take never
    // stops this relay reading the core's output, which would fill the SSH
    // channel and stall every other one on the connection.
    let mut writer = {
        let terminals = Arc::clone(terminals);
        tokio::spawn(async move {
            loop {
                let next = lock(&terminals.held).next(Instant::now(), &terminals.dropped);
                match next {
                    Some(line) => {
                        if sink
                            .send(tungstenite::Message::Text(line.into()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    None => terminals.ready.notified().await,
                }
            }
        })
    };
    let _abort = AbortOnDrop(writer.abort_handle());
    loop {
        tokio::select! {
            changed = live.changed() => {
                let same = changed.is_ok()
                    && live
                        .borrow()
                        .as_ref()
                        .is_some_and(|now| now.generation == link.generation);
                if !same {
                    return "link_ended".to_owned();
                }
            }
            _ = &mut writer => return "core_closed".to_owned(),
            from_core = from_core.next() => match from_core {
                Some(Ok(tungstenite::Message::Text(text))) => {
                    ingest(hub, &terminals.own_prefix, &text);
                }
                Some(Ok(tungstenite::Message::Close(_))) | None | Some(Err(_)) => {
                    return "core_closed".to_owned();
                }
                Some(Ok(_)) => {}
            },
        }
    }
}

/// Ends a task when its owner ends.
struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The core's panes' output, one terminal line each, into the hub.
fn ingest(hub: &TerminalHub, own_prefix: &str, text: &str) {
    for line in text.lines() {
        let output = match serde_json::from_str::<TerminalLine<TerminalUp>>(line) {
            Ok(TerminalLine {
                terminal: TerminalUp::Output(output),
            }) => output,
            // The pane is gone on the core: nothing of it is kept here.
            Ok(TerminalLine {
                terminal: TerminalUp::Forget { pane },
            }) => {
                if !pane.starts_with(own_prefix) {
                    hub.forget(&pane);
                }
                continue;
            }
            _ => continue,
        };
        // The core sends no pane of this machine; one it did is not drawn
        // over this machine's own.
        if output.pane.starts_with(own_prefix) || output.pane.len() > MAX_PANE_ID_BYTES {
            continue;
        }
        if let Ok(bytes) = decode_base64(&output.data) {
            hub.output(&output.pane, &bytes, output.full);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys for the core's panes typed while the terminals relay is away
    /// wait in order; past the byte bound the screen is refused at once,
    /// and a key that waited too long is dropped and its screen told (R5).
    #[test]
    fn keys_for_the_core_s_panes_wait_for_the_relay_in_order_and_none_is_lost_unsaid() {
        let terminals =
            ScreenTerminals::new("remote:screen:pane:".to_owned(), watch::channel(None).1);
        let mut dropped = terminals.dropped.subscribe();
        let key = |text: &str| KeyTarget::Pane(format!("core-pane-{text}"));
        terminals.key(1, key("a"), b"a".to_vec(), 1).unwrap();
        terminals.key(2, key("b"), b"b".to_vec(), 2).unwrap();
        let now = Instant::now();
        let mut held = lock(&terminals.held);
        let first = held.next(now, &terminals.dropped).expect("the first key");
        assert!(first.contains("core-pane-a"), "{first}");
        // The second waited too long: dropped, and its screen told.
        let late = now + HELD_FOR + Duration::from_millis(1);
        assert_eq!(held.next(late, &terminals.dropped), None);
        assert_eq!(dropped.try_recv().unwrap(), 2);
        drop(held);

        // A paste larger than the bound goes alone; behind it, a key that
        // would cross the bound is refused.
        let paste = vec![b'x'; HELD_BYTES * 2];
        terminals.key(3, key("c"), paste, 3).unwrap();
        let refused = terminals.key(3, key("d"), b"d".to_vec(), 4).unwrap_err();
        assert!(refused.contains("not sent"), "{refused}");
        // This machine's own panes never wait for the relay.
        terminals
            .key(
                3,
                KeyTarget::Pane("remote:screen:pane:w:p".to_owned()),
                b"x".to_vec(),
                5,
            )
            .expect("an own pane's key");
    }

    #[test]
    fn a_screen_tells_its_core_it_types_into_a_pane_once_a_second() {
        let mut told = InputNotices::default();
        let start = Instant::now();
        let notice = told.notice("p", start).expect("the first key is told");
        let notice: Value = serde_json::from_str(&notice).unwrap();
        assert_eq!(notice["kind"], "terminal_input");
        assert_eq!(notice["payload"]["pane_id"], "p");
        assert!(
            told.notice("p", start + Duration::from_millis(999))
                .is_none()
        );
        assert!(told.notice("q", start).is_some(), "each pane on its own");
        assert!(told.notice("p", start + INPUT_NOTICE_EVERY).is_some());
        // Past the cap the panes start over rather than grow.
        for pane in 0..INPUT_NOTICE_PANES * 2 {
            told.notice(&pane.to_string(), start);
        }
        assert!(told.told.len() <= INPUT_NOTICE_PANES);
    }
}
