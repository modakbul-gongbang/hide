//! Keys typed before the pane that should get them can take them (PRD
//! instant-pane-topology D-11).
//!
//! Two holds, both capped at `INPUT_HOLD_LIMIT_BYTES` per holder:
//!
//! - A creation request. After the operator asks for a new tab or a split,
//!   the shell sends keys against the request rather than a pane. They wait
//!   here until Herdr's answer names the new pane and are then written to it
//!   in order; later keys for the request go straight to that pane. A refused
//!   or failed creation discards them. The only pane a request's keys can
//!   reach is the one Herdr's answer for that request names.
//! - An attaching pane. A pane whose terminal session is starting, waiting
//!   for its view's size or reconnecting keeps its input until the control
//!   session opens, instead of dropping it.
//!
//! Crossing a cap discards what was held, with a diagnostic naming the holder
//! and the byte count; nothing reaches the screen (design principle 13).

use std::collections::{HashMap, VecDeque};

use crate::model::{InputRequestSnapshot, InputRequestState as WireState};

pub(super) const INPUT_HOLD_LIMIT_BYTES: usize = 64 * 1024;
/// Creation requests remembered at once. The shell's mark ends long before
/// this many newer creations push one out.
pub(super) const INPUT_REQUEST_LIMIT: usize = 8;
/// The longest request id a client may name. The shell's ids are about 50
/// bytes; every row rides each snapshot to every client, so a longer one is
/// refused rather than copied there.
const REQUEST_ID_LIMIT: usize = 128;

/// Whether a client's request id may name a holder: 1 to
/// `REQUEST_ID_LIMIT` ASCII letters, digits, `-` and `_`.
fn valid_request_id(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= REQUEST_ID_LIMIT
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn report_invalid_request_id(request_id: &str) {
    crate::diagnostic!(serde_json::json!({
        "component": "terminal_input",
        "kind": "terminal.input_request_invalid",
        "length": request_id.len(),
    }));
}

/// What started a request, which is how its answer finds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum InputOrigin {
    /// A `tab.create` on the control lane, by admission id.
    TabCreate(u64),
    /// A split, by pane operation id.
    Split(String),
    /// A task-slot creation, by task operation id; its keys go to the pane
    /// once the task's agent start has resolved.
    Task(u64),
    /// Refused before it reached Herdr; nothing answers it.
    Refused,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum InputRequestState {
    Pending,
    Ready(String),
    Discarded,
}

#[derive(Debug)]
struct InputRequest {
    request_id: String,
    origin: InputOrigin,
    state: InputRequestState,
    /// The chunks as they were typed, so each is written as its own key.
    held: Vec<Vec<u8>>,
    held_bytes: usize,
    dropped_bytes: usize,
    /// Whether a session layout has carried the pane this request names. A
    /// pane that leaves the layouts after that is gone, and Herdr can give
    /// its id to another pane, so the request stops routing to it.
    laid_out: bool,
}

/// Where a key sent against a request goes.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum KeyRoute {
    /// Write it to this pane.
    Pane(String),
    /// Kept until the request resolves.
    Held,
    /// Not delivered; the diagnostic has been written.
    Dropped,
}

#[derive(Debug, Default)]
pub(super) struct InputRequests {
    requests: VecDeque<InputRequest>,
    /// Unknown request ids already reported, newest last, so keys sent
    /// against one write one diagnostic rather than one per key.
    unknown_reported: VecDeque<String>,
}

impl InputRequests {
    /// Starts holding keys for `request_id`. The oldest request leaves at the
    /// cap; one still pending discards what it held. An id already naming
    /// another creation is that client's newer request, so the old row
    /// leaves rather than keep pointing keys at the old pane.
    pub(super) fn open(&mut self, request_id: &str, origin: InputOrigin) {
        if !valid_request_id(request_id) {
            report_invalid_request_id(request_id);
            return;
        }
        if let Some(index) = self
            .requests
            .iter()
            .position(|request| request.request_id == request_id)
        {
            if self.requests[index].origin == origin {
                return;
            }
            if let Some(mut reused) = self.requests.remove(index) {
                discard(&mut reused, "reused");
            }
        }
        if self.requests.len() >= INPUT_REQUEST_LIMIT
            && let Some(mut evicted) = self.requests.pop_front()
            && evicted.state == InputRequestState::Pending
        {
            discard(&mut evicted, "evicted");
        }
        self.requests.push_back(InputRequest {
            request_id: request_id.to_owned(),
            origin,
            state: InputRequestState::Pending,
            held: Vec::new(),
            held_bytes: 0,
            dropped_bytes: 0,
            laid_out: false,
        });
    }

