//! What a node does with a key before its pane's writer takes it: whether it
//! submits, holding it while its pane cannot take it yet, and the facts the
//! core hears about it (PRD core-host-node-terminal D-13, D-15).
//!
//! Two holds, both capped at [`INPUT_HOLD_LIMIT_BYTES`] per holder (PRD
//! instant-pane-topology D-11):
//!
//! - A creation request. After the operator asks for a new tab or a split,
//!   the screen sends keys against the request rather than a pane. They wait
//!   here until Herdr's answer names the new pane and are then written to it
//!   in order; a refused or failed creation discards them. A request the
//!   core has not told this node about yet is held as pending too, because
//!   the screen's keys reach the node on their own path and can arrive before
//!   the core's word about the creation.
//! - An attaching pane, whose control session is starting, waiting for its
//!   view's size or reconnecting.
//!
//! Crossing a cap discards what was held, with a diagnostic naming the
//! holder and the byte count, never the content. So does each key held
//! longer than [`HELD_INPUT_MAX_AGE`] when its pane could take it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use hide_node_link::terminal::{
    HELD_INPUT_MAX_AGE_MS, INPUT_HOLD_LIMIT_BYTES, INPUT_REQUEST_LIMIT,
};
use serde_json::json;

pub(super) const HELD_INPUT_MAX_AGE: Duration = Duration::from_millis(HELD_INPUT_MAX_AGE_MS);
/// A request the core never told this node about stops holding keys after
/// this long; its keys are older than the age limit by then anyway.
pub(super) const UNKNOWN_REQUEST_TTL: Duration = Duration::from_secs(10);
/// Requests the core has not named yet, held beside the ones it has.
const UNKNOWN_REQUEST_LIMIT: usize = INPUT_REQUEST_LIMIT;
/// The longest request id a screen may name.
const REQUEST_ID_LIMIT: usize = 128;
/// How long the core may go without hearing about further keys to a pane
/// it has already heard about (D-13). The first key after a gap this long,
/// an Enter and a key that moves the keyboard are reported at once.
pub(super) const INPUT_REPORT_WINDOW: Duration = Duration::from_secs(1);

/// The longest chunk looked at; anything longer is a paste.
const TYPED_BURST: usize = 64;

/// Whether keyboard bytes submit what was typed: a carriage return that is
/// not part of an escape sequence (`ESC CR` is a newline in Claude Code) and
/// not inside a bracketed paste. A chunk longer than any typed burst is a
/// paste and is not looked at. This is the one submit rule (PRD
/// overview-request-view D-19).
pub(super) fn submits(bytes: &[u8]) -> bool {
    const PASTE_START: &[u8] = b"\x1b[200~";
    const PASTE_END: &[u8] = b"\x1b[201~";
    if bytes.len() > TYPED_BURST {
        return false;
    }
    let mut pasting = false;
    let mut index = 0;
    while index < bytes.len() {
        let rest = &bytes[index..];
        if rest.starts_with(PASTE_START) {
            pasting = true;
            index += PASTE_START.len();
            continue;
        }
        if rest.starts_with(PASTE_END) {
            pasting = false;
            index += PASTE_END.len();
            continue;
        }
        if bytes[index] == b'\r' && !pasting && (index == 0 || bytes[index - 1] != 0x1b) {
            return true;
        }
        index += 1;
    }
    false
}

fn valid_request_id(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= REQUEST_ID_LIMIT
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// One key as it was typed, held until its pane can take it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HeldChunk {
    pub(super) typed_at: Instant,
    pub(super) typed_at_unix_ms: u64,
    pub(super) bytes: Vec<u8>,
}

