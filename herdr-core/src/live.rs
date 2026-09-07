//! Live Herdr commands and pane byte transport. Session state synchronization
//! lives in `session_sync` and uses the sequenced socket event stream.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
#[cfg(test)]
use std::sync::mpsc::Receiver;
use std::sync::mpsc::{Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use crate::wire;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ffi::ChangeNotifier;
use crate::find::PaneFindOptions;
use crate::fork::{ForkRequest, fork_arguments};
use crate::herdr_api::{ApiConnector, UnixSocketConnector, request_with_connector};
#[cfg(test)]
use crate::herdr_api::{HERDR_PROTOCOL_REVISION, request};
use crate::model::{
    PaneLayoutDirection, PaneLayoutNodeSnapshot, PaneLayoutSnapshot, WorkspaceRegistration,
    WorkspaceSnapshot,
};
use crate::remote::RusshRemoteClient;
use crate::runtime::Runtime;
use crate::sidebar::{
    SessionLayoutPanePayload, SessionLayoutPayload, SessionLayoutRect, SessionSnapshotPayload,
};
use crate::workspace;

#[path = "worktree_control.rs"]
mod worktree_control;
pub use worktree_control::{spawn_worktree_close, spawn_worktree_open};

/// Everything a terminal session spawn needs from the live configuration.
#[derive(Clone)]
pub struct LiveContext {
    pub socket_path: PathBuf,
    pub herdr_bin: Option<PathBuf>,
    pub runtime: Weak<Mutex<Runtime>>,
    pub notifier: ChangeNotifier,
    pub(crate) api_connector: Arc<dyn ApiConnector>,
}

/// Everything an official remote terminal session needs. SSH transports the
/// CLI's NDJSON stream; pane state and terminal semantics remain Herdr-owned.
#[derive(Clone)]
pub struct RemoteTerminalContext {
    target_id: String,
    client: Arc<RusshRemoteClient>,
    socket_path: String,
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

impl RemoteTerminalContext {
    pub(crate) fn new(
        target_id: impl Into<String>,
        client: Arc<RusshRemoteClient>,
        socket_path: impl Into<String>,
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) -> Self {
        Self {
            target_id: target_id.into(),
            client,
            socket_path: socket_path.into(),
            runtime,
            notifier,
        }
    }

    pub(crate) fn target_id(&self) -> &str {
        &self.target_id
    }
}

#[derive(Clone)]
pub enum TerminalSessionContext {
    Local(LiveContext),
    Remote {
        context: RemoteTerminalContext,
        source_pane_id: String,
    },
}

impl TerminalSessionContext {
    fn runtime(&self) -> &Weak<Mutex<Runtime>> {
        match self {
            Self::Local(context) => &context.runtime,
            Self::Remote { context, .. } => &context.runtime,
        }
    }

    fn notifier(&self) -> &ChangeNotifier {
        match self {
            Self::Local(context) => &context.notifier,
            Self::Remote { context, .. } => &context.notifier,
        }
    }
}

pub struct WorkspaceCreationOutcome {
    pub registration: WorkspaceRegistration,
    pub base_registrations: Vec<WorkspaceRegistration>,
    pub registrations: Vec<WorkspaceRegistration>,
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub session: SessionSnapshotPayload,
    pub created_pane_id: Option<String>,
    pub git_init_error: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct CreatedWorkspace {
    pane_id: String,
}

pub fn spawn_workspace_creation(
    context: LiveContext,
    path: String,
    label: String,
    initialize_git: bool,
    base_registrations: Vec<WorkspaceRegistration>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-workspace-create".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let request_path = path.clone();
            // The catalog this worker builds replaces the navigator's, so it
            // must carry the worktree rows the reader has already found.
            // Building it from an empty catalog would drop every worktree
            // without a pane until the next read.
            let (worktrees, scratch_root) = context
                .runtime
                .upgrade()
                .and_then(|runtime| {
                    let read = runtime
                        .lock()
                        .ok()
                        .map(|guard| (guard.worktree_catalog(), guard.scratch_root()));
                    drop(runtime);
                    read
                })
                .unwrap_or_default();
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
                    let before = fetch_session_with_connector(context.api_connector.as_ref())
                        .map_err(|error| {
                            format!(
                                "session.snapshot before workspace creation failed: {}",
                                error.message()
                            )
                        })?;
                    let before_spaces = Runtime::session_spaces(&before, &scratch_root);
                    let before_catalog =
                        workspace::build_catalog(&registrations, &before_spaces, &worktrees);
                    let needs_herdr_workspace = before_catalog
                        .iter()
                        .any(|workspace| workspace.id == registration.id);
                    let created = needs_herdr_workspace
                        .then(|| {
                            create_herdr_workspace(
                                context.api_connector.as_ref(),
                                &registration.path,
                                &registration.label,
                            )
                        })
                        .transpose()?;
                    let session = if created.is_some() {
                        fetch_session_with_connector(context.api_connector.as_ref()).map_err(
                            |error| {
                                format!(
                                    "session.snapshot after workspace creation failed: {}",
                                    error.message()
                                )
                            },
                        )?
                    } else {
                        before
                    };
                    let spaces = Runtime::session_spaces(&session, &scratch_root);
                    let workspaces = workspace::build_catalog(&registrations, &spaces, &worktrees);
                    Ok(WorkspaceCreationOutcome {
                        registration,
                        base_registrations,
                        registrations,
                        workspaces,
                        session,
                        created_pane_id: created.map(|created| created.pane_id),
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

fn create_herdr_workspace(
    connector: &dyn ApiConnector,
    cwd: &str,
    label: &str,
) -> Result<CreatedWorkspace, String> {
    let result = control_request(
        connector,
        "workspace.create",
        wire::workspace_create_params(cwd, label)?,
    )?;
    let pane_id = wire::created_workspace_pane(result)?;
    Ok(CreatedWorkspace { pane_id })
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

impl PaneControlAction {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Project { .. } => "pane.project",
            Self::Focus { .. } => "pane.focus",
            Self::Split { .. } => "pane.split",
            Self::Resize { .. } => "pane.resize",
            Self::ToggleZoom { .. } => "pane.zoom",
            Self::Close { .. } => "pane.close",
        }
    }
}

#[derive(Debug)]
pub enum PaneControlOutcome {
    Projected { layout: PaneLayoutSnapshot },
    Acknowledged { created_pane_id: Option<String> },
}

#[derive(Clone, Debug)]
pub enum RemoteControlAction {
    Pane(PaneControlAction),
    FocusWorkspace {
        workspace_id: String,
    },
    FocusTab {
        tab_id: String,
    },
    CreateTab {
        workspace_id: String,
        cwd: String,
        label: String,
    },
    CloseTab {
        tab_id: String,
    },
    /// Asks Herdr to put one of its tabs at a new place in its workspace.
    ///
    /// Herdr owns the order of its own tabs, so this is a request, not a
    /// local edit: the strip keeps the order Herdr last reported until Herdr
    /// reports the new one. `expected_order` is the tab order the operator
    /// asked for *within the single Herdr workspace that owns the moved tab*,
    /// not across the checkout: a checkout can hold tabs from several
    /// workspaces, and `tab.move` answers with one workspace's list. It is
    /// carried so the acknowledgement can be checked against what was
    /// requested rather than assumed.
    MoveTab {
        checkout_id: String,
        tab_id: String,
        /// Herdr's insertion index, which counts positions in the tab list
        /// *before* the moved tab is taken out of it.
        insert_index: usize,
        expected_order: Vec<String>,
        /// Which reorder this request belongs to, so a result that a later
        /// drag has already superseded cannot cancel the newer one.
        generation: u64,
    },
}

impl RemoteControlAction {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Pane(action) => action.kind(),
            Self::FocusWorkspace { .. } => "workspace.focus",
            Self::FocusTab { .. } => "tab.focus",
            Self::CreateTab { .. } => "tab.create",
            Self::CloseTab { .. } => "tab.close",
            Self::MoveTab { .. } => "tab.move",
        }
    }
}

#[derive(Debug)]
pub enum RemoteControlOutcome {
    Acknowledged {
        created_tab_id: Option<String>,
        created_pane_id: Option<String>,
    },
    /// The workspace tab order Herdr reported back, in Herdr's order.
    TabsOrdered { tab_ids: Vec<String> },
}

#[derive(Clone)]
pub struct RemoteControlContext {
    target_id: String,
    api_connector: Arc<dyn ApiConnector>,
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

impl RemoteControlContext {
    pub(crate) fn new(
        target_id: impl Into<String>,
        api_connector: Arc<dyn ApiConnector>,
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) -> Self {
        Self {
            target_id: target_id.into(),
            api_connector,
            runtime,
            notifier,
        }
    }

    pub(crate) fn target_id(&self) -> &str {
        &self.target_id
    }
}

fn execute_remote_control(
    connector: &dyn ApiConnector,
    action: &RemoteControlAction,
) -> Result<RemoteControlOutcome, String> {
    let (created_tab_id, created_pane_id) = match action {
        RemoteControlAction::Pane(action) => match action {
            PaneControlAction::Focus { .. }
            | PaneControlAction::Split { .. }
            | PaneControlAction::ToggleZoom { .. }
            | PaneControlAction::Close { .. } => match execute_pane_control(connector, action)? {
                PaneControlOutcome::Acknowledged { created_pane_id } => (None, created_pane_id),
                PaneControlOutcome::Projected { .. } => {
                    return Err("remote pane mutation returned a layout projection".to_owned());
                }
            },
            PaneControlAction::Project { .. } | PaneControlAction::Resize { .. } => {
                return Err("unsupported remote pane control action".to_owned());
            }
        },
        RemoteControlAction::FocusWorkspace { workspace_id } => {
            control_request(
                connector,
                "workspace.focus",
                wire::workspace_target_params(workspace_id)?,
            )?;
            (None, None)
        }
        RemoteControlAction::FocusTab { tab_id } => {
            control_request(connector, "tab.focus", wire::tab_target_params(tab_id)?)?;
            (None, None)
        }
        RemoteControlAction::CreateTab {
            workspace_id,
            cwd,
            label,
        } => {
            let result = control_request(
                connector,
                "tab.create",
                wire::tab_create_params(workspace_id, cwd, label)?,
            )?;
            let (tab_id, pane_id) = wire::created_tab(result)?;
            (Some(tab_id), Some(pane_id))
        }
        RemoteControlAction::CloseTab { tab_id } => {
            control_request(connector, "tab.close", wire::tab_target_params(tab_id)?)?;
            (None, None)
        }
        RemoteControlAction::MoveTab {
            tab_id,
            insert_index,
            ..
        } => {
            // `tab.move` answers with the workspace's whole tab list in its
            // new order, so the caller can check what Herdr actually did
            // instead of assuming the request landed as asked.
            let result = control_request(
                connector,
                "tab.move",
                wire::tab_move_params(tab_id, *insert_index)?,
            )?;
            let tab_ids = wire::moved_tabs(result)?;
            return Ok(RemoteControlOutcome::TabsOrdered { tab_ids });
        }
    };
    Ok(RemoteControlOutcome::Acknowledged {
        created_tab_id,
        created_pane_id,
    })
}

fn execute_pane_control(
    connector: &dyn ApiConnector,
    action: &PaneControlAction,
) -> Result<PaneControlOutcome, String> {
    if let PaneControlAction::Project { pane_id } = action {
        return fetch_pane_layout(connector, pane_id)
            .map(|layout| PaneControlOutcome::Projected { layout });
    }
    if let PaneControlAction::Focus { pane_id } = action {
        control_request(connector, "pane.focus", wire::pane_target_params(pane_id)?)?;
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
        control_request(
            connector,
            "pane.resize",
            wire::pane_resize_params(pane_id, *direction, *amount)?,
        )?;
        return Ok(PaneControlOutcome::Acknowledged {
            created_pane_id: None,
        });
    }

    let created_pane_id = match action {
        PaneControlAction::Split {
            pane_id,
            direction,
            cwd,
        } => {
            // The user split to work in the new pane, so Herdr focuses it as
            // part of the split and the pane_focused event lands the shell's
            // selection there with no second round trip.
            let params = wire::pane_split_params(pane_id, *direction, cwd.as_deref())?;
            let result = control_request(connector, "pane.split", params)?;
            Some(wire::split_pane(result)?)
        }
        PaneControlAction::ToggleZoom { pane_id } => {
            control_request(connector, "pane.zoom", wire::pane_zoom_params(pane_id)?)?;
            None
        }
        PaneControlAction::Close { pane_id } => {
            control_request(connector, "pane.close", wire::pane_target_params(pane_id)?)?;
            None
        }
        PaneControlAction::Project { .. }
        | PaneControlAction::Focus { .. }
        | PaneControlAction::Resize { .. } => unreachable!("handled above"),
    };
    Ok(PaneControlOutcome::Acknowledged { created_pane_id })
}

fn control_request(
    connector: &dyn ApiConnector,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    request_with_connector(connector, method, params, Duration::from_secs(5))
        .map_err(|error| format!("{method} failed: {error}"))
}

fn fetch_pane_layout(
    connector: &dyn ApiConnector,
    pane_id: &str,
) -> Result<PaneLayoutSnapshot, String> {
    let result = control_request(connector, "pane.layout", wire::pane_layout_params(pane_id)?)?;
    let layout = wire::pane_layout(result)?;
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
            let result = execute_pane_control(context.api_connector.as_ref(), &action);
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

/// Runs one `herdr agent new` and reports what it produced.
///
/// This is a CLI wrapper rather than a socket request because agent lifecycle
/// is CLI-owned: the command starts a process, waits for the agent to come up,
/// and reports a startup failure as its own exit status. `HERDR_SOCKET_PATH` is
/// set from the live context for the same reason every other spawned herdr
/// process sets it - it is what keeps this build's commands on this build's
/// server.
/// How many lines of history a pane search asks Herdr for.
///
/// Herdr caps what it keeps; this is the ceiling on what is searched, and it
/// is reported alongside the count so a truncated buffer is visible rather
/// than quietly reported as the whole thing.
const PANE_FIND_LINE_LIMIT: u32 = 10_000;

/// What a pane search needs, gathered under the runtime mutex so the worker
/// carries no reference back into runtime state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneFindRequest {
    pub pane_id: String,
    pub term: String,
    pub options: PaneFindOptions,
    /// Which match to move to once the search lands: 0 keeps the current one,
    /// and stepping is relative so a search and a step share one path.
    pub step: i64,
    /// The index the shell is on now, so a step continues from it rather than
    /// restarting at the top every time.
    pub current_index: usize,
}

/// What a pane search found, in the shape the runtime stores.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaneFindOutcome {
    pub term: String,
    pub total: usize,
    /// 1-based position of the current match, or 0 when there is none.
    pub index: usize,
    pub truncated: bool,
    /// The viewport move that puts the current match on screen, as a direction
    /// and a line count. `None` when it is already there.
    pub scroll: Option<(String, u16)>,
}

/// Searches a pane's whole scrollback and moves the viewport to the match.
///
/// The two reads and the search happen on this thread, never under the runtime
/// mutex: the buffer is thousands of lines and every shell snapshot read blocks
/// on that mutex.
pub fn spawn_pane_find(context: LiveContext, request: PaneFindRequest) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-pane-find".to_owned())
        .spawn(move || {
            let pane_id = request.pane_id.clone();
            let result = run_pane_find(context.api_connector.as_ref(), &request);
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_pane_find(&pane_id, result),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("pane find worker could not be started: {error}"))
}

fn run_pane_find(
    connector: &dyn ApiConnector,
    request: &PaneFindRequest,
) -> Result<PaneFindOutcome, String> {
    if request.term.is_empty() {
        return Ok(PaneFindOutcome {
            term: String::new(),
            ..PaneFindOutcome::default()
        });
    }
    let buffer = read_pane_text(connector, &request.pane_id, "recent")?;
    let matches = crate::find::find_matches(&buffer.text, &request.term, &request.options)?;
    let total = matches.len();
    if total == 0 {
        return Ok(PaneFindOutcome {
            term: request.term.clone(),
            total: 0,
            index: 0,
            truncated: buffer.truncated,
            scroll: None,
        });
    }

    // Stepping wraps, because a search that stops at the end of the buffer
    // makes the reader guess whether there is more or they have gone round.
    let count = total as i64;
    let current = request.current_index as i64;
    let next = (current - 1 + request.step).rem_euclid(count);
    let target = matches[next as usize];

    let visible = read_pane_text(connector, &request.pane_id, "visible")?;
    let viewport_rows = visible.text.lines().count();
    let scroll = crate::find::viewport_anchor(&buffer.text, &visible.text)
        .map(|top| crate::find::scroll_delta(target.line, top, viewport_rows))
        .filter(|delta| *delta != 0)
        .and_then(|delta| {
            let lines = u16::try_from(delta.unsigned_abs()).ok()?;
            Some((if delta > 0 { "up" } else { "down" }.to_owned(), lines))
        });

    Ok(PaneFindOutcome {
        term: request.term.clone(),
        total,
        index: next as usize + 1,
        truncated: buffer.truncated,
        scroll,
    })
}

pub(crate) struct PaneText {
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

fn read_pane_text(
    connector: &dyn ApiConnector,
    pane_id: &str,
    source: &str,
) -> Result<PaneText, String> {
    let response = control_request(
        connector,
        "pane.read",
        wire::pane_read_params(pane_id, source, PANE_FIND_LINE_LIMIT)?,
    )?;
    wire::pane_text(response)
}

pub fn spawn_agent_fork(context: LiveContext, request: ForkRequest) -> Result<(), String> {
    let herdr_bin = context.herdr_bin.clone().ok_or_else(|| {
        "herdr binary was not found; install herdr or set its path in the app options".to_owned()
    })?;
    thread::Builder::new()
        .name("herdr-core-agent-fork".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let parent_pane_id = request.parent_pane_id.clone();
            let result = run_agent_fork(&herdr_bin, &context.socket_path, &request);
            let elapsed_ms = started.elapsed().as_millis();
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_fork_result(&parent_pane_id, result, elapsed_ms),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("fork worker could not be started: {error}"))
}

fn run_agent_fork(
    herdr_bin: &Path,
    socket_path: &Path,
    request: &ForkRequest,
) -> Result<String, String> {
    let output = Command::new(herdr_bin)
        .args(fork_arguments(request))
        .env("HERDR_SOCKET_PATH", socket_path)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("herdr agent new could not be run: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!("herdr agent new exited with {}", output.status)
        } else {
            stderr
        });
    }
    wire::created_agent_pane(&output.stdout)
}

pub fn spawn_remote_control(
    context: RemoteControlContext,
    request_id: String,
    action: RemoteControlAction,
) -> Result<(), String> {
    let target_id = context.target_id.clone();
    let worker_name = match &action {
        RemoteControlAction::Pane(PaneControlAction::Focus { .. }) => {
            format!("herdr-core-remote-{target_id}-pane-focus")
        }
        RemoteControlAction::Pane(PaneControlAction::Split { direction, .. }) => format!(
            "herdr-core-remote-{target_id}-pane-split-{}",
            direction.as_str()
        ),
        RemoteControlAction::Pane(PaneControlAction::ToggleZoom { .. }) => {
            format!("herdr-core-remote-{target_id}-pane-zoom")
        }
        RemoteControlAction::Pane(PaneControlAction::Close { .. }) => {
            format!("herdr-core-remote-{target_id}-pane-close")
        }
        RemoteControlAction::Pane(
            PaneControlAction::Project { .. } | PaneControlAction::Resize { .. },
        ) => return Err("unsupported remote pane control action".to_owned()),
        RemoteControlAction::FocusWorkspace { .. } => {
            format!("herdr-core-remote-{target_id}-workspace-focus")
        }
        RemoteControlAction::FocusTab { .. } => {
            format!("herdr-core-remote-{target_id}-tab-focus")
        }
        RemoteControlAction::CreateTab { .. } => {
            format!("herdr-core-remote-{target_id}-tab-create")
        }
        RemoteControlAction::CloseTab { .. } => {
            format!("herdr-core-remote-{target_id}-tab-close")
        }
        // A remote context browses Herdr's own tab order and owns no strip
        // slots, so there is nothing here for a reorder to move.
        RemoteControlAction::MoveTab { .. } => {
            return Err("a remote context's tab order cannot be reordered".to_owned());
        }
    };
    thread::Builder::new()
        .name(worker_name)
        .spawn(move || {
            let started = Instant::now();
            let result = execute_remote_control(context.api_connector.as_ref(), &action);
            let elapsed_ms = started.elapsed().as_millis();
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_remote_control_result(
                    &target_id,
                    &request_id,
                    action,
                    result,
                    elapsed_ms,
                ),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("remote control worker could not be started: {error}"))
}

pub fn spawn_local_control(
    context: LiveContext,
    action: RemoteControlAction,
) -> Result<(), String> {
    let worker_name = match &action {
        RemoteControlAction::FocusTab { .. } => "herdr-core-tab-focus",
        RemoteControlAction::CreateTab { .. } => "herdr-core-tab-create",
        RemoteControlAction::CloseTab { .. } => "herdr-core-tab-close",
        RemoteControlAction::MoveTab { .. } => "herdr-core-tab-move",
        _ => return Err(format!("{} is not a local tab action", action.kind())),
    };
    thread::Builder::new()
        .name(worker_name.to_owned())
        .spawn(move || {
            let started = Instant::now();
            let result = execute_remote_control(context.api_connector.as_ref(), &action);
            let elapsed_ms = started.elapsed().as_millis();
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_local_control_result(action, result, elapsed_ms),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("local tab control worker could not be started: {error}"))
}

/// What a Scratch tab needs to exist: the folder, and either the Herdr
/// workspace already holding Scratch or a new one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScratchTabRequest {
    /// The folder Scratch runs in. Created here when it is missing, because
    /// the first tab is what brings Scratch into existence.
    pub root: String,
    /// The Herdr workspace to add a tab to, or `None` to create the
    /// workspace. Herdr drops a workspace with its last pane, so Scratch is
    /// regularly in the second state without the operator having done
    /// anything.
    pub workspace_id: Option<String>,
    pub label: String,
}

/// Creates a Scratch tab off the runtime lock.
///
/// Three steps that can each fail, and each failure names its own step: a
/// folder that could not be created is not a Herdr error, and the operator is
/// owed the difference.
pub fn spawn_scratch_tab_creation(
    context: LiveContext,
    request: ScratchTabRequest,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-scratch-tab".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let result = create_scratch_tab(context.api_connector.as_ref(), &request);
            let elapsed_ms = started.elapsed().as_millis();
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_scratch_tab_result(result, elapsed_ms),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("scratch tab worker could not be started: {error}"))
}

fn create_scratch_tab(
    connector: &dyn ApiConnector,
    request: &ScratchTabRequest,
) -> Result<String, String> {
    // An existing folder is a success, so a second tab costs nothing and a
    // repeated request cannot fail on the folder it already made.
    std::fs::create_dir_all(&request.root).map_err(|error| {
        format!(
            "scratch folder: {} could not be created: {error}",
            request.root
        )
    })?;
    match request.workspace_id.as_deref() {
        Some(workspace_id) => {
            let result = control_request(
                connector,
                "tab.create",
                wire::tab_create_params(workspace_id, &request.root, &request.label)?,
            )
            .map_err(|error| format!("tab.create: {error}"))?;
            let (_, pane_id) = wire::created_tab(result)?;
            Ok(pane_id)
        }
        None => {
            let created = create_herdr_workspace(connector, &request.root, &request.label)
                .map_err(|error| format!("workspace.create: {error}"))?;
            Ok(created.pane_id)
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
    match crate::session_sync::spawn(
        crate::session_sync::SessionSyncContext::local(&context),
        home_path,
    ) {
        Ok(handle) => Some(handle),
        Err(message) => {
            crate::diagnostic!(json!({
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
    fetch_session_with_connector(&UnixSocketConnector::new(socket_path))
}

fn fetch_session_with_connector(
    connector: &dyn ApiConnector,
) -> Result<SessionSnapshotPayload, SessionFetchError> {
    let result = request_with_connector(
        connector,
        "session.snapshot",
        wire::empty_params(),
        Duration::from_secs(5),
    )
    .map_err(|error| SessionFetchError::Unreachable(error.to_string()))?;
    wire::live_session_response(result)
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

/// Every tab's layout in the session, in the order Herdr sent them, plus the
/// tabs whose layout could not be read.
///
/// One unreadable layout excludes only its own tab. Refusing the whole
/// session for it would empty every other tab's canvas over a fault in one,
/// which is the failure shape the caller is being given all of them to avoid.
pub fn project_layouts(
    payload: &SessionSnapshotPayload,
) -> (Vec<PaneLayoutSnapshot>, Vec<(String, String)>) {
    let mut layouts = Vec::with_capacity(payload.layouts.len());
    let mut rejected = Vec::new();
    for layout in &payload.layouts {
        match project_layout(layout) {
            Ok(projected) => layouts.push(projected),
            Err(reason) => rejected.push((layout.tab_id.clone(), reason)),
        }
    }
    (layouts, rejected)
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
    wire::terminal_input_line(bytes)
}

pub fn terminal_resize_line(rows: u16, cols: u16) -> Result<String, String> {
    wire::terminal_resize_line(rows, cols)
}

pub fn terminal_release_line() -> String {
    wire::terminal_release_line()
}

pub fn terminal_closed_category(reason: Option<&str>) -> &'static str {
    let Some(reason) = reason else {
        return "transport_eof";
    };
    let normalized = reason.to_ascii_lowercase();
    if normalized == "terminal attach taken over"
        || (normalized.contains("already has an attached client")
            && normalized.contains("retry with --takeover"))
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
    cleanup: Option<TerminalSessionCleanup>,
    writer: Option<Sender<TerminalWriterCommand>>,
    reader: Option<Box<dyn Read + Send>>,
    /// The stub's own end of its writer channel. A test asks what a pane was
    /// told to do by reading the lines the session would have sent, so the
    /// receiver has to outlive the send; dropping it here would close the
    /// channel and turn every write into an error.
    #[cfg(test)]
    written: Option<Receiver<TerminalWriterCommand>>,
}

enum TerminalSessionCleanup {
    Local(Child),
    Remote(Box<dyn FnOnce() + Send>),
}

/// A wheel carries the pointer's cell and modifiers because Herdr uses them
/// when the application tracks the mouse. Coordinates are zero-based.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScrollRequest {
    pub lines: i32,
    pub column: Option<u16>,
    pub row: Option<u16>,
    pub modifiers: u8,
}

impl ScrollRequest {
    fn line(self) -> Result<String, String> {
        wire::terminal_scroll_line(
            if self.lines > 0 { "up" } else { "down" },
            self.lines.unsigned_abs().min(u16::MAX.into()) as u16,
            self.column,
            self.row,
            self.modifiers,
        )
    }
}

enum TerminalWriterCommand {
    Scroll(ScrollRequest),
    Resize {
        line: String,
    },
    Input {
        line: String,
        trace: Option<crate::model::TerminalInputTrace>,
    },
    Release {
        line: String,
        acknowledged: Sender<()>,
    },
}

impl TerminalSession {
    #[cfg(test)]
    pub fn test_stub(pane_id: &str, generation: u64, mode: TerminalSessionMode) -> Self {
        // The stub keeps a writer so a test can read what the runtime asked
        // Herdr to do. A session with no writer is the read-only case, and
        // standing in for a controlling session with one would make every
        // write look like a client conflict.
        let (writer, written) = channel();
        Self {
            pane_id: pane_id.to_owned(),
            generation,
            mode,
            cleanup: None,
            writer: Some(writer),
            reader: None,
            written: Some(written),
        }
    }

    /// Every protocol line this session was asked to write, oldest first.
    #[cfg(test)]
    pub fn test_written_lines(&self) -> Vec<String> {
        let Some(written) = self.written.as_ref() else {
            return Vec::new();
        };
        written
            .try_iter()
            .filter_map(|command| match command {
                TerminalWriterCommand::Resize { line, .. }
                | TerminalWriterCommand::Input { line, .. } => Some(line),
                TerminalWriterCommand::Release { line, .. } => Some(line),
                TerminalWriterCommand::Scroll(request) => request.line().ok(),
            })
            .collect()
    }

    pub fn spawn(
        context: &TerminalSessionContext,
        pane_id: &str,
        generation: u64,
        mode: TerminalSessionMode,
        rows: u16,
        cols: u16,
    ) -> Result<Self, String> {
        match context {
            TerminalSessionContext::Local(context) => {
                Self::spawn_local(context, pane_id, generation, mode, rows, cols)
            }
            TerminalSessionContext::Remote {
                context,
                source_pane_id,
            } => Self::spawn_remote(
                context,
                pane_id,
                source_pane_id,
                generation,
                mode,
                rows,
                cols,
            ),
        }
    }

    fn spawn_local(
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
                context.runtime.clone(),
                context.notifier.clone(),
                pane_id,
                generation,
                Box::new(stdin),
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
            cleanup: Some(TerminalSessionCleanup::Local(child)),
            reader: Some(Box::new(reader)),
            #[cfg(test)]
            written: None,
            writer,
        })
    }

    fn spawn_remote(
        context: &RemoteTerminalContext,
        pane_id: &str,
        source_pane_id: &str,
        generation: u64,
        mode: TerminalSessionMode,
        rows: u16,
        cols: u16,
    ) -> Result<Self, String> {
        let process = context
            .client
            .open_terminal_session(
                &context.socket_path,
                source_pane_id,
                mode.as_str(),
                rows,
                cols,
            )
            .map_err(|error| error.to_string())?;
        let (reader, transport_writer, shutdown) = process.into_parts();
        let writer = match (mode, transport_writer) {
            (TerminalSessionMode::Control, Some(writer)) => {
                match spawn_terminal_control_writer(
                    context.runtime.clone(),
                    context.notifier.clone(),
                    pane_id,
                    generation,
                    writer,
                ) {
                    Ok(writer) => Some(writer),
                    Err(error) => {
                        shutdown();
                        return Err(error);
                    }
                }
            }
            (TerminalSessionMode::Control, None) => {
                shutdown();
                return Err("remote terminal control stream has no writer".to_owned());
            }
            (TerminalSessionMode::Observe, None) => None,
            (TerminalSessionMode::Observe, Some(_)) => {
                shutdown();
                return Err("remote terminal observer unexpectedly exposed a writer".to_owned());
            }
        };
        Ok(Self {
            pane_id: pane_id.to_owned(),
            generation,
            mode,
            cleanup: Some(TerminalSessionCleanup::Remote(shutdown)),
            writer,
            reader: Some(reader),
            #[cfg(test)]
            written: None,
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
                            Ok(TerminalSessionEvent::Frame {
                                bytes,
                                width,
                                height,
                                full,
                                ..
                            }) => {
                                if !deliver_terminal_session_frame(
                                    &runtime,
                                    &notifier,
                                    &reader_pane,
                                    generation,
                                    mode,
                                    &bytes,
                                    crate::model::TerminalFrame {
                                        width,
                                        height,
                                        full,
                                    },
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

    pub fn write_bytes(
        &self,
        bytes: &[u8],
        trace: Option<crate::model::TerminalInputTrace>,
    ) -> Result<(), String> {
        let Some(writer) = self.writer.as_ref() else {
            return Err(format!(
                "Pane {} is read-only because another client owns terminal control",
                self.pane_id
            ));
        };
        let line = terminal_input_line(bytes)?;
        writer
            .send(TerminalWriterCommand::Input { line, trace })
            .map_err(|_| "terminal control input channel is closed".to_owned())
    }

    pub fn scroll(&self, request: ScrollRequest) -> Result<(), String> {
        let writer = self.writer.as_ref().ok_or_else(|| {
            format!(
                "Pane {} is read-only because another client owns terminal control",
                self.pane_id
            )
        })?;
        writer
            .send(TerminalWriterCommand::Scroll(request))
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
            .send(TerminalWriterCommand::Resize { line })
            .map_err(|_| "terminal control resize channel is closed".to_owned())?;
        // The writer thread reports a failed write on the pane, so a queued
        // resize with no failure after it is one the session received.
        crate::diagnostic!(json!({
                "component": "terminal_session",
                "kind": "terminal.resize_queued", "pane_id": self.pane_id,
                "generation": self.generation, "rows": rows, "cols": cols,
            })
        );
        Ok(())
    }
}

fn monotonic_ns() -> u64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    #[cfg(target_os = "macos")]
    let clock = libc::CLOCK_UPTIME_RAW;
    #[cfg(not(target_os = "macos"))]
    let clock = libc::CLOCK_MONOTONIC;
    // Both ends of a macOS input interval use CLOCK_UPTIME_RAW.
    let result = unsafe { libc::clock_gettime(clock, &mut time) };
    assert_eq!(result, 0, "monotonic clock unavailable");
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}

fn spawn_terminal_control_writer(
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    pane_id: &str,
    generation: u64,
    mut stdin: Box<dyn Write + Send>,
) -> Result<Sender<TerminalWriterCommand>, String> {
    let (sender, receiver) = channel::<TerminalWriterCommand>();
    let writer_pane = pane_id.to_owned();
    thread::Builder::new()
        .name(format!("herdr-core-terminal-writer-{writer_pane}"))
        .spawn(move || {
            let mut carried = None;
            loop {
                let command = match carried.take() {
                    Some(command) => command,
                    None => match receiver.recv() {
                        Ok(command) => command,
                        Err(_) => return,
                    },
                };

                let (line, release_acknowledgement, is_release, trace) = match command {
                    TerminalWriterCommand::Scroll(mut request) => {
                        // The shell has already converted precise trackpad
                        // movement into whole rows. Combine only wheels that
                        // are waiting in the channel right now. Never wait for
                        // a terminal frame or a timer: neither is an
                        // acknowledgement for this request.
                        let mut disconnected = false;
                        loop {
                            match receiver.try_recv() {
                                Ok(TerminalWriterCommand::Scroll(next)) => {
                                    request = ScrollRequest {
                                        lines: request.lines.saturating_add(next.lines),
                                        ..next
                                    };
                                }
                                Ok(command) => {
                                    carried = Some(command);
                                    break;
                                }
                                Err(TryRecvError::Empty) => break,
                                Err(TryRecvError::Disconnected) => {
                                    disconnected = true;
                                    break;
                                }
                            }
                        }
                        if request.lines == 0 {
                            if disconnected {
                                return;
                            }
                            continue;
                        }
                        let line = match request.line() {
                            Ok(line) => line,
                            Err(message) => {
                                deliver_terminal_session_write_failure(
                                    &runtime,
                                    &notifier,
                                    &writer_pane,
                                    generation,
                                    message,
                                );
                                return;
                            }
                        };
                        (line, None, false, None)
                    }
                    TerminalWriterCommand::Resize { line } => (line, None, false, None),
                    TerminalWriterCommand::Input { line, trace } => (line, None, false, trace),
                    TerminalWriterCommand::Release { line, acknowledged } => {
                        (line, Some(acknowledged), true, None)
                    }
                };
                let result = stdin
                    .write_all(line.as_bytes())
                    .and_then(|()| stdin.flush());
                if let Some(trace) = trace {
                    // Stamp completion before waiting on Runtime to publish it.
                    let elapsed =
                        monotonic_ns().saturating_sub(trace.started_ns) as f64 / 1_000_000.0;
                    let sent = crate::model::TerminalInputSent {
                        id: trace.id,
                        milliseconds: elapsed,
                        outcome: if result.is_ok() {
                            "completed"
                        } else {
                            "write_failed"
                        }
                        .to_owned(),
                    };
                    if let Some(runtime) = runtime.upgrade() {
                        let changed = runtime
                            .lock()
                            .map(|mut r| {
                                r.ingest_terminal_input_sent(&writer_pane, generation, sent)
                            })
                            .unwrap_or(false);
                        if changed {
                            notifier.notify();
                        }
                    }
                }
                if let Some(acknowledgement) = release_acknowledgement {
                    let _ = acknowledgement.send(());
                }
                if let Err(error) = result {
                    // A release is the session letting go; a child that
                    // already left, the pane having closed under it, has
                    // nothing to be told and no failure to report.
                    if !is_release {
                        deliver_terminal_session_write_failure(
                            &runtime,
                            &notifier,
                            &writer_pane,
                            generation,
                            format!("terminal control write failed: {error}"),
                        );
                    }
                    return;
                }
                if is_release {
                    return;
                }
            }
        })
        .map_err(|error| format!("terminal control writer could not be started: {error}"))?;
    Ok(sender)
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let release_acknowledgement = self.writer.take().and_then(|writer| {
            let (acknowledged, acknowledgement) = channel();
            writer
                .send(TerminalWriterCommand::Release {
                    line: terminal_release_line(),
                    acknowledged,
                })
                .ok()
                .map(|()| acknowledgement)
        });
        let Some(cleanup) = self.cleanup.take() else {
            return;
        };
        let pane_id = self.pane_id.clone();
        if let Err(error) = thread::Builder::new()
            .name(format!("herdr-core-terminal-reaper-{pane_id}"))
            .spawn(move || {
                if let Some(acknowledgement) = release_acknowledgement
                    && acknowledgement
                        .recv_timeout(Duration::from_secs(1))
                        .is_err()
                {
                    crate::diagnostic!(json!({
                            "component": "terminal_session",
                            "kind": "terminal.release_unacknowledged",
                            "pane_id": pane_id,
                        })
                    );
                }
                match cleanup {
                    TerminalSessionCleanup::Local(mut child) => {
                        reap_local_terminal_child(&mut child, &pane_id)
                    }
                    TerminalSessionCleanup::Remote(shutdown) => shutdown(),
                }
            })
        {
            crate::diagnostic!(json!({
                    "component": "terminal_session",
                    "kind": "terminal.session_reaper_spawn_failed",
                    "message": error.to_string(),
                })
            );
        }
    }
}

fn reap_local_terminal_child(child: &mut Child, pane_id: &str) {
    for _ in 0..20 {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                crate::diagnostic!(json!({
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
        crate::diagnostic!(json!({
                "component": "terminal_session",
                "kind": "terminal.session_kill_failed",
                "pane_id": pane_id,
                "message": error.to_string(),
            })
        );
    }
    if let Err(error) = child.wait() {
        crate::diagnostic!(json!({
                "component": "terminal_session",
                "kind": "terminal.session_wait_failed",
                "pane_id": pane_id,
                "message": error.to_string(),
            })
        );
    }
}

pub fn spawn_terminal_session(
    context: TerminalSessionContext,
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
            let worker_runtime = context.runtime().clone();
            let notifier = context.notifier().clone();
            let Some(runtime) = worker_runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_terminal_session_spawn(
                    generation,
                    &pane_id,
                    mode,
                    result,
                    elapsed_ms,
                    worker_runtime,
                    notifier.clone(),
                ),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                notifier.notify();
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
    frame: crate::model::TerminalFrame,
) -> bool {
    let Some(runtime) = runtime.upgrade() else {
        return false;
    };
    let delivered = match runtime.lock() {
        Ok(mut guard) => {
            guard.ingest_terminal_session_frame(pane_id, generation, mode, bytes, frame)
        }
        Err(_) => return false,
    };
    drop(runtime);
    if delivered == Some(true) {
        notifier.notify();
    }
    delivered.is_some()
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

    struct ScrollSink {
        pending: Vec<u8>,
        flushed: Sender<Vec<Value>>,
    }

    impl Write for ScrollSink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.pending.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            let lines = String::from_utf8(std::mem::take(&mut self.pending))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let _ = self.flushed.send(lines);
            Ok(())
        }
    }

    fn scroll_writer() -> (Sender<TerminalWriterCommand>, Receiver<Vec<Value>>) {
        let (flushed, received) = channel();
        let writer = spawn_terminal_control_writer(
            Weak::new(),
            ChangeNotifier::noop(),
            "fixture:p1",
            1,
            Box::new(ScrollSink {
                pending: Vec::new(),
                flushed,
            }),
        )
        .unwrap();
        (writer, received)
    }

    fn wheel(lines: i32, column: u16) -> TerminalWriterCommand {
        TerminalWriterCommand::Scroll(ScrollRequest {
            lines,
            column: Some(column),
            row: Some(12),
            modifiers: 2,
        })
    }

    #[test]
    fn first_wheel_reaches_the_pipe_without_waiting_for_a_frame() {
        let (writer, received) = scroll_writer();
        writer.send(wheel(3, 24)).unwrap();
        let lines = received.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(
            lines,
            vec![json!({
                "type": "terminal.scroll", "direction": "up", "lines": 3,
                "source": "wheel", "column": 24, "row": 12, "modifiers": 2,
            })]
        );
    }

    #[test]
    fn next_wheel_reaches_the_pipe_without_waiting_for_a_terminal_frame() {
        let (writer, received) = scroll_writer();
        writer.send(wheel(3, 24)).unwrap();
        received.recv_timeout(Duration::from_secs(1)).unwrap();

        writer.send(wheel(2, 25)).unwrap();
        let lines = received
            .recv_timeout(Duration::from_millis(50))
            .expect("a later wheel must not wait for an unrelated terminal frame");
        assert_eq!(lines[0]["lines"], 2);
        assert_eq!(lines[0]["column"], 25);
    }

    #[test]
    fn keyboard_input_keeps_its_order_after_scroll_input() {
        let (writer, received) = scroll_writer();
        writer.send(wheel(3, 24)).unwrap();
        received.recv_timeout(Duration::from_secs(1)).unwrap();
        writer.send(wheel(2, 25)).unwrap();
        writer
            .send(TerminalWriterCommand::Input {
                line: terminal_input_line(b"x").unwrap(),
                trace: None,
            })
            .unwrap();
        let scroll = received.recv_timeout(Duration::from_secs(1)).unwrap();
        let input = received.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(scroll[0]["type"], "terminal.scroll");
        assert_eq!(input[0]["type"], "terminal.input");
    }

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
        assert_eq!(
            terminal_closed_category(Some("terminal attach taken over")),
            "owner_conflict"
        );
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
    fn workspace_creation_uses_the_official_focused_root_pane_contract() {
        let root = std::path::PathBuf::from("/tmp").join(format!(
            "herdr-core-workspace-create-contract-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create socket directory");
        let socket_path = root.join("herdr.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake herdr socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut line)
                .expect("read request");
            let request: Value = serde_json::from_str(&line).expect("request JSON");
            assert_eq!(request["method"], "workspace.create");
            assert_eq!(
                request["params"],
                json!({
                    "cwd": "/tmp/herdr-ide-verify-workspace",
                    "focus": true,
                    "label": "Verify workspace",
                })
            );
            writeln!(
                stream,
                "{}",
                json!({
                    "id": request["id"],
                    "result": {
                        "type": "workspace_created",
                        "workspace": {"workspace_id": "w1", "number": 1, "label": "fixture", "focused": false, "pane_count": 1, "tab_count": 1, "active_tab_id": "w1:t1", "agent_status": "idle"},
                        "tab": {"tab_id": "w1:t1", "workspace_id": "w1", "number": 1, "label": "fixture", "focused": false, "pane_count": 1, "agent_status": "idle"},
                        "root_pane": {"pane_id": "w1:p1", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1},
                    }
                })
            )
            .expect("write response");
        });

        let created = create_herdr_workspace(
            &UnixSocketConnector::new(&socket_path),
            "/tmp/herdr-ide-verify-workspace",
            "Verify workspace",
        )
        .expect("workspace create request");
        assert_eq!(
            created,
            CreatedWorkspace {
                pane_id: "w1:p1".to_owned()
            }
        );

        server.join().expect("fake server joins");
        std::fs::remove_file(&socket_path).expect("remove socket");
        std::fs::remove_dir(&root).expect("remove socket directory");
    }

    #[test]
    fn pane_control_uses_the_official_socket_contract_for_every_mutation() {
        let root = std::path::PathBuf::from("/tmp")
            .join(format!("herdr-core-pane-control-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create socket directory");
        let socket_path = root.join("herdr.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake herdr socket");
        let server = std::thread::spawn(move || {
            let expected = [
                (
                    "pane.split",
                    json!({
                        "target_pane_id": "w1:p1",
                        "direction": "right",
                        "right_click": "herdr",
                        "focus": true,
                        "cwd": "/tmp/herdr-ide-verify-shortcuts"
                    }),
                ),
                (
                    "pane.split",
                    json!({
                        "target_pane_id": "w1:p1",
                        "direction": "down",
                        "right_click": "herdr",
                        "focus": true
                    }),
                ),
                ("pane.zoom", json!({"pane_id": "w1:p1", "mode": "toggle"})),
                ("pane.close", json!({"pane_id": "w1:p1"})),
            ];
            for (method, params) in expected {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut line = String::new();
                BufReader::new(stream.try_clone().expect("clone stream"))
                    .read_line(&mut line)
                    .expect("read request");
                let request: Value = serde_json::from_str(&line).expect("request JSON");
                assert_eq!(request["method"], method);
                assert_eq!(request["params"], params);
                let result = if method == "pane.split" {
                    json!({"type": "pane_info", "pane": {"pane_id": "w1:p2", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}})
                } else {
                    json!({"type": "ok"})
                };
                writeln!(
                    stream,
                    "{}",
                    wire::checked_response_fixture(&request["id"], result)
                )
                .expect("write response");
            }
        });
        let connector = UnixSocketConnector::new(&socket_path);
        for action in [
            PaneControlAction::Split {
                pane_id: "w1:p1".to_owned(),
                direction: PaneSplitDirection::Right,
                cwd: Some("/tmp/herdr-ide-verify-shortcuts".to_owned()),
            },
            PaneControlAction::Split {
                pane_id: "w1:p1".to_owned(),
                direction: PaneSplitDirection::Down,
                cwd: None,
            },
            PaneControlAction::ToggleZoom {
                pane_id: "w1:p1".to_owned(),
            },
            PaneControlAction::Close {
                pane_id: "w1:p1".to_owned(),
            },
        ] {
            execute_pane_control(&connector, &action).expect("control request");
        }
        server.join().expect("fake server joins");
        std::fs::remove_file(&socket_path).expect("remove socket");
        std::fs::remove_dir(&root).expect("remove socket directory");
    }

    #[test]
    fn remote_session_control_uses_the_official_socket_contract() {
        let root = std::path::PathBuf::from("/tmp")
            .join(format!("herdr-core-remote-control-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create socket directory");
        let socket_path = root.join("herdr.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake herdr socket");
        let server = std::thread::spawn(move || {
            let expected = [
                (
                    "workspace.focus",
                    json!({"workspace_id": "w1"}),
                    json!({"type": "ok"}),
                ),
                (
                    "tab.focus",
                    json!({"tab_id": "w1:t2"}),
                    json!({"type": "ok"}),
                ),
                (
                    "tab.create",
                    json!({
                        "workspace_id": "w1",
                        "cwd": "/tmp/herdr-ide-remote-tab",
                        "focus": true,
                        "label": "New tab"
                    }),
                    json!({
                        "type": "tab_created",
                        "tab": {"tab_id": "w1:t3", "workspace_id": "w1", "number": 1, "label": "fixture", "focused": false, "pane_count": 1, "agent_status": "idle"},
                        "root_pane": {"pane_id": "w1:p3", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}
                    }),
                ),
                (
                    "tab.close",
                    json!({"tab_id": "w1:t3"}),
                    json!({"type": "ok"}),
                ),
            ];
            for (method, params, result) in expected {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut line = String::new();
                BufReader::new(stream.try_clone().expect("clone stream"))
                    .read_line(&mut line)
                    .expect("read request");
                let request: Value = serde_json::from_str(&line).expect("request JSON");
                assert_eq!(request["method"], method);
                assert_eq!(request["params"], params);
                writeln!(
                    stream,
                    "{}",
                    wire::checked_response_fixture(&request["id"], result)
                )
                .expect("write response");
            }
        });
        let connector = UnixSocketConnector::new(&socket_path);
        let actions = [
            RemoteControlAction::FocusWorkspace {
                workspace_id: "w1".to_owned(),
            },
            RemoteControlAction::FocusTab {
                tab_id: "w1:t2".to_owned(),
            },
            RemoteControlAction::CreateTab {
                workspace_id: "w1".to_owned(),
                cwd: "/tmp/herdr-ide-remote-tab".to_owned(),
                label: "New tab".to_owned(),
            },
            RemoteControlAction::CloseTab {
                tab_id: "w1:t3".to_owned(),
            },
        ];
        let mut outcomes = actions
            .iter()
            .map(|action| execute_remote_control(&connector, action).expect("control request"));
        assert!(matches!(
            outcomes.next(),
            Some(RemoteControlOutcome::Acknowledged {
                created_tab_id: None,
                created_pane_id: None,
            })
        ));
        assert!(matches!(
            outcomes.next(),
            Some(RemoteControlOutcome::Acknowledged {
                created_tab_id: None,
                created_pane_id: None,
            })
        ));
        assert!(matches!(
            outcomes.next(),
            Some(RemoteControlOutcome::Acknowledged {
                created_tab_id: Some(tab_id),
                created_pane_id: Some(pane_id),
            }) if tab_id == "w1:t3" && pane_id == "w1:p3"
        ));
        assert!(matches!(
            outcomes.next(),
            Some(RemoteControlOutcome::Acknowledged {
                created_tab_id: None,
                created_pane_id: None,
            })
        ));
        server.join().expect("fake server joins");
        std::fs::remove_file(&socket_path).expect("remove socket");
        std::fs::remove_dir(&root).expect("remove socket directory");
    }

    #[test]
    #[ignore = "requires an owned remote fixture and HERDR_TEST_REMOTE_CONTROL_* variables"]
    fn official_remote_control_fixture_probe() {
        let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS")
            .expect("HERDR_TEST_SSH_ALIAS names a configured SSH host");
        let socket_path = std::env::var("HERDR_TEST_SOCKET_PATH")
            .expect("HERDR_TEST_SOCKET_PATH is the absolute remote Unix socket path");
        let workspace_id = std::env::var("HERDR_TEST_REMOTE_CONTROL_WORKSPACE_ID")
            .expect("HERDR_TEST_REMOTE_CONTROL_WORKSPACE_ID names the owned fixture workspace");
        let cwd = std::env::var("HERDR_TEST_REMOTE_CONTROL_CWD")
            .expect("HERDR_TEST_REMOTE_CONTROL_CWD names the owned fixture directory");
        assert!(
            cwd.starts_with("/tmp/herdr-ide-verify-"),
            "remote control fixture must use the owned fixture namespace"
        );

        let home = std::env::var_os("HOME").expect("HOME is configured");
        let alias = crate::remote::SshAlias::from_config_file(
            &std::path::PathBuf::from(home).join(".ssh/config"),
            &alias_name,
        )
        .expect("SSH alias resolves");
        let client =
            crate::remote::RusshRemoteClient::new(alias).expect("remote client initializes");
        let connector = client
            .herdr_api_connector(socket_path)
            .expect("remote connector initializes");

        let response = request_with_connector(
            &connector,
            "session.snapshot",
            json!({}),
            Duration::from_secs(5),
        )
        .expect("fixture session snapshot");
        let snapshot = response["snapshot"]
            .as_object()
            .map(|_| &response["snapshot"])
            .expect("session.snapshot response contains a snapshot");
        let workspace = snapshot["workspaces"]
            .as_array()
            .and_then(|workspaces| {
                workspaces.iter().find(|workspace| {
                    workspace["workspace_id"].as_str() == Some(workspace_id.as_str())
                })
            })
            .expect("owned fixture workspace is present");
        assert!(
            workspace["label"]
                .as_str()
                .is_some_and(|label| label.starts_with("herdr-ide-verify-")),
            "remote control refused a workspace outside the owned fixture namespace"
        );
        let original_tab_id = snapshot["tabs"]
            .as_array()
            .and_then(|tabs| {
                tabs.iter()
                    .find(|tab| tab["workspace_id"].as_str() == Some(workspace_id.as_str()))
            })
            .and_then(|tab| tab["tab_id"].as_str())
            .expect("owned fixture workspace has a tab")
            .to_owned();

        execute_remote_control(
            &connector,
            &RemoteControlAction::FocusWorkspace {
                workspace_id: workspace_id.clone(),
            },
        )
        .expect("focus owned fixture workspace");
        execute_remote_control(
            &connector,
            &RemoteControlAction::FocusTab {
                tab_id: original_tab_id,
            },
        )
        .expect("focus owned fixture tab");

        let RemoteControlOutcome::Acknowledged {
            created_tab_id: Some(created_tab_id),
            created_pane_id: Some(created_root_pane_id),
        } = execute_remote_control(
            &connector,
            &RemoteControlAction::CreateTab {
                workspace_id: workspace_id.clone(),
                cwd: cwd.clone(),
                label: "Herdr IDE remote control probe".to_owned(),
            },
        )
        .expect("create fixture tab")
        else {
            panic!("tab.create did not return the created tab and root pane ids");
        };

        let RemoteControlOutcome::Acknowledged {
            created_tab_id: None,
            created_pane_id: Some(created_split_pane_id),
        } = execute_remote_control(
            &connector,
            &RemoteControlAction::Pane(PaneControlAction::Split {
                pane_id: created_root_pane_id.clone(),
                direction: PaneSplitDirection::Right,
                cwd: Some(cwd),
            }),
        )
        .expect("split fixture pane")
        else {
            panic!("pane.split did not return the created pane id");
        };

        for action in [
            RemoteControlAction::Pane(PaneControlAction::Focus {
                pane_id: created_split_pane_id.clone(),
            }),
            RemoteControlAction::Pane(PaneControlAction::ToggleZoom {
                pane_id: created_split_pane_id.clone(),
            }),
            RemoteControlAction::Pane(PaneControlAction::ToggleZoom {
                pane_id: created_split_pane_id.clone(),
            }),
            RemoteControlAction::Pane(PaneControlAction::Close {
                pane_id: created_split_pane_id.clone(),
            }),
        ] {
            execute_remote_control(&connector, &action).expect("mutate only the fixture pane");
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let response = request_with_connector(
                &connector,
                "session.snapshot",
                json!({}),
                Duration::from_secs(5),
            )
            .expect("post-control session snapshot");
            let snapshot = response["snapshot"]
                .as_object()
                .map(|_| &response["snapshot"])
                .expect("session.snapshot response contains a snapshot");
            let created_tab_visible = snapshot["tabs"].as_array().is_some_and(|tabs| {
                tabs.iter()
                    .any(|tab| tab["tab_id"].as_str() == Some(created_tab_id.as_str()))
            });
            let created_root_visible = snapshot["panes"].as_array().is_some_and(|panes| {
                panes
                    .iter()
                    .any(|pane| pane["pane_id"].as_str() == Some(created_root_pane_id.as_str()))
            });
            let closed_split_absent = snapshot["panes"].as_array().is_some_and(|panes| {
                panes
                    .iter()
                    .all(|pane| pane["pane_id"].as_str() != Some(created_split_pane_id.as_str()))
            });
            if created_tab_visible && created_root_visible && closed_split_absent {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "authoritative snapshot did not converge after remote controls"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn pane_control_worker_returns_before_the_socket_receipt() {
        let root = std::path::PathBuf::from("/tmp")
            .join(format!("herdr-core-pane-worker-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create socket directory");
        let socket_path = root.join("herdr.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake herdr socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut line)
                .expect("read request");
            let request: Value = serde_json::from_str(&line).expect("request JSON");
            assert_eq!(request["method"], "pane.split");
            std::thread::sleep(Duration::from_millis(500));
            writeln!(
                stream,
                "{}",
                json!({
                    "id": request["id"],
                    "result": {"type": "pane_info", "pane": {"pane_id": "w1:p2", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}}
                })
            )
            .expect("write response");
        });
        let context = LiveContext {
            socket_path: socket_path.clone(),
            herdr_bin: None,
            runtime: Weak::new(),
            notifier: ChangeNotifier::noop(),
            api_connector: Arc::new(UnixSocketConnector::new(&socket_path)),
        };

        let started = Instant::now();
        spawn_pane_control(
            context,
            PaneControlAction::Split {
                pane_id: "w1:p1".to_owned(),
                direction: PaneSplitDirection::Right,
                cwd: Some("/tmp".to_owned()),
            },
        )
        .expect("worker starts");
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_millis(100),
            "pane control spawn waited {elapsed:?} for the socket receipt"
        );
        server.join().expect("fake server joins");
        std::fs::remove_file(&socket_path).expect("remove socket");
        std::fs::remove_dir(&root).expect("remove socket directory");
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
                    json!({"type": "pane_info", "pane": {"pane_id": "fixture:p2", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "fixture", "tab_id": "fixture:t1", "focused": false, "agent_status": "idle", "revision": 1}})
                } else {
                    json!({
                        "type": "pane_layout",
                        "layout": {
                            "workspace_id": "fixture",
                            "tab_id": "fixture:t1",
                            "zoomed": false,
                            "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                            "focused_pane_id": "fixture:p2",
                            "panes": [
                                {"pane_id": "fixture:p1", "focused": false, "rect": {"x": 0, "y": 0, "width": 60, "height": 60}},
                                {"pane_id": "fixture:p2", "focused": true, "rect": {"x": 60, "y": 0, "width": 60, "height": 60}}
                            ],
                            "splits": [
                                {"id": "fixture:split1", "direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 120, "height": 60}}
                            ]
                        }
                    })
                };
                writeln!(
                    stream,
                    "{}",
                    wire::checked_response_fixture(&request["id"], result)
                )
                .expect("write response");
            }
        });

        request(&socket_path, "pane.focus", json!({"pane_id": "fixture:p2"}))
            .expect("focus request");
        let layout = fetch_pane_layout(&UnixSocketConnector::new(&socket_path), "fixture:p2")
            .expect("focused layout");
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
            "version": "fixture",
            "host": {"host_id": "fixture-host", "session_id": "fixture"},
            "event_sequence": 0,
            "panes": [],
            "tabs": [],
            "lineage": [],
            "workspaces": [
                {"workspace_id": "w1", "label": "herdr-ide", "active_tab_id": "w1:t1", "number": 1, "focused": true, "pane_count": 1, "tab_count": 1, "agent_status": "idle"},
            ],
            "layouts": [],
            "agents": [
                {
                    "pane_id": "w1:p1",
                    "terminal_id": "terminal-1", "tab_id": "w1:t1", "focused": true, "revision": 1,
                    "workspace_id": "w1",
                    "agent": "claude",
                    "agent_status": "working",
                    "cwd": "/tmp/project",
                    "tokens": {
                        "status_working": "●",
                        "activity": "1787963036671",
                        "summary": "doing things",
                        "elapsed": "6h"
                    }
                },
                {
                    "pane_id": "w9:p2",
                    "terminal_id": "terminal-2", "tab_id": "w9:t1", "focused": false, "revision": 1, "agent_status": "idle",
                    "workspace_id": "w9",
                    "tokens": {"status_idle": "○", "activity": "1787963036672"}
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
        assert_eq!(
            (projected[0].demand.as_str(), projected[0].activity.as_str()),
            ("none", "working")
        );
        assert_eq!(
            (projected[1].demand.as_str(), projected[1].activity.as_str()),
            ("none", "stopped")
        );
    }

    #[test]
    fn wire_snapshot_preserves_herdr_tab_labels() {
        let snapshot = json!({
            "protocol": HERDR_PROTOCOL_REVISION,
            "version": "fixture",
            "host": {"host_id": "fixture-host", "session_id": "fixture"},
            "event_sequence": 0,
            "panes": [],
            "lineage": [],
            "workspaces": [{"workspace_id": "w1", "label": "verify", "active_tab_id": "w1:t1", "number": 1, "focused": true, "pane_count": 1, "tab_count": 1, "agent_status": "idle"}],
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
            "version": "fixture",
            "host": {"host_id": "fixture-host", "session_id": "fixture"},
            "event_sequence": 0,
            "panes": [],
            "tabs": [],
            "lineage": [],
            "workspaces": [{"workspace_id": "w1", "label": "verify", "active_tab_id": "w1:t1", "number": 1, "focused": true, "pane_count": 1, "tab_count": 1, "agent_status": "idle"}],
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