    /// Routes `bytes` sent against `request_id`. Returns the route and
    /// whether the request's published state changed.
    pub(super) fn route(&mut self, request_id: &str, bytes: &[u8]) -> (KeyRoute, bool) {
        if !valid_request_id(request_id) {
            report_invalid_request_id(request_id);
            return (KeyRoute::Dropped, false);
        }
        let Some(request) = self
            .requests
            .iter_mut()
            .find(|request| request.request_id == request_id)
        else {
            if !self.unknown_reported.iter().any(|id| id == request_id) {
                if self.unknown_reported.len() >= INPUT_REQUEST_LIMIT {
                    self.unknown_reported.pop_front();
                }
                self.unknown_reported.push_back(request_id.to_owned());
                crate::diagnostic!(serde_json::json!({
                    "component": "terminal_input",
                    "kind": "terminal.input_request_unknown",
                    "request_id": request_id,
                    "bytes": bytes.len(),
                }));
            }
            return (KeyRoute::Dropped, false);
        };
        match &request.state {
            InputRequestState::Ready(pane_id) => (KeyRoute::Pane(pane_id.clone()), false),
            InputRequestState::Discarded => {
                // One line when the first dropped key arrives; the count rides
                // on later lines only when the holder is discarded again.
                if request.dropped_bytes == 0 {
                    crate::diagnostic!(serde_json::json!({
                        "component": "terminal_input",
                        "kind": "terminal.input_after_discard",
                        "request_id": request.request_id,
                        "bytes": bytes.len(),
                    }));
                }
                request.dropped_bytes = request.dropped_bytes.saturating_add(bytes.len());
                (KeyRoute::Dropped, false)
            }
            InputRequestState::Pending => {
                if request.held_bytes + bytes.len() > INPUT_HOLD_LIMIT_BYTES {
                    request.dropped_bytes = bytes.len();
                    discard(request, "limit");
                    return (KeyRoute::Dropped, true);
                }
                request.held_bytes += bytes.len();
                request.held.push(bytes.to_vec());
                (KeyRoute::Held, false)
            }
        }
    }

    /// Herdr's answer for `origin` named `pane_id`: the request is ready and
    /// what it held is handed back to be written, in order, before anything
    /// else reaches the pane. None when no pending request has this origin.
    pub(super) fn resolve(&mut self, origin: &InputOrigin, pane_id: &str) -> Option<Vec<Vec<u8>>> {
        let request = self.requests.iter_mut().find(|request| {
            &request.origin == origin && request.state == InputRequestState::Pending
        })?;
        request.state = InputRequestState::Ready(pane_id.to_owned());
        request.held_bytes = 0;
        Some(std::mem::take(&mut request.held))
    }

    /// The creation `origin` did not produce a pane the keys may follow.
    /// Returns whether a pending request changed.
    pub(super) fn discard(&mut self, origin: &InputOrigin, reason: &str) -> bool {
        let Some(request) = self.requests.iter_mut().find(|request| {
            &request.origin == origin && request.state == InputRequestState::Pending
        }) else {
            return false;
        };
        discard(request, reason);
        true
    }

    /// A creation the core refused before it reached Herdr: its request is
    /// recorded as discarded, so the shell's mark ends and keys already sent
    /// against it are dropped rather than delivered anywhere. Returns whether
    /// a row was added.
    pub(super) fn refuse_unopened(&mut self, request_id: &str) -> bool {
        if !valid_request_id(request_id) {
            report_invalid_request_id(request_id);
            return false;
        }
        if self
            .requests
            .iter()
            .any(|request| request.request_id == request_id)
        {
            return false;
        }
        if self.requests.len() >= INPUT_REQUEST_LIMIT
            && let Some(mut evicted) = self.requests.pop_front()
            && evicted.state == InputRequestState::Pending
        {
            discard(&mut evicted, "evicted");
        }
        self.requests.push_back(InputRequest {
            request_id: request_id.to_owned(),
            origin: InputOrigin::Refused,
            state: InputRequestState::Discarded,
            held: Vec::new(),
            held_bytes: 0,
            dropped_bytes: 0,
            laid_out: false,
        });
        true
    }