/// The keys a pane can still be given at `now`, in order; those held longer
/// than [`HELD_INPUT_MAX_AGE`] are discarded with one diagnostic.
pub(super) fn fresh_chunks(pane_id: &str, chunks: Vec<HeldChunk>, now: Instant) -> Vec<HeldChunk> {
    let (fresh, stale): (Vec<_>, Vec<_>) = chunks
        .into_iter()
        .partition(|chunk| now.saturating_duration_since(chunk.typed_at) <= HELD_INPUT_MAX_AGE);
    let bytes = stale.iter().map(|chunk| chunk.bytes.len()).sum::<usize>();
    if bytes > 0 {
        crate::diagnostic!(json!({
            "component": "terminal_input",
            "kind": "terminal.input_discarded",
            "pane_id": pane_id,
            "reason": "stale",
            "bytes": bytes,
        }));
    }
    fresh
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RequestState {
    Pending,
    Ready(String),
    Discarded,
}

#[derive(Debug)]
struct Request {
    id: String,
    /// The core named it; an unnamed one is a request whose keys arrived
    /// first.
    known: bool,
    opened: Instant,
    state: RequestState,
    held: Vec<HeldChunk>,
    held_bytes: usize,
    dropped_bytes: usize,
}

impl Request {
    fn new(id: &str, known: bool, now: Instant) -> Self {
        Self {
            id: id.to_owned(),
            known,
            opened: now,
            state: RequestState::Pending,
            held: Vec::new(),
            held_bytes: 0,
            dropped_bytes: 0,
        }
    }

    fn discard(&mut self, reason: &str) {
        self.held.clear();
        let held = std::mem::take(&mut self.held_bytes);
        self.state = RequestState::Discarded;
        crate::diagnostic!(json!({
            "component": "terminal_input",
            "kind": "terminal.input_discarded",
            "request_id": self.id,
            "reason": reason,
            "bytes": held + self.dropped_bytes,
        }));
    }
}

/// Where a key sent against a request goes.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum KeyRoute {
    /// Write it to this pane.
    Pane(String),
    /// Kept until the request resolves.
    Held,
    /// Not delivered; the diagnostic has been written. `limit` is a request
    /// this key pushed over its cap, which the core hears about.
    Dropped { limit: bool },
}

#[derive(Debug, Default)]
pub(super) struct Requests {
    requests: VecDeque<Request>,
}

impl Requests {
    /// The core opened `request_id`; keys that arrived before stay held.
    pub(super) fn open(&mut self, request_id: &str, now: Instant) {
        if !valid_request_id(request_id) {
            return;
        }
        if let Some(request) = self.find(request_id) {
            if !request.known {
                request.known = true;
            } else {
                // A screen's newer creation reuses the id: what the older one
                // held must not reach the newer one's pane.
                if request.state == RequestState::Pending {
                    request.discard("reused");
                }
                *request = Request::new(request_id, true, now);
            }
            return;
        }
        self.make_room(true);
        self.requests.push_back(Request::new(request_id, true, now));
    }

    fn make_room(&mut self, known: bool) {
        let limit = if known {
            INPUT_REQUEST_LIMIT
        } else {
            UNKNOWN_REQUEST_LIMIT
        };
        let count = self
            .requests
            .iter()
            .filter(|request| request.known == known)
            .count();
        if count < limit {
            return;
        }
        if let Some(index) = self
            .requests
            .iter()
            .position(|request| request.known == known)
            && let Some(mut evicted) = self.requests.remove(index)
            && evicted.state == RequestState::Pending
        {
            evicted.discard("evicted");
        }
    }

    fn find(&mut self, request_id: &str) -> Option<&mut Request> {
        self.requests
            .iter_mut()
            .find(|request| request.id == request_id)
    }

    /// Routes `chunk`, sent against `request_id`.
    pub(super) fn route(
        &mut self,
        request_id: &str,
        chunk: HeldChunk,
    ) -> (KeyRoute, Option<HeldChunk>) {
        if !valid_request_id(request_id) {
            crate::diagnostic!(json!({
                "component": "terminal_input",
                "kind": "terminal.input_request_invalid",
                "length": request_id.len(),
            }));
            return (KeyRoute::Dropped { limit: false }, None);
        }
        self.expire_unknown(chunk.typed_at);
        if self.find(request_id).is_none() {
            self.make_room(false);
            self.requests
                .push_back(Request::new(request_id, false, chunk.typed_at));
        }
        let request = self.find(request_id).expect("just made");
        match &request.state {
            RequestState::Ready(pane_id) => (KeyRoute::Pane(pane_id.clone()), Some(chunk)),
            RequestState::Discarded => {
                if request.dropped_bytes == 0 {
                    crate::diagnostic!(json!({
                        "component": "terminal_input",
                        "kind": "terminal.input_after_discard",
                        "request_id": request.id,
                        "bytes": chunk.bytes.len(),
                    }));
                }
                request.dropped_bytes = request.dropped_bytes.saturating_add(chunk.bytes.len());
                (KeyRoute::Dropped { limit: false }, None)
            }
            RequestState::Pending => {
                if request.held_bytes + chunk.bytes.len() > INPUT_HOLD_LIMIT_BYTES {
                    request.dropped_bytes = chunk.bytes.len();
                    request.discard("limit");
                    return (KeyRoute::Dropped { limit: true }, None);
                }
                request.held_bytes += chunk.bytes.len();
                request.held.push(chunk);
                (KeyRoute::Held, None)
            }
        }
    }

