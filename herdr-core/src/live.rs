//! Live herdr integration: session snapshot polling over the local API socket
//! and pane byte transport through `herdr pane attach` under a PTY.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ffi::ChangeNotifier;
use crate::model::{
    PaneLayoutDirection, PaneLayoutNodeSnapshot, PaneLayoutSnapshot, WorkspaceRegistration,
    WorkspaceSnapshot,
};
use crate::runtime::Runtime;
use crate::sidebar::{
    SessionAgentPayload, SessionLayoutPanePayload, SessionLayoutPayload, SessionLayoutRect,
    SessionPanePayload, SessionSnapshotPayload, SessionTabPayload,
};
use crate::workspace;

/// Herdr API protocol revision this core speaks. A mismatch is a hard,
/// explicit failure instead of a partially working sidebar.
pub const HERDR_PROTOCOL_REVISION: u64 = crate::herdr_contract::HERDR_PROTOCOL_REVISION as u64;

const API_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Everything an attach spawn needs from the live configuration.
#[derive(Clone)]
pub struct LiveContext {
    pub socket_path: PathBuf,
    pub herdr_bin: Option<PathBuf>,
    pub runtime: Weak<Mutex<Runtime>>,
    pub notifier: ChangeNotifier,
}

/// A workspace catalog the poller built outside the runtime lock, together
/// with the registrations it was built from so the runtime can detect and
/// discard a stale one.
pub struct PrecomputedCatalog {
    pub registrations: Vec<WorkspaceRegistration>,
    pub workspaces: Vec<WorkspaceSnapshot>,
}

