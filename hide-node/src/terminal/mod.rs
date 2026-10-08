//! A node's terminal service (PRD core-host-node-terminal): its panes'
//! attaches, the bytes that flow through them and the rules they keep.
//!
//! The core decides which panes attach and says so with
//! [`TerminalControl`]; this service attaches, holds, writes and reads, and
//! tells the core what changed with [`TerminalReport`]. A key from the screen
//! reaches a pane's writer through [`TerminalNode::key`] and a frame reaches
//! the screen through the [`OutputSink`], neither passing the core.
//!
//! The rules are the ones the core ran before this layer (D-12): an attach
//! waits for a size, a pane another client controls is observed once, a
//! frame at a grid the view is not drawn at is held until a full frame at
//! the view's grid, an observer whose view changed size attaches again, a
//! failed control attach is retried four times, and keys typed before a pane
//! can take them are held within their caps.
//!
//! One lock guards the service. A key, a frame and a control each take it
//! for a few map lookups; nothing under it waits on a process, a socket or
//! the core. Reports leave after it is released; output is handed to the
//! sink under it, so a pane's bytes keep their order, and the sink must
//! return at once and never call back.

pub mod device;
mod input;
pub mod protocol;
pub mod router;
pub mod session;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hide_node_link::terminal::{
    AttachmentInputOutcome, GridSize, KeyTarget, MAX_ATTACHED_PANES, MAX_ATTACHMENT_INPUT_BYTES,
    PaneTerminalState, TerminalControl, TerminalNode, TerminalReport,
};
use serde_json::json;

use self::input::{AttachmentHold, HeldChunk, InputFacts, KeyRoute, PaneHold, Requests};
pub use self::protocol::Mode;
use self::protocol::SessionEvent;
use self::session::Session;
pub use self::session::{Attacher, Cleanup, LocalAttacher, SessionParts};

/// Where a node's frames go: the screen-side hub on this machine, or the
/// link up from a device.
pub trait OutputSink: Send + Sync {
    /// One pane's bytes, in order. Called under the service's lock: it must
    /// return at once and never call the service.
    fn output(&self, pane: &str, bytes: &[u8], full: bool);
    /// The pane's output so far is no longer anyone's: it was released or is
    /// gone.
    fn forget(&self, pane: &str);
}

pub use hide_node_link::terminal::ReportSink;

/// How long an observed view's size has to hold before its observer is
/// attached again at it: longer than the gap between a window drag's
/// resizes.
const OBSERVER_RESIZE_QUIET: Duration = Duration::from_millis(150);

/// Seconds between a failed control attach and its next attempt.
const RETRY_SECONDS: [u64; 4] = [5, 10, 20, 30];

/// What a node does on its own about a failed attach.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryPolicy {
    /// Try again on [`RETRY_SECONDS`] while the pane is shown, then wait for
    /// the operator's Reconnect.
    Automatic,
    /// Wait for the operator's Reconnect. A device's panes have always been
    /// left to their connection's own reconnect.
    Manual,
}

#[derive(Clone, Debug)]
struct Recovery {
    due: Option<Instant>,
    retries: usize,
    reason: String,
    last_attempt_at_unix_ms: Option<u64>,
}

impl Recovery {
    fn new(now: Instant, reason: String) -> Self {
        Self {
            due: Some(now + Duration::from_secs(RETRY_SECONDS[0])),
            retries: 0,
            reason,
            last_attempt_at_unix_ms: None,
        }
    }

    fn advance(&mut self, now: Instant) -> bool {
        if self.due.is_none_or(|due| now < due) {
            return false;
        }
        if self.retries == RETRY_SECONDS.len() {
            self.due = None;
            return false;
        }
        self.retries += 1;
        // Give the final attempt time to receive a frame before exhausting it.
        let seconds = RETRY_SECONDS.get(self.retries).copied().unwrap_or(5);
        self.due = Some(now + Duration::from_secs(seconds));
        true
    }

    fn decision(&self) -> &'static str {
        if self.due.is_some() {
            "automatic_bounded"
        } else {
            "manual"
        }
    }

    fn message(&self) -> String {
        if self.due.is_none() {
            format!(
                "{}. Automatic retries exhausted; use Reconnect to try again.",
                self.reason
            )
        } else if self.retries == 0 {
            format!("{}. Retrying in 5 seconds.", self.reason)
        } else {
            format!(
                "Retrying terminal connection ({}/4). {}.",
                self.retries, self.reason
            )
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Lifecycle {
    state: &'static str,
    message: Option<String>,
    generation: u64,
    attempt: u64,
    mode: Option<Mode>,
    exit_category: Option<String>,
    retry_decision: &'static str,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            state: "idle",
            message: None,
            generation: 0,
            attempt: 0,
            mode: None,
            exit_category: None,
            retry_decision: "automatic_initial",
        }
    }
}

#[derive(Default)]
struct Pane {
    lifecycle: Lifecycle,
    /// The generation a session of this pane may deliver for.
    current_generation: Option<u64>,
    session: Option<Session>,
    /// The PTY size last settled, which an attach asks for.
    size: Option<GridSize>,
    /// The grid the screen draws the pane at, which a frame must match.
    view: Option<GridSize>,
    recovery: Option<Recovery>,
    need_full: bool,
    foreign_grid: Option<GridSize>,
    observer_resized_at: Option<Instant>,
    awaiting_size: bool,
    closing: bool,
    asleep: bool,
    asleep_reported: bool,
    hold: PaneHold,
    first_frame_pending: bool,
    watch_frame: bool,
    reported: Option<PaneTerminalState>,
    /// The last refusal reported for each kind, so a key flood into a pane
    /// that cannot take it reports at most once a window.
    refused: HashMap<&'static str, Instant>,
}

struct SpawnRequest {
    pane: String,
    generation: u64,
    mode: Mode,
    size: GridSize,
}

struct Attachment {
    pane: String,
    hold: AttachmentHold,
}

#[derive(Default)]
struct Inner {
    panes: HashMap<String, Pane>,
    requests: Requests,
    facts: HashMap<String, InputFacts>,
    /// The pane the core last heard a screen key for.
    focus_pane: Option<String>,
    shown: HashSet<String>,
    attachment: Option<Attachment>,
    next_generation: u64,
    reports: Vec<TerminalReport>,
    spawns: Vec<SpawnRequest>,
    /// Something may now be due earlier than the clock's current wait.
    wake_clock: bool,
}

#[derive(Default)]
struct Clock {
    stopping: bool,
    /// Moves whenever a deadline may have moved, so a wake between the
    /// clock reading its deadline and waiting on it is never lost.
    version: u64,
}

