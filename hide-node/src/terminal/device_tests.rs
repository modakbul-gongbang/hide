use std::sync::mpsc::{Receiver, Sender, channel};

use hide_host::serve::Terminals;
use hide_node_link::terminal::GridSize;

use super::super::tests::{FakeAttacher, Opened, WAIT};
use super::*;

const SIZE: GridSize = GridSize { rows: 24, cols: 80 };

fn up(line: &[u8]) -> TerminalUp {
    serde_json::from_slice::<TerminalLine<TerminalUp>>(line)
        .unwrap()
        .terminal
}

fn down(control: TerminalControl) -> Vec<u8> {
    line_of(TerminalDown::Control { control }).unwrap()
}

fn key_line(pane: &str, bytes: &[u8]) -> Vec<u8> {
    line_of(TerminalDown::Key {
        target: KeyTarget::Pane(pane.to_owned()),
        data: encode_base64(bytes),
        typed_at_unix_ms: 1,
    })
    .unwrap()
}

#[test]
fn reports_go_up_first_and_panes_take_turns() {
    let uplink = Uplink::default();
    for index in 0..50 {
        uplink.push_output("w1:p1", format!("flood {index}").as_bytes(), false);
    }
    uplink.push_output("w1:p2", b"quiet", false);
    if let Some(line) = line_of(TerminalUp::Report {
        report: TerminalReport::FirstFrame {
            pane: "w1:p2".into(),
            generation: 1,
        },
    }) {
        uplink.push_report(line);
    }
    let (first, _) = uplink.next().unwrap();
    assert!(matches!(up(&first.unwrap()), TerminalUp::Report { .. }));
    let mut panes = Vec::new();
    for _ in 0..3 {
        let (line, _) = uplink.next().unwrap();
        let TerminalUp::Output(output) = up(&line.unwrap()) else {
            panic!("output");
        };
        panes.push(output.pane);
    }
    assert_eq!(panes, ["w1:p1", "w1:p2", "w1:p1"]);
}

#[test]
fn a_pane_past_its_unsent_output_loses_it_and_is_drawn_again_from_a_full_frame() {
    let uplink = Uplink::default();
    uplink.push_output("w1:p2", b"neighbour", false);
    let chunk = vec![b'x'; 64 * 1024];
    for _ in 0..20 {
        uplink.push_output("w1:p1", &chunk, false);
    }
    // The flooding pane's queue is gone and it is asked to draw again; its
    // neighbour's output is untouched.
    let (line, redraws) = uplink.next().unwrap();
    assert_eq!(redraws, ["w1:p1"]);
    let TerminalUp::Output(output) = up(&line.unwrap()) else {
        panic!("output");
    };
    assert_eq!(output.pane, "w1:p2");
    // Until the full frame, nothing of the dropped pane goes up.
    uplink.push_output("w1:p1", b"tail", false);
    uplink.push_output("w1:p1", b"\x1bcwhole", true);
    let (line, redraws) = uplink.next().unwrap();
    assert!(redraws.is_empty());
    let TerminalUp::Output(output) = up(&line.unwrap()) else {
        panic!("output");
    };
    assert!(output.full);
    assert_eq!(decode_base64(&output.data).unwrap(), b"\x1bcwhole");
    let state = lock(&uplink.state);
    assert!(
        state
            .panes
            .values()
            .all(|pane| pane.bytes <= MAX_UNSENT_OUTPUT_BYTES)
    );
}

