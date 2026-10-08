//! What every node says about its terminals reaches the runtime here: a
//! node's service sends a report from its own threads, sometimes while the
//! runtime lock is held by the control that caused it, so sending never
//! waits; one thread takes what has arrived into the runtime in a batch and
//! announces once (PRD core-host-node-terminal D-05).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;

use hide_node_link::terminal::{ReportSink, TerminalReport};

use crate::handle::ChangeNotifier;
use crate::runtime::Runtime;

/// Reports waiting for the runtime. Every report is a pane's state change, a
/// coalesced input fact or an armed frame, so this many waiting means the
/// runtime lock has been held for a long time; past it a report is dropped
/// and logged (rule 15).
const WAITING_LIMIT: usize = 16_384;

enum Message {
    Report(TerminalReport),
    Stop,
}

/// The sending half, given to the node services and the router.
pub struct ReportChannel {
    sender: mpsc::Sender<Message>,
    waiting: Arc<AtomicUsize>,
}

/// The receiving half, given to [`crate::Core::create`].
pub struct TerminalReports {
    receiver: mpsc::Receiver<Message>,
    sender: mpsc::Sender<Message>,
    waiting: Arc<AtomicUsize>,
}

/// A connected pair: the routes report into the first, the core reads the
/// second.
pub fn terminal_reports() -> (ReportChannel, TerminalReports) {
    let (sender, receiver) = mpsc::channel();
    let waiting = Arc::new(AtomicUsize::new(0));
    (
        ReportChannel {
            sender: sender.clone(),
            waiting: Arc::clone(&waiting),
        },
        TerminalReports {
            receiver,
            sender,
            waiting,
        },
    )
}

impl ReportSink for ReportChannel {
    fn report(&self, report: TerminalReport) {
        if self.waiting.fetch_add(1, Ordering::AcqRel) >= WAITING_LIMIT {
            self.waiting.fetch_sub(1, Ordering::AcqRel);
            crate::diagnostic!(serde_json::json!({
                "kind": "terminal.report_dropped",
                "pane_id": report_pane(&report),
                "waiting": WAITING_LIMIT,
            }));
            return;
        }
        if self.sender.send(Message::Report(report)).is_err() {
            // The core has stopped; nothing reads terminal state any more.
            self.waiting.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

fn report_pane(report: &TerminalReport) -> Option<String> {
    let mut report = report.clone();
    report.pane_mut().map(|pane| pane.clone())
}

/// The thread that applies reports, ended and joined with the core.
pub(crate) struct ReportPump {
    stop: mpsc::Sender<Message>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ReportPump {
    pub(crate) fn spawn(
        reports: TerminalReports,
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) -> std::io::Result<Self> {
        let TerminalReports {
            receiver,
            sender,
            waiting,
        } = reports;
        let worker = thread::Builder::new()
            .name("terminal-reports".into())
            .spawn(move || {
                while let Ok(first) = receiver.recv() {
                    let mut batch = Vec::new();
                    let mut stop = false;
                    for message in std::iter::once(first).chain(receiver.try_iter()) {
                        match message {
                            Message::Report(report) => batch.push(report),
                            Message::Stop => stop = true,
                        }
                    }
                    waiting.fetch_sub(batch.len(), Ordering::AcqRel);
                    let Some(runtime) = runtime.upgrade() else {
                        break;
                    };
                    if !batch.is_empty()
                        && runtime
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .ingest_terminal_reports(batch)
                    {
                        notifier.notify();
                    }
                    if stop {
                        break;
                    }
                }
            })?;
        Ok(Self {
            stop: sender,
            worker: Some(worker),
        })
    }
}

impl Drop for ReportPump {
    fn drop(&mut self) {
        let _ = self.stop.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