struct Shared {
    inner: Mutex<Inner>,
    wake: Condvar,
    clock: Mutex<Clock>,
    attacher: Box<dyn Attacher>,
    outputs: Arc<dyn OutputSink>,
    reports: Arc<dyn ReportSink>,
    policy: RetryPolicy,
}

/// A node's terminals. Dropping it ends every session it holds and joins
/// its clock.
pub struct Service {
    shared: Arc<Shared>,
    clock: Option<JoinHandle<()>>,
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

impl Service {
    pub fn start(
        attacher: Box<dyn Attacher>,
        outputs: Arc<dyn OutputSink>,
        reports: Arc<dyn ReportSink>,
        policy: RetryPolicy,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            inner: Mutex::default(),
            wake: Condvar::new(),
            clock: Mutex::default(),
            attacher,
            outputs,
            reports,
            policy,
        });
        let clock_shared = Arc::downgrade(&shared);
        let clock = thread::Builder::new()
            .name("hide-node-terminal-clock".into())
            .spawn(move || run_clock(clock_shared))?;
        Ok(Self {
            shared,
            clock: Some(clock),
        })
    }

    /// Panes holding a session or starting one.
    pub fn attached(&self) -> usize {
        self.shared.lock().attached()
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.shared.clock_lock().stopping = true;
        self.shared.wake.notify_all();
        // Sessions end with their panes: their reapers release and end the
        // attach children.
        self.shared.lock().panes.clear();
        if let Some(clock) = self.clock.take() {
            let _ = clock.join();
        }
    }
}

impl TerminalNode for Service {
    fn control(&self, control: TerminalControl) {
        self.shared
            .run(|inner, shared| inner.control(shared, control));
    }

    fn key(&self, target: KeyTarget, bytes: Vec<u8>, typed_at_unix_ms: u64) {
        let now = Instant::now();
        let chunk = HeldChunk {
            typed_at: now,
            typed_at_unix_ms,
            bytes,
        };
        self.shared.run(|inner, _| match target {
            KeyTarget::Pane(pane) => inner.write_key(&pane, chunk, true, now),
            KeyTarget::Request(request) => match inner.requests.route(&request, chunk) {
                (KeyRoute::Pane(pane), Some(chunk)) => inner.write_key(&pane, chunk, false, now),
                (KeyRoute::Dropped { limit: true }, _) => {
                    inner.reports.push(TerminalReport::RequestDiscarded {
                        request,
                        reason: "limit".to_owned(),
                    });
                }
                _ => {}
            },
        });
    }

    fn view(&self, pane: &str, size: GridSize, new_view: bool) {
        self.shared.run(|inner, _| {
            let entry = inner.panes.entry(pane.to_owned()).or_default();
            let changed = entry.view.replace(size) != Some(size);
            if changed || new_view {
                entry.need_full = true;
            }
        });
    }

