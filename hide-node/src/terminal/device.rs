//! A device's terminals inside its one node link (PRD core-host-node-terminal
//! D-10, D-18, D-19, D-20).
//!
//! On the device, [`NodeTerminals`] runs the node's [`Service`] for the
//! device's Herdr and sends what it says up the link: reports first, then
//! one line of output per pane in turn, so a pane that floods its output
//! cannot hold another pane's frames or the core's answers back. A pane
//! whose unsent output passes [`MAX_UNSENT_OUTPUT_BYTES`] loses it and is
//! drawn again from a full frame once the link has caught up.
//!
//! On the core's side, [`DeviceTerminals`] is the device's [`TerminalNode`]:
//! it writes controls and keys down the link in the order they were given
//! and keeps at most [`MAX_UNSENT_KEY_BYTES`] of one pane's keys unsent,
//! counted as the keys' own bytes. Keys for one pane that wait behind each
//! other go down as one line carrying the last key's time, so a link that
//! falls behind costs one frame per run of keys, not one per key. A pane
//! past the cap reads ended and refuses keys until it is attached again;
//! the keys it already took are written in order, or, when the link fails,
//! named in the log as unwritten. A link that ends takes every flow with it
//! and refuses later keys rather than keep them for another link.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::thread;

use hide_node_link::terminal::{
    GridSize, KeyTarget, MAX_UNSENT_KEY_BYTES, MAX_UNSENT_OUTPUT_BYTES, PaneTerminalState,
    ReportSink, TerminalControl, TerminalDown, TerminalLine, TerminalNode, TerminalOutput,
    TerminalReport, TerminalUp,
};
use serde_json::json;

use super::protocol::{decode_base64, encode_base64};
use super::{LocalAttacher, OutputSink, RetryPolicy, Service};

/// Report and diagnostic lines a device keeps unsent before it drops new
/// ones; a link that stalls this long has stopped being read.
const MAX_UNSENT_REPORT_LINES: usize = 4096;

/// What a link hands each terminal line it reads.
pub type LineHandler = Box<dyn Fn(&[u8]) + Send + Sync>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn line_of<T: serde::Serialize>(terminal: T) -> Option<Vec<u8>> {
    let mut line = serde_json::to_vec(&TerminalLine { terminal }).ok()?;
    line.push(b'\n');
    Some(line)
}

// ---------------------------------------------------------------------------
// The device's side.

#[derive(Default)]
struct PaneUplink {
    lines: VecDeque<Vec<u8>>,
    bytes: usize,
    /// Output was dropped: nothing but a full frame is sent until one comes.
    dropping: bool,
    /// Ask for that full frame once the pane's queue is empty.
    redraw: bool,
}

#[derive(Default)]
struct UplinkState {
    stopped: bool,
    reports: VecDeque<Vec<u8>>,
    reports_dropped: usize,
    panes: HashMap<String, PaneUplink>,
    /// Panes with output waiting, in turn.
    order: VecDeque<String>,
}

#[derive(Default)]
struct Uplink {
    state: Mutex<UplinkState>,
    ready: Condvar,
}

impl Uplink {
    fn push_report(&self, line: Vec<u8>) {
        let mut state = lock(&self.state);
        if state.stopped {
            return;
        }
        if state.reports.len() >= MAX_UNSENT_REPORT_LINES {
            state.reports_dropped += 1;
            return;
        }
        state.reports.push_back(line);
        drop(state);
        self.ready.notify_one();
    }

