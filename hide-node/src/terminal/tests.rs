use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};

use super::*;

const WAIT: Duration = Duration::from_secs(5);

/// A session's stdout as the test writes it.
struct LineReader {
    lines: Receiver<String>,
    pending: Vec<u8>,
}

impl Read for LineReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.lines.recv() {
                Ok(line) => {
                    self.pending = line.into_bytes();
                    self.pending.push(b'\n');
                }
                Err(_) => return Ok(0),
            }
        }
        let count = buffer.len().min(self.pending.len());
        buffer[..count].copy_from_slice(&self.pending[..count]);
        self.pending.drain(..count);
        Ok(count)
    }
}

/// A session's stdin as the test reads it, one line per write.
struct LineWriter {
    lines: Sender<serde_json::Value>,
    pending: Vec<u8>,
}

impl Write for LineWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line = self.pending.drain(..=end).collect::<Vec<_>>();
            let value = serde_json::from_slice(&line[..line.len() - 1]).unwrap();
            self.lines
                .send(value)
                .map_err(|_| std::io::Error::other("closed"))?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One session the fake attacher opened.
struct Opened {
    pane: String,
    mode: Mode,
    size: GridSize,
    output: Sender<String>,
    input: Receiver<serde_json::Value>,
}

impl Opened {
    fn frame(&self, size: GridSize, full: bool, bytes: &[u8]) {
        self.output
            .send(
                json!({
                    "type": "terminal.frame", "seq": 1, "encoding": "ansi",
                    "width": size.cols, "height": size.rows, "full": full,
                    "bytes": protocol::encode_base64(bytes),
                })
                .to_string(),
            )
            .unwrap();
    }

    fn close(&self, reason: Option<&str>) {
        self.output
            .send(json!({"type": "terminal.closed", "reason": reason}).to_string())
            .unwrap();
    }

    /// The next line the session was asked to write.
    fn next_line(&self) -> serde_json::Value {
        self.input.recv_timeout(WAIT).expect("a line was written")
    }

    fn next_input(&self) -> Vec<u8> {
        loop {
            let line = self.next_line();
            if line["type"] == "terminal.input" {
                return protocol::decode_base64(line["bytes"].as_str().unwrap()).unwrap();
            }
        }
    }
}

struct FakeAttacher {
    opened: Mutex<Sender<Opened>>,
    refuse: Mutex<Option<String>>,
}

impl Attacher for FakeAttacher {
    fn open(&self, pane: &str, mode: Mode, rows: u16, cols: u16) -> Result<SessionParts, String> {
        if let Some(reason) = self.refuse.lock().unwrap().clone() {
            return Err(reason);
        }
        let (output, lines) = channel();
        let (written, input) = channel();
        self.opened
            .lock()
            .unwrap()
            .send(Opened {
                pane: pane.to_owned(),
                mode,
                size: GridSize { rows, cols },
                output,
                input,
            })
            .unwrap();
        Ok(SessionParts {
            reader: Box::new(LineReader {
                lines,
                pending: Vec::new(),
            }),
            writer: Some(Box::new(LineWriter {
                lines: written,
                pending: Vec::new(),
            })),
            cleanup: Cleanup::None,
        })
    }
}

#[derive(Default)]
struct Outputs {
    written: Mutex<Vec<(String, Vec<u8>, bool)>>,
    forgotten: Mutex<Vec<String>>,
}

impl OutputSink for Outputs {
    fn output(&self, pane: &str, bytes: &[u8], full: bool) {
        self.written
            .lock()
            .unwrap()
            .push((pane.to_owned(), bytes.to_vec(), full));
    }

    fn forget(&self, pane: &str) {
        self.forgotten.lock().unwrap().push(pane.to_owned());
    }
}

struct Reports(Mutex<Sender<TerminalReport>>);

impl ReportSink for Reports {
    fn report(&self, report: TerminalReport) {
        let _ = self.0.lock().unwrap().send(report);
    }
}

struct Harness {
    service: Service,
    opened: Receiver<Opened>,
    reports: Receiver<TerminalReport>,
    outputs: Arc<Outputs>,
    refuse: Arc<FakeAttacher>,
}