fn node(socket_seen: Sender<String>) -> (NodeTerminals, Receiver<Opened>) {
    let (opened_tx, opened) = channel();
    let attacher = Arc::new(FakeAttacher {
        opened: Mutex::new(opened_tx),
        refuse: Mutex::new(None),
    });
    let socket_seen = Mutex::new(socket_seen);
    let node = NodeTerminals::with_attacher(Box::new(move |socket: &str| {
        socket_seen.lock().unwrap().send(socket.to_owned()).unwrap();
        struct Shared(Arc<FakeAttacher>);
        impl super::super::Attacher for Shared {
            fn open(
                &self,
                pane: &str,
                mode: super::super::Mode,
                rows: u16,
                cols: u16,
            ) -> Result<super::super::SessionParts, String> {
                self.0.open(pane, mode, rows, cols)
            }
        }
        Box::new(Shared(Arc::clone(&attacher)))
    }));
    (node, opened)
}

fn attach(node: &NodeTerminals, opened: &Receiver<Opened>, pane: &str) -> Opened {
    node.line(
        down(TerminalControl::Attach {
            pane: pane.into(),
            size: Some(SIZE),
            manual: false,
        })
        .trim_ascii_end(),
    );
    opened.recv_timeout(WAIT).expect("the pane attached")
}

/// D-18, B18: one pane flooding its output holds neither another pane's
/// keys nor its frames.
#[test]
fn a_flooding_pane_holds_back_neither_another_panes_keys_nor_its_frames() {
    let (socket_tx, socket) = channel();
    let (node, opened) = node(socket_tx);
    node.start("/tmp/herdr.sock").unwrap();
    assert_eq!(socket.recv().unwrap(), "/tmp/herdr.sock");
    let flooding = attach(&node, &opened, "w1:p1");
    let quiet = attach(&node, &opened, "w1:p2");
    flooding.frame(SIZE, true, b"start");
    quiet.frame(SIZE, true, b"start");
    // Nobody reads the link: the flooding pane's output piles up past its
    // cap while the other pane's keys still reach its writer.
    for _ in 0..40 {
        flooding.frame(SIZE, false, &[b'y'; 48 * 1024]);
    }
    node.line(key_line("w1:p2", b"typed").trim_ascii_end());
    assert_eq!(quiet.next_input(), b"typed");
    quiet.frame(SIZE, false, b"echo");
    // The quiet pane's echo goes up within a few lines of the reader
    // coming back, not behind the flood.
    let mut seen = Vec::new();
    let deadline = std::time::Instant::now() + WAIT;
    while std::time::Instant::now() < deadline {
        let line = node.next_up().unwrap();
        if let TerminalUp::Output(output) = up(&line) {
            let echo =
                output.pane == "w1:p2" && decode_base64(&output.data).unwrap().ends_with(b"echo");
            seen.push(output.pane);
            if echo {
                break;
            }
        }
    }
    let flood_lines_before_echo = seen.iter().filter(|pane| *pane == "w1:p1").count();
    assert!(
        seen.last().is_some_and(|pane| pane == "w1:p2"),
        "the quiet pane's echo went up: {seen:?}"
    );
    assert!(
        flood_lines_before_echo <= 3,
        "the echo waited behind {flood_lines_before_echo} flood lines"
    );
    node.stop();
    assert!(node.next_up().is_none());
}

/// A link that takes lines when the test lets it.
#[derive(Default)]
struct GatedLink {
    open: Mutex<bool>,
    opened: Condvar,
    written: Mutex<Vec<Vec<u8>>>,
    fail: Mutex<Option<String>>,
}

impl GatedLink {
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.opened.notify_all();
    }

    fn lines(&self) -> Vec<TerminalDown> {
        self.written
            .lock()
            .unwrap()
            .iter()
            .map(|line| {
                serde_json::from_slice::<TerminalLine<TerminalDown>>(line)
                    .unwrap()
                    .terminal
            })
            .collect()
    }
}

impl LineLink for GatedLink {
    fn send_line(&self, line: &[u8]) -> Result<(), String> {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.opened.wait(open).unwrap();
        }
        if let Some(reason) = self.fail.lock().unwrap().clone() {
            return Err(reason);
        }
        self.written.lock().unwrap().push(line.to_vec());
        Ok(())
    }
}

/// A device, pane, the bytes, and whether they draw the whole screen.
type HeardOutput = (String, String, Vec<u8>, bool);