    fn redraw(&self, pane: &str) {
        self.shared
            .run(|inner, shared| inner.redraw(shared, pane, "redraw"));
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn clock_lock(&self) -> MutexGuard<'_, Clock> {
        self.clock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs `work` under the lock, then sends its reports, starts its
    /// attaches and wakes the clock when a deadline may have moved, all with
    /// the lock released.
    fn run<T>(self: &Arc<Self>, work: impl FnOnce(&mut Inner, &Arc<Shared>) -> T) -> T {
        let (value, reports, spawns, wake) = {
            let mut inner = self.lock();
            let value = work(&mut inner, self);
            (
                value,
                std::mem::take(&mut inner.reports),
                std::mem::take(&mut inner.spawns),
                std::mem::take(&mut inner.wake_clock),
            )
        };
        for report in reports {
            self.reports.report(report);
        }
        for spawn in spawns {
            self.spawn(spawn);
        }
        if wake {
            self.clock_lock().version += 1;
            self.wake.notify_all();
        }
        value
    }

    fn spawn(self: &Arc<Self>, request: SpawnRequest) {
        let shared = Arc::downgrade(self);
        let name = format!(
            "hide-node-terminal-{}-spawn-{}",
            request.mode.as_str(),
            request.pane
        );
        let pane = request.pane.clone();
        let generation = request.generation;
        let mode = request.mode;
        let started = thread::Builder::new().name(name).spawn(move || {
            let Some(shared) = shared.upgrade() else {
                return;
            };
            let began = Instant::now();
            let result = shared.attacher.open(
                &request.pane,
                request.mode,
                request.size.rows,
                request.size.cols,
            );
            let elapsed_ms = began.elapsed().as_millis();
            shared.run(|inner, shared| {
                inner.ingest_spawn(
                    shared,
                    &request.pane,
                    request.generation,
                    request.mode,
                    result,
                    elapsed_ms,
                )
            });
        });
        if let Err(error) = started {
            let message = format!("terminal session worker could not be started: {error}");
            self.run(|inner, shared| {
                inner.ingest_spawn(shared, &pane, generation, mode, Err(message), 0)
            });
        }
    }
}

/// Sleeps until the next retry, observer re-attach or held input report is
/// due; an idle service waits without waking.
fn run_clock(shared: Weak<Shared>) {
    let mut seen = 0;
    loop {
        let Some(strong) = shared.upgrade() else {
            return;
        };
        seen = strong.clock_lock().version.max(seen);
        let deadline = strong.run(|inner, shared| {
            let deadline = inner.tick(shared, Instant::now());
            // This pass already accounts for what it scheduled.
            inner.wake_clock = false;
            deadline
        });
        let mut clock = strong.clock_lock();
        loop {
            if clock.stopping {
                return;
            }
            if clock.version != seen {
                seen = clock.version;
                break;
            }
            match deadline {
                Some(deadline) => {
                    let wait = deadline.saturating_duration_since(Instant::now());
                    if wait.is_zero() {
                        break;
                    }
                    let (guard, timeout) = strong
                        .wake
                        .wait_timeout(clock, wait)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    clock = guard;
                    if timeout.timed_out() {
                        break;
                    }
                }
                None => {
                    clock = strong
                        .wake
                        .wait(clock)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }
        }
    }
}

impl Inner {
    fn attached(&self) -> usize {
        self.panes
            .values()
            .filter(|pane| pane.session.is_some() || pane.lifecycle.state == "starting")
            .count()
    }

    fn report(&mut self, report: TerminalReport) {
        self.reports.push(report);
    }

    fn note(&mut self, pane: &str, kind: &str, message: String) {
        crate::diagnostic!(json!({"kind": kind, "pane_id": pane, "message": message}));
        self.report(TerminalReport::Note {
            pane: pane.to_owned(),
            kind: kind.to_owned(),
            message,
        });
    }

    fn error(&mut self, pane: &str, kind: &'static str, message: String) {
        self.report(TerminalReport::Error {
            pane: pane.to_owned(),
            kind: kind.to_owned(),
            message,
        });
    }

    /// A refusal of a key, at most once a window per pane and kind.
    fn refuse(&mut self, pane: &str, kind: &'static str, message: String, now: Instant) {
        if let Some(entry) = self.panes.get_mut(pane) {
            if entry.refused.get(kind).is_some_and(|last| {
                now.saturating_duration_since(*last) < input::INPUT_REPORT_WINDOW
            }) {
                return;
            }
            entry.refused.insert(kind, now);
        }
        self.error(pane, kind, message);
    }

    /// Reports the pane's state when it changed since the core last heard.
    fn sync(&mut self, pane_id: &str) {
        let Some(pane) = self.panes.get_mut(pane_id) else {
            return;
        };
        let state = PaneTerminalState {
            state: pane.lifecycle.state.to_owned(),
            mode: pane.lifecycle.mode.map(|mode| mode.as_str().to_owned()),
            generation: pane.lifecycle.generation,
            attempt: pane.lifecycle.attempt,
            message: pane.lifecycle.message.clone(),
            exit_category: pane.lifecycle.exit_category.clone(),
            retry_decision: pane.lifecycle.retry_decision.to_owned(),
            last_attempt_at_unix_ms: pane
                .recovery
                .as_ref()
                .and_then(|recovery| recovery.last_attempt_at_unix_ms),
        };
        if pane.reported.as_ref() == Some(&state) {
            return;
        }
        pane.reported = Some(state.clone());
        self.reports.push(TerminalReport::State {
            pane: pane_id.to_owned(),
            state,
        });
    }

    fn control(&mut self, shared: &Arc<Shared>, control: TerminalControl) {
        let now = Instant::now();
        match control {
            TerminalControl::Attach { pane, size, manual } => {
                let entry = self.panes.entry(pane.clone()).or_default();
                if entry.session.is_none()
                    && let Some(size) = size
                {
                    entry.size = Some(size);
                }
                if manual {
                    entry.recovery = None;
                    entry.session = None;
                    entry.lifecycle = Lifecycle {
                        attempt: entry.lifecycle.attempt,
                        retry_decision: "manual",
                        ..Lifecycle::default()
                    };
                }
                self.request_control(shared, &pane, now);
            }
            TerminalControl::Release { pane, message } => self.release(shared, &pane, message),
            TerminalControl::Forget { pane } => {
                if let Some(mut entry) = self.panes.remove(&pane) {
                    entry.hold.discard(&pane, "pane_gone");
                }
                self.facts.remove(&pane);
                shared.outputs.forget(&pane);
            }
            TerminalControl::Closing { pane, closing } => {
                if let Some(entry) = self.panes.get_mut(&pane) {
                    entry.closing = closing;
                }
            }
            TerminalControl::Resize { pane, size, force } => {
                self.resize(shared, &pane, size, force, now)
            }
            TerminalControl::Shown { panes } => {
                self.shown = panes.into_iter().collect();
                self.wake_clock = true;
            }
            TerminalControl::Scroll {
                pane,
                lines,
                column,
                row,
                modifiers,
            } => {
                let result = match self
                    .panes
                    .get(&pane)
                    .and_then(|entry| entry.session.as_ref())
                {
                    Some(session) if session.mode == Mode::Control => {
                        session.scroll(lines, column, row, modifiers)
                    }
                    _ => Err(format!(
                        "Pane {pane} has no terminal control session to scroll"
                    )),
                };
                if let Err(message) = result {
                    self.error(&pane, "terminal.scroll_failed", message);
                }
            }
            TerminalControl::Write { pane, data } => match protocol::decode_base64(&data) {
                Ok(bytes) => self.write_control(&pane, &bytes, now),
                Err(message) => self.error(&pane, "terminal.invalid_input", message),
            },
            TerminalControl::Asleep { pane, asleep } => {
                let entry = self.panes.entry(pane).or_default();
                entry.asleep = asleep;
                if !asleep {
                    entry.asleep_reported = false;
                }
            }
            TerminalControl::RequestOpen { request } => self.requests.open(&request, now),
            TerminalControl::RequestResolve { request, pane } => {
                let held = self.requests.resolve(&request, &pane, now);
                let held = input::fresh_chunks(&pane, held, now);
                if held.is_empty() {
                    return;
                }
                crate::diagnostic!(json!({
                    "component": "terminal_input",
                    "kind": "terminal.input_handed_over",
                    "pane_id": pane,
                    "chunks": held.len(),
                    "bytes": held.iter().map(|chunk| chunk.bytes.len()).sum::<usize>(),
                }));
                for chunk in held {
                    self.write_key(&pane, chunk, false, now);
                }
            }
            TerminalControl::RequestDiscard { request, reason } => {
                self.requests.discard(&request, &reason, now)
            }
            TerminalControl::AttachmentHold { pane, intent } => {
                self.attachment = Some(Attachment {
                    pane,
                    hold: AttachmentHold {
                        intent,
                        queued: Vec::new(),
                        cancelling: false,
                    },
                });
            }
            TerminalControl::AttachmentRefuse { intent } => {
                if let Some(attachment) = self
                    .attachment
                    .as_mut()
                    .filter(|attachment| attachment.hold.intent == intent)
                {
                    attachment.hold.cancelling = true;
                    attachment.hold.queued.clear();
                }
            }
            TerminalControl::AttachmentDeliver {
                intent,
                generation,
                paste,
            } => self.deliver_attachment(&intent, generation, &paste),
            TerminalControl::AttachmentRelease { intent } => {
                if self
                    .attachment
                    .as_ref()
                    .is_some_and(|attachment| attachment.hold.intent == intent)
                {
                    self.attachment = None;
                }
            }
            TerminalControl::WatchFrame { pane } => {
                self.panes.entry(pane).or_default().watch_frame = true;
            }
        }
    }

    fn deliver_attachment(&mut self, intent: &str, generation: u64, paste: &str) {
        let Some(attachment) = self
            .attachment
            .as_ref()
            .filter(|attachment| attachment.hold.intent == intent)
        else {
            self.report(TerminalReport::AttachmentDelivered {
                intent: intent.to_owned(),
                written: false,
            });
            return;
        };
        let pane = attachment.pane.clone();
        let written = protocol::decode_base64(paste).is_ok_and(|mut bytes| {
            bytes.extend_from_slice(&attachment.hold.queued);
            self.panes
                .get(&pane)
                .and_then(|entry| entry.session.as_ref())
                .filter(|session| session.generation == generation)
                .is_some_and(|session| session.write(&bytes).is_ok())
        });
        // A paste that could not be written keeps what was typed behind it
        // for the retry.
        if written {
            self.attachment = None;
        }
        self.report(TerminalReport::AttachmentDelivered {
            intent: intent.to_owned(),
            written,
        });
    }

    /// Writes one key to its pane. `focus` is a key the operator typed into
    /// this pane on the screen; a key held for a creation reaches its new
    /// pane without moving the keyboard.
    fn write_key(&mut self, pane: &str, chunk: HeldChunk, focus: bool, now: Instant) {
        let submitted = input::submits(&chunk.bytes);
        let asleep = self.panes.get(pane).is_some_and(|entry| entry.asleep);
        if asleep {
            self.input_fact(pane, chunk.typed_at_unix_ms, false, false, now);
            let entry = self.panes.get_mut(pane).expect("checked above");
            if !entry.asleep_reported {
                entry.asleep_reported = true;
                crate::diagnostic!(json!({
                    "component": "agent_sleep",
                    "kind": "agent_sleep.input_dropped",
                    "pane_id": pane,
                }));
            }
            return;
        }
        if let Some(attachment) = self
            .attachment
            .as_mut()
            .filter(|attachment| attachment.pane == pane)
        {
            let intent = attachment.hold.intent.clone();
            let outcome = if attachment.hold.cancelling {
                Some(AttachmentInputOutcome::Cancelling)
            } else if attachment.hold.queued.len() + chunk.bytes.len() > MAX_ATTACHMENT_INPUT_BYTES
            {
                Some(AttachmentInputOutcome::Limit)
            } else {
                attachment.hold.queued.extend_from_slice(&chunk.bytes);
                None
            };
            self.input_fact(pane, chunk.typed_at_unix_ms, false, false, now);
            if let Some(outcome) = outcome {
                self.report(TerminalReport::AttachmentInput {
                    intent,
                    outcome,
                    bytes: chunk.bytes.len(),
                });
            }
            return;
        }
        self.input_fact(pane, chunk.typed_at_unix_ms, submitted, focus, now);
        self.write_typed(pane, chunk, now);
    }

    fn input_fact(&mut self, pane: &str, at: u64, submitted: bool, focus: bool, now: Instant) {
        // Every screen key is the operator's in that pane, but only one into
        // another pane than the last moves the keyboard and cannot wait.
        let moves = focus && self.focus_pane.as_deref() != Some(pane);
        if moves {
            self.focus_pane = Some(pane.to_owned());
        }
        let facts = self.facts.entry(pane.to_owned()).or_default();
        let waiting = facts.deadline().is_some();
        let fact = facts.key(now, at, submitted, moves);
        if !waiting && facts.deadline().is_some() {
            self.wake_clock = true;
        }
        if let Some(fact) = fact {
            self.report(TerminalReport::Input {
                pane: pane.to_owned(),
                at_unix_ms: fact.at_unix_ms,
                submitted: fact.submitted,
                focus: fact.focus,
            });
        }
    }

    /// The writer path every key takes once it is the pane's.
    fn write_typed(&mut self, pane: &str, chunk: HeldChunk, now: Instant) {
        if !self.panes.contains_key(pane) && self.requests.names(pane) {
            // Herdr made the pane for keys the operator is already typing,
            // before the core asked for it to attach.
            self.panes.insert(pane.to_owned(), Pane::default());
        }
        if self
            .panes
            .get(pane)
            .is_some_and(|entry| entry.session.is_none())
            && self.opening(pane)
        {
            let entry = self.panes.get_mut(pane).expect("checked above");
            entry.hold.hold(pane, chunk);
            return;
        }
        self.write_control(pane, &chunk.bytes, now);
    }

    /// Writes bytes to the pane's controller, or says why it cannot.
    fn write_control(&mut self, pane: &str, bytes: &[u8], now: Instant) {
        let Some(entry) = self.panes.get(pane) else {
            self.refuse(
                pane,
                "terminal.unavailable",
                format!("Pane {pane} has no terminal session; use Reconnect to try again"),
                now,
            );
            return;
        };
        if entry.closing {
            self.refuse(
                pane,
                "terminal.close_pending",
                format!("Pane {pane} is closing; input was not sent"),
                now,
            );
            return;
        }
        let result = match entry.session.as_ref() {
            Some(session) if session.mode == Mode::Control => session.write(bytes),
            Some(_) => {
                self.refuse(
                    pane,
                    "terminal.read_only",
                    format!(
                        "Pane {pane} is read-only because another client owns terminal control; use Reconnect to try again"
                    ),
                    now,
                );
                return;
            }
            None => {
                self.refuse(
                    pane,
                    "terminal.unavailable",
                    format!("Pane {pane} has no terminal session; use Reconnect to try again"),
                    now,
                );
                return;
            }
        };
        if let Err(message) = result {
            self.error(pane, "terminal.write_failed", message);
        }
    }

    /// Whether the pane is on its way to a control session: never attached
    /// yet, waiting for its view's size, starting, or retrying after a
    /// failed start. A released, ended or closing pane is not.
    fn opening(&self, pane: &str) -> bool {
        let Some(entry) = self.panes.get(pane) else {
            return false;
        };
        if entry.closing {
            return false;
        }
        match entry.lifecycle.state {
            "idle" | "waiting_size" | "starting" => true,
            "unavailable" => entry
                .recovery
                .as_ref()
                .is_some_and(|recovery| recovery.due.is_some()),
            _ => false,
        }
    }

    fn schedule_recovery(
        &mut self,
        shared: &Arc<Shared>,
        pane: &str,
        reason: String,
        now: Instant,
    ) {
        if shared.policy == RetryPolicy::Manual {
            return;
        }
        let Some(entry) = self.panes.get_mut(pane) else {
            return;
        };
        if entry.recovery.is_none() {
            self.wake_clock = true;
        }
        let recovery = entry
            .recovery
            .get_or_insert_with(|| Recovery::new(now, reason.clone()));
        recovery.reason = reason;
        entry.lifecycle.message = Some(recovery.message());
        entry.lifecycle.retry_decision = recovery.decision();
    }

    fn retry_decision(&self, pane: &str, fallback: &'static str) -> &'static str {
        self.panes
            .get(pane)
            .and_then(|entry| entry.recovery.as_ref())
            .map_or(fallback, Recovery::decision)
    }

    fn request_control(&mut self, shared: &Arc<Shared>, pane: &str, now: Instant) {
        let Some(entry) = self.panes.get(pane) else {
            return;
        };
        let refused_at_cap = entry.lifecycle.state == "unavailable"
            && entry.lifecycle.exit_category.as_deref() == Some("attach_limit");
        let allowed = entry.session.is_none()
            && (refused_at_cap
                || !matches!(
                    entry.lifecycle.state,
                    "starting" | "controlling" | "observing" | "unavailable" | "ended" | "closing"
                ));
        if !allowed {
            self.sync(pane);
            return;
        }
        // Herdr sizes the PTY from the attach, so attaching before a view has
        // reported a size costs a full frame at a guessed size and a second
        // one after the resize.
        if entry.size.is_none() {
            let entry = self.panes.get_mut(pane).expect("checked above");
            let first = !entry.awaiting_size;
            entry.awaiting_size = true;
            entry.lifecycle.state = "waiting_size";
            if first {
                self.note(
                    pane,
                    "terminal.attach_deferred",
                    format!(
                        "Pane {pane} is waiting for its view to report a size before attaching"
                    ),
                );
            }
            self.schedule_recovery(
                shared,
                pane,
                "Waiting for the pane view to report its size".to_owned(),
                now,
            );
            self.sync(pane);
            return;
        }
        let attempt = entry.lifecycle.attempt.saturating_add(1);
        self.start_session(
            shared,
            pane,
            Mode::Control,
            attempt,
            if attempt == 1 {
                "automatic_initial"
            } else {
                "manual"
            },
            None,
            now,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn start_session(
        &mut self,
        shared: &Arc<Shared>,
        pane: &str,
        mode: Mode,
        attempt: u64,
        retry_decision: &'static str,
        message: Option<String>,
        now: Instant,
    ) {
        self.next_generation = self.next_generation.saturating_add(1);
        let generation = self.next_generation;
        let at_cap = self.attached() >= MAX_ATTACHED_PANES
            && !self
                .panes
                .get(pane)
                .is_some_and(|entry| entry.session.is_some());
        let Some(entry) = self.panes.get_mut(pane) else {
            return;
        };
        entry.need_full = true;
        entry.observer_resized_at = None;
        entry.current_generation = Some(generation);
        entry.session = None;
        entry.first_frame_pending = true;
        entry.lifecycle = Lifecycle {
            state: "starting",
            message,
            generation,
            attempt,
            mode: Some(mode),
            exit_category: None,
            retry_decision,
        };
        if mode == Mode::Control {
            self.schedule_recovery(
                shared,
                pane,
                "Waiting for the first terminal frame".to_owned(),
                now,
            );
            if let Some(recovery) = self
                .panes
                .get_mut(pane)
                .and_then(|entry| entry.recovery.as_mut())
            {
                recovery.last_attempt_at_unix_ms = Some(unix_ms());
            }
        }
        let decision = self.retry_decision(pane, retry_decision);
        let entry = self.panes.get_mut(pane).expect("checked above");
        entry.lifecycle.retry_decision = decision;
        // The view's grid is the one the frame guard accepts, so it is the
        // grid the attach asks for.
        if let Some(view) = entry.view {
            entry.size = Some(view);
        }
        let size = entry.size;
        self.sync(pane);
        self.note(
            pane,
            "terminal.session_requested",
            format!(
                "Starting terminal {} session for pane {pane}",
                mode.as_str()
            ),
        );
        if at_cap {
            let message = format!(
                "This machine already keeps {MAX_ATTACHED_PANES} panes attached; Pane {pane} attaches when it is shown again"
            );
            self.record_failure(
                shared,
                pane,
                generation,
                attempt,
                mode,
                "attach_limit",
                &message,
                0,
                now,
            );
            crate::diagnostic!(json!({
                "component": "terminal_session",
                "kind": "terminal.attach_limit",
                "pane_id": pane,
                "generation": generation,
                "limit": MAX_ATTACHED_PANES,
            }));
            return;
        }
        // Herdr sizes the PTY from the attach, so there is no honest size to
        // send when no view has reported one; a pane that arrives here
        // without one is a routing bug, said rather than attached at a guess.
        let Some(size) = size else {
            let message =
                format!("Pane {pane} has no reported terminal size, so it cannot be attached");
            self.record_failure(
                shared,
                pane,
                generation,
                attempt,
                mode,
                "size_unknown",
                &message,
                0,
                now,
            );
            self.error(pane, "terminal.size_unknown", message);
            return;
        };
        self.spawns.push(SpawnRequest {
            pane: pane.to_owned(),
            generation,
            mode,
            size,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn record_failure(
        &mut self,
        shared: &Arc<Shared>,
        pane: &str,
        generation: u64,
        attempt: u64,
        mode: Mode,
        category: &str,
        message: &str,
        elapsed_ms: u128,
        now: Instant,
    ) {
        let Some(entry) = self.panes.get_mut(pane) else {
            return;
        };
        entry.lifecycle = Lifecycle {
            state: "unavailable",
            message: Some(message.to_owned()),
            generation,
            attempt,
            mode: Some(mode),
            exit_category: Some(category.to_owned()),
            retry_decision: "manual",
        };
        if category != "attach_limit" {
            self.schedule_recovery(
                shared,
                pane,
                format!("Terminal start refused: {message}"),
                now,
            );
        }
        self.sync(pane);
        crate::diagnostic!(json!({
            "component": "terminal_session",
            "kind": "terminal.session_unavailable",
            "pane_id": pane,
            "generation": generation,
            "attempt": attempt,
            "mode": mode.as_str(),
            "duration_ms": elapsed_ms,
            "exit_category": category,
            "retry_decision": self.retry_decision(pane, "manual"),
        }));
    }

    fn ingest_spawn(
        &mut self,
        shared: &Arc<Shared>,
        pane: &str,
        generation: u64,
        mode: Mode,
        result: Result<SessionParts, String>,
        elapsed_ms: u128,
    ) {
        let now = Instant::now();
        let current = self.panes.get(pane).is_some_and(|entry| {
            entry.current_generation == Some(generation) && entry.lifecycle.mode == Some(mode)
        });
        if !current {
            // A late session for a pane that moved on ends with its parts.
            drop(result);
            return;
        }
        let attempt = self.panes[pane].lifecycle.attempt;
        let parts = match result {
            Ok(parts) => parts,
            Err(message) => {
                self.spawn_failed(
                    shared,
                    pane,
                    generation,
                    attempt,
                    mode,
                    "spawn_failed",
                    message,
                    elapsed_ms,
                    now,
                );
                return;
            }
        };
        let failure_shared = Arc::downgrade(shared);
        let failure_pane = pane.to_owned();
        let on_write_failure: Box<dyn Fn(String) + Send> = Box::new(move |message| {
            if let Some(shared) = failure_shared.upgrade() {
                shared.run(|inner, shared| {
                    inner.write_failed(shared, &failure_pane, generation, message)
                });
            }
        });
        let (session, reader) =
            match Session::start(pane, generation, mode, parts, on_write_failure) {
                Ok(started) => started,
                Err(message) => {
                    self.spawn_failed(
                        shared,
                        pane,
                        generation,
                        attempt,
                        mode,
                        "spawn_failed",
                        message,
                        elapsed_ms,
                        now,
                    );
                    return;
                }
            };
        let entry = self.panes.get_mut(pane).expect("checked above");
        if mode == Mode::Control
            && let Some(size) = entry.size
            && let Err(message) = session.resize(size.rows, size.cols)
        {
            self.error(pane, "terminal.resize_after_attach_failed", message);
        }
        let entry = self.panes.get_mut(pane).expect("checked above");
        let held = entry.hold.take(pane, now);
        if !held.is_empty() {
            if mode == Mode::Control {
                if let Err(message) = session.write(&held) {
                    self.error(pane, "terminal.write_failed", message);
                }
            } else {
                crate::diagnostic!(json!({
                    "component": "terminal_input",
                    "kind": "terminal.input_discarded",
                    "pane_id": pane,
                    "reason": "read_only",
                    "bytes": held.len(),
                }));
            }
        }
        let reader_shared = Arc::downgrade(shared);
        let reader_pane = pane.to_owned();
        let reader_started = session::spawn_reader(pane, mode, reader, move |event| {
            let Some(shared) = reader_shared.upgrade() else {
                return false;
            };
            shared.run(|inner, shared| {
                inner.session_event(shared, &reader_pane, generation, mode, event)
            })
        });
        if let Err(message) = reader_started {
            drop(session);
            self.spawn_failed(
                shared,
                pane,
                generation,
                attempt,
                mode,
                "reader_start_failed",
                message,
                elapsed_ms,
                now,
            );
            return;
        }
        let entry = self.panes.get_mut(pane).expect("checked above");
        entry.session = Some(session);
        let message = entry.lifecycle.message.clone();
        let decision = self.retry_decision(
            pane,
            if mode == Mode::Observe {
                "manual"
            } else {
                "none"
            },
        );
        let entry = self.panes.get_mut(pane).expect("checked above");
        entry.lifecycle = Lifecycle {
            state: match mode {
                Mode::Control => "controlling",
                Mode::Observe => "observing",
            },
            message,
            generation,
            attempt,
            mode: Some(mode),
            exit_category: None,
            retry_decision: decision,
        };
        self.sync(pane);
        self.note(
            pane,
            "terminal.session_ready",
            format!(
                "Pane {pane} terminal {} session ready in {elapsed_ms} ms",
                mode.as_str()
            ),
        );
        crate::diagnostic!(json!({
            "component": "terminal_session",
            "kind": "terminal.session_ready",
            "pane_id": pane,
            "generation": generation,
            "attempt": attempt,
            "mode": mode.as_str(),
            "duration_ms": elapsed_ms,
            "retry_decision": decision,
        }));
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_failed(
        &mut self,
        shared: &Arc<Shared>,
        pane: &str,
        generation: u64,
        attempt: u64,
        mode: Mode,
        category: &str,
        message: String,
        elapsed_ms: u128,
        now: Instant,
    ) {
        self.record_failure(
            shared, pane, generation, attempt, mode, category, &message, elapsed_ms, now,
        );
        let notice = format!(
            "\r\n[Terminal {} session for {pane} failed: {message}]\r\n",
            mode.as_str()
        );
        shared.outputs.output(pane, notice.as_bytes(), false);
        self.error(
            pane,
            if category == "reader_start_failed" {
                "terminal.session_reader_failed"
            } else {
                "terminal.session_failed"
            },
            message,
        );
    }

    /// One event from a session's reader; answers whether it keeps reading.
    fn session_event(
        &mut self,
        shared: &Arc<Shared>,
        pane: &str,
        generation: u64,
        mode: Mode,
        event: SessionEvent,
    ) -> bool {
        match event {
            SessionEvent::Frame {
                width,
                height,
                full,
                bytes,
            } => self
                .frame(
                    shared,
                    pane,
                    generation,
                    mode,
                    &bytes,
                    GridSize {
                        rows: height,
                        cols: width,
                    },
                    full,
                )
                .is_some(),
            SessionEvent::Closed { reason } => {
                self.closed(shared, pane, generation, mode, reason);
                false
            }
        }
    }

    /// Hands a frame of the current session to the screen, unless its grid
    /// is not the one the view draws at or a full frame is awaited. `None`
    /// retires the reader.
    #[allow(clippy::too_many_arguments)]
    fn frame(
        &mut self,
        shared: &Arc<Shared>,
        pane: &str,
        generation: u64,
        mode: Mode,
        bytes: &[u8],
        arrived: GridSize,
        full: bool,
    ) -> Option<bool> {
        let entry = self.panes.get_mut(pane)?;
        if entry.current_generation != Some(generation)
            || entry
                .session
                .as_ref()
                .is_none_or(|session| session.mode != mode)
        {
            return None;
        }
        let expected = entry.view.or(entry.size);
        if expected != Some(arrived) {
            // Herdr draws an observer at the grid it attached with, and an
            // observer has no writer to resize, so a view that changed size
            // since would hold every frame from now on. It attaches again at
            // the view's grid; this generation's reader retires.
            if mode == Mode::Observe && expected.is_some() {
                if entry.observer_resized_at.is_some() {
                    return Some(false);
                }
                self.reattach_observer(shared, pane, "frame_grid");
                return None;
            }
            entry.need_full = true;
            if entry.foreign_grid.replace(arrived) != Some(arrived) {
                crate::diagnostic!(json!({
                    "component": "terminal", "kind": "terminal.frame_geometry_mismatch",
                    "pane_id": pane, "frame": [arrived.cols, arrived.rows],
                    "expected": expected.map(|size| [size.cols, size.rows]),
                }));
            }
            return Some(false);
        }
        entry.foreign_grid = None;
        if entry.need_full && !full {
            return Some(false);
        }
        // The replacement full frame resets the screen's parser itself, so no
        // empty canvas is ever shown between the two.
        let reset = std::mem::take(&mut entry.need_full);
        let first = std::mem::take(&mut entry.first_frame_pending);
        let watched = std::mem::take(&mut entry.watch_frame);
        if mode == Mode::Control {
            if let Some(recovery) = entry.recovery.take() {
                crate::diagnostic!(json!({
                    "kind": "terminal.control_frame_ready", "pane_id": pane,
                    "generation": generation, "occurred_at": unix_ms(),
                    "retries": recovery.retries,
                    "last_attempt_at_unix_ms": recovery.last_attempt_at_unix_ms,
                    "rows": arrived.rows, "cols": arrived.cols,
                }));
            }
            entry.lifecycle.message = None;
            entry.lifecycle.retry_decision = "none";
            self.sync(pane);
        }
        // Only a frame this node began with a reset draws the whole screen
        // for a reader that lost output; Herdr's own full frames may not.
        if reset {
            let mut drawn = Vec::with_capacity(bytes.len() + 2);
            drawn.extend_from_slice(b"\x1bc");
            drawn.extend_from_slice(bytes);
            shared.outputs.output(pane, &drawn, true);
        } else {
            shared.outputs.output(pane, bytes, false);
        }
        if first {
            self.report(TerminalReport::FirstFrame {
                pane: pane.to_owned(),
                generation,
            });
        }
        if watched {
            self.report(TerminalReport::FrameShown {
                pane: pane.to_owned(),
                at_unix_ms: unix_ms(),
            });
        }
        Some(true)
    }

    /// A `terminal.closed` envelope or the stream's end. An owner conflict
    /// falls back once to Herdr's read-only observer; every other close ends
    /// only the transport, never the pane.
    fn closed(
        &mut self,
        shared: &Arc<Shared>,
        pane: &str,
        generation: u64,
        mode: Mode,
        reason: Option<String>,
    ) {
        let now = Instant::now();
        let Some(entry) = self.panes.get_mut(pane) else {
            return;
        };
        if entry.current_generation != Some(generation)
            || entry
                .session
                .as_ref()
                .is_none_or(|session| session.mode != mode)
        {
            return;
        }
        entry.session = None;
        let attempt = entry.lifecycle.attempt;
        let closing = entry.closing;
        let category = protocol::closed_category(reason.as_deref());
        let message = reason
            .unwrap_or_else(|| format!("Pane {pane} terminal {} session ended", mode.as_str()));
        if mode == Mode::Control && category == "owner_conflict" {
            crate::diagnostic!(json!({
                "component": "terminal_session",
                "kind": "terminal.control_owner_conflict",
                "pane_id": pane,
                "generation": generation,
                "attempt": attempt,
                "mode": mode.as_str(),
                "exit_category": category,
                "retry_decision": self.retry_decision(pane, "observe_once"),
            }));
            self.schedule_recovery(
                shared,
                pane,
                "Another client owns terminal control; viewing read-only".to_owned(),
                now,
            );
            let message = self
                .panes
                .get(pane)
                .and_then(|entry| entry.recovery.as_ref())
                .map(Recovery::message);
            self.start_session(
                shared,
                pane,
                Mode::Observe,
                attempt,
                "observe_once",
                message,
                now,
            );
            return;
        }
        // A pane Hide asked Herdr to close ends its transport as a
        // consequence of the close. It is not a failure, so nothing is drawn
        // over its last frame.
        if closing {
            let entry = self.panes.get_mut(pane).expect("checked above");
            entry.lifecycle = Lifecycle {
                state: "closing",
                message: None,
                generation,
                attempt,
                mode: Some(mode),
                exit_category: Some(category.to_owned()),
                retry_decision: "none",
            };
            self.sync(pane);
            crate::diagnostic!(json!({
                "component": "terminal_session",
                "kind": "terminal.session_closed_with_pane",
                "pane_id": pane,
                "generation": generation,
                "attempt": attempt,
                "mode": mode.as_str(),
                "exit_category": category,
                "retry_decision": "none",
            }));
            return;
        }
        let entry = self.panes.get_mut(pane).expect("checked above");
        entry.lifecycle = Lifecycle {
            state: "ended",
            message: Some(message.clone()),
            generation,
            attempt,
            mode: Some(mode),
            exit_category: Some(category.to_owned()),
            retry_decision: "manual",
        };
        self.schedule_recovery(shared, pane, message.clone(), now);
        self.sync(pane);
        let notice = format!("\r\n[{message}]\r\n");
        shared.outputs.output(pane, notice.as_bytes(), false);
        crate::diagnostic!(json!({
            "component": "terminal_session",
            "kind": "terminal.session_ended",
            "pane_id": pane,
            "generation": generation,
            "attempt": attempt,
            "mode": mode.as_str(),
            "exit_category": category,
            "retry_decision": self.retry_decision(pane, "manual"),
        }));
    }

    fn write_failed(&mut self, shared: &Arc<Shared>, pane: &str, generation: u64, message: String) {
        let now = Instant::now();
        let Some(entry) = self.panes.get_mut(pane) else {
            return;
        };
        if entry.current_generation != Some(generation) {
            return;
        }
        self.error(pane, "terminal.write_failed", message.clone());
        let entry = self.panes.get_mut(pane).expect("checked above");
        entry.session = None;
        entry.lifecycle.state = "unavailable";
        self.schedule_recovery(shared, pane, message.clone(), now);
        self.sync(pane);
        crate::diagnostic!(json!({
            "component": "terminal_session",
            "kind": "terminal.control_write_failed",
            "pane_id": pane,
            "generation": generation,
            "message": message,
            "retry_decision": self.retry_decision(pane, "manual"),
        }));
    }

    fn release(&mut self, shared: &Arc<Shared>, pane: &str, message: String) {
        let Some(entry) = self.panes.get_mut(pane) else {
            return;
        };
        if entry.lifecycle.state == "released" {
            return;
        }
        let generation = entry.current_generation.unwrap_or_default();
        entry.session = None;
        entry.awaiting_size = false;
        entry.recovery = None;
        entry.hold.discard(pane, "released");
        entry.lifecycle = Lifecycle {
            state: "released",
            message: Some(message),
            generation,
            attempt: entry.lifecycle.attempt,
            mode: None,
            exit_category: None,
            retry_decision: "on_next_visit",
        };
        self.sync(pane);
        shared.outputs.forget(pane);
        crate::diagnostic!(json!({
            "component": "terminal_session",
            "kind": "terminal.session_released",
            "pane_id": pane,
            "generation": generation,
        }));
    }

    fn resize(
        &mut self,
        shared: &Arc<Shared>,
        pane: &str,
        size: GridSize,
        force: bool,
        now: Instant,
    ) {
        let entry = self.panes.entry(pane.to_owned()).or_default();
        if entry.awaiting_size {
            entry.awaiting_size = false;
            entry.recovery = None;
            entry.size = Some(size);
            entry.lifecycle.state = "idle";
            self.request_control(shared, pane, now);
            return;
        }
        let previous = entry.size.replace(size);
        if !force && previous == Some(size) && !entry.need_full {
            return;
        }
        crate::diagnostic!(json!({
            "kind": "terminal.resize_settled", "pane_id": pane,
            "rows": size.rows, "cols": size.cols,
        }));
        let result = match entry.session.as_ref() {
            Some(session) if session.mode == Mode::Control => session.resize(size.rows, size.cols),
            // An observer cannot resize; it attaches again at the new grid
            // once the size settles.
            Some(_) if previous != Some(size) => {
                entry.observer_resized_at = Some(now);
                self.wake_clock = true;
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(message) = result {
            self.error(pane, "terminal.resize_failed", message);
        }
    }

    /// A reader lost output: the next frame it draws is a full one.
    fn redraw(&mut self, shared: &Arc<Shared>, pane: &str, cause: &'static str) {
        let Some(entry) = self.panes.get_mut(pane) else {
            return;
        };
        entry.need_full = true;
        let size = entry.size;
        let mode = entry.session.as_ref().map(|session| session.mode);
        crate::diagnostic!(json!({
            "component": "terminal", "kind": "terminal.redraw_requested",
            "pane_id": pane, "cause": cause,
            "generation": entry.current_generation,
        }));
        match (mode, size) {
            (Some(Mode::Control), Some(size)) => {
                let result = entry
                    .session
                    .as_ref()
                    .expect("mode read from it")
                    .resize(size.rows, size.cols);
                if let Err(message) = result {
                    self.error(pane, "terminal.resize_failed", message);
                }
            }
            (Some(Mode::Observe), _) => self.reattach_observer(shared, pane, cause),
            _ => {}
        }
    }

    /// Starts the pane's observer again at the view's grid. Herdr draws an
    /// observer at the grid it attached with, so this is how an observed
    /// view changes size.
    fn reattach_observer(&mut self, shared: &Arc<Shared>, pane: &str, cause: &'static str) {
        let Some(entry) = self.panes.get(pane) else {
            return;
        };
        let lifecycle = entry.lifecycle.clone();
        crate::diagnostic!(json!({
            "component": "terminal", "kind": "terminal.observer_reattached",
            "pane_id": pane, "cause": cause,
            "view": entry.view.or(entry.size).map(|size| [size.cols, size.rows]),
        }));
        self.start_session(
            shared,
            pane,
            Mode::Observe,
            lifecycle.attempt,
            lifecycle.retry_decision,
            lifecycle.message,
            Instant::now(),
        );
    }

    /// Advances what time alone moves, and answers when it must run next.
    fn tick(&mut self, shared: &Arc<Shared>, now: Instant) -> Option<Instant> {
        let settled = self
            .panes
            .iter()
            .filter(|(_, entry)| {
                entry
                    .observer_resized_at
                    .is_some_and(|at| now.saturating_duration_since(at) >= OBSERVER_RESIZE_QUIET)
            })
            .map(|(pane, _)| pane.clone())
            .collect::<Vec<_>>();
        for pane in settled {
            let entry = self.panes.get_mut(&pane).expect("collected above");
            entry.observer_resized_at = None;
            if entry
                .session
                .as_ref()
                .is_some_and(|session| session.mode == Mode::Observe)
            {
                self.reattach_observer(shared, &pane, "resize");
            }
        }
        let due = self
            .panes
            .iter()
            .filter(|(pane, entry)| {
                self.shown.contains(*pane)
                    && entry
                        .recovery
                        .as_ref()
                        .and_then(|recovery| recovery.due)
                        .is_some_and(|due| now >= due)
            })
            .map(|(pane, _)| pane.clone())
            .collect::<Vec<_>>();
        for pane in due {
            self.retry(shared, &pane, now);
        }
        let reports = self
            .facts
            .iter_mut()
            .filter_map(|(pane, facts)| facts.due(now).map(|fact| (pane.clone(), fact)))
            .collect::<Vec<_>>();
        for (pane, fact) in reports {
            self.report(TerminalReport::Input {
                pane,
                at_unix_ms: fact.at_unix_ms,
                submitted: fact.submitted,
                focus: fact.focus,
            });
        }
        self.next_deadline()
    }

    fn retry(&mut self, shared: &Arc<Shared>, pane: &str, now: Instant) {
        let entry = self.panes.get_mut(pane).expect("collected by the caller");
        if entry.closing {
            entry.recovery = None;
            return;
        }
        let recovery = entry.recovery.as_mut().expect("collected by the caller");
        let retry = recovery.advance(now);
        let message = recovery.message();
        if retry && entry.size.is_some() {
            let attempt = entry.lifecycle.attempt + 1;
            self.start_session(
                shared,
                pane,
                Mode::Control,
                attempt,
                "automatic_bounded",
                Some(message.clone()),
                now,
            );
        } else {
            entry.lifecycle.message = Some(message.clone());
            entry.lifecycle.retry_decision = if retry { "automatic_bounded" } else { "manual" };
            if !retry && entry.lifecycle.state != "observing" {
                entry.lifecycle.state = "unavailable";
                entry.session = None;
                // A late spawn cannot revive an exhausted attempt.
                entry.current_generation = None;
                // The pane is no longer connecting; keys held for it would
                // run whenever the operator reconnects, so they end here.
                entry.hold.discard(pane, "session_failed");
            }
            self.sync(pane);
        }
        self.note(
            pane,
            if retry {
                "terminal.retrying"
            } else {
                "terminal.retries_exhausted"
            },
            format!("Pane {pane}: {message}"),
        );
    }

    fn next_deadline(&self) -> Option<Instant> {
        let panes = self.panes.iter().flat_map(|(pane, entry)| {
            let retry = entry
                .recovery
                .as_ref()
                .and_then(|recovery| recovery.due)
                .filter(|_| self.shown.contains(pane));
            let observer = entry
                .observer_resized_at
                .map(|at| at + OBSERVER_RESIZE_QUIET);
            [retry, observer]
        });
        let facts = self.facts.values().map(InputFacts::deadline);
        panes.chain(facts).flatten().min()
    }
}

#[cfg(test)]
mod tests;