fn harness(policy: RetryPolicy) -> Harness {
    let (opened_tx, opened) = channel();
    let (reports_tx, reports) = channel();
    let attacher = Arc::new(FakeAttacher {
        opened: Mutex::new(opened_tx),
        refuse: Mutex::new(None),
    });
    let outputs = Arc::new(Outputs::default());
    struct Shared(Arc<FakeAttacher>);
    impl Attacher for Shared {
        fn open(
            &self,
            pane: &str,
            mode: Mode,
            rows: u16,
            cols: u16,
        ) -> Result<SessionParts, String> {
            self.0.open(pane, mode, rows, cols)
        }
    }
    let service = Service::start(
        Box::new(Shared(Arc::clone(&attacher))),
        Arc::clone(&outputs) as Arc<dyn OutputSink>,
        Arc::new(Reports(Mutex::new(reports_tx))),
        policy,
    )
    .unwrap();
    Harness {
        service,
        opened,
        reports,
        outputs,
        refuse: attacher,
    }
}

const SIZE: GridSize = GridSize { rows: 24, cols: 80 };

impl Harness {
    fn attach(&self, pane: &str, size: Option<GridSize>) {
        self.service.control(TerminalControl::Attach {
            pane: pane.into(),
            size,
            manual: false,
        });
    }

    fn opened(&self) -> Opened {
        self.opened.recv_timeout(WAIT).expect("a session opened")
    }

    /// Reports until one matches, failing after the wait.
    fn report_where(&self, mut matches: impl FnMut(&TerminalReport) -> bool) -> TerminalReport {
        loop {
            let report = self.reports.recv_timeout(WAIT).expect("a report");
            if matches(&report) {
                return report;
            }
        }
    }

    fn state(&self, pane: &str, wanted: &str) -> PaneTerminalState {
        match self.report_where(|report| {
            matches!(report, TerminalReport::State { pane: p, state } if p == pane && state.state == wanted)
        }) {
            TerminalReport::State { state, .. } => state,
            _ => unreachable!(),
        }
    }

    /// Attaches `pane` in control and returns its session.
    fn controlling(&self, pane: &str) -> Opened {
        self.attach(pane, Some(SIZE));
        let opened = self.opened();
        self.state(pane, "controlling");
        opened
    }

    fn written(&self) -> Vec<(String, Vec<u8>, bool)> {
        self.outputs.written.lock().unwrap().clone()
    }

    fn wait_written(&self, count: usize) -> Vec<(String, Vec<u8>, bool)> {
        let started = Instant::now();
        loop {
            let written = self.written();
            if written.len() >= count {
                return written;
            }
            assert!(
                started.elapsed() < WAIT,
                "output did not arrive: {written:?}"
            );
            // The reader thread delivers on its own; wait for its report.
            let _ = self.reports.recv_timeout(Duration::from_millis(20));
        }
    }

    fn key(&self, pane: &str, bytes: &[u8]) {
        self.service
            .key(KeyTarget::Pane(pane.into()), bytes.to_vec(), unix_ms());
    }
}

#[test]
fn an_attach_without_a_size_waits_for_one_then_attaches_at_it() {
    let harness = harness(RetryPolicy::Automatic);
    harness.attach("w1:p1", None);
    harness.state("w1:p1", "waiting_size");
    harness.service.control(TerminalControl::Resize {
        pane: "w1:p1".into(),
        size: GridSize {
            rows: 30,
            cols: 100,
        },
        force: false,
    });
    let opened = harness.opened();
    assert_eq!(
        opened.size,
        GridSize {
            rows: 30,
            cols: 100
        }
    );
    assert_eq!(opened.mode, Mode::Control);
    harness.state("w1:p1", "controlling");
    // The settled size is sent to the session it opened.
    assert_eq!(opened.next_line()["type"], "terminal.resize");
}

#[test]
fn a_key_reaches_the_writer_and_an_enter_is_reported_at_once() {
    let harness = harness(RetryPolicy::Automatic);
    let opened = harness.controlling("w1:p1");
    harness.key("w1:p1", b"ls");
    assert_eq!(opened.next_input(), b"ls");
    let first = harness.report_where(|report| matches!(report, TerminalReport::Input { .. }));
    assert!(matches!(
        first,
        TerminalReport::Input {
            submitted: false,
            focus: true,
            ..
        }
    ));
    harness.key("w1:p1", b"\r");
    assert_eq!(opened.next_input(), b"\r");
    let enter = harness.report_where(|report| matches!(report, TerminalReport::Input { .. }));
    assert!(matches!(
        enter,
        TerminalReport::Input {
            submitted: true,
            ..
        }
    ));
}