#[derive(Default)]
struct Heard {
    output: Mutex<Vec<HeardOutput>>,
    reports: Mutex<Vec<(String, TerminalReport)>>,
}

impl DeviceSink for Heard {
    fn output(&self, device: &str, pane: &str, bytes: &[u8], full: bool) {
        self.output
            .lock()
            .unwrap()
            .push((device.into(), pane.into(), bytes.to_vec(), full));
    }

    fn report(&self, device: &str, report: TerminalReport) {
        self.reports.lock().unwrap().push((device.into(), report));
    }
}

fn proxy() -> (DeviceTerminals, Arc<GatedLink>, Arc<Heard>) {
    let link = Arc::new(GatedLink::default());
    let heard = Arc::new(Heard::default());
    let proxy = DeviceTerminals::start(
        "mini",
        Arc::clone(&link) as Arc<dyn LineLink>,
        Arc::clone(&heard) as Arc<dyn DeviceSink>,
    )
    .unwrap();
    (proxy, link, heard)
}

fn wait_for(mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + WAIT;
    while !done() {
        assert!(std::time::Instant::now() < deadline, "timed out");
        std::thread::yield_now();
    }
}

fn controlling() -> PaneTerminalState {
    PaneTerminalState {
        state: "controlling".into(),
        mode: Some("control".into()),
        generation: 4,
        attempt: 1,
        message: None,
        exit_category: None,
        retry_decision: "none".into(),
        last_attempt_at_unix_ms: None,
    }
}

#[test]
fn a_pane_past_its_unsent_keys_reads_ended_while_other_panes_keys_go_on() {
    let (proxy, link, heard) = proxy();
    let inbound = proxy.inbound();
    inbound(
        &line_of(TerminalUp::Report {
            report: TerminalReport::State {
                pane: "w1:p1".into(),
                state: controlling(),
            },
        })
        .unwrap(),
    );
    // The link takes nothing: 256 KiB of one pane's keys fill its flow.
    let chunk = vec![b'k'; 16 * 1024];
    let mut taken = 0;
    while heard
        .reports
        .lock()
        .unwrap()
        .iter()
        .all(|(_, report)| !matches!(report, TerminalReport::Error { .. }))
    {
        proxy.key(KeyTarget::Pane("w1:p1".into()), chunk.clone(), 1);
        taken += 1;
        assert!(taken < 64, "the cap was never reached");
    }
    let reports = heard.reports.lock().unwrap().clone();
    let ended = reports
        .iter()
        .rev()
        .find_map(|(_, report)| match report {
            TerminalReport::State { pane, state } if pane == "w1:p1" => Some(state.clone()),
            _ => None,
        })
        .expect("the pane reads ended");
    assert_eq!(ended.state, "ended");
    assert_eq!(ended.generation, 4);
    // Refused once; later keys add nothing and send nothing.
    proxy.key(KeyTarget::Pane("w1:p1".into()), b"late".to_vec(), 1);
    assert_eq!(heard.reports.lock().unwrap().len(), reports.len());
    // The device's own word on the pane waits until it is attached again.
    inbound(
        &line_of(TerminalUp::Report {
            report: TerminalReport::State {
                pane: "w1:p1".into(),
                state: controlling(),
            },
        })
        .unwrap(),
    );
    assert_eq!(heard.reports.lock().unwrap().len(), reports.len());
    // Another pane's keys still go.
    proxy.key(KeyTarget::Pane("w1:p2".into()), b"other".to_vec(), 1);
    link.release();
    wait_for(|| {
        link.lines().iter().any(|line| {
            matches!(line, TerminalDown::Key { target: KeyTarget::Pane(pane), .. } if pane == "w1:p2")
        })
    });
    // The keys it took before the cap were written, in order, before it.
    let lines = link.lines();
    let first_keys = lines
        .iter()
        .take_while(|line| {
            matches!(line, TerminalDown::Key { target: KeyTarget::Pane(pane), .. } if pane == "w1:p1")
        })
        .count();
    assert_eq!(first_keys, taken - 1);
    // Attaching again lets keys through.
    proxy.control(TerminalControl::Attach {
        pane: "w1:p1".into(),
        size: None,
        manual: true,
    });
    proxy.key(KeyTarget::Pane("w1:p1".into()), b"again".to_vec(), 1);
    wait_for(|| {
        link.lines().iter().any(|line| {
            matches!(line, TerminalDown::Key { data, .. } if decode_base64(data).unwrap() == b"again")
        })
    });
}