    /// Follows the panes the session lays out: a request's pane counts as
    /// laid out once a layout carries it, and a request whose laid-out pane
    /// no longer is, or is closing, is discarded so its keys cannot reach a
    /// pane that later reuses the id. Returns whether a request changed.
    pub(super) fn follow_panes(
        &mut self,
        live: impl Fn(&str) -> bool,
        closing: impl Fn(&str) -> bool,
    ) -> bool {
        let mut changed = false;
        for request in &mut self.requests {
            let InputRequestState::Ready(pane_id) = &request.state else {
                continue;
            };
            let gone = closing(pane_id) || (request.laid_out && !live(pane_id));
            if gone {
                discard(request, "pane_gone");
                changed = true;
            } else if live(pane_id) {
                request.laid_out = true;
            }
        }
        changed
    }

    /// Whether a creation's answer named `pane_id`: Herdr made it for keys
    /// the operator may already be typing, before any layout carries it.
    pub(super) fn names(&self, pane_id: &str) -> bool {
        self.requests.iter().any(
            |request| matches!(&request.state, InputRequestState::Ready(pane) if pane == pane_id),
        )
    }

    #[cfg(test)]
    pub(super) fn origin_of(&self, request_id: &str) -> Option<InputOrigin> {
        self.requests
            .iter()
            .find(|request| request.request_id == request_id)
            .map(|request| request.origin.clone())
    }

    #[cfg(test)]
    pub(super) fn state_of(&self, request_id: &str) -> Option<InputRequestState> {
        self.requests
            .iter()
            .find(|request| request.request_id == request_id)
            .map(|request| request.state.clone())
    }

    pub(super) fn snapshot(&self) -> Vec<InputRequestSnapshot> {
        self.requests
            .iter()
            .map(|request| InputRequestSnapshot {
                request_id: request.request_id.clone(),
                state: match request.state {
                    InputRequestState::Pending => WireState::Pending,
                    InputRequestState::Ready(_) => WireState::Ready,
                    InputRequestState::Discarded => WireState::Discarded,
                },
            })
            .collect()
    }
}

fn discard(request: &mut InputRequest, reason: &str) {
    request.held.clear();
    let held = std::mem::take(&mut request.held_bytes);
    request.state = InputRequestState::Discarded;
    crate::diagnostic!(serde_json::json!({
        "component": "terminal_input",
        "kind": "terminal.input_discarded",
        "request_id": request.request_id,
        "reason": reason,
        "bytes": held + request.dropped_bytes,
    }));
}

/// What a pane whose control session is not open yet has been sent.
#[derive(Debug)]
enum PaneHeld {
    Holding(Vec<u8>),
    /// The cap was crossed: everything typed until the session opens is
    /// dropped too, so the pane never receives the tail of a lost paste.
    Overflowed,
}

/// Input for panes whose control session is not open yet, in arrival order.
#[derive(Debug, Default)]
pub(super) struct PaneInputHold {
    held: HashMap<String, PaneHeld>,
}

impl PaneInputHold {
    /// Keeps `bytes` for `pane_id`. Crossing the cap discards everything
    /// held for the pane together with `bytes`, and everything sent after it
    /// until the session opens; returns false when `bytes` was dropped.
    pub(super) fn hold(&mut self, pane_id: &str, bytes: &[u8]) -> bool {
        let entry = self
            .held
            .entry(pane_id.to_owned())
            .or_insert_with(|| PaneHeld::Holding(Vec::new()));
        let PaneHeld::Holding(held) = entry else {
            return false;
        };
        if held.len() + bytes.len() > INPUT_HOLD_LIMIT_BYTES {
            let dropped = held.len() + bytes.len();
            *entry = PaneHeld::Overflowed;
            crate::diagnostic!(serde_json::json!({
                "component": "terminal_input",
                "kind": "terminal.input_discarded",
                "pane_id": pane_id,
                "reason": "limit",
                "bytes": dropped,
            }));
            return false;
        }
        held.extend_from_slice(bytes);
        true
    }

    /// What the pane's open control session writes first.
    pub(super) fn take(&mut self, pane_id: &str) -> Option<Vec<u8>> {
        match self.held.remove(pane_id)? {
            PaneHeld::Holding(bytes) => Some(bytes).filter(|bytes| !bytes.is_empty()),
            PaneHeld::Overflowed => None,
        }
    }

