//! What the window sees of a move and how it asks for one (PRD
//! core-host-node-move B2 to B5): the process's supervisor owns the move,
//! which outlives the core role that took the request, so its state is the
//! supervisor's `core_move` frame rather than the core's snapshot. Every
//! role this process mounts (the core, the move screen, the node) sends the
//! frame to its windows.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use super::journal::{MoveFailure, MoveStep};

/// What a window asks of the move.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum MoveRequest {
    /// Run the checks for moving the core to `device`; nothing changes.
    Check { device: String },
    /// Move the core to `device`, its checks run once more first.
    Start { device: String },
    /// From a node's window: run the checks for moving the core back to
    /// this machine; nothing changes.
    CheckBack,
    /// From a node's window: move the core back to this machine.
    Back,
}

/// The core's own machine asking its running core to stop for a move back
/// (`attach`, from `hided core-move release`): the core answers once it
/// has checked and recorded the stop, then stops.
#[derive(Debug)]
pub struct Release {
    pub intent: String,
    /// The node the core goes back to.
    pub target: String,
    pub reply: std::sync::mpsc::Sender<Result<(), String>>,
}

/// What the supervisor is asked.
#[derive(Debug)]
pub enum Asked {
    Window(MoveRequest),
    Release(Release),
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveState {
    /// No move asked for since this process started.
    #[default]
    Idle,
    Checking,
    /// Every check passed; the move can start.
    Ready,
    ChecksFailed,
    /// The core is stopping.
    Stopping,
    /// The copy is being made and sent.
    Copying,
    /// The other machine's core is starting.
    Starting,
    /// This machine's node is linking to the new core.
    Linking,
    /// The move committed; this window is a node of the new core.
    Done,
    /// A step failed and the move is being undone.
    RollingBack,
    /// The move was undone; this machine's core runs as before.
    RolledBack,
    /// The other machine cannot be reached at a step where the move can
    /// neither go on nor be undone without it: this machine waits for it.
    Waiting,
}

/// One check that did not pass, with what to do.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FailedCheck {
    pub check: CheckId,
    /// What the check found, for the line under it.
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckId {
    /// The device is a connected device this core dials.
    Connection,
    /// No other node is linked to this core.
    OtherNode,
    /// The device's state folder holds no brain state and no other move.
    TargetState,
    /// Both machines have a Herdr server.
    Herdr,
    /// The device answers as the machine this core knows.
    Identity,
    /// For a move back: this machine's state folder holds no brain state
    /// of its own.
    OwnState,
    /// For a move back: this machine's node is linked to the core.
    Link,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MoveView {
    pub state: MoveState,
    /// Toward the machine named by `device`, or back to this one.
    pub direction: Option<super::journal::Direction>,
    pub device: Option<String>,
    pub intent: Option<String>,
    pub sent: u64,
    pub total: u64,
    pub failed: Vec<FailedCheck>,
    pub step: Option<MoveStep>,
    pub cause: Option<MoveFailure>,
    /// The node the window becomes after the move, so the page names it in
    /// its address.
    pub node: Option<String>,
}

/// The supervisor's side of the move: one request waiting at most, and the
/// view every role sends.
pub struct MoveControl {
    view: watch::Sender<MoveView>,
    requests: mpsc::Sender<Asked>,
    receiver: Mutex<Option<mpsc::Receiver<Asked>>>,
}

/// Why a request was not taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestRefusal {
    /// A move or its checks are running.
    Busy,
    /// No supervisor takes moves in this process (a daemon started on its
    /// own seat).
    Unavailable,
}

impl RequestRefusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::Busy => "move_busy",
            Self::Unavailable => "move_unavailable",
        }
    }
}

impl Default for MoveControl {
    fn default() -> Self {
        let (requests, receiver) = mpsc::channel(1);
        Self {
            view: watch::Sender::new(MoveView::default()),
            requests,
            receiver: Mutex::new(Some(receiver)),
        }
    }
}

impl MoveControl {
    /// Takes `request` when nothing else waits; a move running refuses any.
    pub fn request(&self, request: MoveRequest) -> Result<(), RequestRefusal> {
        let busy = matches!(
            self.view.borrow().state,
            MoveState::Checking
                | MoveState::Stopping
                | MoveState::Copying
                | MoveState::Starting
                | MoveState::Linking
                | MoveState::RollingBack
                | MoveState::Waiting
        );
        if busy {
            return Err(RequestRefusal::Busy);
        }
        self.ask(Asked::Window(request))
    }

    /// Takes a window's `core_move` event; the refusal is its reason code.
    pub fn request_event(&self, event: &serde_json::Value) -> Result<(), &'static str> {
        let request: MoveRequest = serde_json::from_value(
            event
                .get("payload")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )
        .map_err(|_| "move_malformed")?;
        self.request(request).map_err(RequestRefusal::code)
    }

    /// Asks the supervisor to stop its core for a move back and waits for
    /// its answer, which comes before the core stops.
    pub fn release(
        &self,
        intent: &str,
        target: &str,
        within: std::time::Duration,
    ) -> Result<(), String> {
        let (reply, answer) = std::sync::mpsc::channel();
        self.ask(Asked::Release(Release {
            intent: intent.to_owned(),
            target: target.to_owned(),
            reply,
        }))
        .map_err(|refusal| refusal.code().to_owned())?;
        answer
            .recv_timeout(within)
            .map_err(|_| "the core did not answer the release".to_owned())?
    }

    fn ask(&self, asked: Asked) -> Result<(), RequestRefusal> {
        if self
            .receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
        {
            return Err(RequestRefusal::Unavailable);
        }
        self.requests.try_send(asked).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => RequestRefusal::Busy,
            mpsc::error::TrySendError::Closed(_) => RequestRefusal::Unavailable,
        })
    }

    /// The requests' receiver, once: the supervisor takes it at start.
    pub fn take_requests(&self) -> Option<mpsc::Receiver<Asked>> {
        self.receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    pub fn subscribe(&self) -> watch::Receiver<MoveView> {
        self.view.subscribe()
    }

    pub fn view(&self) -> MoveView {
        self.view.borrow().clone()
    }

    pub fn set(&self, view: MoveView) {
        self.view.send_if_modified(|current| {
            let changed = *current != view;
            *current = view;
            changed
        });
    }

    pub fn update(&self, change: impl FnOnce(&mut MoveView)) {
        self.view.send_if_modified(|current| {
            let before = current.clone();
            change(current);
            *current != before
        });
    }
}

/// The `core_move` frame of `view`.
pub fn frame(view: &MoveView) -> String {
    serde_json::json!({"type": "core_move", "payload": view}).to_string()
}

/// The frame a window hears when its `core_move` event is not taken.
pub fn refusal_frame(reason: &str) -> String {
    serde_json::json!({"type": "error", "payload": {"kind": "core_move", "reason": reason}, "message": reason})
        .to_string()
}
