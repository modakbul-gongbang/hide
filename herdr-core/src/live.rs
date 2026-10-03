//! Live Herdr commands and pane byte transport. Session state synchronization
//! lives in `session_sync` and uses the sequenced socket event stream.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

use crate::agent_find::AgentFind;
use crate::checkout_owner::OwnerOpen;
use crate::find::PaneFindOptions;
use crate::fork::ForkRequest;
use crate::handle::ChangeNotifier;
use crate::model::{
    EditorDocumentSnapshot, PaneFindRoute, PaneLayoutDirection, PaneLayoutNodeSnapshot,
    PaneLayoutSnapshot, WorkspaceRegistration, WorkspaceSnapshot,
};
use crate::recent_closed::{
    ClosedAgent, ClosedContext, ClosedItem, ClosedLayoutBranch, ClosedLayoutNode, ClosedPane,
    PanePlacement, resume_arguments,
};
use crate::remote::RusshRemoteClient;
use crate::runtime::Runtime;
use crate::sidebar::{
    SessionLayoutPanePayload, SessionLayoutPayload, SessionLayoutRect, SessionSnapshotPayload,
};
use crate::workspace;
use hide_herdr_client::{
    ApiConnector, ApiError, LocalSocketConnector, request_with_connector,
    request_with_correlation_id,
};
#[cfg(test)]
use hide_herdr_client::{HERDR_PROTOCOL_REVISION, request};
use hide_platform::process::OwnedChild;

#[path = "worktree_cleanup.rs"]
pub(crate) mod cleanup;

#[path = "worktree_control.rs"]
mod worktree_control;
pub(crate) use worktree_control::prompt_argument;
pub use worktree_control::{
    CheckoutTabRequest, HomeStartRequest, IssueWriteFailure, PendingAgentStart, PurposeMirror,
    PurposeTaskOutcome, PurposeTaskRequest, TabTarget, TaskAgentOutcome, WorktreeTarget,
    WorktreeTaskOutcome, WorktreeTaskRequest, spawn_branch_migration, spawn_checkout_tab_create,
    spawn_existing_branch_worktree, spawn_home_link_sync, spawn_home_start, spawn_issue_write,
    spawn_local_issue_write, spawn_purpose_write, spawn_remote_purpose_write,
    spawn_task_agent_start, spawn_workspace_close, spawn_worktree_close, spawn_worktree_create,
    spawn_worktree_open,
};

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
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