    /// Herdr's answer named `pane_id`: what was held is handed back, in
    /// order, to be written before anything typed later.
    pub(super) fn resolve(
        &mut self,
        request_id: &str,
        pane_id: &str,
        now: Instant,
    ) -> Vec<HeldChunk> {
        if !valid_request_id(request_id) {
            return Vec::new();
        }
        if self.find(request_id).is_none() {
            self.make_room(true);
            self.requests.push_back(Request::new(request_id, true, now));
        }
        let request = self.find(request_id).expect("just made");
        request.known = true;
        if request.state != RequestState::Pending {
            return Vec::new();
        }
        request.state = RequestState::Ready(pane_id.to_owned());
        request.held_bytes = 0;
        std::mem::take(&mut request.held)
    }

    /// The creation made no pane the keys may follow.
    pub(super) fn discard(&mut self, request_id: &str, reason: &str, now: Instant) {
        if !valid_request_id(request_id) {
            return;
        }
        if self.find(request_id).is_none() {
            self.make_room(true);
            self.requests.push_back(Request::new(request_id, true, now));
        }
        let request = self.find(request_id).expect("just made");
        request.known = true;
        if request.state != RequestState::Discarded {
            request.discard(reason);
        }
    }

    /// Drops requests the core never named, once their keys are too old to
    /// be written anywhere.
    fn expire_unknown(&mut self, now: Instant) {
        self.requests.retain_mut(|request| {
            let expired = !request.known
                && now.saturating_duration_since(request.opened) > UNKNOWN_REQUEST_TTL;
            if expired && request.state == RequestState::Pending {
                request.discard("unknown");
            }
            !expired
        });
    }

    /// Whether a creation's answer named `pane_id`.
    pub(super) fn names(&self, pane_id: &str) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(&request.state, RequestState::Ready(pane) if pane == pane_id))
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.requests.len()
    }
}

/// What a pane whose control session is not open yet has been sent.
#[derive(Debug, Default)]
pub(super) enum PaneHold {
    #[default]
    Empty,
    Holding {
        chunks: Vec<HeldChunk>,
        bytes: usize,
    },
    /// The cap was crossed: everything typed until the session opens is
    /// dropped too, so the pane never receives the tail of a lost paste.
    Overflowed,
}

impl PaneHold {
    /// Keeps `chunk` for `pane_id`, first discarding held keys already too
    /// old to be written, so they never push fresh ones over the cap.
    pub(super) fn hold(&mut self, pane_id: &str, chunk: HeldChunk) -> bool {
        if matches!(self, Self::Empty) {
            *self = Self::Holding {
                chunks: Vec::new(),
                bytes: 0,
            };
        }
        let Self::Holding { chunks, bytes } = self else {
            return false;
        };
        if chunks.first().is_some_and(|first| {
            chunk.typed_at.saturating_duration_since(first.typed_at) > HELD_INPUT_MAX_AGE
        }) {
            *chunks = fresh_chunks(pane_id, std::mem::take(chunks), chunk.typed_at);
            *bytes = chunks.iter().map(|chunk| chunk.bytes.len()).sum();
        }
        if *bytes + chunk.bytes.len() > INPUT_HOLD_LIMIT_BYTES {
            let dropped = *bytes + chunk.bytes.len();
            *self = Self::Overflowed;
            crate::diagnostic!(json!({
                "component": "terminal_input",
                "kind": "terminal.input_discarded",
                "pane_id": pane_id,
                "reason": "limit",
                "bytes": dropped,
            }));
            return false;
        }
        *bytes += chunk.bytes.len();
        chunks.push(chunk);
        true
    }

    /// What the pane's control session, open at `now`, writes first.
    pub(super) fn take(&mut self, pane_id: &str, now: Instant) -> Vec<u8> {
        match std::mem::take(self) {
            Self::Holding { chunks, .. } => fresh_chunks(pane_id, chunks, now)
                .into_iter()
                .flat_map(|chunk| chunk.bytes)
                .collect(),
            Self::Empty | Self::Overflowed => Vec::new(),
        }
    }

    /// The pane will not get a control session for this input.
    pub(super) fn discard(&mut self, pane_id: &str, reason: &str) {
        if let Self::Holding { bytes, .. } = std::mem::take(self) {
            crate::diagnostic!(json!({
                "component": "terminal_input",
                "kind": "terminal.input_discarded",
                "pane_id": pane_id,
                "reason": reason,
                "bytes": bytes,
            }));
        }
    }
}

