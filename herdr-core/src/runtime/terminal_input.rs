//! The creation requests keys are sent against before Herdr names their pane
//! (PRD instant-pane-topology D-11), as the core decides them.
//!
//! After the operator asks for a new tab or a split, the screen sends keys
//! against the request rather than a pane, straight to the node, which holds
//! them (PRD core-host-node-terminal D-15). The core keeps each request's
//! row, which the shell's mark follows, and tells the node what became of it:
//! opened, answered with a pane, or discarded. The only pane a request's keys
//! can reach is the one Herdr's answer for that request names.

use std::collections::VecDeque;

use hide_node_link::terminal::{INPUT_REQUEST_LIMIT, TerminalControl};

use crate::model::{InputRequestSnapshot, InputRequestState as WireState};

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
    /// Whether a session layout has carried the pane this request names. A
    /// pane that leaves the layouts after that is gone, and Herdr can give
    /// its id to another pane, so the request stops routing to it.
    laid_out: bool,
}

#[derive(Debug, Default)]
pub(super) struct InputRequests {
    requests: VecDeque<InputRequest>,
    /// What the node is told next, in order.
    controls: Vec<TerminalControl>,
}

impl InputRequests {
    /// Opens `request_id`. The oldest request leaves at the cap, and one
    /// still pending is discarded. An id already naming another creation is
    /// that client's newer request, so the old row leaves rather than keep
    /// pointing keys at the old pane.
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
            self.requests.remove(index);
        }
        self.make_room();
        self.requests.push_back(InputRequest {
            request_id: request_id.to_owned(),
            origin,
            state: InputRequestState::Pending,
            laid_out: false,
        });
        self.controls.push(TerminalControl::RequestOpen {
            request: request_id.to_owned(),
        });
    }

    fn make_room(&mut self) {
        if self.requests.len() >= INPUT_REQUEST_LIMIT
            && let Some(mut evicted) = self.requests.pop_front()
            && evicted.state == InputRequestState::Pending
        {
            self.discard_row(&mut evicted, "evicted");
        }
    }

    fn discard_row(&mut self, request: &mut InputRequest, reason: &str) {
        request.state = InputRequestState::Discarded;
        self.controls.push(TerminalControl::RequestDiscard {
            request: request.request_id.clone(),
            reason: reason.to_owned(),
        });
    }

    /// Herdr's answer for `origin` named `pane_id`: the request is ready and
    /// the node writes what it held to that pane. False when no pending
    /// request has this origin.
    pub(super) fn resolve(&mut self, origin: &InputOrigin, pane_id: &str) -> bool {
        let Some(request) = self.requests.iter_mut().find(|request| {
            &request.origin == origin && request.state == InputRequestState::Pending
        }) else {
            return false;
        };
        request.state = InputRequestState::Ready(pane_id.to_owned());
        let request = request.request_id.clone();
        self.controls.push(TerminalControl::RequestResolve {
            request,
            pane: pane_id.to_owned(),
        });
        true
    }

    /// The creation `origin` did not produce a pane the keys may follow.
    /// Returns whether a pending request changed.
    pub(super) fn discard(&mut self, origin: &InputOrigin, reason: &str) -> bool {
        let Some(index) = self.requests.iter().position(|request| {
            &request.origin == origin && request.state == InputRequestState::Pending
        }) else {
            return false;
        };
        let mut request = self.requests.remove(index).expect("found");
        self.discard_row(&mut request, reason);
        self.requests.insert(index, request);
        true
    }

    /// The node discarded what it held for `request_id` at its own cap: the
    /// row ends, so the shell's mark ends with it. Returns whether it changed.
    pub(super) fn discarded_by_node(&mut self, request_id: &str) -> bool {
        let Some(request) = self.requests.iter_mut().find(|request| {
            request.request_id == request_id && request.state == InputRequestState::Pending
        }) else {
            return false;
        };
        request.state = InputRequestState::Discarded;
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
        self.make_room();
        self.requests.push_back(InputRequest {
            request_id: request_id.to_owned(),
            origin: InputOrigin::Refused,
            state: InputRequestState::Discarded,
            laid_out: false,
        });
        self.controls.push(TerminalControl::RequestDiscard {
            request: request_id.to_owned(),
            reason: "refused".to_owned(),
        });
        true
    }

    /// Marks a request's pane laid out once Herdr's own layout carries it.
    /// Only Herdr's layout counts: a pane Hide draws ahead may expire
    /// before Herdr lays it out, and its keys must still reach it then.
    pub(super) fn mark_laid_out(&mut self, in_herdr_layout: impl Fn(&str) -> bool) {
        for request in &mut self.requests {
            if let InputRequestState::Ready(pane_id) = &request.state
                && !request.laid_out
                && in_herdr_layout(pane_id)
            {
                request.laid_out = true;
            }
        }
    }

    /// Discards a request whose pane is closing, or was laid out and no
    /// longer is, so its keys cannot reach a pane that later reuses the id.
    /// Returns whether a request changed.
    pub(super) fn follow_panes(
        &mut self,
        live: impl Fn(&str) -> bool,
        closing: impl Fn(&str) -> bool,
    ) -> bool {
        let gone = self
            .requests
            .iter()
            .enumerate()
            .filter(|(_, request)| match &request.state {
                InputRequestState::Ready(pane_id) => {
                    closing(pane_id) || (request.laid_out && !live(pane_id))
                }
                _ => false,
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for &index in &gone {
            let mut request = self.requests.remove(index).expect("found");
            self.discard_row(&mut request, "pane_gone");
            self.requests.insert(index, request);
        }
        !gone.is_empty()
    }

    /// Whether a creation's answer named `pane_id` and Herdr's layout has not
    /// carried it yet: the pane is not gone, it has not arrived.
    pub(super) fn awaits_layout(&self, pane_id: &str) -> bool {
        self.requests.iter().any(|request| {
            !request.laid_out
                && matches!(&request.state, InputRequestState::Ready(pane) if pane == pane_id)
        })
    }

    /// The controls the node has not been sent yet.
    pub(super) fn take_controls(&mut self) -> Vec<TerminalControl> {
        std::mem::take(&mut self.controls)
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

impl super::Runtime {
    /// Publishes the request rows and tells the node what changed.
    pub(super) fn sync_input_requests(&mut self) {
        self.snapshot.terminal.input_requests = self.input_requests.snapshot();
        for control in self.input_requests.take_controls() {
            self.terminals.control(control);
        }
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
        let changed = self.input_requests.snapshot() != before;
        self.sync_input_requests();
        changed
    }

    /// Herdr's answer for `origin` named `pane_id`: the node writes what was
    /// typed for the request to that pane, ahead of anything typed later and
    /// without moving the keyboard.
    pub(super) fn resolve_input_request(&mut self, origin: &InputOrigin, pane_id: &str) {
        if self.input_requests.resolve(origin, pane_id) {
            self.sync_input_requests();
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

    fn controls(requests: &mut InputRequests) -> Vec<TerminalControl> {
        requests.take_controls()
    }

    #[test]
    fn a_request_tells_the_node_when_it_opens_and_which_pane_answers_it() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::Split("op-1".to_owned()));
        assert!(requests.resolve(&InputOrigin::Split("op-1".to_owned()), "w1:p2"));
        assert_eq!(
            controls(&mut requests),
            [
                TerminalControl::RequestOpen {
                    request: "r1".into()
                },
                TerminalControl::RequestResolve {
                    request: "r1".into(),
                    pane: "w1:p2".into()
                },
            ]
        );
    }

    #[test]
    fn a_failed_creation_is_discarded_and_answers_nothing_later() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::TabCreate(7));
        assert!(requests.discard(&InputOrigin::TabCreate(7), "refused"));
        assert_eq!(requests.state_of("r1"), Some(InputRequestState::Discarded));
        assert!(!requests.resolve(&InputOrigin::TabCreate(7), "w1:p9"));
        assert_eq!(
            controls(&mut requests).last(),
            Some(&TerminalControl::RequestDiscard {
                request: "r1".into(),
                reason: "refused".into()
            })
        );
    }

    #[test]
    fn a_creation_refused_before_herdr_is_discarded_for_the_shell() {
        let mut requests = InputRequests::default();
        assert!(requests.refuse_unopened("r1"));
        assert!(!requests.refuse_unopened("r1"));
        assert_eq!(requests.state_of("r1"), Some(InputRequestState::Discarded));
        assert_eq!(controls(&mut requests).len(), 1);
    }

    #[test]
    fn an_id_a_client_cannot_name_opens_no_row() {
        let mut requests = InputRequests::default();
        let long = "r".repeat(REQUEST_ID_LIMIT + 1);
        requests.open(&long, InputOrigin::TabCreate(1));
        assert!(!requests.refuse_unopened("has space"));
        assert!(requests.snapshot().is_empty());
        assert!(controls(&mut requests).is_empty());
    }

    #[test]
    fn a_reused_id_follows_the_newer_creation() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::TabCreate(1));
        requests.resolve(&InputOrigin::TabCreate(1), "w1:p2");
        requests.open("r1", InputOrigin::Split("op-9".to_owned()));
        assert_eq!(
            requests.origin_of("r1"),
            Some(InputOrigin::Split("op-9".to_owned()))
        );
        assert_eq!(requests.state_of("r1"), Some(InputRequestState::Pending));
    }

    #[test]
    fn a_request_whose_pane_left_the_layouts_reaches_no_pane_that_reuses_its_id() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::TabCreate(1));
        requests.resolve(&InputOrigin::TabCreate(1), "w1:p2");
        // Not laid out yet: the pane has not arrived, it is not gone.
        assert!(!requests.follow_panes(|_| false, |_| false));
        assert!(requests.awaits_layout("w1:p2"));
        requests.mark_laid_out(|pane| pane == "w1:p2");
        assert!(requests.follow_panes(|_| false, |_| false));
        assert_eq!(requests.state_of("r1"), Some(InputRequestState::Discarded));
        assert_eq!(
            controls(&mut requests).last(),
            Some(&TerminalControl::RequestDiscard {
                request: "r1".into(),
                reason: "pane_gone".into()
            })
        );
    }

    #[test]
    fn a_request_whose_pane_is_closing_reaches_no_pane() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::TabCreate(1));
        requests.resolve(&InputOrigin::TabCreate(1), "w1:p2");
        assert!(requests.follow_panes(|_| true, |pane| pane == "w1:p2"));
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
        assert!(
            controls(&mut requests).contains(&TerminalControl::RequestDiscard {
                request: "r0".into(),
                reason: "evicted".into()
            })
        );
    }

    #[test]
    fn a_request_the_node_gave_up_on_ends_its_row() {
        let mut requests = InputRequests::default();
        requests.open("r1", InputOrigin::TabCreate(1));
        assert!(requests.discarded_by_node("r1"));
        assert_eq!(requests.state_of("r1"), Some(InputRequestState::Discarded));
        assert!(!requests.discarded_by_node("r1"));
    }
}
