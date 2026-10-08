//! The official remote terminal session fixture probe, against a real
//! device's owned fixture pane. It moved here from the core
//! (`herdr-core/src/remote.rs`) with the terminal path (PRD
//! core-host-node-terminal D-10): the session is now the device's node's,
//! driven through the node link the way the screen's router drives it, and
//! the probe keeps its fixture variables, markers and assertions.
//!
//! It connects with the operator's SSH configuration under `HOME` and
//! installs this build's node on the device under the folders
//! `HERDR_TEST_REMOTE_HELPER_ROOT` and `HERDR_TEST_REMOTE_CLI_DIR` name, so
//! run it only against an isolated device account: a private sshd with its
//! own HOME and state, never an operator's. Both are absolute: the install
//! spells `~/` from the SFTP home, which is the account's own home even
//! where the private sshd gives commands another `HOME`, so a `~/` folder
//! lands in the operator's real one. The fixture pane must be fresh: the
//! scroll to the top of its history has to land on the probe's own first
//! line.

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hide_node::terminal::device::DeviceSink;
use hide_node_link::device::{DeviceConnector, HOST_CONSENT_CONTRACT, HostConsent};
use hide_node_link::terminal::{GridSize, KeyTarget, TerminalControl, TerminalReport};
use serde_json::json;

const SSH_OPERATION_TIMEOUT: Duration = Duration::from_secs(15);
const TAIL_MARKER: &str = "HERDR_IDE_SCROLL_080";
const HISTORY_MARKER: &str = "HERDR_IDE_SCROLL_001";

/// What the probe heard from the device, and who waits on it. Herdr sends
/// a repaint as the cells that changed, so the markers are read from the
/// screen those bytes draw, at the view's grid, not from the bytes.
struct Heard {
    pane: String,
    screen: Mutex<vt100::Parser>,
    full_frame_seen: Mutex<bool>,
    scrolled: Mutex<bool>,
    states: Mutex<Vec<String>>,
    progress: Mutex<Option<Sender<&'static str>>>,
}

impl Heard {
    fn tell(&self, what: &'static str) {
        if let Some(progress) = self.progress.lock().unwrap().as_ref() {
            let _ = progress.send(what);
        }
    }
}

impl DeviceSink for Heard {
    fn output(&self, _device: &str, pane: &str, bytes: &[u8], full: bool) {
        if pane != self.pane {
            return;
        }
        let mut screen = self.screen.lock().unwrap();
        screen.process(bytes);
        *self.full_frame_seen.lock().unwrap() |= full;
        let shown = screen.screen().contents();
        if shown.contains(TAIL_MARKER) {
            self.tell("tail");
        }
        if *self.scrolled.lock().unwrap() && shown.contains(HISTORY_MARKER) {
            self.tell("history");
        }
    }

    fn report(&self, _device: &str, report: TerminalReport) {
        if let TerminalReport::State { pane, state } = report
            && pane == self.pane
        {
            self.states.lock().unwrap().push(state.state.clone());
            match state.state.as_str() {
                "controlling" => self.tell("controlling"),
                "released" | "ended" | "idle" => self.tell("released"),
                _ => {}
            }
        }
    }
}

fn wait_for(receiver: &std::sync::mpsc::Receiver<&'static str>, what: &str, within: Duration) {
    let deadline = std::time::Instant::now() + within;
    loop {
        let left = deadline
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or_else(|| panic!("the device never reported {what}"));
        match receiver.recv_timeout(left) {
            Ok(heard) if heard == what => return,
            Ok(_) => {}
            Err(_) => panic!("the device never reported {what}"),
        }
    }
}