/// The input facts the core has heard about one pane, and what it has not
/// heard yet (D-13).
#[derive(Debug, Default)]
pub(super) struct InputFacts {
    last_report: Option<Instant>,
    /// When the last key the core has not heard about was typed, and
    /// whether any of them was typed on the screen.
    pending: Option<(u64, bool)>,
}

/// A report the core should get now.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct InputFact {
    pub(super) at_unix_ms: u64,
    pub(super) submitted: bool,
    pub(super) focus: bool,
}

impl InputFacts {
    /// Records a key typed at `at_unix_ms`. The first key after a gap of
    /// [`INPUT_REPORT_WINDOW`], an Enter and a key that moves the keyboard
    /// to another pane (`focus`) are reported at once; the rest wait for
    /// [`InputFacts::due`].
    pub(super) fn key(
        &mut self,
        now: Instant,
        at_unix_ms: u64,
        submitted: bool,
        focus: bool,
    ) -> Option<InputFact> {
        let quiet = self
            .last_report
            .is_none_or(|last| now.saturating_duration_since(last) >= INPUT_REPORT_WINDOW);
        if quiet || submitted || focus {
            self.last_report = Some(now);
            self.pending = None;
            return Some(InputFact {
                at_unix_ms,
                submitted,
                focus,
            });
        }
        self.pending = Some(match self.pending {
            Some((held, held_focus)) => (held.max(at_unix_ms), held_focus || focus),
            None => (at_unix_ms, focus),
        });
        None
    }

    /// When the keys the core has not heard about are reported.
    pub(super) fn deadline(&self) -> Option<Instant> {
        self.pending?;
        self.last_report.map(|last| last + INPUT_REPORT_WINDOW)
    }

    /// The report of the keys held back, once their window has passed.
    pub(super) fn due(&mut self, now: Instant) -> Option<InputFact> {
        let deadline = self.deadline()?;
        if now < deadline {
            return None;
        }
        let (at_unix_ms, focus) = self.pending.take()?;
        self.last_report = Some(now);
        Some(InputFact {
            at_unix_ms,
            submitted: false,
            focus,
        })
    }
}