    fn push_output(&self, pane: &str, bytes: &[u8], full: bool) {
        let mut state = lock(&self.state);
        if state.stopped {
            return;
        }
        let entry = state.panes.entry(pane.to_owned()).or_default();
        if entry.dropping && !full {
            return;
        }
        if full {
            // A full frame draws the whole screen: what waited before it
            // would only be drawn over.
            entry.lines.clear();
            entry.bytes = 0;
            entry.dropping = false;
            entry.redraw = false;
        }
        let Some(line) = line_of(TerminalUp::Output(TerminalOutput {
            pane: pane.to_owned(),
            data: encode_base64(bytes),
            full,
        })) else {
            return;
        };
        if entry.bytes + line.len() > MAX_UNSENT_OUTPUT_BYTES {
            let dropped = entry.bytes + line.len();
            entry.lines.clear();
            entry.bytes = 0;
            entry.dropping = true;
            entry.redraw = true;
            state.order.retain(|queued| queued != pane);
            drop(state);
            // The redraw is asked for by the writer, once the link has
            // taken what was ahead of it.
            self.ready.notify_one();
            crate::diagnostic!(json!({
                "component": "node_terminal",
                "kind": "terminal.output_overflow",
                "pane_id": pane,
                "bytes": dropped,
                "cap": MAX_UNSENT_OUTPUT_BYTES,
            }));
            return;
        }
        entry.bytes += line.len();
        entry.lines.push_back(line);
        if !state.order.iter().any(|queued| queued == pane) {
            state.order.push_back(pane.to_owned());
        }
        drop(state);
        self.ready.notify_one();
    }

    fn forget(&self, pane: &str) {
        let mut state = lock(&self.state);
        state.panes.remove(pane);
        state.order.retain(|queued| queued != pane);
    }