/// The cap counts keys, not the frames they travel in: 10,000 one-byte keys
/// typed while the link takes nothing stay far under it, and go down in
/// order as a few lines, the waiting run carrying its last key's time.
#[test]
fn keys_waiting_on_a_stalled_link_count_as_keys_and_go_down_as_one_run() {
    let (proxy, link, heard) = proxy();
    let typed: Vec<u8> = (0..10_000).map(|index| b'a' + (index % 26) as u8).collect();
    for (index, key) in typed.iter().enumerate() {
        proxy.key(
            KeyTarget::Pane("w1:p1".into()),
            vec![*key],
            1_000 + index as u64,
        );
    }
    assert!(
        heard.reports.lock().unwrap().is_empty(),
        "no key was refused"
    );
    link.release();
    let joined = || {
        link.lines()
            .iter()
            .filter_map(|line| match line {
                TerminalDown::Key { data, .. } => Some(decode_base64(data).unwrap()),
                _ => None,
            })
            .flatten()
            .collect::<Vec<u8>>()
    };
    wait_for(|| joined().len() == typed.len());
    assert_eq!(joined(), typed);
    let lines = link.lines();
    // The first key may already have been taken alone when the link stalled.
    assert!(
        lines.len() <= 2,
        "{} lines for one run of keys",
        lines.len()
    );
    let TerminalDown::Key {
        typed_at_unix_ms, ..
    } = lines.last().unwrap()
    else {
        panic!("a key line");
    };
    assert_eq!(*typed_at_unix_ms, 1_000 + 9_999);
}

#[test]
fn a_failed_link_refuses_keys_once_per_pane_and_never_keeps_them() {
    let (proxy, link, heard) = proxy();
    *link.fail.lock().unwrap() = Some("the link closed".into());
    proxy.key(KeyTarget::Pane("w1:p1".into()), b"lost".to_vec(), 1);
    proxy.key(KeyTarget::Pane("w1:p1".into()), b"queued".to_vec(), 1);
    link.release();
    wait_for(|| lock(&proxy.shared.state).failed.is_some());
    assert!(lock(&proxy.shared.state).lines.is_empty());
    proxy.key(KeyTarget::Pane("w1:p1".into()), b"after".to_vec(), 1);
    proxy.key(KeyTarget::Pane("w1:p1".into()), b"after".to_vec(), 1);
    let errors = heard
        .reports
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, report)| {
            matches!(report, TerminalReport::Error { kind, .. } if kind == "terminal.device_disconnected")
        })
        .count();
    assert_eq!(errors, 1);
    assert!(link.lines().is_empty());
}

#[test]
fn output_from_the_device_reaches_the_hub_decoded_and_named_by_device() {
    let (proxy, _link, heard) = proxy();
    let inbound = proxy.inbound();
    inbound(
        &line_of(TerminalUp::Output(TerminalOutput {
            pane: "w1:p1".into(),
            data: encode_base64(b"\x1bcscreen"),
            full: true,
        }))
        .unwrap(),
    );
    assert_eq!(
        heard.output.lock().unwrap().as_slice(),
        [(
            "mini".to_owned(),
            "w1:p1".to_owned(),
            b"\x1bcscreen".to_vec(),
            true
        )]
    );
}