    /// The pane will not get a control session for this input.
    pub(super) fn discard(&mut self, pane_id: &str, reason: &str) {
        if let Some(PaneHeld::Holding(bytes)) = self.held.remove(pane_id) {
            crate::diagnostic!(serde_json::json!({
                "component": "terminal_input",
                "kind": "terminal.input_discarded",
                "pane_id": pane_id,
                "reason": reason,
                "bytes": bytes.len(),
            }));
        }
    }

    /// Discards the input of every pane `keep` rejects.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        let gone = self
            .held
            .keys()
            .filter(|pane_id| !keep(pane_id))
            .cloned()
            .collect::<Vec<_>>();
        for pane_id in gone {
            self.discard(&pane_id, "pane_gone");
        }
    }

    pub(super) fn len(&self) -> usize {
        self.held.len()
    }
}

impl super::Runtime {
    pub(super) fn sync_input_requests(&mut self) {
        self.snapshot.terminal.input_requests = self.input_requests.snapshot();
    }

    /// After a creation event: a request the event opened stays as it is; a
    /// local task start opens one for its task; anything else was refused
    /// before reaching Herdr and is recorded as discarded.
    pub(super) fn settle_creation_request(&mut self, request_id: &str) -> bool {
        let task = self
            .snapshot
            .task_operation
            .as_ref()
            .filter(|operation| {
                operation.request_id.as_deref() == Some(request_id)
                    && operation.device_id.is_none()
                    && operation.phase == "working"
            })
            .map(|operation| operation.id);
        let before = self.input_requests.snapshot();
        match task {
            Some(id) => self.input_requests.open(request_id, InputOrigin::Task(id)),
            None => {
                self.input_requests.refuse_unopened(request_id);
            }
        }
        if self.input_requests.snapshot() == before {
            return false;
        }
        self.sync_input_requests();
        true
    }

    /// Herdr's answer for `origin` named `pane_id`: what was typed for the
    /// request goes to that pane now, ahead of anything typed later.
    /// Each chunk is written as the key it was, but without moving the
    /// keyboard: the operator may have gone to another pane since typing.
    pub(super) fn resolve_input_request(&mut self, origin: &InputOrigin, pane_id: &str) {
        let Some(held) = self.input_requests.resolve(origin, pane_id) else {
            return;
        };
        self.sync_input_requests();
        if held.is_empty() {
            return;
        }
        crate::diagnostic!(serde_json::json!({
            "component": "terminal_input",
            "kind": "terminal.input_delivered",
            "pane_id": pane_id,
            "chunks": held.len(),
            "bytes": held.iter().map(Vec::len).sum::<usize>(),
        }));
        for chunk in held {
            self.write_key(
                super::events::KeyPayload {
                    pane_id: pane_id.to_owned(),
                    bytes_base64: crate::live::encode_base64(&chunk),
                },
                false,
            );
        }
    }

    pub(super) fn discard_input_request(&mut self, origin: &InputOrigin, reason: &str) {
        if self.input_requests.discard(origin, reason) {
            self.sync_input_requests();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_wait_for_the_answer_then_follow_the_pane_it_names() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::Split("op-1".to_owned()));
        assert_eq!(requests.route("r1", b"ls"), (KeyRoute::Held, false));
        assert_eq!(requests.route("r1", b"\r"), (KeyRoute::Held, false));
        assert_eq!(
            requests.resolve(&InputOrigin::Split("op-1".to_owned()), "w1:p2"),
            Some(vec![b"ls".to_vec(), b"\r".to_vec()])
        );
        assert_eq!(
            requests.route("r1", b"x"),
            (KeyRoute::Pane("w1:p2".to_owned()), false)
        );
    }