    /// The next line to send, and the panes to draw again now that the link
    /// caught up with them.
    fn next(&self) -> Option<(Option<Vec<u8>>, Vec<String>)> {
        let mut state = lock(&self.state);
        loop {
            if state.stopped {
                return None;
            }
            let redraws = state
                .panes
                .iter_mut()
                .filter(|(_, pane)| pane.redraw && pane.lines.is_empty())
                .map(|(id, pane)| {
                    pane.redraw = false;
                    id.clone()
                })
                .collect::<Vec<_>>();
            if state.reports_dropped > 0 && state.reports.len() < MAX_UNSENT_REPORT_LINES {
                let dropped = std::mem::take(&mut state.reports_dropped);
                if let Some(line) = line_of(TerminalUp::Diagnostic {
                    record: json!({
                        "component": "node_terminal",
                        "kind": "terminal.reports_dropped",
                        "lines": dropped,
                        "cap": MAX_UNSENT_REPORT_LINES,
                    }),
                }) {
                    state.reports.push_back(line);
                }
            }
            if let Some(line) = state.reports.pop_front() {
                return Some((Some(line), redraws));
            }
            while let Some(pane) = state.order.pop_front() {
                let Some(entry) = state.panes.get_mut(&pane) else {
                    continue;
                };
                let Some(line) = entry.lines.pop_front() else {
                    continue;
                };
                entry.bytes -= line.len();
                if !entry.lines.is_empty() {
                    state.order.push_back(pane);
                }
                return Some((Some(line), redraws));
            }
            if !redraws.is_empty() {
                return Some((None, redraws));
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    fn stop(&self) {
        let mut state = lock(&self.state);
        state.stopped = true;
        state.reports.clear();
        state.panes.clear();
        state.order.clear();
        drop(state);
        self.ready.notify_all();
    }
}

struct UplinkOutputs(Arc<Uplink>);

impl OutputSink for UplinkOutputs {
    fn output(&self, pane: &str, bytes: &[u8], full: bool) {
        self.0.push_output(pane, bytes, full);
    }

    fn forget(&self, pane: &str) {
        self.0.forget(pane);
    }
}

struct UplinkReports(Arc<Uplink>);

impl ReportSink for UplinkReports {
    fn report(&self, report: TerminalReport) {
        if let Some(line) = line_of(TerminalUp::Report { report }) {
            self.0.push_report(line);
        }
    }
}

/// The uplink this process's diagnostics go up, once a link started one.
static DIAGNOSTICS: OnceLock<Mutex<Weak<Uplink>>> = OnceLock::new();

/// A diagnostic sink for a device's node process ([`crate::diagnostics::install`]):
/// a record goes up the link that started the terminal service, since a
/// device's own stderr reaches no log, and to stderr when there is none.
pub fn forward_diagnostic(record: serde_json::Value) {
    let uplink = DIAGNOSTICS
        .get()
        .and_then(|current| lock(current).upgrade());
    match uplink {
        Some(uplink) => {
            if let Some(line) = line_of(TerminalUp::Diagnostic { record }) {
                uplink.push_report(line);
            }
        }
        None => eprintln!("{record}"),
    }
}

/// How a device node attaches to the Herdr at a socket.
type AttacherFor = Box<dyn Fn(&str) -> Box<dyn super::Attacher> + Send + Sync>;

/// A device node's terminal service, as its link's serve loop reaches it.
pub struct NodeTerminals {
    service: Mutex<Option<Service>>,
    uplink: Arc<Uplink>,
    attacher: AttacherFor,
}

impl Default for NodeTerminals {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeTerminals {
    /// The service of a device's node: `herdr terminal session` children of
    /// the `herdr` on the account's own search path.
    pub fn new() -> Self {
        Self::with_attacher(Box::new(|socket: &str| {
            let herdr = hide_platform::programs::find_cli("herdr");
            Box::new(LocalAttacher::new(herdr, socket.into())) as Box<dyn super::Attacher>
        }))
    }

    fn with_attacher(attacher: AttacherFor) -> Self {
        Self {
            service: Mutex::default(),
            uplink: Arc::default(),
            attacher,
        }
    }

    fn with_service(&self, work: impl FnOnce(&Service)) {
        // The service's own lock serializes its work; this one only guards
        // its start and stop, and a call never waits on the link.
        if let Some(service) = lock(&self.service).as_ref() {
            work(service);
        }
    }
}

impl hide_host::serve::Terminals for NodeTerminals {
    fn start(&self, herdr_socket: &str) -> Result<(), String> {
        let mut service = lock(&self.service);
        if service.is_some() {
            return Ok(());
        }
        let started = Service::start(
            (self.attacher)(herdr_socket),
            Arc::new(UplinkOutputs(Arc::clone(&self.uplink))),
            Arc::new(UplinkReports(Arc::clone(&self.uplink))),
            // A device's panes have always waited for the operator's
            // Reconnect after a failed attach.
            RetryPolicy::Manual,
        )
        .map_err(|error| format!("The device's terminal service could not start: {error}"))?;
        *service = Some(started);
        *lock(DIAGNOSTICS.get_or_init(|| Mutex::new(Weak::new()))) = Arc::downgrade(&self.uplink);
        Ok(())
    }

    fn line(&self, line: &[u8]) {
        let down = match serde_json::from_slice::<TerminalLine<TerminalDown>>(line) {
            Ok(line) => line.terminal,
            Err(error) => {
                crate::diagnostic!(json!({
                    "component": "node_terminal",
                    "kind": "terminal.line_unreadable",
                    "class": format!("{:?}", error.classify()),
                    "bytes": line.len(),
                }));
                return;
            }
        };
        if lock(&self.service).is_none() {
            crate::diagnostic!(json!({
                "component": "node_terminal",
                "kind": "terminal.line_before_start",
            }));
            return;
        }
        match down {
            TerminalDown::Control { control } => {
                self.with_service(|service| service.control(control))
            }
            TerminalDown::Key {
                target,
                data,
                typed_at_unix_ms,
            } => match decode_base64(&data) {
                Ok(bytes) => {
                    self.with_service(|service| service.key(target, bytes, typed_at_unix_ms))
                }
                Err(message) => crate::diagnostic!(json!({
                    "component": "node_terminal",
                    "kind": "terminal.key_unreadable",
                    "message": message,
                })),
            },
            TerminalDown::View {
                pane,
                size,
                new_view,
            } => self.with_service(|service| service.view(&pane, size, new_view)),
            TerminalDown::Redraw { pane } => self.with_service(|service| service.redraw(&pane)),
        }
    }

    fn next_up(&self) -> Option<Vec<u8>> {
        loop {
            let (line, redraws) = self.uplink.next()?;
            for pane in redraws {
                self.with_service(|service| service.redraw(&pane));
            }
            if let Some(line) = line {
                return Some(line);
            }
        }
    }

    fn stop(&self) {
        self.uplink.stop();
        // Dropped outside the lock: ending each session releases it and
        // ends its attach child.
        let service = lock(&self.service).take();
        drop(service);
    }
}

// ---------------------------------------------------------------------------
// The core's side.

/// Where a device's terminals report on the core's side: the screen's hub
/// for output and the core for reports, each told which device it is.
pub trait DeviceSink: Send + Sync {
    fn output(&self, device: &str, pane: &str, bytes: &[u8], full: bool);
    fn report(&self, device: &str, report: TerminalReport);
}

/// The link a device's terminal lines are written on.
pub trait LineLink: Send + Sync + 'static {
    /// Writes one whole line; an error means the link can take no more.
    fn send_line(&self, line: &[u8]) -> Result<(), String>;
}

/// The most key bytes one waiting line gathers; a longer run of keys goes
/// down as several lines, in order.
const MAX_KEY_LINE_BYTES: usize = 16 * 1024;

/// One thing waiting to go down the link.
enum Waiting {
    Line(Vec<u8>),
    /// A run of one pane's keys, typed back to back while the link was
    /// behind, and when the last of them was typed.
    Keys {
        pane: String,
        bytes: Vec<u8>,
        typed_at_unix_ms: u64,
    },
}

#[derive(Default)]
struct ProxyState {
    lines: VecDeque<Waiting>,
    /// Key bytes each pane has waiting in `lines`.
    key_bytes: HashMap<String, usize>,
    /// Panes whose keys passed the cap: they refuse keys until attached.
    overflowed: HashSet<String>,
    /// Why the link took no more; every key after it is refused.
    failed: Option<String>,
    /// Panes already told the link failed, so a key flood reports once.
    told_failed: HashSet<String>,
    /// Each pane's state as the device last reported it.
    states: HashMap<String, PaneTerminalState>,
    stopping: bool,
}

impl ProxyState {
    /// Queues `bytes` for `pane`, onto the run of its keys already waiting
    /// last in line when there is one.
    fn queue_keys(&mut self, pane: String, bytes: Vec<u8>, typed_at_unix_ms: u64) {
        *self.key_bytes.entry(pane.clone()).or_default() += bytes.len();
        if let Some(Waiting::Keys {
            pane: waiting_pane,
            bytes: waiting,
            typed_at_unix_ms: waiting_at,
        }) = self.lines.back_mut()
            && *waiting_pane == pane
            && waiting.len() + bytes.len() <= MAX_KEY_LINE_BYTES
        {
            waiting.extend_from_slice(&bytes);
            *waiting_at = typed_at_unix_ms;
            return;
        }
        self.lines.push_back(Waiting::Keys {
            pane,
            bytes,
            typed_at_unix_ms,
        });
    }
}

struct ProxyShared {
    device: String,
    state: Mutex<ProxyState>,
    ready: Condvar,
    sink: Arc<dyn DeviceSink>,
}

/// A device's terminals on the core's side of its link.
pub struct DeviceTerminals {
    shared: Arc<ProxyShared>,
}

impl DeviceTerminals {
    /// Starts the writer for `device`'s `link`. Lines the device sends up
    /// reach [`DeviceTerminals::inbound`]'s handler.
    pub fn start(
        device: &str,
        link: Arc<dyn LineLink>,
        sink: Arc<dyn DeviceSink>,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(ProxyShared {
            device: device.to_owned(),
            state: Mutex::default(),
            ready: Condvar::new(),
            sink,
        });
        let writer = Arc::clone(&shared);
        thread::Builder::new()
            .name("hide-device-terminal-writer".into())
            .spawn(move || write_lines(&writer, link.as_ref()))?;
        Ok(Self { shared })
    }

    /// What reads the device's terminal lines off the link: output to the
    /// screen's hub, reports to the core.
    pub fn inbound(&self) -> LineHandler {
        let shared = Arc::clone(&self.shared);
        Box::new(move |line| shared.inbound(line))
    }

    fn send(&self, down: TerminalDown) {
        let Some(line) = line_of(down) else {
            return;
        };
        let mut state = lock(&self.shared.state);
        if state.failed.is_some() || state.stopping {
            return;
        }
        state.lines.push_back(Waiting::Line(line));
        drop(state);
        self.shared.ready.notify_one();
    }
}

impl Drop for DeviceTerminals {
    fn drop(&mut self) {
        // The writer ends after the line it is writing, if any; nothing
        // waits for it here, since this runs under the core's lock.
        lock(&self.shared.state).stopping = true;
        self.shared.ready.notify_all();
    }
}

impl TerminalNode for DeviceTerminals {
    fn control(&self, control: TerminalControl) {
        if let TerminalControl::Attach { pane, .. } = &control {
            // Attached again: the pane's flow starts over.
            let mut state = lock(&self.shared.state);
            state.overflowed.remove(pane);
            state.told_failed.remove(pane);
        }
        if let TerminalControl::Forget { pane } = &control {
            lock(&self.shared.state).states.remove(pane);
        }
        self.send(TerminalDown::Control { control });
    }

    fn key(&self, target: KeyTarget, bytes: Vec<u8>, typed_at_unix_ms: u64) {
        let KeyTarget::Pane(pane) = target else {
            // The core opens creation requests for this machine's panes
            // only; a device pane's keys always name it.
            crate::diagnostic!(json!({
                "component": "device_terminal",
                "kind": "terminal.device_request_key",
                "device": self.shared.device,
            }));
            return;
        };
        let refusal = {
            let mut state = lock(&self.shared.state);
            if let Some(reason) = state.failed.clone() {
                if !state.told_failed.insert(pane.clone()) {
                    return;
                }
                Some(("terminal.device_disconnected", reason, None))
            } else if state.overflowed.contains(&pane) {
                // Reported once, when the cap was passed.
                return;
            } else if state.stopping {
                return;
            } else {
                let unsent = state.key_bytes.get(&pane).copied().unwrap_or(0);
                if unsent + bytes.len() > MAX_UNSENT_KEY_BYTES {
                    state.overflowed.insert(pane.clone());
                    let ended = state.states.get(&pane).cloned().map(|mut ended| {
                        ended.state = "ended".to_owned();
                        ended.retry_decision = "manual".to_owned();
                        ended.message = Some(overflow_message());
                        ended
                    });
                    Some(("terminal.device_input_overflow", overflow_message(), ended))
                } else {
                    state.queue_keys(pane.clone(), bytes, typed_at_unix_ms);
                    None
                }
            }
        };
        if let Some((kind, message, ended)) = refusal {
            crate::diagnostic!(json!({
                "component": "device_terminal",
                "kind": kind,
                "device": self.shared.device,
                "pane_id": pane,
                "cap": MAX_UNSENT_KEY_BYTES,
            }));
            if let Some(ended) = ended {
                self.shared.sink.report(
                    &self.shared.device,
                    TerminalReport::State {
                        pane: pane.clone(),
                        state: ended,
                    },
                );
            }
            self.shared.sink.report(
                &self.shared.device,
                TerminalReport::Error {
                    pane,
                    kind: kind.to_owned(),
                    message,
                },
            );
            return;
        }
        self.shared.ready.notify_one();
    }

    fn view(&self, pane: &str, size: GridSize, new_view: bool) {
        self.send(TerminalDown::View {
            pane: pane.to_owned(),
            size,
            new_view,
        });
    }

    fn redraw(&self, pane: &str) {
        self.send(TerminalDown::Redraw {
            pane: pane.to_owned(),
        });
    }
}

fn overflow_message() -> String {
    format!(
        "Keys for this pane did not reach the device fast enough ({} KiB waiting); new keys are refused until the pane is reconnected",
        MAX_UNSENT_KEY_BYTES / 1024
    )
}

impl ProxyShared {
    fn inbound(&self, line: &[u8]) {
        let up = match serde_json::from_slice::<TerminalLine<TerminalUp>>(line) {
            Ok(line) => line.terminal,
            Err(error) => {
                crate::diagnostic!(json!({
                    "component": "device_terminal",
                    "kind": "terminal.up_unreadable",
                    "device": self.device,
                    "class": format!("{:?}", error.classify()),
                    "bytes": line.len(),
                }));
                return;
            }
        };
        match up {
            TerminalUp::Output(output) => match decode_base64(&output.data) {
                Ok(bytes) => self
                    .sink
                    .output(&self.device, &output.pane, &bytes, output.full),
                Err(message) => crate::diagnostic!(json!({
                    "component": "device_terminal",
                    "kind": "terminal.output_unreadable",
                    "device": self.device,
                    "pane_id": output.pane,
                    "message": message,
                })),
            },
            TerminalUp::Report { report } => {
                if let TerminalReport::State { pane, state } = &report {
                    let mut proxy = lock(&self.state);
                    proxy.states.insert(pane.clone(), state.clone());
                    // The pane reads ended until it is attached again; the
                    // device's own word on it waits until then.
                    if proxy.overflowed.contains(pane) {
                        return;
                    }
                }
                self.sink.report(&self.device, report);
            }
            TerminalUp::Diagnostic { mut record } => {
                if let Some(record) = record.as_object_mut() {
                    record.insert("device".to_owned(), json!(self.device));
                }
                crate::diagnostic!(record);
            }
        }
    }
}

fn write_lines(shared: &ProxyShared, link: &dyn LineLink) {
    loop {
        let next = {
            let mut state = lock(&shared.state);
            loop {
                if state.stopping {
                    return;
                }
                if let Some(next) = state.lines.pop_front() {
                    break next;
                }
                state = shared
                    .ready
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        // A run of keys stays counted against its pane until it is written.
        let (keys, line) = match next {
            Waiting::Line(line) => (None, line),
            Waiting::Keys {
                pane,
                bytes,
                typed_at_unix_ms,
            } => {
                let line = line_of(TerminalDown::Key {
                    target: KeyTarget::Pane(pane.clone()),
                    data: encode_base64(&bytes),
                    typed_at_unix_ms,
                })
                .unwrap_or_default();
                (Some((pane, bytes.len())), line)
            }
        };
        let sent = link.send_line(&line);
        let mut state = lock(&shared.state);
        if let Some((pane, written)) = &keys
            && let Some(bytes) = state.key_bytes.get_mut(pane)
        {
            *bytes = bytes.saturating_sub(*written);
            if *bytes == 0 {
                state.key_bytes.remove(pane);
            }
        }
        if let Err(reason) = sent {
            // The keys still waiting were taken and will not be written:
            // each pane's count goes to the log, never to another link.
            let mut unwritten = HashMap::<String, usize>::new();
            if let Some((pane, written)) = keys {
                *unwritten.entry(pane).or_default() += written;
            }
            for waiting in state.lines.drain(..) {
                if let Waiting::Keys { pane, bytes, .. } = waiting {
                    *unwritten.entry(pane).or_default() += bytes.len();
                }
            }
            state.key_bytes.clear();
            state.failed = Some(reason.clone());
            drop(state);
            for (pane, bytes) in unwritten {
                crate::diagnostic!(json!({
                    "component": "device_terminal",
                    "kind": "terminal.keys_unwritten",
                    "device": shared.device,
                    "pane_id": pane,
                    "key_bytes": bytes,
                    "reason": reason,
                }));
            }
            return;
        }
    }
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
