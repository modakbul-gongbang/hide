//! A release-core boundary probe, separate from the product-page comparison.
//! The real Local node and Herdr supply the catalog and confirmed topology.
//! Terminal drawing is deliberately absent: this does not measure PTY or UI latency.
use herdr_core::{Core, CoreOptions};
use hide_node_link::protocol::Call;
use hide_node_link::terminal::*;
use hide_node_link::{LinkAnswer, LinkError, NodeLink};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Gate {
    local: hide_node::Local,
    armed: AtomicBool,
    path: String,
    started: mpsc::Sender<Vec<String>>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl NodeLink for Gate {
    fn call(&self, call: Call, timeout: Duration) -> std::result::Result<LinkAnswer, LinkError> {
        if let Call::PathFacts { ref paths } = call
            && paths.contains(&self.path)
            && self.armed.swap(false, Ordering::AcqRel)
        {
            let _ = self.started.send(paths.clone());
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| LinkError::Unknown("measurement gate deadline reached".into()))?;
        }
        self.local.call(call, timeout)
    }
    fn reader_features(&self) -> Option<&hide_node_link::sessions::ReaderFeatures> {
        self.local.reader_features()
    }
    fn call_with_progress(
        &self,
        call: Call,
        timeout: Duration,
        progress: &mut dyn FnMut(Value) -> bool,
    ) -> std::result::Result<LinkAnswer, LinkError> {
        if matches!(call, Call::PathFacts { .. }) {
            self.call(call, timeout)
        } else {
            self.local.call_with_progress(call, timeout, progress)
        }
    }
    fn predates_current_protocol(&self) -> bool {
        self.local.predates_current_protocol()
    }
    fn in_process(&self) -> bool {
        self.local.in_process()
    }
    fn closed_reason(&self) -> Option<String> {
        self.local.closed_reason()
    }
    fn close(&self, reason: &str) {
        self.local.close(reason);
    }
    fn close_when_idle(&self, reason: &str) {
        self.local.close_when_idle(reason);
    }
    fn pin(&self, root: &str, identity: Option<hide_node_link::RootIdentity>) {
        self.local.pin(root, identity);
    }
    fn pinned(&self, root: &str) -> Option<hide_node_link::RootIdentity> {
        self.local.pinned(root)
    }
}

// No bytes are sent or attach claims made in this observation-only probe.
struct NoDrawing;
impl TerminalNode for NoDrawing {
    fn control(&self, _: TerminalControl) {}
    fn key(&self, _: KeyTarget, _: Vec<u8>, _: u64) {
        panic!("probe must not type terminal input");
    }
    fn view(&self, _: &str, _: GridSize, _: bool) {
        panic!("probe must not draw a terminal");
    }
    fn redraw(&self, _: &str) {
        panic!("probe must not redraw a terminal");
    }
}
impl TerminalRoutes for NoDrawing {
    fn install_device(&self, _: &str, _: Arc<dyn TerminalNode>) {}
    fn remove_device(&self, _: &str) {}
    fn terminals_unstarted(&self, _: &str, _: &str) {}
}

fn event(core: &Core, kind: &str, payload: Value) -> Result<()> {
    if !core.dispatch_bytes(&serde_json::to_vec(
        &json!({"schema_version":2,"kind":kind,"payload":payload}),
    )?) {
        return Err(format!("core refused {kind}").into());
    }
    Ok(())
}

#[cfg(typed_snapshot)]
fn read(core: &Core, cursor: u64) -> Result<(u64, Value, f64, f64, usize)> {
    let started = Instant::now();
    let payload = core.snapshot_delta_payload(cursor)?;
    let owner_ms = started.elapsed().as_secs_f64() * 1000.0;
    #[derive(serde::Serialize)]
    struct Envelope<'a> {
        #[serde(rename = "type")]
        kind: &'static str,
        payload: herdr_core::SnapshotDeltaWire<'a>,
    }
    let encode_started = Instant::now();
    let wire = serde_json::to_string(&Envelope {
        kind: if cursor == 0 { "snapshot" } else { "delta" },
        payload: herdr_core::SnapshotDeltaWire::granular(&payload),
    })?;
    let encode_ms = encode_started.elapsed().as_secs_f64() * 1000.0;
    // Observation parsing is after both clocks and is not product processing.
    Ok((
        payload.revision,
        serde_json::from_str::<Value>(&wire)?["payload"].take(),
        owner_ms,
        encode_ms,
        wire.len(),
    ))
}

#[cfg(not(typed_snapshot))]
fn read(core: &Core, cursor: u64) -> Result<(u64, Value, f64, f64, usize)> {
    let started = Instant::now();
    let bytes = core.snapshot_delta(cursor);
    let owner_ms = started.elapsed().as_secs_f64() * 1000.0;
    // This is the baseline transport's bytes -> Value -> envelope path.
    let encode_started = Instant::now();
    let payload: Value = serde_json::from_slice(&bytes)?;
    let revision = payload["revision"].as_u64().ok_or("missing revision")?;
    let wire = serde_json::to_string(
        &json!({"type":if cursor == 0 {"snapshot"} else {"delta"},"payload":payload}),
    )?;
    let encode_ms = encode_started.elapsed().as_secs_f64() * 1000.0;
    Ok((revision, payload, owner_ms, encode_ms, wire.len()))
}

fn herdr(options: &CoreOptions, args: &[&str]) -> Result<Value> {
    let answer = std::process::Command::new(
        options
            .herdr_bin_path
            .as_ref()
            .ok_or("Herdr binary missing")?,
    )
    .env(
        "HERDR_SOCKET_PATH",
        options
            .herdr_socket_path
            .as_ref()
            .ok_or("Herdr socket missing")?,
    )
    .args(args)
    .output()?;
    if !answer.status.success() {
        return Err(String::from_utf8_lossy(&answer.stderr).to_string().into());
    }
    Ok(serde_json::from_slice(&answer.stdout)?)
}