impl RemoteTerminalContext {
    pub(crate) fn new(
        target_id: impl Into<String>,
        client: Arc<RusshRemoteClient>,
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) -> Self {
        Self {
            target_id: target_id.into(),
            client,
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

pub fn spawn_workspace_creation(
    context: LiveContext,
    path: String,
    label: String,
    initialize_git: bool,
    new_folder: bool,
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
            let worktrees = context
                .runtime
                .upgrade()
                .and_then(|runtime| {
                    let read = runtime.lock().ok().map(|guard| guard.worktree_catalog());
                    drop(runtime);
                    read
                })
                .unwrap_or_default();
            // A new folder is made at the literal path hided checked, before
            // the registration canonicalizes it: canonicalizing first would
            // follow a symlink planted at the name since the check, and make
            // or continue into its target, wherever that is.
            let made = if new_folder {
                workspace::create_project_folder(Path::new(&path))
            } else {
                Ok(())
            };
            let result = made
                .and_then(|()| workspace::registration(&path, &label, workspace::LOCAL_DEVICE_ID))
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
                    let before_spaces = Runtime::session_spaces(&before);
                    let before_catalog =
                        workspace::build_catalog(&registrations, &before_spaces, &worktrees);
                    let needs_herdr_workspace = before_catalog
                        .iter()
                        .any(|workspace| workspace.id == registration.id);
                    // The project's first workspace is its checkout's owner
                    // (PRD checkout-workspace-binding B12), never a plain
                    // workspace a later tab would pass over.
                    let created = needs_herdr_workspace
                        .then(|| {
                            ensure_owner(
                                context.api_connector.as_ref(),
                                &registered_owner(&registration.path, &registration.label),
                                Default::default(),
                            )
                            .map_err(|error| error.message().to_owned())
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
                    let spaces = Runtime::session_spaces(&session);
                    let workspaces = workspace::build_catalog(&registrations, &spaces, &worktrees);
                    Ok(WorkspaceCreationOutcome {
                        registration,
                        base_registrations,
                        registrations,
                        workspaces,
                        session,
                        created_pane_id: created
                            .and_then(|(_, first_tab)| first_tab.map(|(_, pane_id)| pane_id)),
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

/// The owner a newly registered project's checkout gets: a Git checkout is
/// opened from its repository's main worktree, a plain folder is created
/// and marked at the registered path, the same path the catalog keys it by.
fn registered_owner(path: &str, label: &str) -> OwnerOpen {
    match hide_project::facts(Path::new(path)) {
        Ok(facts) if facts.kind == hide_project::ProjectKind::Git => OwnerOpen::for_checkout(
            crate::workspace::LOCAL_DEVICE_ID,
            &hide_platform::path::to_wire_lossy(&facts.checkout_root),
            &hide_platform::path::to_wire_lossy(&facts.root),
            true,
            label,
        ),
        _ => OwnerOpen::for_checkout(crate::workspace::LOCAL_DEVICE_ID, path, path, false, label),
    }
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
    /// Takes a delegated child out of the tab it was split into and gives it
    /// a tab of its own, so the canvas stays one pane (PRD B1, D-15).
    MoveToNewTab {
        pane_id: String,
        workspace_id: String,
        label: String,
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
            Self::MoveToNewTab { .. } => "pane.move",
        }
    }

    /// The pane the action is about, for the caller that has to match a
    /// result back to what it asked for.
    pub fn pane_id(&self) -> &str {
        match self {
            Self::Project { pane_id }
            | Self::Focus { pane_id }
            | Self::Split { pane_id, .. }
            | Self::Resize { pane_id, .. }
            | Self::ToggleZoom { pane_id }
            | Self::Close { pane_id }
            | Self::MoveToNewTab { pane_id, .. } => pane_id,
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
    RenameTab {
        tab_id: String,
        label: String,
        request_id: String,
    },
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
        area_id: Option<String>,
        admission_id: Option<u64>,
    },
    /// A new tab in a checkout whose owner Herdr workspace is not open: the
    /// owner is opened first and the tab lands in it (`checkout_owner`).
    OpenOwner {
        owner: OwnerOpen,
        cwd: String,
        label: String,
        area_id: Option<String>,
        admission_id: Option<u64>,
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
        /// The Herdr connection that carried the request. A late response
        /// from an older socket cannot settle a move on a reconnected host.
        connection_generation: u64,
    },
}

impl RemoteControlAction {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Pane(action) => action.kind(),
            Self::FocusWorkspace { .. } => "workspace.focus",
            Self::FocusTab { .. } => "tab.focus",
            Self::CreateTab { .. } => "tab.create",
            Self::OpenOwner { owner, .. } => owner.kind(),
            Self::CloseTab { .. } => "tab.close",
            Self::MoveTab { .. } => "tab.move",
            Self::RenameTab { .. } => "tab.rename",
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

/// A control result is either a known refusal or an answer that may have been
/// lost after Herdr accepted the request. The runtime must never retry the
/// latter mutation without first reading fresh topology.
#[derive(Clone, Debug)]
pub(crate) enum ControlFailure {
    Definite(String),
    Ambiguous(String),
}

impl ControlFailure {
    pub(crate) fn is_ambiguous(&self) -> bool {
        matches!(self, Self::Ambiguous(_))
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Definite(message) | Self::Ambiguous(message) => message,
        }
    }
}

impl From<String> for ControlFailure {
    fn from(message: String) -> Self {
        Self::Definite(message)
    }
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

    /// The Herdr API connection this device's control requests use.
    pub(crate) fn api_connector(&self) -> Arc<dyn ApiConnector> {
        Arc::clone(&self.api_connector)
    }
}

fn execute_remote_control(
    connector: &dyn ApiConnector,
    action: &RemoteControlAction,
) -> Result<RemoteControlOutcome, ControlFailure> {
    let (created_tab_id, created_pane_id) = match action {
        RemoteControlAction::Pane(action) => match action {
            PaneControlAction::Focus { .. }
            | PaneControlAction::Split { .. }
            | PaneControlAction::ToggleZoom { .. }
            | PaneControlAction::Close { .. } => match execute_pane_control(connector, action)? {
                PaneControlOutcome::Acknowledged { created_pane_id } => (None, created_pane_id),
                PaneControlOutcome::Projected { .. } => {
                    return Err(ControlFailure::Definite(
                        "remote pane mutation returned a layout projection".to_owned(),
                    ));
                }
            },
            // A remote pane is never relocated: Hide leaves another
            // machine's layout alone (PRD D-28, D-49).
            PaneControlAction::Project { .. }
            | PaneControlAction::Resize { .. }
            | PaneControlAction::MoveToNewTab { .. } => {
                return Err(ControlFailure::Definite(
                    "unsupported remote pane control action".to_owned(),
                ));
            }
        },
        RemoteControlAction::FocusWorkspace { workspace_id } => {
            mutation_request(
                connector,
                "workspace.focus",
                wire::workspace_target_params(workspace_id)?,
            )?;
            (None, None)
        }
        RemoteControlAction::FocusTab { tab_id } => {
            mutation_request(connector, "tab.focus", wire::tab_target_params(tab_id)?)?;
            (None, None)
        }
        RemoteControlAction::CreateTab {
            workspace_id,
            cwd,
            label,
            admission_id,
            ..
        } => {
            let result = mutation_request(
                connector,
                "tab.create",
                wire::tab_create_with_env_params(
                    workspace_id,
                    cwd,
                    label,
                    local_create_env(*admission_id),
                )?,
            )?;
            let (tab_id, pane_id) = wire::created_tab(result).map_err(ControlFailure::Ambiguous)?;
            (Some(tab_id), Some(pane_id))
        }
        RemoteControlAction::OpenOwner {
            owner,
            cwd,
            label,
            admission_id,
            ..
        } => {
            let tab = open_owner_tab(
                connector,
                owner,
                cwd,
                label,
                local_create_env(*admission_id),
            )?;
            (Some(tab.tab_id), Some(tab.pane_id))
        }
        RemoteControlAction::CloseTab { tab_id } => {
            mutation_request(connector, "tab.close", wire::tab_target_params(tab_id)?)?;
            (None, None)
        }
        RemoteControlAction::RenameTab { tab_id, label, .. } => {
            mutation_request(
                connector,
                "tab.rename",
                wire::tab_rename_params(tab_id, label)?,
            )?;
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
            let result = mutation_request(
                connector,
                "tab.move",
                wire::tab_move_params(tab_id, *insert_index)?,
            )?;
            let tab_ids = wire::moved_tabs(result).map_err(ControlFailure::Ambiguous)?;
            return Ok(RemoteControlOutcome::TabsOrdered { tab_ids });
        }
    };
    Ok(RemoteControlOutcome::Acknowledged {
        created_tab_id,
        created_pane_id,
    })
}

/// A tab Hide created in a checkout's owner Herdr workspace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OwnedTab {
    pub(crate) workspace_id: String,
    pub(crate) tab_id: String,
    pub(crate) pane_id: String,
}

/// Plain-folder owners are found and made one at a time, so two requests
/// racing for a folder with no owner converge on one workspace; a Git
/// checkout converges through `worktree.open` itself (`already_open`). The
/// lock spans every folder and device rather than one folder: an open here
/// happens once per folder per Herdr server life (D-11), and each call it
/// holds across is bounded by its own timeout, so the rare wait this costs
/// an unrelated folder is not worth a keyed lock registry.
static FOLDER_OWNER_OPEN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Opens a checkout's owner and creates one tab in it (PRD
/// checkout-workspace-binding B2, B12, B13): a newly opened owner's first tab
/// is the new tab, and an owner that was already open gets a new tab. A
/// refusal fails the request; nothing is created in any other workspace (B3).
pub(crate) fn open_owner_tab(
    connector: &dyn ApiConnector,
    owner: &OwnerOpen,
    cwd: &str,
    label: &str,
    env: std::collections::BTreeMap<String, String>,
) -> Result<OwnedTab, ControlFailure> {
    let (workspace_id, first_tab) = ensure_owner(connector, owner, env.clone())?;
    let owned = match first_tab {
        Some((tab_id, pane_id)) => {
            // The owner's first tab is the new tab, so it takes the label the
            // request carries, as `tab.create` would give it; the tab is real
            // either way.
            if !label.trim().is_empty()
                && let Err(error) = mutation_request(
                    connector,
                    "tab.rename",
                    wire::tab_rename_params(&tab_id, label)?,
                )
            {
                crate::diagnostic!(json!({
                    "component": "checkout_owner",
                    "kind": "owner.tab_label_failed",
                    "workspace_id": workspace_id,
                    "tab_id": tab_id,
                    "message": error.message(),
                }));
            }
            OwnedTab {
                workspace_id,
                tab_id,
                pane_id,
            }
        }
        None => create_tab_in(connector, &workspace_id, cwd, label, env)?,
    };
    crate::diagnostic!(json!({
        "component": "checkout_owner",
        "kind": "owner.tab_created",
        "path": owner.path(),
        "method": owner.kind(),
        "workspace_id": owned.workspace_id,
        "tab_id": owned.tab_id,
    }));
    Ok(owned)
}

/// The checkout's owner workspace, opened when none is open, and the tab and
/// pane Herdr made with it when this call opened it (`None` when it was
/// already open). `env` reaches the first pane only where Herdr takes it
/// (`workspace.create`); `worktree.open` takes none.
fn ensure_owner(
    connector: &dyn ApiConnector,
    owner: &OwnerOpen,
    env: std::collections::BTreeMap<String, String>,
) -> Result<(String, Option<(String, String)>), ControlFailure> {
    match owner {
        OwnerOpen::Worktree {
            path,
            repository_root,
            label,
        } => {
            let opened = wire::opened_worktree(mutation_request(
                connector,
                "worktree.open",
                wire::worktree_open_params(path, repository_root)?,
            )?)
            .map_err(ControlFailure::Ambiguous)?;
            if opened.already_open {
                return Ok((opened.workspace_id, None));
            }
            // Only a workspace Herdr opened for this request takes the
            // checkout's name (D-14); the tab is real either way.
            if !label.trim().is_empty()
                && opened.label != *label
                && let Err(error) = mutation_request(
                    connector,
                    "workspace.rename",
                    wire::workspace_rename_params(&opened.workspace_id, label)?,
                )
            {
                crate::diagnostic!(json!({
                    "component": "checkout_owner",
                    "kind": "owner.label_failed",
                    "workspace_id": opened.workspace_id,
                    "path": path,
                    "message": error.message(),
                }));
            }
            Ok((opened.workspace_id, Some((opened.tab_id, opened.pane_id))))
        }
        OwnerOpen::Folder { path, label, mark } => {
            let _serialized = FOLDER_OWNER_OPEN
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let listed = request_with_connector(
                connector,
                "workspace.list",
                wire::workspace_list_params()?,
                Duration::from_secs(5),
            )
            .map_err(|error| ControlFailure::Definite(format!("workspace.list failed: {error}")))
            .and_then(|value| {
                wire::listed_workspace_tokens(value, crate::checkout_owner::OWNER_TOKEN)
                    .map_err(ControlFailure::Definite)
            })?;
            if let Some((workspace_id, _)) = listed
                .into_iter()
                .find(|(_, value)| value.as_deref() == Some(mark.as_str()))
            {
                return Ok((workspace_id, None));
            }
            let created = mutation_request(
                connector,
                "workspace.create",
                wire::workspace_create_with_env_params(path, label, env)?,
            )?;
            let (workspace_id, tab_id, pane_id) =
                wire::created_workspace(created).map_err(ControlFailure::Ambiguous)?;
            // The tab is real either way; without its mark only the next tab
            // in this folder makes another owner.
            if let Err(error) = mutation_request(
                connector,
                "workspace.report_metadata",
                wire::workspace_owner_mark_params(&workspace_id, mark)?,
            ) {
                crate::diagnostic!(json!({
                    "component": "checkout_owner",
                    "kind": "owner.mark_failed",
                    "workspace_id": workspace_id,
                    "path": path,
                    "message": error.message(),
                }));
            }
            Ok((workspace_id, Some((tab_id, pane_id))))
        }
    }
}

fn create_tab_in(
    connector: &dyn ApiConnector,
    workspace_id: &str,
    cwd: &str,
    label: &str,
    env: std::collections::BTreeMap<String, String>,
) -> Result<OwnedTab, ControlFailure> {
    let result = mutation_request(
        connector,
        "tab.create",
        wire::tab_create_with_env_params(workspace_id, cwd, label, env)?,
    )?;
    let (tab_id, pane_id) = wire::created_tab(result).map_err(ControlFailure::Ambiguous)?;
    Ok(OwnedTab {
        workspace_id: workspace_id.to_owned(),
        tab_id,
        pane_id,
    })
}

fn execute_pane_control(
    connector: &dyn ApiConnector,
    action: &PaneControlAction,
) -> Result<PaneControlOutcome, ControlFailure> {
    if let PaneControlAction::Project { pane_id } = action {
        return fetch_pane_layout(connector, pane_id)
            .map(|layout| PaneControlOutcome::Projected { layout })
            .map_err(ControlFailure::Definite);
    }
    if let PaneControlAction::Focus { pane_id } = action {
        mutation_request(connector, "pane.focus", wire::pane_target_params(pane_id)?)?;
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
        mutation_request(
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
            let result = mutation_request(connector, "pane.split", params)?;
            Some(wire::split_pane(result).map_err(ControlFailure::Ambiguous)?)
        }
        PaneControlAction::ToggleZoom { pane_id } => {
            mutation_request(connector, "pane.zoom", wire::pane_zoom_params(pane_id)?)?;
            None
        }
        PaneControlAction::Close { pane_id } => {
            mutation_request(connector, "pane.close", wire::pane_target_params(pane_id)?)?;
            None
        }
        PaneControlAction::MoveToNewTab {
            pane_id,
            workspace_id,
            label,
        } => {
            let params = wire::pane_move_to_new_tab_params(pane_id, workspace_id, label)?;
            let result = mutation_request(connector, "pane.move", params)?;
            // The created tab is checked here so a refusal Herdr reported as
            // an unchanged move fails the action rather than reading as done.
            wire::moved_pane_tab(result).map_err(ControlFailure::Ambiguous)?;
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

fn mutation_request(
    connector: &dyn ApiConnector,
    method: &str,
    params: Value,
) -> Result<Value, ControlFailure> {
    request_with_connector(connector, method, params, Duration::from_secs(5)).map_err(|error| {
        match error {
            ApiError::Remote { code, message } => {
                ControlFailure::Definite(format!("{method} was refused: {code}: {message}"))
            }
            ApiError::Transport(message) | ApiError::Malformed(message) => {
                ControlFailure::Ambiguous(format!("{method} result is unknown: {message}"))
            }
        }
    })
}

fn reopen_request(
    connector: &dyn ApiConnector,
    request_id: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    request_with_correlation_id(
        connector,
        request_id,
        method,
        params,
        Duration::from_secs(5),
    )
    .map_err(|error| format!("{method} failed: {error}"))
}

#[derive(Clone, Debug)]
pub enum CloseCaptureTarget {
    Pane { pane_id: String },
    Tab { tab_id: String },
}

#[derive(Clone, Debug)]
pub struct CloseCaptureRequest {
    pub key: String,
    /// A response from an older live connection must never settle a newer
    /// close intent after reconnect.
    pub connection_generation: u64,
    pub context: ClosedContext,
    pub panes: Vec<ClosedPane>,
    pub target: CloseCaptureTarget,
}

#[derive(Debug)]
pub struct CloseCaptureOutcome {
    pub item: Option<ClosedItem>,
}

#[derive(Clone, Debug)]
pub struct CloseEffectRequest {
    pub allow_replacement_create: bool,
    pub replacement: Option<ClosedContext>,
    pub key: String,
    pub connection_generation: u64,
    pub target: CloseCaptureTarget,
}

pub fn spawn_close_capture(
    context: LiveContext,
    request: CloseCaptureRequest,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-close-capture".to_owned())
        .spawn(move || {
            let result = capture_close_item(context.api_connector.as_ref(), &request);
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let (changed, effects) = match runtime.lock() {
                Ok(mut guard) => guard.ingest_close_capture_result(&request, result),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
            for effect in effects {
                let prepared = if let Some(replacement) = &effect.replacement {
                    prepare_close_replacement(
                        context.api_connector.as_ref(),
                        &effect.key,
                        replacement,
                        effect.allow_replacement_create,
                    )
                    .and_then(|(tab_id, payload)| {
                        let runtime = context.runtime.upgrade().ok_or("runtime stopped")?;
                        let mut guard = runtime.lock().map_err(|_| "runtime unavailable")?;
                        guard.ingest_close_replacement(&effect, &tab_id, payload)?;
                        context.notifier.notify();
                        Ok(())
                    })
                    .map_err(|message| ApiError::Remote {
                        code: "replacement_failed".into(),
                        message,
                    })
                } else {
                    Ok(())
                };
                let result = prepared
                    .and_then(|()| run_close_effect(context.api_connector.as_ref(), &effect));
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_close_effect_result(&effect, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            }
        })
        .map(|_| ())
        .map_err(|error| format!("close capture worker could not be started: {error}"))
}

fn capture_close_item(
    connector: &dyn ApiConnector,
    request: &CloseCaptureRequest,
) -> Result<CloseCaptureOutcome, String> {
    let layout_value = reopen_request(
        connector,
        &format!("herdr-core:{}:export", request.key),
        "layout.export",
        wire::layout_export_params(&request.context.tab_id)?,
    )?;
    let layout = wire::exported_layout(layout_value)?;
    let item = match &request.target {
        CloseCaptureTarget::Pane { pane_id } => {
            let pane = request
                .panes
                .iter()
                .find(|pane| pane.pane_id == *pane_id)
                .cloned()
                .ok_or_else(|| format!("pane {pane_id} disappeared before close capture"))?;
            let placement = layout.root.placement_for(pane_id).unwrap_or(PanePlacement {
                neighbor_pane_id: None,
                parent_path: Vec::new(),
                direction: crate::recent_closed::ClosedSplitDirection::Right,
                ratio: 0.5,
                target_was_first: false,
            });
            Some(ClosedItem::Pane {
                key: request.key.clone(),
                context: request.context.clone(),
                pane,
                placement,
            })
        }
        CloseCaptureTarget::Tab { .. } => Some(ClosedItem::Tab {
            key: request.key.clone(),
            context: request.context.clone(),
            layout,
            panes: request.panes.clone(),
        }),
    };
    Ok(CloseCaptureOutcome { item })
}

fn prepare_close_replacement(
    connector: &dyn ApiConnector,
    key: &str,
    context: &ClosedContext,
    allow_create: bool,
) -> Result<(String, SessionSnapshotPayload), String> {
    let started = Instant::now();
    let snapshot =
        fetch_session_with_connector(connector).map_err(|error| error.message().to_owned())?;
    if !snapshot
        .workspaces
        .iter()
        .any(|workspace| workspace.workspace_id == context.workspace_id)
    {
        return Err("primary workspace disappeared before replacement".into());
    }
    let candidates = snapshot
        .tabs
        .iter()
        .filter(|tab| {
            tab.workspace_id == context.workspace_id
                && !context.tab_ids_before_close.contains(&tab.tab_id)
        })
        .collect::<Vec<_>>();
    if candidates.len() > 64 {
        return Err("too many tabs to inspect for replacement intent".into());
    }
    let marker = reopen_intent_marker(key, ReopenIntentStage::CloseReplacement);
    let mut owned = None;
    for tab in candidates {
        if started.elapsed() > Duration::from_secs(5) {
            return Err("replacement inspection timed out".into());
        }
        let layout = export_reopen_layout(connector, key, &tab.tab_id)?;
        if layout_has_reopen_marker(&layout, &marker) {
            if owned.is_some() {
                return Err("multiple shells carry the same close intent".into());
            }
            owned = Some(tab.tab_id.clone());
        }
    }
    let tab_id = match owned {
        Some(id) => id,
        None if !allow_create => {
            return Err(
                "The replacement shell disappeared; retry after making room for a tab".into(),
            );
        }
        None => {
            let value = reopen_request(
                connector,
                &format!("herdr-core:{key}:replacement"),
                "tab.create",
                wire::replacement_tab_params(
                    &context.workspace_id,
                    &context.checkout_path,
                    reopen_intent_env(key, ReopenIntentStage::CloseReplacement),
                )?,
            )?;
            wire::created_tab(value)?.0
        }
    };
    let snapshot =
        fetch_session_with_connector(connector).map_err(|error| error.message().to_owned())?;
    if !snapshot
        .tabs
        .iter()
        .any(|tab| tab.tab_id == tab_id && tab.workspace_id == context.workspace_id)
    {
        return Err("replacement shell is not confirmed in the primary workspace".into());
    }
    Ok((tab_id, snapshot))
}

fn run_close_effect(
    connector: &dyn ApiConnector,
    request: &CloseEffectRequest,
) -> Result<(), ApiError> {
    let (method, params) = match &request.target {
        CloseCaptureTarget::Pane { pane_id } => (
            "pane.close",
            wire::pane_target_params(pane_id).map_err(ApiError::Malformed)?,
        ),
        CloseCaptureTarget::Tab { tab_id } => (
            "tab.close",
            wire::tab_target_params(tab_id).map_err(ApiError::Malformed)?,
        ),
    };
    request_with_correlation_id(
        connector,
        &format!("herdr-core:{}:close", request.key),
        method,
        params,
        Duration::from_secs(5),
    )
    .map(|_| ())
}

/// A read-only fresh session check for a close whose transport result was
/// ambiguous. It does not repeat the mutation and carries the connection
/// generation so a late answer from an older socket cannot change current
/// state.
#[derive(Clone, Debug)]
pub struct CloseStatusCheckRequest {
    pub key: String,
    pub target: CloseCaptureTarget,
}

pub fn spawn_close_status_check(
    context: LiveContext,
    key: String,
    connection_generation: u64,
    target: CloseCaptureTarget,
) -> Result<(), String> {
    spawn_close_status_checks(
        context,
        connection_generation,
        vec![CloseStatusCheckRequest { key, target }],
    )
}

/// Runs one read for all close operations waiting on the same authoritative
/// session. The result is fanned out while the runtime lock is held, so a
/// single fresh snapshot cannot settle one close and leave its siblings stale.
pub fn spawn_close_status_checks(
    context: LiveContext,
    connection_generation: u64,
    requests: Vec<CloseStatusCheckRequest>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-close-status-check".to_owned())
        .spawn(move || {
            let result = fetch_session_with_connector(context.api_connector.as_ref());
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => {
                    guard.ingest_close_status_results(connection_generation, &requests, result)
                }
                Err(_) => false,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("close status worker could not be started: {error}"))
}

#[derive(Clone, Debug)]
pub struct ReopenRequest {
    pub item: ClosedItem,
    pub workspace_exists: bool,
    pub tab_exists: bool,
    pub fallback_pane_id: Option<String>,
    /// The owner of the checkout the closed item belongs to: a tab that has to
    /// be made again lands there, never in the workspace it was closed from
    /// (PRD checkout-workspace-binding D-16). `None` for a file.
    pub owner: Option<OwnerOpen>,
}

const REOPEN_INTENT_ENV: &str = "HIDE_REOPEN_INTENT";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReopenIntentStage {
    LocalCreate,
    Workspace,
    Layout,
    Pane,
    CloseReplacement,
}

fn reopen_intent_marker(key: &str, stage: ReopenIntentStage) -> String {
    let stage = match stage {
        ReopenIntentStage::LocalCreate => "local-create",
        ReopenIntentStage::Workspace => "workspace",
        ReopenIntentStage::Layout => "layout",
        ReopenIntentStage::Pane => "pane",
        ReopenIntentStage::CloseReplacement => "close-replacement",
    };
    format!("{key}:{stage}")
}

fn reopen_intent_env(
    key: &str,
    stage: ReopenIntentStage,
) -> std::collections::BTreeMap<String, String> {
    [(
        REOPEN_INTENT_ENV.to_owned(),
        reopen_intent_marker(key, stage),
    )]
    .into_iter()
    .collect()
}

fn tag_reopen_layout(
    node: &ClosedLayoutNode,
    key: &str,
    stage: ReopenIntentStage,
) -> ClosedLayoutNode {
    match node {
        ClosedLayoutNode::Pane {
            pane_id,
            label,
            cwd,
            command,
            env,
        } => {
            let mut env = env.clone();
            env.insert(
                REOPEN_INTENT_ENV.to_owned(),
                reopen_intent_marker(key, stage),
            );
            ClosedLayoutNode::Pane {
                pane_id: pane_id.clone(),
                label: label.clone(),
                cwd: cwd.clone(),
                command: command.clone(),
                env,
            }
        }
        ClosedLayoutNode::Split {
            direction,
            ratio,
            first,
            second,
        } => ClosedLayoutNode::Split {
            direction: *direction,
            ratio: *ratio,
            first: Box::new(tag_reopen_layout(first, key, stage)),
            second: Box::new(tag_reopen_layout(second, key, stage)),
        },
    }
}

fn pane_ids_with_reopen_marker(node: &ClosedLayoutNode, marker: &str, output: &mut Vec<String>) {
    match node {
        ClosedLayoutNode::Pane { pane_id, env, .. } => {
            if env
                .get(REOPEN_INTENT_ENV)
                .is_some_and(|value| value == marker)
            {
                output.extend(pane_id.iter().cloned());
            }
        }
        ClosedLayoutNode::Split { first, second, .. } => {
            pane_ids_with_reopen_marker(first, marker, output);
            pane_ids_with_reopen_marker(second, marker, output);
        }
    }
}

fn layout_has_reopen_marker(layout: &crate::recent_closed::ClosedLayout, marker: &str) -> bool {
    let mut pane_ids = Vec::new();
    pane_ids_with_reopen_marker(&layout.root, marker, &mut pane_ids);
    !pane_ids.is_empty()
}

fn export_reopen_layout(
    connector: &dyn ApiConnector,
    key: &str,
    tab_id: &str,
) -> Result<crate::recent_closed::ClosedLayout, String> {
    let exported = reopen_request(
        connector,
        &format!("herdr-core:{key}:inspect:{tab_id}"),
        "layout.export",
        wire::layout_export_params(tab_id)?,
    )?;
    wire::exported_layout(exported)
}

fn restore_tab_position(
    connector: &dyn ApiConnector,
    key: &str,
    context: &ClosedContext,
    layout: crate::recent_closed::ClosedLayout,
    notices: &mut Vec<String>,
) -> crate::recent_closed::ClosedLayout {
    // The position is an index among the tabs of the workspace it was closed
    // from; a tab that reopened in the checkout's owner elsewhere keeps the
    // place Herdr gave it. That is the usual case for a tab closed outside
    // the owner, and for a plain folder whose owner closed with its last tab.
    if layout.workspace_id != context.workspace_id {
        return layout;
    }
    let params = match wire::tab_move_params(&layout.tab_id, context.tab_index) {
        Ok(params) => params,
        Err(message) => {
            notices.push(format!(
                "The tab reopened but its original position could not be restored: {message}"
            ));
            return layout;
        }
    };
    let move_result = reopen_request(
        connector,
        &format!("herdr-core:{key}:tab-position"),
        "tab.move",
        params,
    )
    .and_then(wire::moved_tabs);
    if let Err(message) = move_result {
        notices.push(format!(
            "The tab reopened but its original position could not be restored: {message}"
        ));
    }
    layout
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReopenNotice {
    pub pane_id: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReopenOutcome {
    pub tab_id: Option<String>,
    pub consumed: bool,
    pub focused_pane_id: Option<String>,
    pub notices: Vec<ReopenNotice>,
}

#[derive(Debug)]
pub enum FileReopenResult {
    Opened(Box<(EditorDocumentSnapshot, crate::files::DocumentPlace)>),
    Missing,
    Failed(String),
}

pub fn spawn_reopen(context: LiveContext, request: ReopenRequest) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-reopen-closed".to_owned())
        .spawn(move || {
            let result = run_herdr_reopen(context.api_connector.as_ref(), &request)
                .map(FileReopenResultOrHerdr::Herdr);
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_reopen_result(&request, result),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("reopen worker could not be started: {error}"))
}

pub fn spawn_file_reopen(
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    request: ReopenRequest,
    root: crate::files::DocumentRoot,
    channel: std::sync::Arc<dyn crate::host_access::HostChannel>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-file-reopen".to_owned())
        .spawn(move || {
            let ClosedItem::File { path, .. } = &request.item else {
                return;
            };
            let result = Ok(run_file_reopen(channel.as_ref(), &root, path));
            let Some(runtime) = runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_reopen_result(&request, result),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("file reopen worker could not be started: {error}"))
}

#[derive(Debug)]
pub enum FileReopenResultOrHerdr {
    File(FileReopenResult),
    Herdr(ReopenOutcome),
}

fn run_file_reopen(
    channel: &dyn crate::host_access::HostChannel,
    root: &crate::files::DocumentRoot,
    path: &str,
) -> FileReopenResultOrHerdr {
    FileReopenResultOrHerdr::File(match crate::files::open_document(channel, root, path) {
        Ok(opened) => FileReopenResult::Opened(Box::new(opened)),
        Err(crate::files::OpenFailure::Missing) => FileReopenResult::Missing,
        Err(failure) => FileReopenResult::Failed(failure.message()),
    })
}

fn run_herdr_reopen(
    connector: &dyn ApiConnector,
    request: &ReopenRequest,
) -> Result<ReopenOutcome, String> {
    match &request.item {
        ClosedItem::Pane {
            key,
            context,
            pane,
            placement,
        } => reopen_pane(connector, key, context, pane, placement, request),
        ClosedItem::Tab {
            key,
            context,
            layout,
            panes,
        } => reopen_tab(
            connector,
            key,
            context,
            reopen_owner(request)?,
            &layout.root,
            panes,
        ),
        ClosedItem::File { .. } => unreachable!("file reopen uses the filesystem worker"),
    }
}

fn ensure_workspace_and_tab(
    connector: &dyn ApiConnector,
    key: &str,
    context: &ClosedContext,
    owner: &OwnerOpen,
    tab_exists: bool,
    root: &ClosedLayoutNode,
    notices: &mut Vec<String>,
) -> Result<crate::recent_closed::ClosedLayout, String> {
    if tab_exists {
        return Err("the requested tab already exists".into());
    }
    let snapshot = fetch_session_with_connector(connector)
        .map_err(|error| format!("session.snapshot before reopen failed: {}", error.message()))?;
    // The tab goes to the checkout's owner. A retry adopts only a layout or a
    // workspace seed carrying this intent's marker: in the owner when it is
    // open, else in a workspace that appeared since the close.
    let open_owner = owner
        .find_in(
            snapshot
                .workspaces
                .iter()
                .map(crate::checkout_owner::WorkspaceFacts::from),
        )
        .map(str::to_owned);
    let workspace_marker = reopen_intent_marker(key, ReopenIntentStage::Workspace);
    let layout_marker = reopen_intent_marker(key, ReopenIntentStage::Layout);
    let candidate_workspace_ids = match &open_owner {
        Some(owner) => vec![owner.clone()],
        None => snapshot
            .workspaces
            .iter()
            .filter(|workspace| {
                !context
                    .workspace_ids_before_close
                    .contains(&workspace.workspace_id)
            })
            .map(|workspace| workspace.workspace_id.clone())
            .collect(),
    };
    let mut recovered_layouts = Vec::new();
    let mut recovered_workspace_seeds = Vec::new();
    for tab in snapshot.tabs.iter().filter(|tab| {
        candidate_workspace_ids.contains(&tab.workspace_id)
            && !context.tab_ids_before_close.contains(&tab.tab_id)
    }) {
        let layout = export_reopen_layout(connector, key, &tab.tab_id)?;
        if layout_has_reopen_marker(&layout, &layout_marker) {
            recovered_layouts.push(layout);
        } else if layout_has_reopen_marker(&layout, &workspace_marker) {
            recovered_workspace_seeds.push((tab.workspace_id.clone(), tab.tab_id.clone()));
        }
    }
    if recovered_layouts.len() > 1 {
        return Err(format!(
            "reopen retry found {} layouts owned by {key}; refusing to choose one",
            recovered_layouts.len()
        ));
    }
    if let Some(layout) = recovered_layouts.pop() {
        let layout = repair_incomplete_tab_layout(connector, key, context, root, layout, notices)?;
        return Ok(restore_tab_position(
            connector, key, context, layout, notices,
        ));
    }
    if recovered_workspace_seeds.len() > 1 {
        return Err(format!(
            "reopen retry found {} workspace seeds owned by {key}; refusing to choose one",
            recovered_workspace_seeds.len()
        ));
    }
    let (workspace_id, seed_tab_id) = if let Some(seed) = recovered_workspace_seeds.pop() {
        (seed.0, Some(seed.1))
    } else if let Some(owner) = open_owner {
        (owner, None)
    } else {
        // The owner's first tab is the seed the layout replaces.
        let (workspace_id, first_tab) = ensure_owner(
            connector,
            owner,
            reopen_intent_env(key, ReopenIntentStage::Workspace),
        )
        .map_err(|failure| failure.message().to_owned())?;
        (workspace_id, first_tab.map(|(tab_id, _)| tab_id))
    };
    let tagged_root = tag_reopen_layout(root, key, ReopenIntentStage::Layout);
    let applied = reopen_request(
        connector,
        &format!("herdr-core:{key}:layout"),
        "layout.apply",
        wire::layout_apply_params(
            &workspace_id,
            seed_tab_id.as_deref(),
            &context.tab_label,
            &tagged_root,
        )?,
    )?;
    let layout = wire::applied_layout(applied)?;
    let layout = repair_incomplete_tab_layout(connector, key, context, root, layout, notices)?;
    Ok(restore_tab_position(
        connector, key, context, layout, notices,
    ))
}

fn repair_incomplete_tab_layout(
    connector: &dyn ApiConnector,
    key: &str,
    context: &ClosedContext,
    root: &ClosedLayoutNode,
    mut layout: crate::recent_closed::ClosedLayout,
    notices: &mut Vec<String>,
) -> Result<crate::recent_closed::ClosedLayout, String> {
    let expected = root.pane_count();
    let first_actual = layout.root.pane_count();
    if first_actual == expected {
        return Ok(layout);
    }
    if first_actual > expected {
        return Err(format!(
            "the reopened tab returned {first_actual} panes, expected {expected}; extra panes were not adopted or overwritten and retry is available"
        ));
    }
    let repaired = reopen_request(
        connector,
        &format!("herdr-core:{key}:layout-repair"),
        "layout.apply",
        wire::layout_apply_params(
            &layout.workspace_id,
            Some(&layout.tab_id),
            &context.tab_label,
            &tag_reopen_layout(root, key, ReopenIntentStage::Layout),
        )?,
    )?;
    layout = wire::applied_layout(repaired)?;
    let repaired_actual = layout.root.pane_count();
    if repaired_actual > expected {
        return Err(format!(
            "the reopened tab restored {first_actual} of {expected} panes, then returned {repaired_actual} after repair; extra panes were not adopted or overwritten and retry is available"
        ));
    }
    if repaired_actual < expected {
        return Err(format!(
            "the reopened tab restored {first_actual} of {expected} panes, then {repaired_actual} after repair; retry is available"
        ));
    }
    notices.push("The incomplete tab restore was repaired before reopening sessions".into());
    Ok(layout)
}

fn reopen_pane(
    connector: &dyn ApiConnector,
    key: &str,
    context: &ClosedContext,
    pane: &ClosedPane,
    placement: &PanePlacement,
    request: &ReopenRequest,
) -> Result<ReopenOutcome, String> {
    let mut notices = Vec::new();
    let cwd = restored_cwd(&pane.cwd, &context.checkout_path, &mut notices);
    let (restored_pane_id, restored_tab_id) = if request.tab_exists {
        let current_layout = export_reopen_layout(connector, key, &context.tab_id)?;
        let marker = reopen_intent_marker(key, ReopenIntentStage::Pane);
        let mut recovered_panes = Vec::new();
        pane_ids_with_reopen_marker(&current_layout.root, &marker, &mut recovered_panes);
        if recovered_panes.len() > 1 {
            return Err(format!(
                "reopen retry found {} panes owned by {key}; refusing to choose one",
                recovered_panes.len(),
            ));
        }
        let (new_pane_id, current_target_was_first) = if let Some(pane_id) = recovered_panes.pop() {
            let current = current_layout.root.placement_for(&pane_id).ok_or_else(|| {
                format!("reopen retry could not locate owned pane {pane_id} in its layout")
            })?;
            (pane_id, current.target_was_first)
        } else if let Some(target) = placement.neighbor_pane_id.as_ref() {
            if request.fallback_pane_id.as_deref() != Some(target.as_str()) {
                return Err(format!(
                    "the original sibling pane {target} is no longer available"
                ));
            }
            let value = reopen_request(
                connector,
                &format!("herdr-core:{key}:pane"),
                "pane.split",
                wire::pane_split_with_ratio_params(
                    target,
                    placement.direction,
                    &cwd,
                    placement.ratio,
                    reopen_intent_env(key, ReopenIntentStage::Pane),
                )?,
            )?;
            (wire::split_pane(value)?, false)
        } else {
            let pane_root = tag_reopen_layout(
                &ClosedLayoutNode::Pane {
                    pane_id: None,
                    label: pane.label.clone(),
                    cwd: Some(cwd.clone()),
                    command: None,
                    env: Default::default(),
                },
                key,
                ReopenIntentStage::Pane,
            );
            let root = wrap_surviving_subtree(
                current_layout.root.clone(),
                &placement.parent_path,
                pane_root,
                placement.direction,
                placement.ratio,
                placement.target_was_first,
            )?;
            let value = reopen_request(
                connector,
                &format!("herdr-core:{key}:pane-layout"),
                "layout.apply",
                wire::layout_apply_params(
                    &current_layout.workspace_id,
                    Some(&context.tab_id),
                    &context.tab_label,
                    &root,
                )?,
            )?;
            let applied = wire::applied_layout(value)?;
            let mut owned = Vec::new();
            pane_ids_with_reopen_marker(&applied.root, &marker, &mut owned);
            if owned.len() != 1 {
                return Err(format!(
                    "reopen layout created {} owned panes instead of one",
                    owned.len()
                ));
            }
            (owned.remove(0), placement.target_was_first)
        };
        if current_target_was_first != placement.target_was_first
            && let Some(target) = placement.neighbor_pane_id.as_ref()
            && let Err(message) = reopen_request(
                connector,
                &format!("herdr-core:{key}:swap"),
                "pane.swap",
                wire::pane_swap_params(&new_pane_id, target)?,
            )
        {
            notices.push(format!(
                "The pane reopened but its original side could not be restored: {message}"
            ));
        }
        (new_pane_id, context.tab_id.clone())
    } else {
        let root = ClosedLayoutNode::Pane {
            pane_id: None,
            label: pane.label.clone(),
            cwd: Some(cwd.clone()),
            command: None,
            env: Default::default(),
        };
        let layout = ensure_workspace_and_tab(
            connector,
            key,
            context,
            reopen_owner(request)?,
            false,
            &root,
            &mut notices,
        )?;
        (layout.focused_pane_id, layout.tab_id)
    };
    if let Some(agent) = &pane.agent {
        start_or_degrade_agent(connector, key, 0, &restored_pane_id, agent, &mut notices);
    }
    Ok(ReopenOutcome {
        tab_id: Some(restored_tab_id),
        consumed: true,
        focused_pane_id: Some(restored_pane_id.clone()),
        notices: notices
            .into_iter()
            .map(|message| ReopenNotice {
                pane_id: Some(restored_pane_id.clone()),
                message,
            })
            .collect(),
    })
}

fn wrap_surviving_subtree(
    surviving: ClosedLayoutNode,
    parent_path: &[ClosedLayoutBranch],
    reopened: ClosedLayoutNode,
    direction: crate::recent_closed::ClosedSplitDirection,
    ratio: f32,
    target_was_first: bool,
) -> Result<ClosedLayoutNode, String> {
    if parent_path.is_empty() {
        let (first, second) = if target_was_first {
            (reopened, surviving)
        } else {
            (surviving, reopened)
        };
        return Ok(ClosedLayoutNode::Split {
            direction,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        });
    }
    let ClosedLayoutNode::Split {
        direction: surviving_direction,
        ratio: surviving_ratio,
        first,
        second,
    } = surviving
    else {
        return Err("the surviving layout no longer matches the closed pane's parent path".into());
    };
    let (first, second) = match parent_path[0] {
        ClosedLayoutBranch::First => (
            Box::new(wrap_surviving_subtree(
                *first,
                &parent_path[1..],
                reopened,
                direction,
                ratio,
                target_was_first,
            )?),
            second,
        ),
        ClosedLayoutBranch::Second => (
            first,
            Box::new(wrap_surviving_subtree(
                *second,
                &parent_path[1..],
                reopened,
                direction,
                ratio,
                target_was_first,
            )?),
        ),
    };
    Ok(ClosedLayoutNode::Split {
        direction: surviving_direction,
        ratio: surviving_ratio,
        first,
        second,
    })
}

fn reopen_owner(request: &ReopenRequest) -> Result<&OwnerOpen, String> {
    request
        .owner
        .as_ref()
        .ok_or_else(|| "the closed item names no checkout to reopen in".to_owned())
}

fn reopen_tab(
    connector: &dyn ApiConnector,
    key: &str,
    context: &ClosedContext,
    owner: &OwnerOpen,
    root: &ClosedLayoutNode,
    panes: &[ClosedPane],
) -> Result<ReopenOutcome, String> {
    let pane_map = panes
        .iter()
        .map(|pane| (pane.pane_id.clone(), pane.clone()))
        .collect();
    let mut terminal_ids = Vec::new();
    root.known_pane_ids(&pane_map, &mut terminal_ids);
    let mut common_notices = Vec::new();
    let Some(root) = root.resolve_panes(&pane_map, &context.checkout_path, &mut common_notices)
    else {
        return Err("the closed tab contained no panes".into());
    };
    let layout = ensure_workspace_and_tab(
        connector,
        key,
        context,
        owner,
        false,
        &root,
        &mut common_notices,
    )?;
    let mut new_ids = Vec::new();
    layout.root.pane_ids(&mut new_ids);
    let terminal_panes = terminal_ids
        .iter()
        .filter_map(|id| pane_map.get(id))
        .collect::<Vec<_>>();
    ensure_complete_tab_restore(terminal_panes.len(), new_ids.len())?;
    let mut notices = Vec::new();
    for (index, (pane, new_id)) in terminal_panes.iter().zip(new_ids.iter()).enumerate() {
        let mut pane_notices = Vec::new();
        if !Path::new(&pane.cwd).is_dir() {
            pane_notices.push(format!(
                "{} no longer exists; reopened in the checkout root",
                pane.cwd
            ));
        }
        if let Some(agent) = &pane.agent {
            start_or_degrade_agent(connector, key, index, new_id, agent, &mut pane_notices);
        }
        notices.extend(pane_notices.into_iter().map(|message| ReopenNotice {
            pane_id: Some(new_id.clone()),
            message,
        }));
    }
    let first = new_ids.first().cloned();
    notices.extend(common_notices.into_iter().map(|message| ReopenNotice {
        pane_id: first.clone(),
        message,
    }));
    Ok(ReopenOutcome {
        tab_id: Some(layout.tab_id),
        consumed: true,
        focused_pane_id: first,
        notices,
    })
}

fn ensure_complete_tab_restore(expected: usize, actual: usize) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "the reopened tab restored {actual} of {expected} terminal panes; retry is available"
        ))
    }
}

fn restored_cwd(cwd: &str, checkout_root: &str, notices: &mut Vec<String>) -> String {
    if Path::new(cwd).is_dir() {
        cwd.to_owned()
    } else {
        notices.push(format!(
            "{cwd} no longer exists; reopened in the checkout root"
        ));
        checkout_root.to_owned()
    }
}

fn start_or_degrade_agent(
    connector: &dyn ApiConnector,
    key: &str,
    index: usize,
    pane_id: &str,
    agent: &ClosedAgent,
    notices: &mut Vec<String>,
) {
    let name = format!("reopen-{key}-{index}");
    let interrupted_reused_agent = interrupt_reused_agent(connector, key, index, pane_id);
    let pane_was_clear = matches!(&interrupted_reused_agent, Ok(false));
    let resume = resume_arguments(agent);
    if let Some(args) = resume {
        let result = start_agent(
            connector,
            &format!("herdr-core:{key}:agent:{index}:resume"),
            pane_id,
            &name,
            &agent.kind,
            args,
        );
        if result.is_ok() || (pane_was_clear && agent_is_running(connector, pane_id, &agent.kind)) {
            return;
        }
        let preparation = interrupted_reused_agent
            .as_ref()
            .err()
            .map(|message| format!("; restored pane preparation failed: {message}"))
            .unwrap_or_default();
        notices.push(format!(
            "Session {} could not be resumed{preparation}; started a new conversation",
            agent.session_id.as_deref().unwrap_or("unknown"),
        ));
    } else {
        notices
            .push("Previous conversation could not be resumed; started a new conversation".into());
    }
    if let Err(message) = start_agent(
        connector,
        &format!("herdr-core:{key}:agent:{index}:fresh"),
        pane_id,
        &name,
        &agent.kind,
        Vec::new(),
    ) && !(pane_was_clear && agent_is_running(connector, pane_id, &agent.kind))
    {
        notices.push(format!(
            "Agent could not be started; the shell was kept: {message}"
        ));
    }
}

/// Herdr can reuse the just-closed pane id while its old PTY is still alive.
/// A layout-only restore then surfaces that old process in the new tab before
/// `agent.start` can apply the explicit resume arguments. Closing the process
/// here returns the restored pane to its shell prompt; the start waits for
/// that prompt (`agent_start`) and `agent.start` remains the single owner of
/// session resumption.
fn interrupt_reused_agent(
    connector: &dyn ApiConnector,
    key: &str,
    index: usize,
    pane_id: &str,
) -> Result<bool, String> {
    let snapshot = fetch_session_with_connector(connector).map_err(|error| {
        format!(
            "session.snapshot before agent resume failed: {}",
            error.message()
        )
    })?;
    if !snapshot
        .agents
        .iter()
        .any(|candidate| candidate.pane_id.as_deref() == Some(pane_id))
    {
        return Ok(false);
    }
    reopen_request(
        connector,
        &format!("herdr-core:{key}:agent:{index}:interrupt-reused"),
        "pane.send_text",
        wire::pane_send_text_params(pane_id, "\u{3}")?,
    )?;
    Ok(true)
}

fn agent_is_running(connector: &dyn ApiConnector, pane_id: &str, kind: &str) -> bool {
    fetch_session_with_connector(connector).is_ok_and(|snapshot| {
        snapshot.agents.iter().any(|agent| {
            agent.pane_id.as_deref() == Some(pane_id)
                && agent
                    .agent
                    .as_deref()
                    .is_some_and(|agent_kind| agent_kind.eq_ignore_ascii_case(kind))
        })
    })
}

fn start_agent(
    connector: &dyn ApiConnector,
    request_id: &str,
    pane_id: &str,
    name: &str,
    kind: &str,
    args: Vec<String>,
) -> Result<String, String> {
    crate::agent_start::start_at_shell(
        connector,
        request_id,
        pane_id,
        wire::agent_start_params(pane_id, name, kind, args)?,
        Duration::from_secs(5),
    )
    .map_err(|error| match error {
        crate::agent_start::StartError::NotStarted(message) => message,
        crate::agent_start::StartError::Herdr(error) => format!("agent.start failed: {error}"),
    })
    .and_then(wire::started_agent)
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
    spawn_pane_control_with_generation(context, action, None)
}

/// Starts a pane control worker with the connection generation that created it.
/// The generation is part of the operation identity, so a late answer from an
/// older Herdr socket cannot settle a newer request for the same pane.
pub fn spawn_pane_control_with_generation(
    context: LiveContext,
    action: PaneControlAction,
    connection_generation: Option<u64>,
) -> Result<(), String> {
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
        PaneControlAction::MoveToNewTab { .. } => "herdr-core-pane-move".to_owned(),
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
                Ok(mut guard) => guard.ingest_pane_control_failure_with_generation(
                    action,
                    result,
                    elapsed_ms,
                    connection_generation,
                ),
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

/// Where one pane's socket requests go: this machine's Herdr, or a device's,
/// with the id that Herdr knows the pane by beside the id the core projects.
#[derive(Clone)]
pub struct PaneApiRoute {
    pane_id: String,
    herdr_pane_id: String,
    api_connector: Arc<dyn ApiConnector>,
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

impl PaneApiRoute {
    pub(crate) fn local(context: &LiveContext, pane_id: &str) -> Self {
        Self {
            pane_id: pane_id.to_owned(),
            herdr_pane_id: pane_id.to_owned(),
            api_connector: Arc::clone(&context.api_connector),
            runtime: context.runtime.clone(),
            notifier: context.notifier.clone(),
        }
    }

    pub(crate) fn remote(
        context: &RemoteControlContext,
        pane_id: &str,
        herdr_pane_id: &str,
    ) -> Self {
        Self {
            pane_id: pane_id.to_owned(),
            herdr_pane_id: herdr_pane_id.to_owned(),
            api_connector: Arc::clone(&context.api_connector),
            runtime: context.runtime.clone(),
            notifier: context.notifier.clone(),
        }
    }

    /// Runs `work` on its own thread and hands its result to `ingest` under
    /// the runtime lock, notifying when that changed the snapshot.
    fn spawn<T: Send + 'static>(
        self,
        name: &str,
        work: impl FnOnce(&dyn ApiConnector, &str) -> T + Send + 'static,
        ingest: impl FnOnce(&mut Runtime, &str, T) -> bool + Send + 'static,
    ) -> Result<(), String> {
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let result = work(self.api_connector.as_ref(), &self.herdr_pane_id);
                let Some(runtime) = self.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => ingest(&mut guard, &self.pane_id, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    self.notifier.notify();
                }
            })
            .map(|_| ())
            .map_err(|error| format!("{name} worker could not be started: {error}"))
    }
}

/// Searches a pane's whole scrollback and moves the viewport to the match.
///
/// The two reads and the search happen on this thread, never under the runtime
/// mutex: the buffer is thousands of lines and every shell snapshot read blocks
/// on that mutex.
pub fn spawn_pane_find(route: PaneApiRoute, request: PaneFindRequest) -> Result<(), String> {
    route.spawn(
        "herdr-core-pane-find",
        move |connector, herdr_pane_id| run_pane_find(connector, herdr_pane_id, &request),
        |runtime, pane_id, result| runtime.ingest_pane_find(pane_id, result),
    )
}

/// Opens an agent's own search in its pane, unless Herdr holds the pane's
/// history, which Hide's find bar searches instead (`agent_find.rs`).
///
/// The requests run on this thread, never under the runtime mutex.
pub(crate) fn spawn_agent_find(
    route: PaneApiRoute,
    request_id: String,
    find: AgentFind,
) -> Result<(), String> {
    route.spawn(
        "herdr-core-agent-find",
        move |connector, herdr_pane_id| open_agent_find(connector, herdr_pane_id, find),
        move |runtime, pane_id, result| runtime.ingest_agent_find(pane_id, request_id, result),
    )
}

fn open_agent_find(
    connector: &dyn ApiConnector,
    pane_id: &str,
    find: AgentFind,
) -> Result<PaneFindRoute, String> {
    // An agent that draws inline leaves its conversation in Herdr's history,
    // where the find bar reaches all of it; its own search may not exist in
    // that mode (Claude Code's default renderer has none).
    let pane = control_request(connector, "pane.get", wire::pane_target_params(pane_id)?)?;
    if wire::pane_scroll(pane)?.is_some_and(|scroll| scroll.max_offset_from_bottom > 0) {
        return Ok(PaneFindRoute::Bar);
    }
    let visible = match find.already_open {
        Some(_) => Some(read_pane_text(connector, pane_id, "visible")?.text),
        None => None,
    };
    control_request(
        connector,
        "pane.send_keys",
        wire::pane_send_keys_params(pane_id, find.keys(visible.as_deref()))?,
    )?;
    Ok(PaneFindRoute::Agent)
}

/// Herdr's scroll position for one pane, in lines above the bottom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneScroll {
    pub offset_from_bottom: u64,
    pub max_offset_from_bottom: u64,
}

/// Moves an observed pane's viewport by `lines` (positive shows older lines).
///
/// `pane.scroll` takes an absolute offset and any socket client may send it,
/// which is what an observer without a terminal writer has; the offset comes
/// from the metrics `pane.get` reports, and Herdr clamps an overshoot at the
/// top. Two requests off the runtime mutex; the core keeps one in flight per
/// pane and sums the wheels that arrive meanwhile.
pub fn spawn_viewport_scroll(route: PaneApiRoute, lines: i32) -> Result<(), String> {
    route.spawn(
        "herdr-core-viewport-scroll",
        move |connector, herdr_pane_id| scroll_viewport(connector, herdr_pane_id, lines),
        |runtime, pane_id, result| runtime.ingest_viewport_scroll(pane_id, result),
    )
}

/// Why an observed pane's viewport did not move.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViewportScrollError {
    /// Herdr answered and refused the request (an older device Herdr without
    /// `pane.scroll`, say): this client cannot scroll the pane.
    Refused(String),
    /// Herdr was not reached or answered something unreadable; nothing is
    /// known about the pane's scrolling.
    Unreachable(String),
}

fn scroll_request(
    connector: &dyn ApiConnector,
    method: &str,
    params: Value,
) -> Result<Option<PaneScroll>, ViewportScrollError> {
    let answer = request_with_connector(connector, method, params, Duration::from_secs(5))
        .map_err(|error| match error {
            ApiError::Remote { code, message } => {
                ViewportScrollError::Refused(format!("{method} was refused: {code}: {message}"))
            }
            ApiError::Transport(message) | ApiError::Malformed(message) => {
                ViewportScrollError::Unreachable(format!("{method} failed: {message}"))
            }
        })?;
    wire::pane_scroll(answer).map_err(ViewportScrollError::Unreachable)
}

fn scroll_viewport(
    connector: &dyn ApiConnector,
    pane_id: &str,
    lines: i32,
) -> Result<Option<PaneScroll>, ViewportScrollError> {
    let target = wire::pane_target_params(pane_id).map_err(ViewportScrollError::Unreachable)?;
    let Some(current) = scroll_request(connector, "pane.get", target)? else {
        return Ok(None);
    };
    let distance = u64::from(lines.unsigned_abs());
    let target = if lines > 0 {
        current.offset_from_bottom.saturating_add(distance)
    } else {
        current.offset_from_bottom.saturating_sub(distance)
    };
    if target == current.offset_from_bottom {
        return Ok(Some(current));
    }
    let params =
        wire::pane_scroll_params(pane_id, target).map_err(ViewportScrollError::Unreachable)?;
    scroll_request(connector, "pane.scroll", params)
}

fn run_pane_find(
    connector: &dyn ApiConnector,
    pane_id: &str,
    request: &PaneFindRequest,
) -> Result<PaneFindOutcome, String> {
    if request.term.is_empty() {
        return Ok(PaneFindOutcome {
            term: String::new(),
            ..PaneFindOutcome::default()
        });
    }
    let buffer = read_pane_text(connector, pane_id, "recent")?;
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

    let visible = read_pane_text(connector, pane_id, "visible")?;
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
    thread::Builder::new()
        .name("herdr-core-agent-fork".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let parent_pane_id = request.parent_pane_id.clone();
            let result = run_agent_fork(context.api_connector.as_ref(), &request);
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

/// Splits beside the parent and starts the agent in the new pane. A pane whose
/// agent never started is closed again; hcoord registration happens only
/// after start succeeded and never turns a working fork into a failure.
fn run_agent_fork(connector: &dyn ApiConnector, request: &ForkRequest) -> Result<String, String> {
    run_agent_fork_with_registration(connector, request, register_fork_lineage)
}

fn run_agent_fork_with_registration(
    connector: &dyn ApiConnector,
    request: &ForkRequest,
    register: impl FnOnce(&str, &str) -> Result<(), String>,
) -> Result<String, String> {
    let child_pane_id = control_request(
        connector,
        "pane.split",
        wire::fork_split_params(&request.parent_pane_id, request.cwd.as_deref())?,
    )
    .and_then(wire::split_pane)?;
    let started = start_agent(
        connector,
        &format!("herdr-core:fork:{}:start", request.name),
        &child_pane_id,
        &request.name,
        request.agent.kind(),
        request.agent.resume_arguments(&request.session_id),
    );
    match started {
        Ok(pane_id) => {
            if let Err(message) = register(&request.parent_pane_id, &pane_id) {
                crate::diagnostic!(serde_json::json!({
                    "component": "fork",
                    "kind": "hcoord_registration_failed",
                    "parent_pane_id": request.parent_pane_id,
                    "child_pane_id": pane_id,
                    "message": message,
                }));
            }
            Ok(pane_id)
        }
        Err(error) => {
            let closed = control_request(
                connector,
                "pane.close",
                wire::pane_target_params(&child_pane_id)?,
            );
            Err(match closed {
                Ok(_) => error,
                Err(close_error) => format!(
                    "{error}; the pane {child_pane_id} it split could not be closed: {close_error}"
                ),
            })
        }
    }
}

fn register_fork_lineage(parent_pane_id: &str, child_pane_id: &str) -> Result<(), String> {
    let home = hide_platform::host::home_dir().map_err(|error| {
        format!("The home folder is unavailable ({error}), so the hcoord command cannot register the fork")
    })?;
    // The command the kit installs in hcoord's home (PRD hide-home-layout
    // D-08), relocated with hcoord when HCOORD_HOME is set.
    let binary = hide_kit::layout::hcoord_command(&hide_kit::layout::hcoord_home(
        &home,
        hide_kit::layout::hcoord_home_override().as_deref(),
    ));
    register_fork_lineage_with_binary(
        &binary,
        parent_pane_id,
        child_pane_id,
        Duration::from_secs(25),
    )
}

const HCOORD_LINK_OUTPUT_BYTES: usize = 64 * 1024;

fn register_fork_lineage_with_binary(
    binary: &Path,
    parent_pane_id: &str,
    child_pane_id: &str,
    timeout: Duration,
) -> Result<(), String> {
    let mut command = Command::new(binary);
    command
        .args([
            "agent",
            "link",
            "--parent-pane",
            parent_pane_id,
            "--child-pane",
            child_pane_id,
            "--json",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = OwnedChild::spawn(&mut command)
        .map_err(|error| format!("{} could not run: {error}", binary.display()))?;
    let stdout = child.take_stdout().expect("hcoord stdout is piped");
    let stderr = child.take_stderr().expect("hcoord stderr is piped");
    let stdout = thread::spawn(move || drain_capped(stdout, HCOORD_LINK_OUTPUT_BYTES));
    let stderr = thread::spawn(move || drain_capped(stderr, HCOORD_LINK_OUTPUT_BYTES));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(10)),
            outcome => {
                let reason = match outcome {
                    Err(error) => format!("hcoord wait failed: {error}"),
                    _ => format!("hcoord timed out after {} ms", timeout.as_millis()),
                };
                // The shim can exec a runtime that starts helpers. Kill the
                // invocation's whole tree so no descendant can retain a pipe
                // and keep this worker alive past its deadline.
                let _ = child.kill_tree();
                let _ = child.wait();
                break Err(reason);
            }
        }
    };
    let stdout = join_capped_output(stdout, "stdout")?;
    let stderr = join_capped_output(stderr, "stderr")?;
    let status = status?;
    if status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(if stderr.is_empty() { &stdout } else { &stderr });
    Err(format!(
        "hcoord refused fork registration ({}): {}",
        status,
        detail.trim().chars().take(300).collect::<String>()
    ))
}

fn drain_capped(mut reader: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut captured = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(captured);
        }
        let retained = (limit - captured.len()).min(read);
        captured.extend_from_slice(&buffer[..retained]);
    }
}

fn join_capped_output(
    reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stream: &str,
) -> Result<Vec<u8>, String> {
    reader
        .join()
        .map_err(|_| format!("hcoord {stream} reader failed"))?
        .map_err(|error| format!("hcoord {stream}: {error}"))
}

pub fn spawn_remote_control(
    context: RemoteControlContext,
    request_id: String,
    action: RemoteControlAction,
    connection_generation: u64,
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
        RemoteControlAction::Pane(PaneControlAction::MoveToNewTab { .. }) => {
            format!("herdr-core-remote-{target_id}-pane-move")
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
        RemoteControlAction::OpenOwner { .. } => {
            format!("herdr-core-remote-{target_id}-owner-open")
        }
        RemoteControlAction::CloseTab { .. } => {
            format!("herdr-core-remote-{target_id}-tab-close")
        }
        RemoteControlAction::RenameTab { .. } => {
            format!("herdr-core-remote-{target_id}-tab-rename")
        }
        RemoteControlAction::MoveTab { .. } => {
            format!("herdr-core-remote-{target_id}-tab-move")
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
                Ok(mut guard) => guard.ingest_remote_control_failure_with_generation(
                    &target_id,
                    &request_id,
                    action,
                    result,
                    elapsed_ms,
                    Some(connection_generation),
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

// A local create is tagged once. A lost reply gets one bounded read-only
// recovery, never a repeated mutation; a still-unknown result retains its claim.
fn local_create_key(id: u64) -> String {
    format!("create-{}-{id}", std::process::id())
}
fn local_create_env(id: Option<u64>) -> std::collections::BTreeMap<String, String> {
    id.map(|id| reopen_intent_env(&local_create_key(id), ReopenIntentStage::LocalCreate))
        .unwrap_or_default()
}
fn execute_local_control(
    connector: &dyn ApiConnector,
    action: &RemoteControlAction,
) -> Result<RemoteControlOutcome, ControlFailure> {
    let result = execute_remote_control(connector, action);
    if !result.as_ref().is_err_and(ControlFailure::is_ambiguous) {
        return result;
    }
    match recover_local_creation(connector, action) {
        Ok(outcome) => Ok(outcome),
        Err(message) => {
            crate::diagnostic!(
                json!({"component":"tab_control", "kind":"tab.create.recovery_unknown", "message":message})
            );
            result
        }
    }
}
fn recover_local_creation(
    connector: &dyn ApiConnector,
    action: &RemoteControlAction,
) -> Result<RemoteControlOutcome, String> {
    let (id, workspace) = match action {
        RemoteControlAction::CreateTab {
            admission_id: Some(id),
            workspace_id,
            ..
        } => (*id, Some(workspace_id.as_str())),
        RemoteControlAction::OpenOwner {
            admission_id: Some(id),
            ..
        } => (*id, None),
        _ => return Err("No local creation claim to recover".into()),
    };
    let started = Instant::now();
    let snapshot =
        fetch_session_with_connector(connector).map_err(|error| error.message().to_owned())?;
    let tabs = snapshot
        .tabs
        .iter()
        .filter(|tab| workspace.is_none_or(|id| tab.workspace_id == id))
        .collect::<Vec<_>>();
    if tabs.len() > 64 {
        return Err("Too many tabs for bounded creation recovery".into());
    }
    let key = local_create_key(id);
    let marker = reopen_intent_marker(&key, ReopenIntentStage::LocalCreate);
    let mut found = None;
    for tab in tabs {
        if started.elapsed() > Duration::from_secs(5) {
            return Err("Creation recovery timed out".into());
        }
        let layout = export_reopen_layout(connector, &key, &tab.tab_id)?;
        let mut panes = Vec::new();
        pane_ids_with_reopen_marker(&layout.root, &marker, &mut panes);
        if let Some(pane) = panes.first() {
            if found.is_some() {
                return Err("Multiple tabs carry the creation claim".into());
            }
            found = Some(RemoteControlOutcome::Acknowledged {
                created_tab_id: Some(tab.tab_id.clone()),
                created_pane_id: Some(pane.clone()),
            });
        }
    }
    found.ok_or_else(|| "Creation is not yet confirmed".into())
}

pub fn spawn_local_control(
    context: LiveContext,
    action: RemoteControlAction,
) -> Result<(), String> {
    let worker_name = match &action {
        RemoteControlAction::FocusTab { .. } => "herdr-core-tab-focus",
        RemoteControlAction::CreateTab { .. } => "herdr-core-tab-create",
        RemoteControlAction::OpenOwner { .. } => "herdr-core-owner-open",
        RemoteControlAction::CloseTab { .. } => "herdr-core-tab-close",
        RemoteControlAction::MoveTab { .. } => "herdr-core-tab-move",
        RemoteControlAction::RenameTab { .. } => "herdr-core-tab-rename",
        _ => return Err(format!("{} is not a local tab action", action.kind())),
    };
    thread::Builder::new()
        .name(worker_name.to_owned())
        .spawn(move || {
            let started = Instant::now();
            let result = execute_local_control(context.api_connector.as_ref(), &action);
            let elapsed_ms = started.elapsed().as_millis();
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_local_control_failure(action, result, elapsed_ms),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionFetchError {
    /// The socket file itself does not exist: the herdr server is not running.
    SocketMissing(String),
    /// The socket exists but the request failed (connect, timeout, IO).
    Unreachable(String),
    /// The server answered with an incompatible protocol revision.
    Protocol {
        message: String,
        expected_protocol: u64,
        received_protocol: u64,
        received_version: Option<String>,
    },
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
            Self::Protocol { .. } => "protocol_mismatch",
            Self::Stale(_) => "stale",
            Self::Malformed(_) => "malformed",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::SocketMissing(message)
            | Self::Unreachable(message)
            | Self::Stale(message)
            | Self::Malformed(message) => message,
            Self::Protocol { message, .. } => message,
        }
    }

    pub fn protocol_details(&self) -> Option<(u64, u64, Option<&str>)> {
        match self {
            Self::Protocol {
                expected_protocol,
                received_protocol,
                received_version,
                ..
            } => Some((
                *expected_protocol,
                *received_protocol,
                received_version.as_deref(),
            )),
            _ => None,
        }
    }
}

/// Installs live command context and starts event-driven session sync.
pub(crate) fn install(
    runtime: &Arc<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    socket_path: &str,
    herdr_bin: Option<&str>,
    usage_paths: crate::usage::UsagePaths,
) -> Option<crate::session_sync::SessionSyncHandle> {
    let context = LiveContext {
        socket_path: PathBuf::from(socket_path),
        herdr_bin: herdr_bin.map(PathBuf::from),
        runtime: Arc::downgrade(runtime),
        notifier: notifier.clone(),
        api_connector: Arc::new(LocalSocketConnector::new(socket_path)),
    };
    if let Ok(mut guard) = runtime.lock() {
        guard.set_live(context.clone());
    }
    match crate::session_sync::spawn(
        crate::session_sync::SessionSyncContext::local(&context),
        Some(usage_paths),
    ) {
        Ok(handle) => Some(handle),
        Err(message) => {
            crate::diagnostic!(json!({
                "component": "session_sync",
                "kind": "coordinator.spawn_failed",
                "message": message,
            }));
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
    fetch_session_with_connector(&LocalSocketConnector::new(socket_path))
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
    Local(OwnedChild),
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
        let mut child = OwnedChild::spawn(&mut command).map_err(|error| {
            format!(
                "herdr terminal session {} could not be spawned: {error}",
                mode.as_str()
            )
        })?;
        let writer = if mode == TerminalSessionMode::Control {
            let stdin = child
                .take_stdin()
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
            .take_stdout()
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
            .open_terminal_session(source_pane_id, mode.as_str(), rows, cols)
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

    pub fn write_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        let Some(writer) = self.writer.as_ref() else {
            return Err(format!(
                "Pane {} is read-only because another client owns terminal control",
                self.pane_id
            ));
        };
        let line = terminal_input_line(bytes)?;
        writer
            .send(TerminalWriterCommand::Input { line })
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
        }));
        Ok(())
    }
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

                let (line, release_acknowledgement, is_release) = match command {
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
                        (line, None, false)
                    }
                    TerminalWriterCommand::Resize { line }
                    | TerminalWriterCommand::Input { line } => (line, None, false),
                    TerminalWriterCommand::Release { line, acknowledged } => {
                        (line, Some(acknowledged), true)
                    }
                };
                let result = stdin
                    .write_all(line.as_bytes())
                    .and_then(|()| stdin.flush());
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
                    }));
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
            }));
        }
    }
}

fn reap_local_terminal_child(child: &mut OwnedChild, pane_id: &str) {
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
                }));
                break;
            }
        }
    }
    if let Err(error) = child.kill_tree() {
        crate::diagnostic!(json!({
            "component": "terminal_session",
            "kind": "terminal.session_kill_failed",
            "pane_id": pane_id,
            "message": error.to_string(),
        }));
    }
    if let Err(error) = child.wait() {
        crate::diagnostic!(json!({
            "component": "terminal_session",
            "kind": "terminal.session_wait_failed",
            "pane_id": pane_id,
            "message": error.to_string(),
        }));
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
    use serde_json::json;

    use super::*;
    use crate::fake_herdr::FakeHerdr;

    // The fixture is a shell script.
    #[cfg(unix)]
    #[test]
    fn a_hung_hcoord_link_is_killed_without_closing_the_started_fork() {
        let directory = tempfile::tempdir().expect("create hcoord fixture directory");
        let binary = directory.path().join("hcoord");
        let pid_path = directory.path().join("pid");
        crate::executable_fixture::write_executable(
            &binary,
            &format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec /bin/sleep 300\n",
                pid_path.display()
            ),
        );
        let herdr = FakeHerdr::start("fork-hcoord-timeout", |method, _| match method {
            "pane.split" => json!({"type": "pane_info", "pane": {
                "pane_id": "child-pane", "terminal_id": "child-terminal",
                "workspace_id": "w1", "tab_id": "w1:t1", "focused": true,
                "agent_status": "idle", "revision": 1
            }}),
            "pane.process_info" => json!({"type": "pane_process_info", "process_info": {
                "pane_id": "child-pane", "shell_pid": 42, "foreground_process_group_id": 42,
                "foreground_processes": [{"pid": 42, "name": "zsh"}]
            }}),
            "agent.start" => json!({"type": "agent_started", "argv": [], "agent": {
                "pane_id": "child-pane", "terminal_id": "child-terminal",
                "workspace_id": "w1", "tab_id": "w1:t1", "focused": true,
                "agent_status": "idle", "revision": 2
            }}),
            other => panic!("unexpected {other}"),
        });
        let request = ForkRequest {
            parent_pane_id: "parent-pane".to_owned(),
            agent: crate::fork::ForkableAgent::Codex,
            session_id: "3f2b1c00-0000-4000-8000-000000000001".to_owned(),
            cwd: Some("/checkout".to_owned()),
            name: "fork-parent-pane-1".to_owned(),
        };

        let started = Instant::now();
        let pane_id = run_agent_fork_with_registration(
            &herdr.connector(),
            &request,
            |parent_pane_id, child_pane_id| {
                register_fork_lineage_with_binary(
                    &binary,
                    parent_pane_id,
                    child_pane_id,
                    Duration::from_secs(2),
                )
            },
        )
        .expect("lineage failure does not turn a working fork into a failure");

        assert_eq!(pane_id, "child-pane");
        assert_eq!(
            herdr.methods(),
            ["pane.split", "pane.process_info", "agent.start"]
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the fork worker must settle promptly"
        );
        let pid: u32 = std::fs::read_to_string(pid_path)
            .expect("fixture recorded its process id")
            .parse()
            .expect("fixture process id is numeric");
        assert!(
            !hide_platform::process::is_alive(pid),
            "the timed-out hcoord process must not survive"
        );
    }

    #[test]
    fn tab_rename_uses_the_selected_host_and_reports_refusal() {
        let host = FakeHerdr::start_with_errors("tab-rename", |method, params| {
            assert_eq!(method, "tab.rename");
            assert_eq!(params["tab_id"], "w1:t1");
            if params["label"] == "refuse" {
                Err(("not_found".into(), "tab unavailable".into()))
            } else {
                Ok(json!({"type":"ok"}))
            }
        });
        for label in ["이름", "", "refuse"] {
            let result = execute_remote_control(
                &host.connector(),
                &RemoteControlAction::RenameTab {
                    tab_id: "w1:t1".into(),
                    label: label.into(),
                    request_id: "r1".into(),
                },
            );
            assert_eq!(result.is_ok(), label != "refuse");
        }
        assert_eq!(host.methods(), ["tab.rename", "tab.rename", "tab.rename"]);
    }

    #[test]
    fn nested_split_reopen_applies_the_original_outer_and_inner_geometry() {
        let herdr = FakeHerdr::start("reopen-nested-layout", |method, params| match method {
            "layout.export" => json!({"type":"layout_export","layout": {
                "workspace_id":"w1", "tab_id":"w1:t1", "zoomed":false,
                "focused_pane_id":"outer-left",
                "root":{"type":"split", "direction":"right", "ratio":0.4,
                    "first":{"type":"pane", "pane_id":"outer-left", "cwd":"/tmp", "env":{}},
                    "second":{"type":"split", "direction":"right", "ratio":0.65,
                        "first":{"type":"pane", "pane_id":"inner-left", "cwd":"/tmp", "env":{}},
                        "second":{"type":"pane", "pane_id":"inner-right", "cwd":"/tmp", "env":{}}}}
            }}),
            "layout.apply" => {
                let root = &params["root"];
                assert_eq!(root["direction"], "right");
                assert!((root["ratio"].as_f64().unwrap() - 0.4).abs() < 0.000_001);
                assert_eq!(root["first"]["pane_id"], "outer-left");
                assert_eq!(root["second"]["direction"], "down");
                assert!((root["second"]["ratio"].as_f64().unwrap() - 0.3).abs() < 0.000_001);
                assert_eq!(
                    root["second"]["first"]["env"][REOPEN_INTENT_ENV],
                    reopen_intent_marker("nested-intent", ReopenIntentStage::Pane)
                );
                assert_eq!(root["second"]["second"]["direction"], "right");
                assert!(
                    (root["second"]["second"]["ratio"].as_f64().unwrap() - 0.65).abs() < 0.000_001
                );
                assert_eq!(root["second"]["second"]["first"]["pane_id"], "inner-left");
                assert_eq!(root["second"]["second"]["second"]["pane_id"], "inner-right");
                json!({"type":"layout_apply","layout": {
                    "workspace_id":"w1", "tab_id":"w1:t1", "zoomed":false,
                    "focused_pane_id":"reopened",
                    "root":{"type":"split", "direction":"right", "ratio":0.4,
                        "first":{"type":"pane", "pane_id":"outer-left", "cwd":"/tmp", "env":{}},
                        "second":{"type":"split", "direction":"down", "ratio":0.3,
                            "first":{"type":"pane", "pane_id":"reopened", "cwd":"/tmp", "env":{
                                (REOPEN_INTENT_ENV): reopen_intent_marker("nested-intent", ReopenIntentStage::Pane)
                            }},
                            "second":{"type":"split", "direction":"right", "ratio":0.65,
                                "first":{"type":"pane", "pane_id":"inner-left", "cwd":"/tmp", "env":{}},
                                "second":{"type":"pane", "pane_id":"inner-right", "cwd":"/tmp", "env":{}}}}}
                }})
            }
            other => panic!("unexpected {other}"),
        });
        let context = ClosedContext {
            workspace_id: "w1".into(),
            workspace_label: "Fixture".into(),
            workspace_ids_before_close: vec!["w1".into()],
            tab_ids_before_close: vec!["w1:t1".into()],
            pane_ids_before_close: vec![
                "outer-left".into(),
                "closed".into(),
                "inner-left".into(),
                "inner-right".into(),
            ],
            checkout_id: "checkout".into(),
            checkout_path: "/tmp".into(),
            tab_id: "w1:t1".into(),
            tab_label: "Tab".into(),
            tab_index: 0,
            agent_area: None,
            replacement_shell: false,
        };
        let pane = ClosedPane {
            pane_id: "closed".into(),
            label: None,
            cwd: "/tmp".into(),
            agent: None,
        };
        let placement = PanePlacement {
            neighbor_pane_id: None,
            parent_path: vec![ClosedLayoutBranch::Second],
            direction: crate::recent_closed::ClosedSplitDirection::Down,
            ratio: 0.3,
            target_was_first: true,
        };
        let request = ReopenRequest {
            item: ClosedItem::Pane {
                key: "nested-intent".into(),
                context: context.clone(),
                pane: pane.clone(),
                placement: placement.clone(),
            },
            workspace_exists: true,
            tab_exists: true,
            fallback_pane_id: Some("outer-left".into()),
            owner: None,
        };

        let outcome = reopen_pane(
            &herdr.connector(),
            "nested-intent",
            &context,
            &pane,
            &placement,
            &request,
        )
        .expect("nested pane reopens through layout.apply");

        assert_eq!(outcome.focused_pane_id.as_deref(), Some("reopened"));
        assert_eq!(herdr.methods(), ["layout.export", "layout.apply"]);
    }

    #[test]
    fn over_count_tab_layout_is_not_adopted_or_overwritten() {
        let pane = |id: &str| ClosedLayoutNode::Pane {
            pane_id: Some(id.to_owned()),
            label: None,
            cwd: Some("/tmp".to_owned()),
            command: None,
            env: Default::default(),
        };
        let expected_root = ClosedLayoutNode::Split {
            direction: crate::recent_closed::ClosedSplitDirection::Right,
            ratio: 0.5,
            first: Box::new(pane("expected-a")),
            second: Box::new(pane("expected-b")),
        };
        let actual_root = ClosedLayoutNode::Split {
            direction: crate::recent_closed::ClosedSplitDirection::Right,
            ratio: 0.5,
            first: Box::new(expected_root.clone()),
            second: Box::new(pane("unknown-extra")),
        };
        let context = ClosedContext {
            workspace_id: "w1".into(),
            workspace_label: "Fixture".into(),
            workspace_ids_before_close: vec!["w1".into()],
            tab_ids_before_close: vec!["w1:t1".into()],
            pane_ids_before_close: vec!["w1:p1".into(), "w1:p2".into()],
            checkout_id: "checkout".into(),
            checkout_path: "/tmp".into(),
            tab_id: "w1:t1".into(),
            tab_label: "Tab".into(),
            tab_index: 0,
            agent_area: None,
            replacement_shell: false,
        };
        let error = repair_incomplete_tab_layout(
            &LocalSocketConnector::new("/tmp/hide-reopen-over-count-must-not-connect.sock"),
            "over-count",
            &context,
            &expected_root,
            crate::recent_closed::ClosedLayout {
                workspace_id: "w1".into(),
                tab_id: "w1:t2".into(),
                zoomed: false,
                focused_pane_id: "unknown-extra".into(),
                root: actual_root,
            },
            &mut Vec::new(),
        )
        .expect_err("an extra pane is not owned by the reopen intent");

        assert!(error.contains("returned 3 panes, expected 2"));
        assert!(error.contains("not adopted or overwritten"));
        assert!(error.contains("retry is available"));
    }

    #[test]
    fn partial_tab_restore_is_not_consumed_as_success() {
        let error = ensure_complete_tab_restore(3, 2).expect_err("one pane is still missing");
        assert!(error.contains("restored 2 of 3"));
        assert!(error.contains("retry is available"));
        assert!(ensure_complete_tab_restore(3, 3).is_ok());
    }

    // A folder is made untraversable through its mode bits.
    #[cfg(unix)]
    #[test]
    fn file_reopen_distinguishes_metadata_failure_from_a_missing_file() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("hide-file-metadata-denied-{}", std::process::id()));
        let denied = root.join("denied");
        let file = denied.join("document.txt");
        std::fs::create_dir_all(&denied).expect("create denied fixture");
        std::fs::write(&file, "saved").expect("write fixture");
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000))
            .expect("deny traversal");

        let result = run_file_reopen(
            &crate::host_access::InProcessHost,
            &crate::files::DocumentRoot {
                device_id: "local".to_owned(),
                path: root.to_string_lossy().into_owned(),
                identity: None,
            },
            file.to_str().unwrap(),
        );

        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o700))
            .expect("restore traversal");
        std::fs::remove_dir_all(&root).expect("remove fixture");
        match result {
            FileReopenResultOrHerdr::File(FileReopenResult::Failed(message)) => {
                assert!(message.contains("could not be opened"), "{message}");
            }
            other => panic!("permission failure must stay retryable, got {other:?}"),
        }
    }

    #[test]
    fn pane_reopen_ignores_an_unowned_same_cwd_pane() {
        let herdr = FakeHerdr::start("reopen-owned-pane", |method, _| match method {
            "session.snapshot" => json!({"type":"session_snapshot","snapshot": {
                "version":"fixture", "protocol":HERDR_PROTOCOL_REVISION,
                "workspaces":[], "tabs":[], "agents":[],
                "panes":[{"pane_id":"w1:p3", "terminal_id": "fixture-terminal","workspace_id":"w1","tab_id":"w1:t1","cwd":"/tmp","focused":false,"agent_status":"idle","revision":0}],
                "layouts":[{"workspace_id":"w1","tab_id":"w1:t1","zoomed":false,
                    "area":{"x":0,"y":0,"width":80,"height":24}, "focused_pane_id":"w1:p1",
                    "panes":[{"pane_id":"w1:p1","focused":true,"rect":{"x":0,"y":0,"width":40,"height":24}},
                             {"pane_id":"w1:p3","focused":false,"rect":{"x":40,"y":0,"width":40,"height":24}}],
                    "splits":[] }]
            }}),
            "layout.export" => json!({
                "type": "layout_export",
                "layout": {
                    "workspace_id": "w1", "tab_id": "w1:t1", "zoomed": false,
                    "focused_pane_id": "w1:p1",
                    "root": {"type":"split", "direction":"right", "ratio":0.5,
                        "first":{"type":"pane","pane_id":"w1:p1","cwd":"/tmp","env":{}},
                        "second":{"type":"pane","pane_id":"w1:p3","cwd":"/tmp","env":{}}}
                }
            }),
            "pane.split" => json!({"type": "pane_info", "pane": {
                "pane_id": "w1:p4", "terminal_id": "fixture-terminal", "workspace_id": "w1", "tab_id": "w1:t1",
                "focused": true, "agent_status": "idle", "revision": 1
            }}),
            method => panic!("unexpected method {method}"),
        });
        let context = ClosedContext {
            workspace_id: "w1".into(),
            workspace_label: "fixture".into(),
            workspace_ids_before_close: vec!["w1".into()],
            tab_ids_before_close: vec!["w1:t1".into()],
            pane_ids_before_close: vec!["w1:p1".into(), "w1:p2".into()],
            checkout_id: "checkout".into(),
            checkout_path: "/tmp".into(),
            tab_id: "w1:t1".into(),
            tab_label: "tab".into(),
            tab_index: 0,
            agent_area: None,
            replacement_shell: false,
        };
        let pane = ClosedPane {
            pane_id: "w1:p2".into(),
            label: None,
            cwd: "/tmp".into(),
            agent: None,
        };
        let placement = PanePlacement {
            neighbor_pane_id: Some("w1:p1".into()),
            parent_path: Vec::new(),
            direction: crate::recent_closed::ClosedSplitDirection::Right,
            ratio: 0.5,
            target_was_first: false,
        };
        let request = ReopenRequest {
            item: ClosedItem::Pane {
                key: "first-attempt".into(),
                context: context.clone(),
                pane: pane.clone(),
                placement: placement.clone(),
            },
            workspace_exists: true,
            tab_exists: true,
            fallback_pane_id: Some("w1:p1".into()),
            owner: None,
        };

        let outcome = reopen_pane(
            &herdr.connector(),
            "first-attempt",
            &context,
            &pane,
            &placement,
            &request,
        )
        .expect("fixture reopen succeeds");

        assert_eq!(outcome.focused_pane_id.as_deref(), Some("w1:p4"));
        let requests = herdr.requests();
        let split = requests
            .iter()
            .find(|request| request["method"] == "pane.split")
            .expect("the reopen creates its own pane");
        assert_eq!(
            split["params"]["env"][REOPEN_INTENT_ENV],
            reopen_intent_marker("first-attempt", ReopenIntentStage::Pane)
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "pane.send_text")
        );
    }

    #[test]
    fn tab_reopen_ignores_unowned_same_label_workspace_and_tab() {
        let mut layout_apply_count = 0;
        let herdr = FakeHerdr::start("reopen-owned-layout", move |method, params| match method {
            "session.snapshot" => json!({"type":"session_snapshot","snapshot": {
                "version":"fixture", "protocol":HERDR_PROTOCOL_REVISION,
                "workspaces":[{"workspace_id":"w2","label":"Fixture","active_tab_id":"w2:t1","number":2,"focused":true,"pane_count":1,"tab_count":1,"agent_status":"idle"}],
                "tabs":[{"workspace_id":"w2","tab_id":"w2:t1","label":"Tab","number":1,"focused":true,"pane_count":1,"agent_status":"idle"}],
                "panes":[], "layouts":[], "agents":[]
            }}),
            "layout.export" => json!({"type":"layout_export","layout": {
                "workspace_id":"w2", "tab_id":"w2:t1", "zoomed":false,
                "focused_pane_id":"w2:p9",
                "root":{"type":"pane","pane_id":"w2:p9","cwd":"/tmp","env":{}}
            }}),
            "workspace.create" => {
                assert_eq!(
                    params["env"][REOPEN_INTENT_ENV],
                    reopen_intent_marker("layout-intent", ReopenIntentStage::Workspace)
                );
                json!({"type":"workspace_created",
                    "workspace":{"workspace_id":"w3","number":3,"label":"Fixture","focused":true,"pane_count":1,"tab_count":1,"active_tab_id":"w3:t1","agent_status":"idle"},
                    "tab":{"tab_id":"w3:t1","workspace_id":"w3","number":1,"label":"1","focused":true,"pane_count":1,"agent_status":"idle"},
                    "root_pane":{"pane_id":"w3:p3", "terminal_id": "fixture-terminal","workspace_id":"w3","tab_id":"w3:t1","focused":true,"agent_status":"idle","revision":1}
                })
            }
            "layout.apply" => {
                assert_eq!(params["tab_id"], "w3:t1");
                assert!(params.get("workspace_id").is_none());
                assert_eq!(
                    params["root"]["first"]["env"][REOPEN_INTENT_ENV],
                    reopen_intent_marker("layout-intent", ReopenIntentStage::Layout)
                );
                layout_apply_count += 1;
                if layout_apply_count == 1 {
                    json!({"type":"layout_apply","layout": {
                        "workspace_id":"w3", "tab_id":"w3:t1", "zoomed":false,
                        "focused_pane_id":"w3:p3",
                        "root":{"type":"pane","pane_id":"w3:p3","cwd":"/tmp","env":{
                            (REOPEN_INTENT_ENV): reopen_intent_marker("layout-intent", ReopenIntentStage::Layout)
                        }}
                    }})
                } else {
                    json!({"type":"layout_apply","layout": {
                        "workspace_id":"w3", "tab_id":"w3:t1", "zoomed":false,
                        "focused_pane_id":"w3:p3",
                        "root":{"type":"split","direction":"right","ratio":0.6,
                            "first":{"type":"pane","pane_id":"w3:p3","cwd":"/tmp","env":{
                                (REOPEN_INTENT_ENV): reopen_intent_marker("layout-intent", ReopenIntentStage::Layout)
                            }},
                            "second":{"type":"pane","pane_id":"w3:p4","cwd":"/tmp","env":{
                                (REOPEN_INTENT_ENV): reopen_intent_marker("layout-intent", ReopenIntentStage::Layout)
                            }}}
                    }})
                }
            }
            "workspace.list" => json!({"type":"workspace_list","workspaces":[
                {"workspace_id":"w2","number":2,"label":"other","focused":true,"pane_count":1,"tab_count":1,"active_tab_id":"w2:t1","agent_status":"idle"}
            ]}),
            "workspace.report_metadata" => json!({"type":"ok"}),
            _ => unreachable!(),
        });
        let context = ClosedContext {
            workspace_id: "w1".into(),
            workspace_label: "Fixture".into(),
            workspace_ids_before_close: vec!["w1".into()],
            tab_ids_before_close: vec!["w1:t1".into()],
            pane_ids_before_close: vec!["w1:p1".into()],
            checkout_id: "checkout".into(),
            checkout_path: "/tmp".into(),
            tab_id: "w1:t1".into(),
            tab_label: "Tab".into(),
            tab_index: 0,
            agent_area: None,
            replacement_shell: false,
        };
        let layout_root = ClosedLayoutNode::Split {
            direction: crate::recent_closed::ClosedSplitDirection::Right,
            ratio: 0.6,
            first: Box::new(ClosedLayoutNode::Pane {
                pane_id: None,
                label: None,
                cwd: Some("/tmp".into()),
                command: None,
                env: Default::default(),
            }),
            second: Box::new(ClosedLayoutNode::Pane {
                pane_id: None,
                label: None,
                cwd: Some("/tmp".into()),
                command: None,
                env: Default::default(),
            }),
        };
        let mut notices = Vec::new();
        let restored = ensure_workspace_and_tab(
            &herdr.connector(),
            "layout-intent",
            &context,
            &OwnerOpen::Folder {
                path: "/tmp".into(),
                label: "Fixture".into(),
                mark: crate::checkout_owner::owner_mark("local", "/tmp"),
            },
            false,
            &layout_root,
            &mut notices,
        )
        .expect("owned layout is created");

        assert_eq!(restored.workspace_id, "w3");
        assert_eq!(restored.tab_id, "w3:t1");
        assert_eq!(restored.focused_pane_id, "w3:p3");
        assert_eq!(restored.root.pane_count(), 2);
        assert_eq!(
            notices,
            ["The incomplete tab restore was repaired before reopening sessions"]
        );
        assert_eq!(
            herdr.methods(),
            // The closed tab's workspace is gone, so it reopens in the
            // folder's owner, made and marked here; the old index names a
            // place in another workspace and is not restored.
            [
                "session.snapshot",
                "layout.export",
                "workspace.list",
                "workspace.create",
                "workspace.report_metadata",
                "layout.apply",
                "layout.apply"
            ]
        );
    }

    #[test]
    fn recovered_partial_tab_layout_is_repaired_and_revalidated() {
        let herdr = FakeHerdr::start(
            "reopen-repair-recovered",
            move |method, params| match method {
                "session.snapshot" => json!({"type":"session_snapshot","snapshot": {
                    "version":"fixture", "protocol":HERDR_PROTOCOL_REVISION,
                    "workspaces":[{"workspace_id":"w1","label":"Fixture","active_tab_id":"w1:t2","number":1,"focused":true,"pane_count":1,"tab_count":1,"agent_status":"idle",
                        "worktree":{"repo_key":"/tmp","repo_name":"tmp","repo_root":"/tmp","checkout_path":"/tmp","is_linked_worktree":false}}],
                    "tabs":[{"workspace_id":"w1","tab_id":"w1:t2","label":"Tab","number":1,"focused":true,"pane_count":1,"agent_status":"idle"}],
                    "panes":[], "layouts":[], "agents":[]
                }}),
                "layout.export" => json!({"type":"layout_export","layout": {
                    "workspace_id":"w1", "tab_id":"w1:t2", "zoomed":false,
                    "focused_pane_id":"w1:p2",
                    "root":{"type":"pane","pane_id":"w1:p2","cwd":"/tmp","env":{
                        (REOPEN_INTENT_ENV): reopen_intent_marker("recovered-intent", ReopenIntentStage::Layout)
                    }}
                }}),
                "layout.apply" => {
                    assert_eq!(params["tab_id"], "w1:t2");
                    assert_eq!(
                        params["root"]["second"]["env"][REOPEN_INTENT_ENV],
                        reopen_intent_marker("recovered-intent", ReopenIntentStage::Layout)
                    );
                    json!({"type":"layout_apply","layout": {
                        "workspace_id":"w1", "tab_id":"w1:t2", "zoomed":false,
                        "focused_pane_id":"w1:p2",
                        "root":{"type":"split","direction":"right","ratio":0.6,
                            "first":{"type":"pane","pane_id":"w1:p2","cwd":"/tmp","env":{
                                (REOPEN_INTENT_ENV): reopen_intent_marker("recovered-intent", ReopenIntentStage::Layout)
                            }},
                            "second":{"type":"pane","pane_id":"w1:p3","cwd":"/tmp","env":{
                                (REOPEN_INTENT_ENV): reopen_intent_marker("recovered-intent", ReopenIntentStage::Layout)
                            }}}
                    }})
                }
                "tab.move" => json!({"type":"tab_list","tabs":[]}),
                _ => unreachable!(),
            },
        );
        let context = ClosedContext {
            workspace_id: "w1".into(),
            workspace_label: "Fixture".into(),
            workspace_ids_before_close: vec!["w1".into()],
            tab_ids_before_close: vec!["w1:t1".into()],
            pane_ids_before_close: vec!["w1:p1".into()],
            checkout_id: "checkout".into(),
            checkout_path: "/tmp".into(),
            tab_id: "w1:t1".into(),
            tab_label: "Tab".into(),
            tab_index: 0,
            agent_area: None,
            replacement_shell: false,
        };
        let root = ClosedLayoutNode::Split {
            direction: crate::recent_closed::ClosedSplitDirection::Right,
            ratio: 0.6,
            first: Box::new(ClosedLayoutNode::Pane {
                pane_id: None,
                label: None,
                cwd: Some("/tmp".into()),
                command: None,
                env: Default::default(),
            }),
            second: Box::new(ClosedLayoutNode::Pane {
                pane_id: None,
                label: None,
                cwd: Some("/tmp".into()),
                command: None,
                env: Default::default(),
            }),
        };
        let mut notices = Vec::new();
        let restored = ensure_workspace_and_tab(
            &herdr.connector(),
            "recovered-intent",
            &context,
            &OwnerOpen::Worktree {
                path: "/tmp".into(),
                repository_root: "/tmp".into(),
                label: "Fixture".into(),
            },
            false,
            &root,
            &mut notices,
        )
        .expect("the recovered layout is repaired");

        assert_eq!(restored.tab_id, "w1:t2");
        assert_eq!(restored.root.pane_count(), 2);
        assert_eq!(
            notices,
            ["The incomplete tab restore was repaired before reopening sessions"]
        );
        assert_eq!(
            herdr.methods(),
            [
                "session.snapshot",
                "layout.export",
                "layout.apply",
                "tab.move"
            ]
        );
    }

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
    fn reused_agent_process_is_interrupted_before_resume() {
        let herdr = FakeHerdr::start("reopen-interrupt", |method, params| match method {
            "session.snapshot" => json!({
                "type": "session_snapshot",
                "snapshot": {
                    "version": "fixture",
                    "protocol": HERDR_PROTOCOL_REVISION,
                    "workspaces": [],
                    "tabs": [],
                    "panes": [],
                    "layouts": [],
                    "agents": [{
                        "terminal_id": "term-1",
                        "name": "old-agent",
                        "agent": "claude",
                        "agent_status": "idle",
                        "workspace_id": "w1",
                        "tab_id": "w1:t1",
                        "pane_id": "w1:p1",
                        "focused": false,
                        "interactive_ready": true,
                        "state_change_seq": 1,
                        "cwd": "/tmp",
                        "foreground_cwd": "/tmp",
                        "revision": 0
                    }]
                }
            }),
            "pane.send_text" => {
                assert_eq!(*params, json!({"pane_id": "w1:p1", "text": "\u{3}"}));
                json!({"type": "ok"})
            }
            other => panic!("unexpected {other}"),
        });

        assert!(
            interrupt_reused_agent(&herdr.connector(), "fixture", 0, "w1:p1")
                .expect("reused agent is interrupted")
        );

        assert_eq!(herdr.methods(), ["session.snapshot", "pane.send_text"]);
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
    fn a_registered_project_gets_its_checkouts_owner_not_a_plain_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let folder = root.join("notes");
        let repo = root.join("repo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let folder = folder.to_string_lossy().into_owned();
        let repo = repo.to_string_lossy().into_owned();

        assert_eq!(
            registered_owner(&folder, "Notes"),
            OwnerOpen::for_checkout(
                crate::workspace::LOCAL_DEVICE_ID,
                &folder,
                &folder,
                false,
                "Notes"
            )
        );
        assert_eq!(
            registered_owner(&repo, "Repo"),
            OwnerOpen::Worktree {
                path: repo.clone(),
                repository_root: repo.clone(),
                label: "Repo".to_owned(),
            }
        );
    }

    #[test]
    fn local_tab_control_opens_the_checkouts_bound_workspace_when_none_is_open() {
        let herdr = FakeHerdr::start("owner-open-worktree", |method, _| match method {
            "worktree.open" => json!({
                "type": "worktree_opened",
                "already_open": false,
                "workspace": {"workspace_id": "w9", "number": 9, "label": "repo-feat", "focused": true, "pane_count": 1, "tab_count": 1, "active_tab_id": "w9:t1", "agent_status": "idle"},
                "worktree": {"path": "/repo-feat", "is_bare": false, "is_detached": false, "is_prunable": false, "is_linked_worktree": true, "label": "repo-feat"},
                "tab": {"tab_id": "w9:t1", "workspace_id": "w9", "number": 1, "label": "1", "focused": true, "pane_count": 1, "agent_status": "idle"},
                "root_pane": {"pane_id": "w9:p1", "terminal_id": "fixture-terminal", "workspace_id": "w9", "tab_id": "w9:t1", "focused": true, "agent_status": "idle", "revision": 1}
            }),
            "tab.rename" => json!({
                "type": "tab_info",
                "tab": {"tab_id": "w9:t1", "workspace_id": "w9", "number": 1, "label": "Tab 2", "focused": true, "pane_count": 1, "agent_status": "idle"}
            }),
            "workspace.rename" => json!({
                "type": "workspace_info",
                "workspace": {"workspace_id": "w9", "number": 9, "label": "feat", "focused": true, "pane_count": 1, "tab_count": 1, "active_tab_id": "w9:t1", "agent_status": "idle"}
            }),
            other => panic!("unexpected {other}"),
        });

        let outcome = execute_remote_control(
            &herdr.connector(),
            &RemoteControlAction::OpenOwner {
                owner: OwnerOpen::Worktree {
                    path: "/repo-feat".to_owned(),
                    repository_root: "/repo".to_owned(),
                    label: "feat".to_owned(),
                },
                cwd: "/repo-feat".to_owned(),
                label: "Tab 2".to_owned(),
                area_id: None,
                admission_id: None,
            },
        )
        .expect("owner open");

        // A newly opened owner's first tab is the new tab.
        assert!(matches!(
            outcome,
            RemoteControlOutcome::Acknowledged {
                created_tab_id: Some(ref tab_id),
                created_pane_id: Some(ref pane_id),
            } if tab_id == "w9:t1" && pane_id == "w9:p1"
        ));
        // Herdr opens a linked worktree only from its repository's main
        // worktree, and a label on the open would rename a workspace that was
        // already there, so the new one is named after it opens (D-14).
        assert_eq!(
            herdr.calls(),
            [
                (
                    "worktree.open".to_owned(),
                    json!({"cwd": "/repo", "path": "/repo-feat", "focus": true})
                ),
                (
                    "workspace.rename".to_owned(),
                    json!({"workspace_id": "w9", "label": "feat"})
                ),
                (
                    "tab.rename".to_owned(),
                    json!({"tab_id": "w9:t1", "label": "Tab 2"})
                ),
            ]
        );
    }

    #[test]
    fn a_second_request_for_an_open_owner_adds_a_tab_there_instead_of_another_workspace() {
        let herdr = FakeHerdr::start("owner-already-open", |method, _| match method {
            "worktree.open" => json!({
                "type": "worktree_opened",
                "already_open": true,
                "workspace": {"workspace_id": "w9", "number": 9, "label": "hide", "focused": true, "pane_count": 1, "tab_count": 1, "active_tab_id": "w9:t1", "agent_status": "idle"},
                "worktree": {"path": "/repo", "is_bare": false, "is_detached": false, "is_prunable": false, "is_linked_worktree": false, "label": "hide"},
                "tab": {"tab_id": "w9:t1", "workspace_id": "w9", "number": 1, "label": "1", "focused": true, "pane_count": 1, "agent_status": "idle"},
                "root_pane": {"pane_id": "w9:p1", "terminal_id": "fixture-terminal", "workspace_id": "w9", "tab_id": "w9:t1", "focused": true, "agent_status": "idle", "revision": 1}
            }),
            "tab.create" => json!({
                "type": "tab_created",
                "tab": {"tab_id": "w9:t2", "workspace_id": "w9", "number": 2, "label": "Tab 2", "focused": true, "pane_count": 1, "agent_status": "idle"},
                "root_pane": {"pane_id": "w9:p2", "terminal_id": "fixture-terminal-2", "workspace_id": "w9", "tab_id": "w9:t2", "focused": true, "agent_status": "idle", "revision": 1}
            }),
            other => panic!("unexpected {other}"),
        });

        let tab = open_owner_tab(
            &herdr.connector(),
            &OwnerOpen::Worktree {
                path: "/repo".to_owned(),
                repository_root: "/repo".to_owned(),
                label: "hide".to_owned(),
            },
            "/repo",
            "Tab 2",
            Default::default(),
        )
        .expect("owner tab");

        assert_eq!(
            tab,
            OwnedTab {
                workspace_id: "w9".to_owned(),
                tab_id: "w9:t2".to_owned(),
                pane_id: "w9:p2".to_owned(),
            }
        );
        assert_eq!(herdr.methods(), ["worktree.open", "tab.create"]);
    }

    #[test]
    fn a_refused_owner_open_fails_the_request_and_creates_nothing_elsewhere() {
        let herdr = FakeHerdr::start_with_errors("owner-refused", |method, _| match method {
            "worktree.open" => Err(("not_found".to_owned(), "no such checkout".to_owned())),
            other => panic!("unexpected {other}"),
        });

        let failure = open_owner_tab(
            &herdr.connector(),
            &OwnerOpen::Worktree {
                path: "/gone".to_owned(),
                repository_root: "/gone".to_owned(),
                label: "gone".to_owned(),
            },
            "/gone",
            "Tab 1",
            Default::default(),
        )
        .expect_err("a refused open fails");

        assert!(!failure.is_ambiguous());
        assert!(failure.message().contains("no such checkout"));
        assert_eq!(herdr.methods(), ["worktree.open"]);
    }

    #[test]
    fn a_plain_folder_reuses_its_marked_owner_and_marks_a_new_one_only_when_none_is_live() {
        let mark = crate::checkout_owner::owner_mark("local", "/notes");
        let listed_mark = mark.clone();
        let marked = Arc::new(Mutex::new(false));
        let marked_in_server = marked.clone();
        let herdr = FakeHerdr::start("owner-folder", move |method, params| match method {
            "workspace.list" => {
                let tokens = if *marked_in_server.lock().unwrap() {
                    json!({"hide_owner": listed_mark})
                } else {
                    json!({})
                };
                json!({"type": "workspace_list", "workspaces": [
                    {"workspace_id": "w1", "number": 1, "label": "other", "focused": false, "pane_count": 1, "tab_count": 1, "active_tab_id": "w1:t1", "agent_status": "idle"},
                    {"workspace_id": "w4", "number": 4, "label": "notes", "focused": false, "pane_count": 1, "tab_count": 1, "active_tab_id": "w4:t1", "agent_status": "idle", "tokens": tokens}
                ]})
            }
            "workspace.create" => json!({
                "type": "workspace_created",
                "workspace": {"workspace_id": "w4", "number": 4, "label": "notes", "focused": true, "pane_count": 1, "tab_count": 1, "active_tab_id": "w4:t1", "agent_status": "idle"},
                "tab": {"tab_id": "w4:t1", "workspace_id": "w4", "number": 1, "label": "1", "focused": true, "pane_count": 1, "agent_status": "idle"},
                "root_pane": {"pane_id": "w4:p1", "terminal_id": "fixture-terminal", "workspace_id": "w4", "tab_id": "w4:t1", "focused": true, "agent_status": "idle", "revision": 1}
            }),
            "workspace.report_metadata" => {
                assert_eq!(params["tokens"]["hide_owner"], json!(listed_mark));
                *marked_in_server.lock().unwrap() = true;
                json!({"type": "ok"})
            }
            "tab.rename" => json!({
                "type": "tab_info",
                "tab": {"tab_id": "w4:t1", "workspace_id": "w4", "number": 1, "label": "Tab 1", "focused": true, "pane_count": 1, "agent_status": "idle"}
            }),
            "tab.create" => json!({
                "type": "tab_created",
                "tab": {"tab_id": "w4:t2", "workspace_id": "w4", "number": 2, "label": "Tab 2", "focused": true, "pane_count": 1, "agent_status": "idle"},
                "root_pane": {"pane_id": "w4:p2", "terminal_id": "fixture-terminal-2", "workspace_id": "w4", "tab_id": "w4:t2", "focused": true, "agent_status": "idle", "revision": 1}
            }),
            other => panic!("unexpected {other}"),
        });
        let owner = OwnerOpen::Folder {
            path: "/notes".to_owned(),
            label: "notes".to_owned(),
            mark,
        };

        let first = open_owner_tab(
            &herdr.connector(),
            &owner,
            "/notes",
            "Tab 1",
            Default::default(),
        )
        .expect("first tab");
        let second = open_owner_tab(
            &herdr.connector(),
            &owner,
            "/notes",
            "Tab 2",
            Default::default(),
        )
        .expect("second tab");

        assert_eq!(
            (first.workspace_id.as_str(), first.tab_id.as_str()),
            ("w4", "w4:t1")
        );
        assert_eq!(
            (second.workspace_id.as_str(), second.tab_id.as_str()),
            ("w4", "w4:t2")
        );
        assert!(*marked.lock().unwrap());
        assert_eq!(
            herdr.methods(),
            [
                "workspace.list",
                "workspace.create",
                "workspace.report_metadata",
                "tab.rename",
                "workspace.list",
                "tab.create"
            ]
        );
    }

    #[test]
    fn pane_control_uses_the_official_socket_contract_for_every_mutation() {
        let herdr = FakeHerdr::start("pane-control", |method, _| match method {
            "pane.split" => {
                json!({"type": "pane_info", "pane": {"pane_id": "w1:p2", "terminal_id": "fixture-terminal", "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}})
            }
            "pane.zoom" | "pane.close" => json!({"type": "ok"}),
            other => panic!("unexpected {other}"),
        });
        let connector = herdr.connector();
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
        assert_eq!(
            herdr.calls(),
            [
                (
                    "pane.split".to_owned(),
                    json!({
                        "target_pane_id": "w1:p1",
                        "direction": "right",
                        "right_click": "herdr",
                        "focus": true,
                        "cwd": "/tmp/herdr-ide-verify-shortcuts"
                    }),
                ),
                (
                    "pane.split".to_owned(),
                    json!({
                        "target_pane_id": "w1:p1",
                        "direction": "down",
                        "right_click": "herdr",
                        "focus": true
                    }),
                ),
                (
                    "pane.zoom".to_owned(),
                    json!({"pane_id": "w1:p1", "mode": "toggle"})
                ),
                ("pane.close".to_owned(), json!({"pane_id": "w1:p1"})),
            ]
        );
    }

    #[test]
    fn remote_session_control_uses_the_official_socket_contract() {
        let herdr = FakeHerdr::start("remote-control", |method, _| match method {
            "workspace.focus" | "tab.focus" | "tab.close" => json!({"type": "ok"}),
            "tab.create" => json!({
                "type": "tab_created",
                "tab": {"tab_id": "w1:t3", "workspace_id": "w1", "number": 1, "label": "fixture", "focused": false, "pane_count": 1, "agent_status": "idle"},
                "root_pane": {"pane_id": "w1:p3", "terminal_id": "fixture-terminal", "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}
            }),
            other => panic!("unexpected {other}"),
        });
        let connector = herdr.connector();
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
                area_id: None,
                admission_id: None,
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
        assert_eq!(
            herdr.calls(),
            [
                ("workspace.focus".to_owned(), json!({"workspace_id": "w1"})),
                ("tab.focus".to_owned(), json!({"tab_id": "w1:t2"})),
                (
                    "tab.create".to_owned(),
                    json!({
                        "workspace_id": "w1",
                        "cwd": "/tmp/herdr-ide-remote-tab",
                        "focus": true,
                        "label": "New tab"
                    }),
                ),
                ("tab.close".to_owned(), json!({"tab_id": "w1:t3"})),
            ]
        );
    }

    #[test]
    #[ignore = "requires an owned remote fixture and HERDR_TEST_REMOTE_CONTROL_* variables"]
    fn official_remote_control_fixture_probe() {
        let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS")
            .expect("HERDR_TEST_SSH_ALIAS names a configured SSH host");
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
        let connector = client.herdr_api_connector();

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
                area_id: None,
                admission_id: None,
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
        let herdr = FakeHerdr::start("pane-worker", |method, _| {
            assert_eq!(method, "pane.split");
            std::thread::sleep(Duration::from_millis(500));
            json!({"type": "pane_info", "pane": {"pane_id": "w1:p2", "terminal_id": "fixture-terminal", "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}})
        });
        let context = LiveContext {
            socket_path: herdr.socket_path().to_path_buf(),
            herdr_bin: None,
            runtime: Weak::new(),
            notifier: ChangeNotifier::noop(),
            api_connector: Arc::new(herdr.connector()),
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
        // The worker still delivers the request; the spawn only stopped
        // waiting for its answer.
        herdr.wait_for_requests(1, Duration::from_secs(5));
        assert_eq!(herdr.methods(), ["pane.split"]);
    }

    #[test]
    fn focus_uses_the_direct_socket_contract_before_reading_authoritative_layout() {
        let herdr = FakeHerdr::start("focus-contract", |method, params| {
            assert_eq!(params["pane_id"], "fixture:p2");
            match method {
                "pane.focus" => {
                    json!({"type": "pane_info", "pane": {"pane_id": "fixture:p2", "terminal_id": "fixture-terminal", "workspace_id": "fixture", "tab_id": "fixture:t1", "focused": false, "agent_status": "idle", "revision": 1}})
                }
                "pane.layout" => json!({
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
                }),
                other => panic!("unexpected {other}"),
            }
        });

        request(
            herdr.socket_path(),
            "pane.focus",
            json!({"pane_id": "fixture:p2"}),
        )
        .expect("focus request");
        let layout = fetch_pane_layout(&herdr.connector(), "fixture:p2").expect("focused layout");
        assert_eq!(layout.focused_pane_id, "fixture:p2");
        assert_eq!(layout.pane_ids(), ["fixture:p1", "fixture:p2"]);
        assert_eq!(herdr.methods(), ["pane.focus", "pane.layout"]);
    }

    #[test]
    fn wire_snapshot_projects_agents_with_workspace_labels_and_verbatim_tokens() {
        let snapshot = json!({
            "protocol": HERDR_PROTOCOL_REVISION,
            "version": "fixture",
            "panes": [],
            "tabs": [],
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
            "panes": [],
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
            "panes": [],
            "tabs": [],
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

#[cfg(test)]
#[path = "live_agent_close_tests.rs"]
mod agent_close_tests;