#[test]
#[ignore = "requires an owned remote fixture and HERDR_TEST_REMOTE_TERMINAL_* variables"]
fn official_remote_terminal_session_fixture_probe() {
    let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS")
        .expect("HERDR_TEST_SSH_ALIAS names a configured SSH host");
    let workspace_id = std::env::var("HERDR_TEST_REMOTE_TERMINAL_WORKSPACE_ID")
        .expect("HERDR_TEST_REMOTE_TERMINAL_WORKSPACE_ID names the owned fixture workspace");
    let pane_id = std::env::var("HERDR_TEST_REMOTE_TERMINAL_PANE_ID")
        .expect("HERDR_TEST_REMOTE_TERMINAL_PANE_ID names the owned fixture pane");
    let cwd = std::env::var("HERDR_TEST_REMOTE_TERMINAL_CWD")
        .expect("HERDR_TEST_REMOTE_TERMINAL_CWD names the owned fixture directory");
    // The node this build installs on the device: a folder holding `hided`
    // (and the kit's `hide` and `hide-agent-hooks`) for the device's system.
    let helper_dir = PathBuf::from(
        std::env::var_os("HERDR_TEST_REMOTE_HELPER_DIR")
            .expect("HERDR_TEST_REMOTE_HELPER_DIR names the folder of the build the device runs"),
    );
    let helper_root = std::env::var("HERDR_TEST_REMOTE_HELPER_ROOT")
        .expect("HERDR_TEST_REMOTE_HELPER_ROOT names the isolated account's helper folder");
    let cli_dir = std::env::var("HERDR_TEST_REMOTE_CLI_DIR")
        .expect("HERDR_TEST_REMOTE_CLI_DIR names the isolated account's command folder");
    assert!(
        helper_root.starts_with('/') && cli_dir.starts_with('/'),
        "the probe installs only into absolute folders of the isolated account"
    );
    assert!(
        cwd.starts_with("/tmp/herdr-ide-verify-"),
        "remote terminal fixture must use the owned fixture namespace"
    );

    let size = GridSize {
        rows: 30,
        cols: 100,
    };
    let heard = Arc::new(Heard {
        pane: pane_id.clone(),
        screen: Mutex::new(vt100::Parser::new(size.rows, size.cols, 0)),
        full_frame_seen: Mutex::default(),
        scrolled: Mutex::default(),
        states: Mutex::default(),
        progress: Mutex::default(),
    });
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME is configured"));
    let transport = hide_node::ssh::Connector::new(Some(helper_dir))
        .with_terminals(Arc::clone(&heard) as Arc<dyn DeviceSink>)
        .transport(&home, "probe", &alias_name, None)
        .expect("SSH alias resolves");
    let snapshot = hide_herdr_client::request_with_connector(
        &*transport.herdr_api_connector(),
        "session.snapshot",
        json!({}),
        SSH_OPERATION_TIMEOUT,
    )
    .expect("fixture session snapshot");
    let snapshot = snapshot["snapshot"]
        .as_object()
        .map(|_| &snapshot["snapshot"])
        .expect("session.snapshot response contains a snapshot");
    let owned_workspace = snapshot["workspaces"]
        .as_array()
        .and_then(|workspaces| {
            workspaces
                .iter()
                .find(|workspace| workspace["workspace_id"].as_str() == Some(workspace_id.as_str()))
        })
        .expect("owned fixture workspace is present");
    assert!(
        owned_workspace["label"]
            .as_str()
            .is_some_and(|label| label.starts_with("herdr-ide-verify-")),
        "remote terminal refused a workspace outside the owned fixture namespace"
    );
    assert!(snapshot["panes"].as_array().is_some_and(|panes| {
        let canonical_cwd = cwd
            .strip_prefix("/tmp/")
            .map(|suffix| format!("/private/tmp/{suffix}"));
        panes.iter().any(|pane| {
            pane["pane_id"].as_str() == Some(pane_id.as_str())
                && pane["workspace_id"].as_str() == Some(workspace_id.as_str())
                && (pane["cwd"].as_str() == Some(cwd.as_str())
                    || pane["cwd"].as_str() == canonical_cwd.as_deref())
        })
    }));

    // The device's node link, with its terminal service started.
    let consent = HostConsent {
        contract: HOST_CONSENT_CONTRACT,
        helper_root,
        cli_dir: Some(cli_dir),
        granted_at_unix_ms: 0,
        identity: None,
    };
    let (closed_sender, closed) = channel();
    let established = transport
        .establish(
            &consent,
            &[],
            Box::new(move |reason| {
                let _ = closed_sender.send(reason);
            }),
        )
        .expect("the device's node link is established");
    let terminals = established
        .terminals
        .expect("the device's node started its terminal service");
    let (progress, heard_progress) = channel();
    *heard.progress.lock().unwrap() = Some(progress);

    terminals.view(&pane_id, size, true);
    terminals.control(TerminalControl::Attach {
        pane: pane_id.clone(),
        size: Some(size),
        manual: false,
    });
    wait_for(&heard_progress, "controlling", Duration::from_secs(15));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    terminals.key(
        KeyTarget::Pane(pane_id.clone()),
        b"for i in {1..80}; do printf 'HERDR_IDE_SCROLL_%03d\\n' $i; done\r".to_vec(),
        now,
    );
    wait_for(&heard_progress, "tail", Duration::from_secs(15));
    // Only a full frame at the view's grid reaches the screen first, so
    // output at all means the session runs at 100 by 30.
    assert!(
        *heard.full_frame_seen.lock().unwrap(),
        "the remote terminal drew a full frame at the view's grid"
    );
    *heard.scrolled.lock().unwrap() = true;
    terminals.control(TerminalControl::Scroll {
        pane: pane_id.clone(),
        lines: 1000,
        column: None,
        row: None,
        modifiers: 0,
    });
    wait_for(&heard_progress, "history", Duration::from_secs(15));
    terminals.control(TerminalControl::Release {
        pane: pane_id.clone(),
        message: "probe finished".to_owned(),
    });
    wait_for(&heard_progress, "released", Duration::from_secs(5));
    eprintln!("pane states: {:?}", heard.states.lock().unwrap().as_slice());
    drop(terminals);
    drop(established.host);
    let _ = closed.recv_timeout(Duration::from_secs(5));
}
