//! Live Herdr commands and pane byte transport. Session state synchronization
//! lives in `session_sync` and uses the sequenced socket event stream.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ffi::ChangeNotifier;
#[cfg(test)]
use crate::herdr_api::HERDR_PROTOCOL_REVISION;
use crate::herdr_api::{ApiConnector, UnixSocketConnector, request};
use crate::model::{
    PaneLayoutDirection, PaneLayoutNodeSnapshot, PaneLayoutSnapshot, WorkspaceRegistration,
    WorkspaceSnapshot,
};
use crate::runtime::Runtime;
use crate::sidebar::{
    SessionLayoutPanePayload, SessionLayoutPayload, SessionLayoutRect, SessionSnapshotPayload,
};
use crate::workspace;

/// Everything a terminal session spawn needs from the live configuration.
#[derive(Clone)]
pub struct LiveContext {
    pub socket_path: PathBuf,
    pub herdr_bin: Option<PathBuf>,
    pub runtime: Weak<Mutex<Runtime>>,
    pub notifier: ChangeNotifier,
    pub(crate) api_connector: Arc<dyn ApiConnector>,
}

pub struct WorkspaceCreationOutcome {
    pub registration: WorkspaceRegistration,
    pub base_registrations: Vec<WorkspaceRegistration>,
    pub registrations: Vec<WorkspaceRegistration>,
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub git_init_error: Option<String>,
}

pub fn spawn_workspace_creation(
    context: LiveContext,
    path: String,
    label: String,
    initialize_git: bool,
    base_registrations: Vec<WorkspaceRegistration>,
    spaces: Vec<workspace::SessionSpace>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-workspace-create".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let request_path = path.clone();
            let result = workspace::registration(&path, &label, workspace::LOCAL_DEVICE_ID)
                .and_then(|registration| {
                    let root = Path::new(&registration.path);
                    if !root.exists() {
                        return Err(format!(
                            "Workspace path does not exist: {}",
                            registration.path
                        ));
                    }
                    let git_init_error = initialize_git
                        .then(|| workspace::initialize_git(root).err())
                        .flatten();
                    let mut registrations = base_registrations.clone();
                    if !registrations
                        .iter()
                        .any(|existing| existing.id == registration.id)
                    {
                        registrations.push(registration.clone());
                    }
                    let workspaces = workspace::build_catalog(&registrations, &spaces);
                    Ok(WorkspaceCreationOutcome {
                        registration,
                        base_registrations,
                        registrations,
                        workspaces,
                        git_init_error,
                    })
                });
            let elapsed_ms = started.elapsed().as_millis();
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_workspace_creation(&request_path, result, elapsed_ms),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("workspace creation worker could not be started: {error}"))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PaneSplitDirection {
    Right,
    Down,
}

impl PaneSplitDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PaneResizeDirection {
    Left,
    Right,
    Up,
    Down,
}

impl PaneResizeDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

#[derive(Clone, Debug)]
pub enum PaneControlAction {
    /// Fetches the authoritative layout containing a pane without changing
    /// Herdr's global focus. Checkout navigation uses this to update directly
    /// instead of waiting for the corresponding event-stream projection.
    Project {
        pane_id: String,
    },
    Focus {
        pane_id: String,
    },
    Split {
        pane_id: String,
        direction: PaneSplitDirection,
        cwd: Option<String>,
    },
    Resize {
        pane_id: String,
        direction: PaneResizeDirection,
        amount: f32,
    },
    ToggleZoom {
        pane_id: String,
    },
    Close {
        pane_id: String,
    },
}

#[derive(Debug)]
pub enum PaneControlOutcome {
    Projected { layout: PaneLayoutSnapshot },
    Acknowledged { created_pane_id: Option<String> },
}

fn execute_pane_control(
    context: &LiveContext,
    action: &PaneControlAction,
) -> Result<PaneControlOutcome, String> {
    if let PaneControlAction::Project { pane_id } = action {
        return fetch_pane_layout(&context.socket_path, pane_id)
            .map(|layout| PaneControlOutcome::Projected { layout });
    }
    if let PaneControlAction::Focus { pane_id } = action {
        request(
            &context.socket_path,
            "pane.focus",
            json!({"pane_id": pane_id}),
        )?;
        return Ok(PaneControlOutcome::Acknowledged {
            created_pane_id: None,
        });
    }
    if let PaneControlAction::Resize {
        pane_id,
        direction,
        amount,
    } = action
    {
        request(
            &context.socket_path,
            "pane.resize",
            json!({"pane_id": pane_id, "direction": direction.as_str(), "amount": amount}),
        )?;
        return Ok(PaneControlOutcome::Acknowledged {
            created_pane_id: None,
        });
    }

    let Some(herdr_bin) = context.herdr_bin.as_ref() else {
        return Err("herdr binary was not found; pane control is unavailable".to_owned());
    };
    let arguments = pane_control_arguments(action);
    let output = Command::new(herdr_bin)
        .args(&arguments)
        .env("HERDR_SOCKET_PATH", &context.socket_path)
        .output()
        .map_err(|error| format!("herdr pane control could not start: {error}"))?;
    if output.status.success() {
        let created_pane_id = match action {
            PaneControlAction::Split { .. } => {
                let response: Value = serde_json::from_slice(&output.stdout)
                    .map_err(|_| "herdr pane split returned unreadable JSON".to_owned())?;
                Some(
                    response
                        .pointer("/result/pane/pane_id")
                        .and_then(Value::as_str)
                        .filter(|pane_id| !pane_id.trim().is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| {
                            "herdr pane split response is missing result.pane.pane_id".to_owned()
                        })?,
                )
            }
            PaneControlAction::Project { .. }
            | PaneControlAction::Focus { .. }
            | PaneControlAction::Resize { .. }
            | PaneControlAction::ToggleZoom { .. }
            | PaneControlAction::Close { .. } => None,
        };
        return Ok(PaneControlOutcome::Acknowledged { created_pane_id });
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if stderr.is_empty() {
        format!("herdr pane control exited with {}", output.status)
    } else {
        stderr
    })
}