fn main() -> Result<()> {
    let config: Value = serde_json::from_slice(&std::fs::read(
        std::env::args().nth(1).ok_or("config path required")?,
    )?)?;
    let options: CoreOptions = serde_json::from_value(config["options"].clone())?;
    let pane = config["pane"].as_str().ok_or("pane missing")?;
    let workspace = config["herdr_workspace"]
        .as_str()
        .ok_or("Herdr workspace missing")?;
    let output = std::path::Path::new(config["output"].as_str().ok_or("output missing")?);
    let folder = std::path::Path::new(options.home.as_ref().unwrap()).join("project/held-path");
    let (started, held) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let node = Arc::new(Gate {
        local: hide_node::Local::new(options.home.as_ref().map(std::path::PathBuf::from)),
        armed: AtomicBool::new(false),
        path: folder.to_str().ok_or("held path must be UTF-8")?.into(),
        started,
        release: Mutex::new(wait),
    });
    let (_, reports) = herdr_core::terminal_reports::terminal_reports();
    let core = Core::create(
        options.clone(),
        node.clone(),
        options
            .herdr_socket_path
            .as_deref()
            .map(|socket| hide_node::herdr(std::path::Path::new(socket))),
        Arc::new(hide_node::ssh::Connector::new(
            std::env::current_exe()?
                .parent()
                .map(std::path::Path::to_path_buf),
        )),
        Arc::new(NoDrawing),
        reports,
    )
    .ok_or("core creation refused")?;
    event(&core, "ui_attached", json!({"attached":true}))?;
    let deadline = Instant::now() + Duration::from_secs(180);
    let current = loop {
        let (_, current, _, _, _) = read(&core, 0)?;
        let rows = current
            .pointer("/rest/navigator/workspaces")
            .and_then(Value::as_array);
        if rows.is_some_and(|rows| {
            rows.len() >= config["workspaces"].as_u64().unwrap() as usize
                && rows.iter().all(|row| {
                    row["branches"].as_array().is_some_and(|r| r.len() >= 40)
                        && row["pull_requests"]
                            .as_array()
                            .is_some_and(|r| r.len() >= 40)
                })
        }) {
            break current;
        }
        if Instant::now() >= deadline {
            return Err("release core catalog did not converge".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    std::fs::write(
        output.join("core-cost-initial.json"),
        serde_json::to_vec_pretty(&current)?,
    )?;
    event(
        &core,
        "focus_pane",
        json!({"pane_id":pane,"origin":"operator"}),
    )?;
    let mut samples = Vec::new();
    let mut cursor = read(&core, 0)?.0;
    for sample in 0..100 {
        event(
            &core,
            "ui_state_update",
            json!({"sidebar_width":if sample % 2 == 0 {220} else {221}}),
        )?;
        let (revision, _, owner_ms, encode_ms, bytes) = read(&core, cursor)?;
        samples.push(json!({"owner_read_ms":owner_ms,"transport_encode_ms":encode_ms,"bytes":bytes,"revision":revision}));
        cursor = revision;
    }
    std::fs::write(
        output.join("core-cost.json"),
        serde_json::to_vec_pretty(&json!({
            "typed":cfg!(typed_snapshot), "samples":samples,
            "method":"real release Core, Local node and private Herdr; sidebar state changes; owner clock includes capture/comparison and baseline first encoding, transport clock is remaining baseline parse/envelope or candidate single encoding; excludes observation parse, owner-channel queue, socket and UI"
        }))?,
    )?;
    std::fs::create_dir_all(&folder)?;
    node.armed.store(true, Ordering::Release);
    herdr(
        &options,
        &[
            "tab",
            "create",
            "--workspace",
            workspace,
            "--cwd",
            folder.to_str().unwrap(),
            "--no-focus",
        ],
    )?;
    let held_paths = held.recv_timeout(Duration::from_secs(30))?;
    let zoom_started = Instant::now();
    let confirmation = herdr(&options, &["pane", "zoom", pane])?;
    let confirmation_ms = zoom_started.elapsed().as_secs_f64() * 1000.0;
    let mut first_seen = None;
    let gate_until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < gate_until {
        let (_, frame, _, _, _) = read(&core, 0)?;
        if frame.pointer("/rest/zoomed").and_then(Value::as_str) == Some(pane) {
            first_seen = Some(zoom_started.elapsed().as_secs_f64() * 1000.0);
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(gate_until.saturating_duration_since(Instant::now()));
    let release_ms = zoom_started.elapsed().as_secs_f64() * 1000.0;
    release.send(())?;
    let until = Instant::now() + Duration::from_secs(30);
    let converged = loop {
        let (_, frame, _, _, _) = read(&core, 0)?;
        if frame.pointer("/rest/zoomed").and_then(Value::as_str) == Some(pane) {
            break zoom_started.elapsed().as_secs_f64() * 1000.0;
        }
        if Instant::now() > until {
            return Err("zoom did not converge after releasing PathFacts".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    std::fs::write(
        output.join("held-path.json"),
        serde_json::to_vec_pretty(&json!({
            "method":"real Core and private Herdr; one actual Local PathFacts call gated before answering; no terminal drawing or DOM clock",
            "confirmation":confirmation,"herdr_confirmed_ms":confirmation_ms,
            "core_zoom_before_release_ms":first_seen,"released_ms":release_ms,"converged_ms":converged,
            "held_paths":held_paths,
        }))?,
    )?;
    // Gate is released before Core drops and joins its owned readers.
    drop(core);
    Ok(())
}