#[test]
fn a_pasted_return_or_an_escaped_one_is_not_a_submit() {
    let harness = harness(RetryPolicy::Automatic);
    let opened = harness.controlling("w1:p1");
    harness.key("w1:p1", b"\x1b[200~one\rtwo\x1b[201~");
    opened.next_input();
    let pasted = harness.report_where(|report| matches!(report, TerminalReport::Input { .. }));
    assert!(matches!(
        pasted,
        TerminalReport::Input {
            submitted: false,
            ..
        }
    ));
    // Inside the window an escaped return is still not reported as one.
    harness.key("w1:p1", b"\x1b\r");
    opened.next_input();
    harness.key("w1:p1", b"\r");
    opened.next_input();
    let enter = harness.report_where(|report| matches!(report, TerminalReport::Input { .. }));
    assert!(matches!(
        enter,
        TerminalReport::Input {
            submitted: true,
            ..
        }
    ));
}

#[test]
fn keys_typed_while_the_session_opens_are_written_first_in_order() {
    let harness = harness(RetryPolicy::Automatic);
    harness.attach("w1:p1", Some(SIZE));
    harness.key("w1:p1", b"one");
    harness.key("w1:p1", b"two");
    let opened = harness.opened();
    assert_eq!(opened.next_input(), b"onetwo");
    harness.key("w1:p1", b"three");
    assert_eq!(opened.next_input(), b"three");
}

#[test]
fn a_frame_at_a_foreign_grid_waits_for_a_full_frame_at_the_view_grid() {
    let harness = harness(RetryPolicy::Automatic);
    let opened = harness.controlling("w1:p1");
    harness.service.view("w1:p1", SIZE, true);
    opened.frame(GridSize { rows: 10, cols: 10 }, true, b"foreign");
    opened.frame(SIZE, false, b"partial");
    opened.frame(SIZE, true, b"whole");
    opened.frame(SIZE, false, b"next");
    let written = harness.wait_written(2);
    assert_eq!(written[0].1, b"\x1bcwhole".to_vec());
    assert!(written[0].2, "a reset frame draws the whole screen");
    assert_eq!(written[1].1, b"next".to_vec());
    assert!(!written[1].2);
}

#[test]
fn a_pane_another_client_controls_is_observed_once() {
    let harness = harness(RetryPolicy::Automatic);
    let opened = harness.controlling("w1:p1");
    opened.close(Some("terminal attach taken over"));
    let observer = harness.opened();
    assert_eq!(observer.mode, Mode::Observe);
    let state = harness.state("w1:p1", "observing");
    assert_eq!(state.mode.as_deref(), Some("observe"));
    harness.key("w1:p1", b"x");
    harness.report_where(|report| {
        matches!(report, TerminalReport::Error { kind, .. } if kind == "terminal.read_only")
    });
}

#[test]
fn an_ended_session_says_so_on_the_pane_and_a_closing_one_does_not() {
    let harness = harness(RetryPolicy::Automatic);
    let opened = harness.controlling("w1:p1");
    opened.close(Some("pane exited"));
    let state = harness.state("w1:p1", "ended");
    assert_eq!(state.exit_category.as_deref(), Some("terminal_closed"));
    assert!(state.message.unwrap().contains("Retrying in 5 seconds"));
    let written = harness.wait_written(1);
    assert_eq!(written[0].1, b"\r\n[pane exited]\r\n".to_vec());

    let second = harness.controlling("w1:p2");
    harness.service.control(TerminalControl::Closing {
        pane: "w1:p2".into(),
        closing: true,
    });
    harness.key("w1:p2", b"x");
    harness.report_where(|report| {
        matches!(report, TerminalReport::Error { kind, .. } if kind == "terminal.close_pending")
    });
    second.close(None);
    harness.state("w1:p2", "closing");
    assert_eq!(
        harness.written().len(),
        1,
        "nothing is drawn over a closing pane"
    );
}

#[test]
fn a_released_pane_ends_its_session_and_its_output_is_forgotten() {
    let harness = harness(RetryPolicy::Automatic);
    let _opened = harness.controlling("w1:p1");
    harness.service.control(TerminalControl::Release {
        pane: "w1:p1".into(),
        message: "detached".into(),
    });
    let state = harness.state("w1:p1", "released");
    assert_eq!(state.retry_decision, "on_next_visit");
    assert_eq!(
        harness.outputs.forgotten.lock().unwrap().as_slice(),
        ["w1:p1"]
    );
    // It attaches again when it is shown again.
    harness.attach("w1:p1", Some(SIZE));
    harness.opened();
}

#[test]
fn keys_for_a_sleeping_pane_are_dropped_until_it_wakes() {
    let harness = harness(RetryPolicy::Automatic);
    let opened = harness.controlling("w1:p1");
    harness.service.control(TerminalControl::Asleep {
        pane: "w1:p1".into(),
        asleep: true,
    });
    harness.key("w1:p1", b"lost");
    harness.service.control(TerminalControl::Asleep {
        pane: "w1:p1".into(),
        asleep: false,
    });
    harness.key("w1:p1", b"kept");
    assert_eq!(opened.next_input(), b"kept");
}