fn fetch_pane_layout(socket_path: &Path, pane_id: &str) -> Result<PaneLayoutSnapshot, String> {
    let result = request(socket_path, "pane.layout", json!({"pane_id": pane_id}))?;
    let layout = serde_json::from_value::<SessionLayoutPayload>(
        result
            .get("layout")
            .cloned()
            .ok_or_else(|| "pane.layout response is missing layout".to_owned())?,
    )
    .map_err(|error| format!("pane.layout response is malformed: {error}"))?;
    project_layout(&layout)
}

pub fn spawn_pane_control(context: LiveContext, action: PaneControlAction) -> Result<(), String> {
    let worker_name = match &action {
        PaneControlAction::Project { .. } => "herdr-core-pane-project".to_owned(),
        PaneControlAction::Focus { .. } => "herdr-core-pane-focus".to_owned(),
        PaneControlAction::Split { direction, .. } => {
            format!("herdr-core-pane-split-{}", direction.as_str())
        }
        PaneControlAction::Resize { direction, .. } => {
            format!("herdr-core-pane-resize-{}", direction.as_str())
        }
        PaneControlAction::ToggleZoom { .. } => "herdr-core-pane-zoom".to_owned(),
        PaneControlAction::Close { .. } => "herdr-core-pane-close".to_owned(),
    };
    thread::Builder::new()
        .name(worker_name)
        .spawn(move || {
            let started = Instant::now();
            let result = execute_pane_control(&context, &action);
            let elapsed_ms = started.elapsed().as_millis();
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_pane_control_result(action, result, elapsed_ms),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("pane control worker could not be started: {error}"))
}

fn pane_control_arguments(action: &PaneControlAction) -> Vec<String> {
    match action {
        PaneControlAction::Project { .. }
        | PaneControlAction::Focus { .. }
        | PaneControlAction::Resize { .. } => {
            unreachable!("pane projection and focus use the socket API instead of the CLI")
        }
        PaneControlAction::Split {
            pane_id,
            direction,
            cwd,
        } => {
            let mut arguments = vec![
                "pane".to_owned(),
                "split".to_owned(),
                pane_id.clone(),
                "--direction".to_owned(),
                direction.as_str().to_owned(),
            ];
            if let Some(cwd) = cwd.as_deref().filter(|value| !value.trim().is_empty()) {
                arguments.push("--cwd".to_owned());
                arguments.push(cwd.to_owned());
            }
            arguments
        }
        PaneControlAction::ToggleZoom { pane_id } => vec![
            "pane".to_owned(),
            "zoom".to_owned(),
            pane_id.clone(),
            "--toggle".to_owned(),
        ],
        PaneControlAction::Close { pane_id } => {
            vec!["pane".to_owned(), "close".to_owned(), pane_id.clone()]
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionFetchError {
    /// The socket file itself does not exist: the herdr server is not running.
    SocketMissing(String),
    /// The socket exists but the request failed (connect, timeout, IO).
    Unreachable(String),
    /// The server answered with an incompatible protocol revision.
    Protocol(String),
    /// A previously valid projection is retained while the event stream
    /// reconnects or performs an explicit snapshot resynchronization.
    Stale(String),
    /// The server answered but the payload did not match the expected shape.
    Malformed(String),
}

impl SessionFetchError {
    pub fn state(&self) -> &'static str {
        match self {
            Self::SocketMissing(_) => "socket_missing",
            Self::Unreachable(_) => "unreachable",
            Self::Protocol(_) => "protocol_mismatch",
            Self::Stale(_) => "stale",
            Self::Malformed(_) => "malformed",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::SocketMissing(message)
            | Self::Unreachable(message)
            | Self::Protocol(message)
            | Self::Stale(message)
            | Self::Malformed(message) => message,
        }
    }
}

/// Installs live command context and starts event-driven session sync.
pub(crate) fn install(
    runtime: &Arc<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    socket_path: &str,
    herdr_bin: Option<&str>,
    home_path: Option<PathBuf>,
) -> Option<crate::session_sync::SessionSyncHandle> {
    let context = LiveContext {
        socket_path: PathBuf::from(socket_path),
        herdr_bin: herdr_bin.map(PathBuf::from),
        runtime: Arc::downgrade(runtime),
        notifier: notifier.clone(),
        api_connector: Arc::new(UnixSocketConnector::new(socket_path)),
    };
    if let Ok(mut guard) = runtime.lock() {
        guard.set_live(context.clone());
    }
    match crate::session_sync::spawn(context.clone(), home_path) {
        Ok(handle) => Some(handle),
        Err(message) => {
            eprintln!(
                "{}",
                json!({
                    "component": "session_sync",
                    "kind": "coordinator.spawn_failed",
                    "message": message,
                })
            );
            let changed = runtime.lock().ok().is_some_and(|mut guard| {
                guard.ingest_session(Err(SessionFetchError::Unreachable(message)))
            });
            if changed {
                context.notifier.notify();
            }
            None
        }
    }
}

pub fn fetch_session(socket_path: &Path) -> Result<SessionSnapshotPayload, SessionFetchError> {
    if !socket_path.exists() {
        return Err(SessionFetchError::SocketMissing(format!(
            "Herdr socket file does not exist at {}; the herdr server is not running",
            socket_path.display()
        )));
    }
    let result = request(socket_path, "session.snapshot", json!({}))
        .map_err(SessionFetchError::Unreachable)?;
    let snapshot = result
        .get("snapshot")
        .ok_or_else(|| SessionFetchError::Malformed("response is missing snapshot".to_owned()))?;
    project_session(snapshot)
}

/// Maps the herdr wire snapshot into the sidebar session payload. Tokens are
/// passed through verbatim so the unseen-vs-acknowledged state rules
/// (INV-herdr-unseen-token) stay owned by the sidebar projection.
pub fn project_session(snapshot: &Value) -> Result<SessionSnapshotPayload, SessionFetchError> {
    crate::session_sync::project_snapshot(snapshot)
}

pub fn project_layout_for_pane(
    payload: &SessionSnapshotPayload,
    pane_id: &str,
) -> Result<PaneLayoutSnapshot, String> {
    let layout = payload
        .layouts
        .iter()
        .find(|layout| layout.panes.iter().any(|pane| pane.pane_id == pane_id))
        .ok_or_else(|| format!("Herdr session has no layout containing pane {pane_id}"))?;
    project_layout(layout)
}

fn project_layout(layout: &SessionLayoutPayload) -> Result<PaneLayoutSnapshot, String> {
    if layout.panes.is_empty() {
        return Err(format!("Herdr tab {} has no panes", layout.tab_id));
    }
    let root = project_layout_node(layout.area, &layout.panes, &layout.splits)?;
    Ok(PaneLayoutSnapshot {
        workspace_id: layout.workspace_id.clone(),
        tab_id: layout.tab_id.clone(),
        focused_pane_id: layout.focused_pane_id.clone(),
        zoomed: layout.zoomed,
        root,
    })
}

fn project_layout_node(
    area: SessionLayoutRect,
    panes: &[SessionLayoutPanePayload],
    splits: &[crate::sidebar::SessionLayoutSplitPayload],
) -> Result<PaneLayoutNodeSnapshot, String> {
    let matching_splits = splits
        .iter()
        .filter(|split| split.rect == area)
        .collect::<Vec<_>>();
    if matching_splits.len() > 1 {
        return Err("Herdr layout contains duplicate splits for one area".to_owned());
    }
    let Some(split) = matching_splits.first().copied() else {
        return match panes {
            [pane] => Ok(PaneLayoutNodeSnapshot::Pane {
                pane_id: pane.pane_id.clone(),
            }),
            [] => Err("Herdr layout produced an empty leaf".to_owned()),
            _ => Err("Herdr layout has multiple panes without an authoritative split".to_owned()),
        };
    };
    if !split.ratio.is_finite() || split.ratio <= 0.0 || split.ratio >= 1.0 {
        return Err(format!(
            "Herdr layout split ratio {} is invalid",
            split.ratio
        ));
    }
    let (first_area, second_area, boundary) = split_areas(area, split.direction, split.ratio)?;
    let (first_panes, second_panes): (Vec<_>, Vec<_>) = panes.iter().partition(|pane| {
        let center = match split.direction {
            PaneLayoutDirection::Right => f32::from(pane.rect.x) + f32::from(pane.rect.width) / 2.0,
            PaneLayoutDirection::Down => f32::from(pane.rect.y) + f32::from(pane.rect.height) / 2.0,
        };
        center < boundary
    });
    if first_panes.is_empty() || second_panes.is_empty() {
        return Err("Herdr layout split does not divide panes into two children".to_owned());
    }
    let first = project_layout_node(
        first_area,
        &first_panes.into_iter().cloned().collect::<Vec<_>>(),
        splits,
    )?;
    let second = project_layout_node(
        second_area,
        &second_panes.into_iter().cloned().collect::<Vec<_>>(),
        splits,
    )?;
    Ok(PaneLayoutNodeSnapshot::Split {
        direction: split.direction,
        ratio: split.ratio,
        first: Box::new(first),
        second: Box::new(second),
    })
}

fn split_areas(
    area: SessionLayoutRect,
    direction: PaneLayoutDirection,
    ratio: f32,
) -> Result<(SessionLayoutRect, SessionLayoutRect, f32), String> {
    match direction {
        PaneLayoutDirection::Right => {
            if area.width < 2 {
                return Err("Herdr right split area is too narrow".to_owned());
            }
            let first_width =
                ((f32::from(area.width) * ratio).round() as u16).clamp(1, area.width - 1);
            let second_width = area.width - first_width;
            let second_x = area.x.saturating_add(first_width);
            Ok((
                SessionLayoutRect {
                    width: first_width,
                    ..area
                },
                SessionLayoutRect {
                    x: second_x,
                    width: second_width,
                    ..area
                },
                f32::from(second_x),
            ))
        }
        PaneLayoutDirection::Down => {
            if area.height < 2 {
                return Err("Herdr down split area is too short".to_owned());
            }
            let first_height =
                ((f32::from(area.height) * ratio).round() as u16).clamp(1, area.height - 1);
            let second_height = area.height - first_height;
            let second_y = area.y.saturating_add(first_height);
            Ok((
                SessionLayoutRect {
                    height: first_height,
                    ..area
                },
                SessionLayoutRect {
                    y: second_y,
                    height: second_height,
                    ..area
                },
                f32::from(second_y),
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalSessionMode {
    Control,
    Observe,
}

impl TerminalSessionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Observe => "observe",
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum TerminalSessionEvent {
    Frame {
        seq: u64,
        width: u16,
        height: u16,
        full: bool,
        bytes: Vec<u8>,
    },
    Closed {
        reason: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum TerminalSessionEnvelope {
    #[serde(rename = "terminal.frame")]
    Frame {
        seq: u64,
        encoding: String,
        width: u16,
        height: u16,
        full: bool,
        bytes: String,
    },
    #[serde(rename = "terminal.closed")]
    Closed { reason: Option<String> },
}

pub fn parse_terminal_session_line(line: &str) -> Result<TerminalSessionEvent, String> {
    if line.trim().is_empty() {
        return Err("terminal session emitted an empty NDJSON line".to_owned());
    }
    let envelope: TerminalSessionEnvelope = serde_json::from_str(line)
        .map_err(|error| format!("terminal session emitted invalid NDJSON: {error}"))?;
    match envelope {
        TerminalSessionEnvelope::Frame {
            seq,
            encoding,
            width,
            height,
            full,
            bytes,
        } => {
            if encoding != "ansi" {
                return Err(format!(
                    "terminal session negotiated unsupported encoding {encoding:?}"
                ));
            }
            Ok(TerminalSessionEvent::Frame {
                seq,
                width,
                height,
                full,
                bytes: decode_base64(&bytes)?,
            })
        }
        TerminalSessionEnvelope::Closed { reason } => Ok(TerminalSessionEvent::Closed { reason }),
    }
}

pub fn terminal_input_line(bytes: &[u8]) -> Result<String, String> {
    let mut line = serde_json::to_string(&json!({
        "type": "terminal.input",
        "bytes": encode_base64(bytes),
    }))
    .map_err(|error| format!("terminal input could not be encoded: {error}"))?;
    line.push('\n');
    Ok(line)
}

/// Asks Herdr to move the pane through its own host scrollback.
///
/// Hide receives a rendered stream: Herdr paints the pane at the geometry this
/// client asked for, so no row ever scrolls off the client grid and a local
/// scrollback stays empty however deep its buffer is. Herdr keeps the history
/// instead, and scrolling is a request it answers with a fresh frame. This is
/// the same path the Herdr TUI uses, which is why that client scrolls panes
/// this one could not.
pub fn terminal_scroll_line(direction: &str, lines: u16) -> Result<String, String> {
    if !matches!(direction, "up" | "down") {
        return Err(format!(
            "terminal scroll direction is not up or down: {direction}"
        ));
    }
    if lines == 0 {
        return Err("terminal scroll needs at least one line".to_owned());
    }
    let mut line = serde_json::to_string(&json!({
        "type": "terminal.scroll",
        "direction": direction,
        "lines": lines,
        "source": "wheel",
    }))
    .map_err(|error| format!("terminal scroll could not be encoded: {error}"))?;
    line.push('\n');
    Ok(line)
}

pub fn terminal_resize_line(rows: u16, cols: u16) -> Result<String, String> {
    if rows == 0 || cols == 0 {
        return Err("terminal dimensions must be positive".to_owned());
    }
    let mut line = serde_json::to_string(&json!({
        "type": "terminal.resize",
        "cols": cols,
        "rows": rows,
        "cell_width_px": 0,
        "cell_height_px": 0,
    }))
    .map_err(|error| format!("terminal resize could not be encoded: {error}"))?;
    line.push('\n');
    Ok(line)
}

pub fn terminal_release_line() -> String {
    "{\"type\":\"terminal.release\"}\n".to_owned()
}

pub fn terminal_closed_category(reason: Option<&str>) -> &'static str {
    let Some(reason) = reason else {
        return "transport_eof";
    };
    let normalized = reason.to_ascii_lowercase();
    if normalized.contains("already has an attached client")
        && normalized.contains("retry with --takeover")
    {
        "owner_conflict"
    } else {
        "terminal_closed"
    }
}

fn terminal_session_arguments(
    mode: TerminalSessionMode,
    pane_id: &str,
    rows: u16,
    cols: u16,
) -> Vec<String> {
    vec![
        "terminal".to_owned(),
        "session".to_owned(),
        mode.as_str().to_owned(),
        pane_id.to_owned(),
        "--cols".to_owned(),
        cols.to_string(),
        "--rows".to_owned(),
        rows.to_string(),
    ]
}

/// One official Herdr terminal session process. Control is writable; observe
/// is concurrent and read-only. Dropping it stops only this client process.
pub struct TerminalSession {
    pub pane_id: String,
    pub generation: u64,
    pub mode: TerminalSessionMode,
    child: Option<Child>,
    writer: Option<Sender<String>>,
    reader: Option<ChildStdout>,
}

impl TerminalSession {
    #[cfg(test)]
    pub fn test_stub(pane_id: &str, generation: u64, mode: TerminalSessionMode) -> Self {
        Self {
            pane_id: pane_id.to_owned(),
            generation,
            mode,
            child: None,
            writer: None,
            reader: None,
        }
    }

    pub fn spawn(
        context: &LiveContext,
        pane_id: &str,
        generation: u64,
        mode: TerminalSessionMode,
        rows: u16,
        cols: u16,
    ) -> Result<Self, String> {
        let Some(herdr_bin) = context.herdr_bin.as_ref() else {
            return Err(
                "herdr binary was not found; install herdr or set its path in the app options"
                    .to_owned(),
            );
        };
        let mut command = Command::new(herdr_bin);
        command
            .args(terminal_session_arguments(mode, pane_id, rows, cols))
            .env("HERDR_SOCKET_PATH", &context.socket_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if mode == TerminalSessionMode::Control {
            command.stdin(Stdio::piped());
        } else {
            command.stdin(Stdio::null());
        }
        let mut child = command.spawn().map_err(|error| {
            format!(
                "herdr terminal session {} could not be spawned: {error}",
                mode.as_str()
            )
        })?;
        let writer = if mode == TerminalSessionMode::Control {
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| "terminal control stdin was not piped".to_owned())?;
            Some(spawn_terminal_control_writer(
                context, pane_id, generation, stdin,
            )?)
        } else {
            None
        };
        let reader = child
            .stdout
            .take()
            .ok_or_else(|| "terminal session stdout was not piped".to_owned())?;

        Ok(Self {
            pane_id: pane_id.to_owned(),
            generation,
            mode,
            child: Some(child),
            reader: Some(reader),
            writer,
        })
    }

    pub fn start_reader(
        &mut self,
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) -> Result<(), String> {
        let Some(reader) = self.reader.take() else {
            return Err("terminal session reader was already started".to_owned());
        };
        let generation = self.generation;
        let reader_pane = self.pane_id.clone();
        let mode = self.mode;
        thread::Builder::new()
            .name(format!(
                "herdr-core-terminal-{}-{reader_pane}",
                mode.as_str()
            ))
            .spawn(move || {
                let mut lines = BufReader::new(reader).lines();
                loop {
                    match lines.next() {
                        None => {
                            deliver_terminal_session_closed(
                                &runtime,
                                &notifier,
                                generation,
                                &reader_pane,
                                mode,
                                None,
                            );
                            return;
                        }
                        Some(Ok(line)) => match parse_terminal_session_line(&line) {
                            Ok(TerminalSessionEvent::Frame { bytes, .. }) => {
                                if !deliver_terminal_session_frame(
                                    &runtime,
                                    &notifier,
                                    &reader_pane,
                                    generation,
                                    mode,
                                    &bytes,
                                ) {
                                    return;
                                }
                            }
                            Ok(TerminalSessionEvent::Closed { reason }) => {
                                deliver_terminal_session_closed(
                                    &runtime,
                                    &notifier,
                                    generation,
                                    &reader_pane,
                                    mode,
                                    reason,
                                );
                                return;
                            }
                            Err(message) => {
                                deliver_terminal_session_closed(
                                    &runtime,
                                    &notifier,
                                    generation,
                                    &reader_pane,
                                    mode,
                                    Some(message),
                                );
                                return;
                            }
                        },
                        Some(Err(error)) => {
                            deliver_terminal_session_closed(
                                &runtime,
                                &notifier,
                                generation,
                                &reader_pane,
                                mode,
                                Some(format!("terminal session stream failed: {error}")),
                            );
                            return;
                        }
                    }
                }
            })
            .map(|_| ())
            .map_err(|error| format!("terminal session reader could not be started: {error}"))
    }

    pub fn write_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        let Some(writer) = self.writer.as_ref() else {
            return Err(format!(
                "Pane {} is read-only because another client owns terminal control",
                self.pane_id
            ));
        };
        let line = terminal_input_line(bytes)?;
        writer
            .send(line)
            .map_err(|_| "terminal control input channel is closed".to_owned())
    }

    pub fn scroll(&self, direction: &str, lines: u16) -> Result<(), String> {
        let Some(writer) = self.writer.as_ref() else {
            return Err(format!(
                "Pane {} is read-only because another client owns terminal control",
                self.pane_id
            ));
        };
        let line = terminal_scroll_line(direction, lines)?;
        writer
            .send(line)
            .map_err(|_| "terminal control scroll channel is closed".to_owned())
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        let Some(writer) = self.writer.as_ref() else {
            return Err(format!(
                "Pane {} is read-only because another client owns terminal control",
                self.pane_id
            ));
        };
        let line = terminal_resize_line(rows, cols)?;
        writer
            .send(line)
            .map_err(|_| "terminal control resize channel is closed".to_owned())
    }
}

fn spawn_terminal_control_writer(
    context: &LiveContext,
    pane_id: &str,
    generation: u64,
    mut stdin: ChildStdin,
) -> Result<Sender<String>, String> {
    let (sender, receiver) = channel::<String>();
    let writer_pane = pane_id.to_owned();
    let runtime = context.runtime.clone();
    let notifier = context.notifier.clone();
    thread::Builder::new()
        .name(format!("herdr-core-terminal-writer-{writer_pane}"))
        .spawn(move || {
            for line in receiver {
                if let Err(error) = stdin
                    .write_all(line.as_bytes())
                    .and_then(|()| stdin.flush())
                {
                    deliver_terminal_session_write_failure(
                        &runtime,
                        &notifier,
                        &writer_pane,
                        generation,
                        format!("terminal control write failed: {error}"),
                    );
                    return;
                }
            }
        })
        .map_err(|error| format!("terminal control writer could not be started: {error}"))?;
    Ok(sender)
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.send(terminal_release_line());
        }
        let Some(mut child) = self.child.take() else {
            return;
        };
        let pane_id = self.pane_id.clone();
        if let Err(error) = thread::Builder::new()
            .name(format!("herdr-core-terminal-reaper-{pane_id}"))
            .spawn(move || {
                for _ in 0..20 {
                    match child.try_wait() {
                        Ok(Some(_)) => return,
                        Ok(None) => thread::sleep(Duration::from_millis(10)),
                        Err(error) => {
                            eprintln!(
                                "{}",
                                json!({
                                    "component": "terminal_session",
                                    "kind": "terminal.session_status_failed",
                                    "pane_id": pane_id,
                                    "message": error.to_string(),
                                })
                            );
                            break;
                        }
                    }
                }
                if let Err(error) = child.kill() {
                    eprintln!(
                        "{}",
                        json!({
                            "component": "terminal_session",
                            "kind": "terminal.session_kill_failed",
                            "pane_id": pane_id,
                            "message": error.to_string(),
                        })
                    );
                }
                if let Err(error) = child.wait() {
                    eprintln!(
                        "{}",
                        json!({
                            "component": "terminal_session",
                            "kind": "terminal.session_wait_failed",
                            "pane_id": pane_id,
                            "message": error.to_string(),
                        })
                    );
                }
            })
        {
            eprintln!(
                "{}",
                json!({
                    "component": "terminal_session",
                    "kind": "terminal.session_reaper_spawn_failed",
                    "message": error.to_string(),
                })
            );
        }
    }
}

pub fn spawn_terminal_session(
    context: LiveContext,
    pane_id: String,
    generation: u64,
    mode: TerminalSessionMode,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    thread::Builder::new()
        .name(format!(
            "herdr-core-terminal-{}-spawn-{pane_id}",
            mode.as_str()
        ))
        .spawn(move || {
            let started = Instant::now();
            let result = TerminalSession::spawn(&context, &pane_id, generation, mode, rows, cols);
            let elapsed_ms = started.elapsed().as_millis();
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_terminal_session_spawn(
                    generation, &pane_id, mode, result, elapsed_ms, &context,
                ),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("terminal session worker could not be started: {error}"))
}

fn deliver_terminal_session_frame(
    runtime: &Weak<Mutex<Runtime>>,
    notifier: &ChangeNotifier,
    pane_id: &str,
    generation: u64,
    mode: TerminalSessionMode,
    bytes: &[u8],
) -> bool {
    let Some(runtime) = runtime.upgrade() else {
        return false;
    };
    let delivered = match runtime.lock() {
        Ok(mut guard) => guard.ingest_terminal_session_frame(pane_id, generation, mode, bytes),
        Err(_) => return false,
    };
    drop(runtime);
    if delivered {
        notifier.notify();
    }
    delivered
}

fn deliver_terminal_session_closed(
    runtime: &Weak<Mutex<Runtime>>,
    notifier: &ChangeNotifier,
    generation: u64,
    pane_id: &str,
    mode: TerminalSessionMode,
    reason: Option<String>,
) {
    let Some(runtime) = runtime.upgrade() else {
        return;
    };
    let delivered = match runtime.lock() {
        Ok(mut guard) => guard.ingest_terminal_session_closed(pane_id, generation, mode, reason),
        Err(_) => return,
    };
    drop(runtime);
    if delivered {
        notifier.notify();
    }
}

fn deliver_terminal_session_write_failure(
    runtime: &Weak<Mutex<Runtime>>,
    notifier: &ChangeNotifier,
    pane_id: &str,
    generation: u64,
    message: String,
) {
    let Some(runtime) = runtime.upgrade() else {
        return;
    };
    let delivered = match runtime.lock() {
        Ok(mut guard) => guard.ingest_terminal_session_write_failure(pane_id, generation, message),
        Err(_) => return,
    };
    drop(runtime);
    if delivered {
        notifier.notify();
    }
}

pub fn encode_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

pub fn decode_base64(value: &str) -> Result<Vec<u8>, String> {
    BASE64
        .decode(value)
        .map_err(|error| format!("base64 payload could not be decoded: {error}"))
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use serde_json::json;

    use super::*;

    /// `contracts/herdr-api.schema.json` is the canonical Herdr API contract,
    /// synced from Herdr itself by `scripts/sync-herdr-contract.sh`. A core that
    /// silently spoke a different revision than the contract would fail at
    /// runtime with an empty sidebar, so the divergence is caught here instead.
    #[test]
    fn protocol_revision_matches_the_canonical_contract() {
        const CONTRACT: &str = include_str!("../../contracts/herdr-api.schema.json");

        let schema: Value = serde_json::from_str(CONTRACT).expect("contract is valid JSON");
        assert_eq!(schema["protocol"], HERDR_PROTOCOL_REVISION);
    }

    #[test]
    fn official_terminal_ndjson_boundary_decodes_frames_and_closed_reasons() {
        let frame = parse_terminal_session_line(
            r#"{"type":"terminal.frame","seq":7,"encoding":"ansi","width":100,"height":30,"full":true,"bytes":"G1szMW0="}"#,
        )
        .expect("frame parses");
        assert_eq!(
            frame,
            TerminalSessionEvent::Frame {
                seq: 7,
                width: 100,
                height: 30,
                full: true,
                bytes: b"\x1b[31m".to_vec(),
            }
        );

        let reason = "terminal attach failed: terminal 42 already has an attached client; retry with --takeover";
        let closed = parse_terminal_session_line(&format!(
            r#"{{"type":"terminal.closed","reason":{}}}"#,
            serde_json::to_string(reason).expect("reason JSON")
        ))
        .expect("closed parses");
        assert_eq!(
            closed,
            TerminalSessionEvent::Closed {
                reason: Some(reason.to_owned())
            }
        );
        assert_eq!(terminal_closed_category(Some(reason)), "owner_conflict");
        assert_eq!(terminal_closed_category(None), "transport_eof");
    }

    #[test]
    fn official_terminal_control_boundary_encodes_input_resize_and_release() {
        let input: Value = serde_json::from_str(
            terminal_input_line(b"hello\n")
                .expect("input line")
                .trim_end(),
        )
        .expect("input JSON");
        assert_eq!(input["type"], "terminal.input");
        assert_eq!(input["bytes"], "aGVsbG8K");

        let resize: Value = serde_json::from_str(
            terminal_resize_line(30, 100)
                .expect("resize line")
                .trim_end(),
        )
        .expect("resize JSON");
        assert_eq!(resize["type"], "terminal.resize");
        assert_eq!(resize["cols"], 100);
        assert_eq!(resize["rows"], 30);
        assert_eq!(resize["cell_width_px"], 0);
        assert_eq!(resize["cell_height_px"], 0);

        let release: Value =
            serde_json::from_str(terminal_release_line().trim_end()).expect("release JSON");
        assert_eq!(release, json!({"type": "terminal.release"}));
    }

    #[test]
    fn official_terminal_cli_arguments_never_request_takeover() {
        assert_eq!(
            terminal_session_arguments(TerminalSessionMode::Control, "w1:p2", 30, 100),
            [
                "terminal", "session", "control", "w1:p2", "--cols", "100", "--rows", "30"
            ]
        );
        assert_eq!(
            terminal_session_arguments(TerminalSessionMode::Observe, "w1:p2", 30, 100),
            [
                "terminal", "session", "observe", "w1:p2", "--cols", "100", "--rows", "30"
            ]
        );
        assert!(
            terminal_session_arguments(TerminalSessionMode::Control, "w1:p2", 30, 100)
                .iter()
                .all(|argument| argument != "--takeover")
        );
    }

    #[test]
    fn pane_control_plans_right_down_and_zoom_without_shell_interpolation() {
        assert_eq!(
            pane_control_arguments(&PaneControlAction::Split {
                pane_id: "w1:p1".to_owned(),
                direction: PaneSplitDirection::Right,
                cwd: Some("/tmp/herdr-ide-verify-shortcuts".to_owned()),
            }),
            [
                "pane",
                "split",
                "w1:p1",
                "--direction",
                "right",
                "--cwd",
                "/tmp/herdr-ide-verify-shortcuts",
            ]
        );
        assert_eq!(
            pane_control_arguments(&PaneControlAction::Split {
                pane_id: "w1:p1".to_owned(),
                direction: PaneSplitDirection::Down,
                cwd: None,
            }),
            ["pane", "split", "w1:p1", "--direction", "down"]
        );
        assert_eq!(
            pane_control_arguments(&PaneControlAction::ToggleZoom {
                pane_id: "w1:p1".to_owned(),
            }),
            ["pane", "zoom", "w1:p1", "--toggle"]
        );
        assert_eq!(
            pane_control_arguments(&PaneControlAction::Close {
                pane_id: "w1:p1".to_owned(),
            }),
            ["pane", "close", "w1:p1"]
        );
    }

    #[test]
    fn focus_uses_the_direct_socket_contract_before_reading_authoritative_layout() {
        // A Unix socket path is capped at ~104 bytes, so it cannot be built
        // from TMPDIR: a sandboxed test runner points that at a deep path and
        // the bind fails before the test has said anything about focus.
        let root = std::path::PathBuf::from("/tmp")
            .join(format!("herdr-core-focus-contract-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create socket directory");
        let socket_path = root.join("herdr.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake herdr socket");
        let server = std::thread::spawn(move || {
            for expected_method in ["pane.focus", "pane.layout"] {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut line = String::new();
                BufReader::new(stream.try_clone().expect("clone stream"))
                    .read_line(&mut line)
                    .expect("read request");
                let request: Value = serde_json::from_str(&line).expect("request JSON");
                assert_eq!(request["method"], expected_method);
                assert_eq!(request["params"]["pane_id"], "fixture:p2");
                let result = if expected_method == "pane.focus" {
                    json!({"pane": {"pane_id": "fixture:p2"}})
                } else {
                    json!({
                        "layout": {
                            "workspace_id": "fixture",
                            "tab_id": "fixture:t1",
                            "zoomed": false,
                            "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                            "focused_pane_id": "fixture:p2",
                            "panes": [
                                {"pane_id": "fixture:p1", "rect": {"x": 0, "y": 0, "width": 60, "height": 60}},
                                {"pane_id": "fixture:p2", "rect": {"x": 60, "y": 0, "width": 60, "height": 60}}
                            ],
                            "splits": [
                                {"direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 120, "height": 60}}
                            ]
                        }
                    })
                };
                writeln!(stream, "{}", json!({"id": request["id"], "result": result}))
                    .expect("write response");
            }
        });

        request(&socket_path, "pane.focus", json!({"pane_id": "fixture:p2"}))
            .expect("focus request");
        let layout = fetch_pane_layout(&socket_path, "fixture:p2").expect("focused layout");
        assert_eq!(layout.focused_pane_id, "fixture:p2");
        assert_eq!(layout.pane_ids(), ["fixture:p1", "fixture:p2"]);

        server.join().expect("fake server joins");
        std::fs::remove_file(&socket_path).expect("remove socket");
        std::fs::remove_dir(&root).expect("remove socket directory");
    }

    #[test]
    fn wire_snapshot_projects_agents_with_workspace_labels_and_verbatim_tokens() {
        let snapshot = json!({
            "protocol": HERDR_PROTOCOL_REVISION,
            "workspaces": [
                {"workspace_id": "w1", "label": "herdr-ide", "active_tab_id": "w1:t1"},
            ],
            "layouts": [],
            "agents": [
                {
                    "pane_id": "w1:p1",
                    "workspace_id": "w1",
                    "agent": "claude",
                    "agent_status": "working",
                    "cwd": "/tmp/project",
                    "tokens": {
                        "status_working": "●",
                        "sort_rank": "04",
                        "activity": "1787963036671",
                        "summary": "doing things",
                        "elapsed": "6h"
                    }
                },
                {
                    "pane_id": "w9:p2",
                    "workspace_id": "w9",
                    "tokens": {"status_idle": "○", "sort_rank": "10", "activity": "1787963036672"}
                }
            ]
        });
        let payload = project_session(&snapshot).expect("projects");
        assert_eq!(payload.agents.len(), 2);
        assert_eq!(payload.agents[0].pane_id.as_deref(), Some("w1:p1"));
        assert_eq!(
            payload.agents[0].workspace_label.as_deref(),
            Some("herdr-ide")
        );
        assert_eq!(
            payload.agents[0].tokens.get("status_working"),
            Some(&json!("●"))
        );
        // A workspace without a label entry falls back to its id.
        assert_eq!(payload.agents[1].workspace_label.as_deref(), Some("w9"));

        let projected = crate::sidebar::project_agents(payload).agents;
        assert_eq!(projected[0].state, "working");
        assert_eq!(projected[1].state, "idle");
    }

    #[test]
    fn wire_snapshot_preserves_herdr_tab_labels() {
        let snapshot = json!({
            "protocol": HERDR_PROTOCOL_REVISION,
            "workspaces": [{"workspace_id": "w1", "label": "verify", "active_tab_id": "w1:t1"}],
            "tabs": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "label": "2",
                "number": 2,
                "focused": true,
                "pane_count": 1,
                "agent_status": "idle"
            }],
            "layouts": [],
            "agents": []
        });

        let payload = project_session(&snapshot).expect("projects tab metadata");
        assert_eq!(payload.tabs.len(), 1);
        assert_eq!(payload.tabs[0].tab_id, "w1:t1");
        assert_eq!(payload.tabs[0].label, "2");
    }

    #[test]
    fn protocol_mismatch_is_an_explicit_failure() {
        let snapshot = json!({"protocol": 20, "workspaces": [], "agents": []});
        let error = project_session(&snapshot).expect_err("must fail");
        assert_eq!(error.state(), "protocol_mismatch");
        assert!(error.message().contains("20"));
    }

    #[test]
    fn session_layout_projects_authoritative_nested_tree_and_zoom() {
        let snapshot = json!({
            "protocol": HERDR_PROTOCOL_REVISION,
            "workspaces": [{"workspace_id": "w1", "label": "verify", "active_tab_id": "w1:t1"}],
            "agents": [],
            "focused_pane_id": "w1:p3",
            "layouts": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "zoomed": true,
                "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                "focused_pane_id": "w1:p3",
                "panes": [
                    {"pane_id": "w1:p1", "focused": false, "rect": {"x": 0, "y": 0, "width": 60, "height": 60}},
                    {"pane_id": "w1:p2", "focused": false, "rect": {"x": 60, "y": 0, "width": 60, "height": 30}},
                    {"pane_id": "w1:p3", "focused": true, "rect": {"x": 60, "y": 30, "width": 60, "height": 30}}
                ],
                "splits": [
                    {"id": "split_0_root", "direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 120, "height": 60}},
                    {"id": "split_1_1", "direction": "down", "ratio": 0.5, "rect": {"x": 60, "y": 0, "width": 60, "height": 60}}
                ]
            }]
        });

        let payload = project_session(&snapshot).expect("session projection");
        let layout = project_layout_for_pane(&payload, "w1:p3").expect("layout projection");
        assert_eq!(layout.workspace_id, "w1");
        assert_eq!(layout.tab_id, "w1:t1");
        assert_eq!(layout.focused_pane_id, "w1:p3");
        assert!(layout.zoomed);
        assert_eq!(layout.pane_ids(), ["w1:p1", "w1:p2", "w1:p3"]);
        assert_eq!(
            serde_json::to_value(&layout.root).expect("layout JSON"),
            json!({
                "type": "split",
                "direction": "right",
                "ratio": 0.5,
                "first": {"type": "pane", "pane_id": "w1:p1"},
                "second": {
                    "type": "split",
                    "direction": "down",
                    "ratio": 0.5,
                    "first": {"type": "pane", "pane_id": "w1:p2"},
                    "second": {"type": "pane", "pane_id": "w1:p3"}
                }
            })
        );
    }

    #[test]
    fn missing_socket_file_is_distinguished_from_unreachable() {
        let error =
            fetch_session(Path::new("/nonexistent/herdr-core-test.sock")).expect_err("must fail");
        assert_eq!(error.state(), "socket_missing");
    }
}