    #[test]
    fn a_failed_creation_drops_held_and_later_keys() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::TabCreate(7));
        requests.route("r1", b"echo hi");
        assert!(requests.discard(&InputOrigin::TabCreate(7), "refused"));
        assert_eq!(requests.state_of("r1"), Some(InputRequestState::Discarded));
        assert_eq!(requests.route("r1", b"x"), (KeyRoute::Dropped, false));
        assert_eq!(requests.resolve(&InputOrigin::TabCreate(7), "w1:p9"), None);
    }

    #[test]
    fn a_creation_refused_before_herdr_is_discarded_for_the_shell() {
        let mut requests = InputRequests::default();
        assert!(requests.refuse_unopened("r1"));
        assert!(!requests.refuse_unopened("r1"));
        assert_eq!(requests.state_of("r1"), Some(InputRequestState::Discarded));
        assert_eq!(requests.route("r1", b"x"), (KeyRoute::Dropped, false));
    }

    #[test]
    fn an_id_a_client_cannot_name_opens_no_row_and_reaches_no_pane() {
        let mut requests = InputRequests::default();
        let long = "r".repeat(REQUEST_ID_LIMIT + 1);
        requests.open(&long, InputOrigin::TabCreate(1));
        assert!(!requests.refuse_unopened("has space"));
        assert!(requests.snapshot().is_empty());
        assert_eq!(requests.route(&long, b"x"), (KeyRoute::Dropped, false));
    }

    #[test]
    fn a_reused_id_follows_the_newer_creation() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::TabCreate(1));
        requests.resolve(&InputOrigin::TabCreate(1), "w1:p2");
        requests.open("r1", InputOrigin::Split("op-9".to_owned()));
        assert_eq!(requests.route("r1", b"x"), (KeyRoute::Held, false));
        assert_eq!(
            requests.resolve(&InputOrigin::Split("op-9".to_owned()), "w1:p3"),
            Some(vec![b"x".to_vec()])
        );
    }

    #[test]
    fn a_request_whose_pane_left_the_layouts_reaches_no_pane_that_reuses_its_id() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::Split("op".into()));
        requests.resolve(&InputOrigin::Split("op".into()), "w1:p2");
        // Answered before any layout carries the pane: still routed.
        assert!(!requests.follow_panes(|_| false, |_| false));
        assert_eq!(requests.route("r1", b"a").0, KeyRoute::Pane("w1:p2".into()));

        assert!(!requests.follow_panes(|pane| pane == "w1:p2", |_| false));
        assert!(requests.follow_panes(|_| false, |_| false));
        assert_eq!(requests.state_of("r1"), Some(InputRequestState::Discarded));
        // Herdr gives the id to another pane: the request does not follow it.
        requests.follow_panes(|pane| pane == "w1:p2", |_| false);
        assert_eq!(requests.route("r1", b"b").0, KeyRoute::Dropped);
    }

    #[test]
    fn a_request_whose_pane_is_closing_reaches_no_pane() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::Split("op".into()));
        requests.resolve(&InputOrigin::Split("op".into()), "w1:p2");
        assert!(requests.follow_panes(|_| true, |pane| pane == "w1:p2"));
        assert_eq!(requests.route("r1", b"a").0, KeyRoute::Dropped);
    }

    #[test]
    fn an_unknown_request_never_reaches_a_pane() {
        let mut requests = InputRequests::default();
        assert_eq!(
            requests.route("nobody", b"rm -rf"),
            (KeyRoute::Dropped, false)
        );
    }

    #[test]
    fn crossing_the_request_cap_discards_everything_held() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::TabCreate(1));
        let chunk = vec![b'a'; INPUT_HOLD_LIMIT_BYTES];
        assert_eq!(requests.route("r1", &chunk), (KeyRoute::Held, false));
        assert_eq!(requests.route("r1", b"b"), (KeyRoute::Dropped, true));
        assert_eq!(requests.state_of("r1"), Some(InputRequestState::Discarded));
    }

    #[test]
    fn requests_are_bounded_and_an_evicted_pending_one_is_discarded() {
        let mut requests = InputRequests::default();
        for index in 0..=INPUT_REQUEST_LIMIT {
            requests.open(&format!("r{index}"), InputOrigin::TabCreate(index as u64));
        }
        assert_eq!(requests.snapshot().len(), INPUT_REQUEST_LIMIT);
        assert_eq!(requests.state_of("r0"), None);
        assert_eq!(requests.route("r0", b"x"), (KeyRoute::Dropped, false));
    }

    #[test]
    fn an_attaching_pane_keeps_input_in_order_up_to_the_cap() {
        let mut hold = PaneInputHold::default();
        assert!(hold.hold("w1:p1", b"ab"));
        assert!(hold.hold("w1:p1", b"c"));
        assert_eq!(hold.take("w1:p1"), Some(b"abc".to_vec()));
        assert_eq!(hold.take("w1:p1"), None);
        assert!(hold.hold("w1:p1", &vec![b'a'; INPUT_HOLD_LIMIT_BYTES]));
        assert!(!hold.hold("w1:p1", b"z"));
        assert!(
            !hold.hold("w1:p1", b"tail"),
            "the rest of a lost paste must not reach the pane"
        );
        assert_eq!(hold.take("w1:p1"), None);
        assert!(hold.hold("w1:p1", b"next"), "an opened session starts over");
    }
}