#[test]
fn keys_sent_before_the_core_names_their_creation_reach_the_pane_it_makes() {
    let harness = harness(RetryPolicy::Automatic);
    harness.service.key(
        KeyTarget::Request("req-1".into()),
        b"echo".to_vec(),
        unix_ms(),
    );
    harness.service.control(TerminalControl::RequestOpen {
        request: "req-1".into(),
    });
    harness.service.key(
        KeyTarget::Request("req-1".into()),
        b" hi\r".to_vec(),
        unix_ms(),
    );
    harness.service.control(TerminalControl::RequestResolve {
        request: "req-1".into(),
        pane: "w1:p5".into(),
    });
    harness.attach("w1:p5", Some(SIZE));
    let opened = harness.opened();
    assert_eq!(opened.pane, "w1:p5");
    assert_eq!(opened.next_input(), b"echo hi\r");
}

#[test]
fn a_paste_is_written_before_the_keys_typed_behind_it() {
    let harness = harness(RetryPolicy::Automatic);
    let opened = harness.controlling("w1:p1");
    let generation = harness.service.shared.lock().panes["w1:p1"]
        .current_generation
        .unwrap();
    harness.service.control(TerminalControl::AttachmentHold {
        pane: "w1:p1".into(),
        intent: "i1".into(),
    });
    harness.key("w1:p1", b"\r");
    harness.service.control(TerminalControl::AttachmentDeliver {
        intent: "i1".into(),
        generation,
        paste: protocol::encode_base64(b"'/tmp/a.png' "),
    });
    assert_eq!(opened.next_input(), b"'/tmp/a.png' \r");
    harness.report_where(|report| {
        matches!(
            report,
            TerminalReport::AttachmentDelivered { written: true, .. }
        )
    });
}

#[test]
fn a_failed_attach_is_retried_while_shown_and_reported_with_its_reason() {
    let harness = harness(RetryPolicy::Automatic);
    *harness.refuse.refuse.lock().unwrap() = Some("no herdr".into());
    harness.service.control(TerminalControl::Shown {
        panes: vec!["w1:p1".into()],
    });
    harness.attach("w1:p1", Some(SIZE));
    let state = harness.state("w1:p1", "unavailable");
    assert_eq!(state.exit_category.as_deref(), Some("spawn_failed"));
    assert_eq!(state.retry_decision, "automatic_bounded");
    let due = harness.service.shared.lock().panes["w1:p1"]
        .recovery
        .as_ref()
        .and_then(|recovery| recovery.due)
        .unwrap();
    *harness.refuse.refuse.lock().unwrap() = None;
    harness
        .service
        .shared
        .run(|inner, shared| inner.tick(shared, due));
    harness.opened();
    let state = harness.state("w1:p1", "controlling");
    assert_eq!(state.attempt, 2);
}

#[test]
fn a_device_node_leaves_a_failed_attach_to_the_operator() {
    let harness = harness(RetryPolicy::Manual);
    *harness.refuse.refuse.lock().unwrap() = Some("no herdr".into());
    harness.attach("w1:p1", Some(SIZE));
    let state = harness.state("w1:p1", "unavailable");
    assert_eq!(state.retry_decision, "manual");
    assert!(
        harness.service.shared.lock().panes["w1:p1"]
            .recovery
            .is_none()
    );
}

#[test]
fn a_node_keeps_at_most_its_cap_of_panes_attached() {
    let harness = harness(RetryPolicy::Automatic);
    // The sessions stay open for as long as the test holds them.
    let mut sessions = Vec::new();
    for index in 0..MAX_ATTACHED_PANES {
        harness.attach(&format!("p{index}"), Some(SIZE));
        sessions.push(harness.opened());
    }
    harness.attach("over", Some(SIZE));
    let state = harness.state("over", "unavailable");
    assert_eq!(state.exit_category.as_deref(), Some("attach_limit"));
    // Another pane leaving makes room; the refused one attaches when it is
    // asked for again.
    harness
        .service
        .control(TerminalControl::Forget { pane: "p0".into() });
    harness.attach("over", Some(SIZE));
    assert_eq!(harness.opened().pane, "over");
}

#[test]
fn an_idle_service_has_nothing_due() {
    let harness = harness(RetryPolicy::Automatic);
    let _opened = harness.controlling("w1:p1");
    assert_eq!(harness.service.shared.lock().next_deadline(), None);
}