/// How long a catalog built from unchanged inputs keeps being reused before
/// git is consulted again. Git topology changes made outside the app (a new
/// worktree, a branch switch) surface within this window; changes made
/// through the app rebuild inline in their own event handlers.
const CATALOG_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// The poller's memo of the last catalog build and the inputs it came from.
struct CatalogCache {
    registrations: Vec<WorkspaceRegistration>,
    spaces: Vec<workspace::SessionSpace>,
    workspaces: Vec<WorkspaceSnapshot>,
    built_at: Instant,
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

#[derive(Clone, Debug)]
pub enum PaneControlAction {
    Focus {
        pane_id: String,
    },
    Split {
        pane_id: String,
        direction: PaneSplitDirection,
        cwd: Option<String>,
    },
    ToggleZoom {
        pane_id: String,
    },
    Close {
        pane_id: String,
    },
}

#[derive(Debug)]
pub struct PaneControlOutcome {
    pub created_pane_id: Option<String>,
    pub layout: Option<PaneLayoutSnapshot>,
    pub layout_refresh_error: Option<String>,
}

fn execute_pane_control(
    context: &LiveContext,
    action: &PaneControlAction,
) -> Result<PaneControlOutcome, String> {
    if let PaneControlAction::Focus { pane_id } = action {
        request(
            &context.socket_path,
            "pane.focus",
            json!({"pane_id": pane_id}),
        )?;
        let layout = fetch_pane_layout(&context.socket_path, pane_id)?;
        return Ok(PaneControlOutcome {
            created_pane_id: None,
            layout: Some(layout),
            layout_refresh_error: None,
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
            PaneControlAction::Focus { .. }
            | PaneControlAction::ToggleZoom { .. }
            | PaneControlAction::Close { .. } => None,
        };
        let layout_pane_id = created_pane_id.as_deref().unwrap_or_else(|| match action {
            PaneControlAction::Focus { pane_id }
            | PaneControlAction::Split { pane_id, .. }
            | PaneControlAction::ToggleZoom { pane_id }
            | PaneControlAction::Close { pane_id } => pane_id,
        });
        let (layout, layout_refresh_error) = match action {
            PaneControlAction::Close { .. } => match fetch_session(&context.socket_path) {
                Ok(payload) => {
                    let target = payload.focused_pane_id.as_deref().or_else(|| {
                        payload
                            .layouts
                            .first()
                            .map(|layout| layout.focused_pane_id.as_str())
                    });
                    match target {
                        Some(pane_id) => match project_layout_for_pane(&payload, pane_id) {
                            Ok(layout) => (Some(layout), None),
                            Err(message) => (None, Some(message)),
                        },
                        None => (None, None),
                    }
                }
                Err(error) => (None, Some(error.message().to_owned())),
            },
            _ => match fetch_pane_layout(&context.socket_path, layout_pane_id) {
                Ok(layout) => (Some(layout), None),
                Err(message) => (None, Some(message)),
            },
        };
        return Ok(PaneControlOutcome {
            created_pane_id,
            layout,
            layout_refresh_error,
        });
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
        PaneControlAction::Focus { .. } => "herdr-core-pane-focus".to_owned(),
        PaneControlAction::Split { direction, .. } => {
            format!("herdr-core-pane-split-{}", direction.as_str())
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
        PaneControlAction::Focus { .. } => {
            unreachable!("pane focus uses the socket API instead of the CLI")
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
    /// The server answered but the payload did not match the expected shape.
    Malformed(String),
}

impl SessionFetchError {
    pub fn state(&self) -> &'static str {
        match self {
            Self::SocketMissing(_) => "socket_missing",
            Self::Unreachable(_) => "unreachable",
            Self::Protocol(_) => "protocol_mismatch",
            Self::Malformed(_) => "malformed",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::SocketMissing(message)
            | Self::Unreachable(message)
            | Self::Protocol(message)
            | Self::Malformed(message) => message,
        }
    }
}

/// Installs the live context on the runtime and starts the session poller.
pub fn install(
    runtime: &Arc<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    socket_path: &str,
    herdr_bin: Option<&str>,
) {
    let context = LiveContext {
        socket_path: PathBuf::from(socket_path),
        herdr_bin: herdr_bin.map(PathBuf::from),
        runtime: Arc::downgrade(runtime),
        notifier: notifier.clone(),
    };
    if let Ok(mut guard) = runtime.lock() {
        guard.set_live(context.clone());
    }
    spawn_session_poller(context);
}

fn spawn_session_poller(context: LiveContext) {
    let result = thread::Builder::new()
        .name("herdr-core-session-poller".to_owned())
        .spawn(move || {
            let mut catalog_cache: Option<CatalogCache> = None;
            loop {
                let fetched = fetch_session(&context.socket_path);
                // The catalog shells out to git per workspace and pane cwd,
                // so it is built here, outside the runtime lock; holding the
                // lock through those subprocesses stalls every shell snapshot
                // read behind them. It is also cached: git only runs again
                // when the inputs change or the refresh window lapses, not on
                // every poll tick.
                let precomputed = match &fetched {
                    Ok(payload) => {
                        let Some(runtime) = context.runtime.upgrade() else {
                            return;
                        };
                        let registrations = match runtime.lock() {
                            Ok(guard) => guard.snapshot().ui_state.workspace_registrations.clone(),
                            Err(_) => return,
                        };
                        drop(runtime);
                        let spaces = Runtime::session_spaces(payload);
                        let cache_is_fresh = catalog_cache.as_ref().is_some_and(|cache| {
                            cache.registrations == registrations
                                && cache.spaces == spaces
                                && cache.built_at.elapsed() < CATALOG_REFRESH_INTERVAL
                        });
                        if !cache_is_fresh {
                            let workspaces = workspace::build_catalog(&registrations, &spaces);
                            catalog_cache = Some(CatalogCache {
                                registrations: registrations.clone(),
                                spaces,
                                workspaces,
                                built_at: Instant::now(),
                            });
                        }
                        let cache = catalog_cache
                            .as_ref()
                            .expect("catalog cache is filled on a miss");
                        Some(PrecomputedCatalog {
                            registrations,
                            workspaces: cache.workspaces.clone(),
                        })
                    }
                    Err(_) => None,
                };
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_session_with_catalog(fetched, precomputed),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
                thread::sleep(POLL_INTERVAL);
            }
        });
    if let Err(error) = result {
        eprintln!(
            "{}",
            json!({
                "component": "live",
                "kind": "poller.spawn_failed",
                "message": error.to_string(),
            })
        );
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
    let protocol = snapshot
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| SessionFetchError::Malformed("snapshot is missing protocol".to_owned()))?;
    if protocol != HERDR_PROTOCOL_REVISION {
        return Err(SessionFetchError::Protocol(format!(
            "Herdr protocol revision {protocol} does not match required {HERDR_PROTOCOL_REVISION}"
        )));
    }

    let workspace_labels: std::collections::BTreeMap<&str, &str> = snapshot
        .get("workspaces")
        .and_then(Value::as_array)
        .map(|workspaces| {
            workspaces
                .iter()
                .filter_map(|workspace| {
                    Some((
                        workspace.get("workspace_id")?.as_str()?,
                        workspace.get("label")?.as_str()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let agents = snapshot
        .get("agents")
        .and_then(Value::as_array)
        .ok_or_else(|| SessionFetchError::Malformed("snapshot is missing agents".to_owned()))?
        .iter()
        .filter_map(|agent| {
            let pane_id = agent.get("pane_id")?.as_str()?.to_owned();
            let workspace_id = agent.get("workspace_id").and_then(Value::as_str);
            Some(SessionAgentPayload {
                id: Some(pane_id.clone()),
                pane_id: Some(pane_id),
                workspace_label: workspace_id
                    .and_then(|id| workspace_labels.get(id).copied())
                    .or(workspace_id)
                    .map(str::to_owned),
                cwd: agent.get("cwd").and_then(Value::as_str).map(str::to_owned),
                agent: agent
                    .get("agent")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                agent_status: agent
                    .get("agent_status")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                tokens: agent
                    .get("tokens")
                    .and_then(Value::as_object)
                    .map(|tokens| tokens.clone().into_iter().collect())
                    .unwrap_or_default(),
                ambient: agent.get("ambient").cloned(),
            })
        })
        .collect();

    let layouts =
        serde_json::from_value(snapshot.get("layouts").cloned().ok_or_else(|| {
            SessionFetchError::Malformed("snapshot is missing layouts".to_owned())
        })?)
        .map_err(|error| {
            SessionFetchError::Malformed(format!("snapshot layouts are malformed: {error}"))
        })?;
    let tabs = match snapshot.get("tabs") {
        Some(value) => {
            serde_json::from_value::<Vec<SessionTabPayload>>(value.clone()).map_err(|error| {
                SessionFetchError::Malformed(format!("snapshot tabs are malformed: {error}"))
            })?
        }
        None => Vec::new(),
    };
    let focused_pane_id = snapshot
        .get("focused_pane_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let panes = snapshot
        .get("panes")
        .and_then(Value::as_array)
        .map(|panes| {
            panes
                .iter()
                .filter_map(|pane| {
                    Some(SessionPanePayload {
                        pane_id: pane.get("pane_id")?.as_str()?.to_owned(),
                        cwd: pane.get("cwd").and_then(Value::as_str).map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let workspaces = workspace_labels
        .iter()
        .map(|(workspace_id, label)| crate::sidebar::SessionWorkspacePayload {
            workspace_id: (*workspace_id).to_owned(),
            label: (*label).to_owned(),
        })
        .collect();

    Ok(SessionSnapshotPayload {
        focused_pane_id,
        tabs,
        layouts,
        agents,
        panes,
        workspaces,
    })
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

fn request(socket_path: &Path, method: &str, params: Value) -> Result<Value, String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|error| format!("connect failed for {}: {error}", socket_path.display()))?;
    stream
        .set_read_timeout(Some(API_TIMEOUT))
        .map_err(|error| format!("read timeout could not be set: {error}"))?;
    stream
        .set_write_timeout(Some(API_TIMEOUT))
        .map_err(|error| format!("write timeout could not be set: {error}"))?;
    let envelope = json!({
        "id": format!("herdr-core:{method}"),
        "method": method,
        "params": params,
    });
    let mut request_line = serde_json::to_vec(&envelope)
        .map_err(|error| format!("request could not be encoded: {error}"))?;
    request_line.push(b'\n');
    stream
        .write_all(&request_line)
        .map_err(|error| format!("request could not be written: {error}"))?;

    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|error| format!("response could not be read: {error}"))?;
    if line.trim().is_empty() {
        return Err("response was empty".to_owned());
    }
    let response: Value = serde_json::from_str(&line)
        .map_err(|error| format!("response was not valid JSON: {error}"))?;
    if let Some(error) = response.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("herdr request failed");
        return Err(format!("{method} failed with {code}: {message}"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("{method} response is missing result"))
}

/// A live byte transport to one herdr pane: `herdr pane attach <pane_id>`
/// running under a local PTY. Dropping it kills the attach client.
pub struct PaneAttach {
    pub pane_id: String,
    pub generation: u64,
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn Child + Send + Sync>>,
    writer: Box<dyn Write + Send>,
    reader: Option<Box<dyn Read + Send>>,
}

impl PaneAttach {
    pub fn spawn(
        context: &LiveContext,
        pane_id: &str,
        generation: u64,
        rows: u16,
        cols: u16,
    ) -> Result<Self, String> {
        let Some(herdr_bin) = context.herdr_bin.as_ref() else {
            return Err(
                "herdr binary was not found; install herdr or set its path in the app options"
                    .to_owned(),
            );
        };
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("PTY could not be opened: {error}"))?;
        let mut command = CommandBuilder::new(herdr_bin);
        command.arg("pane");
        command.arg("attach");
        command.arg(pane_id);
        command.env("HERDR_SOCKET_PATH", &context.socket_path);
        command.env("TERM", "xterm-256color");
        command.env("LANG", "en_US.UTF-8");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("herdr pane attach could not be spawned: {error}"))?;
        drop(pair.slave);
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("PTY writer could not be taken: {error}"))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("PTY reader could not be cloned: {error}"))?;

        Ok(Self {
            pane_id: pane_id.to_owned(),
            generation,
            master: pair.master,
            child: Some(child),
            writer,
            reader: Some(reader),
        })
    }

    pub fn start_reader(
        &mut self,
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) -> Result<(), String> {
        let Some(mut reader) = self.reader.take() else {
            return Err("attach reader was already started".to_owned());
        };
        let generation = self.generation;
        let reader_pane = self.pane_id.clone();
        thread::Builder::new()
            .name(format!("herdr-core-attach-{reader_pane}"))
            .spawn(move || {
                let mut bytes = [0_u8; 8192];
                loop {
                    match reader.read(&mut bytes) {
                        Ok(0) => {
                            deliver_attach_exit(
                                &runtime,
                                &notifier,
                                generation,
                                &reader_pane,
                                format!(
                                    "Pane {reader_pane} attach ended; it may be attached elsewhere or closed"
                                ),
                            );
                            return;
                        }
                        Ok(count) => {
                            if !deliver_attach_output(
                                &runtime,
                                &notifier,
                                &reader_pane,
                                generation,
                                &bytes[..count],
                            ) {
                                return;
                            }
                        }
                        Err(error) => {
                            deliver_attach_exit(
                                &runtime,
                                &notifier,
                                generation,
                                &reader_pane,
                                format!("Pane {reader_pane} stream failed: {error}"),
                            );
                            return;
                        }
                    }
                }
            })
            .map(|_| ())
            .map_err(|error| format!("attach reader thread could not be started: {error}"))
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writer
            .write_all(bytes)
            .and_then(|()| self.writer.flush())
            .map_err(|error| format!("pane input could not be written: {error}"))
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), String> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("pane could not be resized: {error}"))
    }
}

impl Drop for PaneAttach {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let pane_id = self.pane_id.clone();
        if let Err(error) = child.kill() {
            eprintln!(
                "{}",
                json!({
                    "component": "live",
                    "kind": "pane.attach_kill_failed",
                    "pane_id": pane_id,
                    "message": error.to_string(),
                })
            );
        }
        if let Err(error) = thread::Builder::new()
            .name(format!("herdr-core-attach-reaper-{pane_id}"))
            .spawn(move || {
                if let Err(error) = child.wait() {
                    eprintln!(
                        "{}",
                        json!({
                            "component": "live",
                            "kind": "pane.attach_wait_failed",
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
                    "component": "live",
                    "kind": "pane.attach_reaper_spawn_failed",
                    "message": error.to_string(),
                })
            );
        }
    }
}

pub fn spawn_pane_attach(
    context: LiveContext,
    pane_id: String,
    generation: u64,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    thread::Builder::new()
        .name(format!("herdr-core-attach-spawn-{pane_id}"))
        .spawn(move || {
            let started = Instant::now();
            let result = PaneAttach::spawn(&context, &pane_id, generation, rows, cols);
            let elapsed_ms = started.elapsed().as_millis();
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => {
                    guard.ingest_attach_spawn(generation, &pane_id, result, elapsed_ms, &context)
                }
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("attach worker could not be started: {error}"))
}

fn deliver_attach_output(
    runtime: &Weak<Mutex<Runtime>>,
    notifier: &ChangeNotifier,
    pane_id: &str,
    generation: u64,
    bytes: &[u8],
) -> bool {
    let Some(runtime) = runtime.upgrade() else {
        return false;
    };
    let delivered = match runtime.lock() {
        Ok(mut guard) => guard.ingest_attach_output(pane_id, generation, bytes),
        Err(_) => return false,
    };
    drop(runtime);
    if delivered {
        notifier.notify();
    }
    delivered
}

fn deliver_attach_exit(
    runtime: &Weak<Mutex<Runtime>>,
    notifier: &ChangeNotifier,
    generation: u64,
    pane_id: &str,
    message: String,
) {
    let Some(runtime) = runtime.upgrade() else {
        return;
    };
    let delivered = match runtime.lock() {
        Ok(mut guard) => guard.ingest_attach_exit(pane_id, generation, message),
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
                {"workspace_id": "w1", "label": "herdr-ide"},
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
            "workspaces": [{"workspace_id": "w1", "label": "verify"}],
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
            "workspaces": [{"workspace_id": "w1", "label": "verify"}],
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