/// Keys typed while a paste is prepared, written behind it (`runtime/
/// attachments.rs` in the core owns the paste).
#[derive(Debug)]
pub(super) struct AttachmentHold {
    pub(super) intent: String,
    pub(super) queued: Vec<u8>,
    pub(super) cancelling: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(bytes: &[u8], typed_at: Instant) -> HeldChunk {
        HeldChunk {
            typed_at,
            typed_at_unix_ms: 0,
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn only_a_typed_return_is_a_submit() {
        assert!(submits(b"\r"));
        assert!(submits(b"yes\r"));
        assert!(!submits(b"\x1b\r"), "a newline in Claude Code");
        assert!(!submits(b"\x1b[200~line one\rline two\x1b[201~"));
        assert!(submits(b"\x1b[200~pasted\x1b[201~\r"));
        assert!(!submits(b"abc"));
        assert!(!submits(format!("{}\r", "a".repeat(80)).as_bytes()));
        assert!(!submits(&[b'\r'; 200]));
    }

    #[test]
    fn keys_wait_for_the_answer_then_follow_the_pane_it_names() {
        let now = Instant::now();
        let mut requests = Requests::default();
        requests.open("r1", now);
        assert_eq!(requests.route("r1", chunk(b"ls", now)).0, KeyRoute::Held);
        assert_eq!(requests.route("r1", chunk(b"\r", now)).0, KeyRoute::Held);
        let held = requests.resolve("r1", "w1:p2", now);
        assert_eq!(
            held.iter().map(|c| c.bytes.clone()).collect::<Vec<_>>(),
            vec![b"ls".to_vec(), b"\r".to_vec()]
        );
        assert_eq!(
            requests.route("r1", chunk(b"x", now)).0,
            KeyRoute::Pane("w1:p2".into())
        );
        assert!(requests.names("w1:p2"));
    }

    #[test]
    fn keys_that_arrive_before_the_core_names_their_request_are_kept_for_it() {
        let now = Instant::now();
        let mut requests = Requests::default();
        assert_eq!(requests.route("early", chunk(b"a", now)).0, KeyRoute::Held);
        requests.open("early", now);
        let held = requests.resolve("early", "w1:p3", now);
        assert_eq!(held.len(), 1);
    }

    #[test]
    fn a_refused_creation_drops_held_and_later_keys() {
        let now = Instant::now();
        let mut requests = Requests::default();
        requests.route("r2", chunk(b"a", now));
        requests.discard("r2", "refused", now);
        assert_eq!(
            requests.route("r2", chunk(b"b", now)).0,
            KeyRoute::Dropped { limit: false }
        );
        assert!(requests.resolve("r2", "w1:p9", now).is_empty());
    }

    #[test]
    fn crossing_a_request_cap_discards_everything_held_and_says_so() {
        let now = Instant::now();
        let mut requests = Requests::default();
        requests.open("big", now);
        let half = vec![b'a'; INPUT_HOLD_LIMIT_BYTES / 2 + 1];
        assert_eq!(requests.route("big", chunk(&half, now)).0, KeyRoute::Held);
        assert_eq!(
            requests.route("big", chunk(&half, now)).0,
            KeyRoute::Dropped { limit: true }
        );
        assert!(requests.resolve("big", "w1:p1", now).is_empty());
    }

    #[test]
    fn requests_are_bounded_and_an_evicted_pending_one_is_discarded() {
        let now = Instant::now();
        let mut requests = Requests::default();
        for index in 0..INPUT_REQUEST_LIMIT + 2 {
            requests.open(&format!("r{index}"), now);
        }
        assert_eq!(requests.len(), INPUT_REQUEST_LIMIT);
        assert!(requests.resolve("r0", "w1:p1", now).is_empty());
    }

    #[test]
    fn an_unnamed_request_stops_holding_after_its_lifetime() {
        let now = Instant::now();
        let mut requests = Requests::default();
        requests.route("ghost", chunk(b"a", now));
        let later = now + UNKNOWN_REQUEST_TTL + Duration::from_millis(1);
        requests.route("other", chunk(b"b", later));
        assert_eq!(requests.len(), 1, "the unnamed request left");
    }

    #[test]
    fn an_attaching_pane_keeps_keys_in_order_up_to_the_cap() {
        let now = Instant::now();
        let mut hold = PaneHold::default();
        assert!(hold.hold("p", chunk(b"one", now)));
        assert!(hold.hold("p", chunk(b"two", now)));
        assert_eq!(hold.take("p", now), b"onetwo".to_vec());
        let big = vec![b'x'; INPUT_HOLD_LIMIT_BYTES];
        assert!(hold.hold("p", chunk(&big, now)));
        assert!(!hold.hold("p", chunk(b"y", now)));
        assert!(
            !hold.hold("p", chunk(b"z", now)),
            "the tail of a lost paste is dropped too"
        );
        assert!(hold.take("p", now).is_empty());
    }

    #[test]
    fn an_attaching_pane_gets_keys_held_up_to_the_age_limit_and_not_older_ones() {
        let now = Instant::now();
        let mut hold = PaneHold::default();
        hold.hold("p", chunk(b"old", now));
        hold.hold("p", chunk(b"new", now + Duration::from_secs(2)));
        assert_eq!(
            hold.take("p", now + Duration::from_millis(3_500)),
            b"new".to_vec()
        );
    }

    #[test]
    fn the_first_key_after_a_quiet_gap_and_every_enter_are_reported_at_once() {
        let start = Instant::now();
        let mut facts = InputFacts::default();
        let first = facts.key(start, 1_000, false, false).expect("first key");
        assert_eq!(first.at_unix_ms, 1_000);
        // Inside the window: held back, carrying the last key's own time.
        assert!(
            facts
                .key(start + Duration::from_millis(100), 1_100, false, false)
                .is_none()
        );
        assert!(
            facts
                .key(start + Duration::from_millis(200), 1_200, false, false)
                .is_none()
        );
        assert_eq!(facts.deadline(), Some(start + INPUT_REPORT_WINDOW));
        assert!(facts.due(start + Duration::from_millis(900)).is_none());
        let trailing = facts
            .due(start + INPUT_REPORT_WINDOW)
            .expect("trailing report");
        assert_eq!(
            trailing.at_unix_ms, 1_200,
            "the last key's time, not the report's"
        );
        // An Enter never waits for the window.
        let enter = facts
            .key(
                start + INPUT_REPORT_WINDOW + Duration::from_millis(10),
                2_010,
                true,
                false,
            )
            .expect("enter at once");
        assert!(enter.submitted);
        // A key after a quiet gap is reported at once again.
        assert!(
            facts
                .key(start + Duration::from_secs(5), 5_000, false, false)
                .is_some()
        );
    }
}
