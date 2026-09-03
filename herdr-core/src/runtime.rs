use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Weak};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::ffi::ChangeNotifier;
use crate::live::{
    LiveContext, PaneControlAction, PaneControlOutcome, PaneResizeDirection, PaneSplitDirection,
    RemoteControlAction, RemoteControlContext, RemoteControlOutcome, RemoteTerminalContext,
    SessionFetchError, TerminalSession, TerminalSessionContext, TerminalSessionMode,
};
use crate::model::{
    CoreOptions, DEFAULT_PANE_TEXT_SCALE, DiagnosticSnapshot, EditorDocumentSnapshot,
    FileTabSnapshot, LastErrorSnapshot, PANE_TEXT_SCALE_STEP, PaneFindSnapshot,
    clamp_pane_text_scale,
    PaneForkSnapshot, PaneLayoutSnapshot, PaneSnapshot, PetBadgesSnapshot, PetOriginSnapshot, PetSnapshot,
    RemoteFileEntrySnapshot, RemoteFileListSnapshot, RemoteSessionSnapshot, RightPanelSection,
    SCHEMA_VERSION, Snapshot, Surface, TabSnapshot, TerminalChunk, TerminalPaneSnapshot,
    UiStateSnapshot,
};
use crate::remote::RusshSftpTransport;
use crate::remote_files::{FileEntry, FileKind, FileService, RemoteFileService};
use crate::fork::{ForkRequest, ForkableAgent, fork_name, is_forkable};
use crate::model::SidebarAgentSnapshot;
use crate::sidebar::{SessionSnapshotPayload, project_agents};
use crate::{chromux, environment, files, live, persistence, pet, session_sync, workspace};

/// How long the pet plays its waking pose after activity interrupts sleep.
const PET_WAKING_MS: u64 = 1_200;

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    schema_version: u32,
    kind: String,
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct KeyPayload {
    pane_id: String,
    bytes_base64: String,
}

#[derive(Debug, Deserialize)]
struct TerminalOutputPayload {
    pane_id: String,
    bytes_base64: String,
}

#[derive(Debug, Deserialize)]
struct ClickPayload {
    surface: Surface,
    x: f64,
    y: f64,
    button: MouseButton,
    click_count: u8,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MouseButton {
    Left,
    Right,
}

#[derive(Debug, Deserialize)]
struct FocusPanePayload {
    pane_id: String,
}

#[derive(Debug, Deserialize)]
struct OpenBrowserPayload {
    profile: String,
}

#[derive(Debug, Deserialize)]
struct BrowserStatusPayload {
    state: String,
    profile: String,
    current_url: Option<String>,
    current_title: Option<String>,
    message: Option<String>,
    last_checked_at_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
struct CreateWorkspacePayload {
    path: String,
    label: String,
    initialize_git: bool,
}

#[derive(Debug, Deserialize)]
struct CreateTabPayload {
    workspace_id: String,
    #[serde(default)]
    checkout_id: Option<String>,
    label: String,
}

#[derive(Debug, Deserialize)]
struct FocusCheckoutPayload {
    workspace_id: String,
    checkout_id: String,
}

#[derive(Debug, Deserialize)]
struct FocusTabPayload {
    workspace_id: String,
    checkout_id: String,
    tab_id: String,
}

#[derive(Debug, Deserialize)]
struct FocusDevicePayload {
    device_id: String,
}

#[derive(Debug, Deserialize)]
struct RemoveWorkspacePayload {
    workspace_id: String,
}

#[derive(Debug, Deserialize)]
struct RegisterDevicePayload {
    id: String,
    label: String,
    ssh_alias: String,
}

#[derive(Debug, Deserialize)]
struct RemoveDevicePayload {
    device_id: String,
}

#[derive(Debug, Deserialize)]
struct TestDevicePayload {
    device_id: String,
}

#[derive(Debug, Deserialize)]
struct CreatePanePayload {
    tab_id: String,
    cwd: String,
    command: Option<String>,
    direction: PaneSplitDirection,
}

#[derive(Debug, Deserialize)]
struct ResizePanePayload {
    pane_id: String,
    direction: PaneResizeDirection,
    amount: f32,
}

#[derive(Debug, Deserialize)]
struct ToggleZoomPayload {
    pane_id: String,
}

#[derive(Debug, Deserialize)]
struct ConfirmedWorkspacePayload {
    workspace_id: String,
    confirmed: bool,
}

#[derive(Debug, Deserialize)]
struct ConfirmedTabPayload {
    tab_id: String,
    confirmed: bool,
}

#[derive(Debug, Deserialize)]
struct ConfirmedPanePayload {
    pane_id: String,
    confirmed: bool,
}

#[derive(Debug, Deserialize)]
struct PaneTargetPayload {
    pane_id: String,
}

#[derive(Debug, Deserialize)]
struct RemoteControlPayload {
    target_id: String,
    request_id: String,
    #[serde(flatten)]
    request: RemoteControlRequest,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum RemoteControlRequest {
    FocusPane {
        pane_id: String,
    },
    SplitPane {
        pane_id: String,
        direction: PaneSplitDirection,
        #[serde(default)]
        cwd: Option<String>,
    },
    TogglePaneZoom {
        pane_id: String,
    },
    ClosePane {
        pane_id: String,
        confirmed: bool,
    },
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
        confirmed: bool,
    },
}

impl RemoteControlRequest {
    fn pane_id(&self) -> Option<&str> {
        match self {
            Self::FocusPane { pane_id }
            | Self::SplitPane { pane_id, .. }
            | Self::TogglePaneZoom { pane_id }
            | Self::ClosePane { pane_id, .. } => Some(pane_id),
            Self::FocusWorkspace { .. }
            | Self::FocusTab { .. }
            | Self::CreateTab { .. }
            | Self::CloseTab { .. } => None,
        }
    }

    fn confirmed(&self) -> bool {
        matches!(
            self,
            Self::ClosePane {
                confirmed: true,
                ..
            } | Self::CloseTab {
                confirmed: true,
                ..
            }
        )
    }
}

fn remote_workspace_source_id<'a>(target_id: &str, projected_id: &'a str) -> Option<&'a str> {
    projected_id
        .strip_prefix(&format!("remote:{target_id}:workspace:"))
        .filter(|workspace_id| !workspace_id.trim().is_empty())
}

fn remote_tab_source_id<'a>(target_id: &str, projected_id: &'a str) -> Option<&'a str> {
    projected_id
        .strip_prefix(&format!("remote:{target_id}:tab:"))
        .filter(|tab_id| !tab_id.trim().is_empty())
}

fn remote_pane_source_id<'a>(target_id: &str, projected_id: &'a str) -> Option<&'a str> {
    projected_id
        .strip_prefix(&format!("remote:{target_id}:pane:"))
        .filter(|pane_id| !pane_id.trim().is_empty())
}

fn remote_terminal_pane_sets(
    session: &RemoteSessionSnapshot,
    target_is_active: bool,
) -> (HashSet<String>, HashSet<String>) {
    let live_pane_ids = session
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .flat_map(|checkout| checkout.tabs.iter())
        .flat_map(|tab| tab.panes.iter())
        .map(|pane| pane.id.clone())
        .collect::<HashSet<_>>();
    if !target_is_active {
        return (live_pane_ids, HashSet::new());
    }
    let active_tab_id = session.focused_tab_id.as_deref().or_else(|| {
        session
            .focused_workspace_id
            .as_deref()
            .and_then(|workspace_id| session.active_tab_ids.get(workspace_id))
            .map(String::as_str)
    });
    let active_tab = active_tab_id.and_then(|tab_id| {
        session
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .find(|tab| tab.id.as_deref() == Some(tab_id))
    });
    let active_pane_ids = active_tab
        .into_iter()
        .flat_map(|tab| tab.panes.iter())
        .map(|pane| pane.id.clone())
        .collect();
    (live_pane_ids, active_pane_ids)
}

fn remote_tab_creation_key(
    target_id: &str,
    action: &RemoteControlAction,
) -> Option<(String, String, String, String)> {
    match action {
        RemoteControlAction::CreateTab {
            workspace_id,
            cwd,
            label,
        } => Some((
            target_id.to_owned(),
            workspace_id.clone(),
            cwd.clone(),
            label.clone(),
        )),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct FileOpenPayload {
    path: String,
    workspace_id: String,
    checkout_id: String,
}

#[derive(Debug, Deserialize)]
struct FileTabPayload {
    tab_id: String,
}

#[derive(Debug, Deserialize)]
struct RemoteFileListPayload {
    target_id: String,
    root_path: String,
}

#[derive(Debug, Deserialize)]
struct FileSavePayload {
    tab_id: String,
    path: String,
    contents_utf8: String,
    expected_modified_at_unix_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FileDraftPayload {
    contents_utf8: String,
}

#[derive(Debug, Deserialize)]
struct FileConflictPayload {
    action: String,
}

#[derive(Debug, Deserialize)]
struct UiStateUpdatePayload {
    #[serde(default)]
    left_sidebar_visible: Option<bool>,
    #[serde(default)]
    right_panel_visible: Option<bool>,
    #[serde(default)]
    right_panel_section: Option<String>,
    expanded_paths: Vec<String>,
    #[serde(default)]
    collapsed_workspace_ids: Vec<String>,
    selected_path: Option<String>,
    selected_pane_id: Option<String>,
    #[serde(default)]
    shortcut_bindings: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    focused_device_id: Option<Option<String>>,
    #[serde(default)]
    focused_checkout_id: Option<Option<String>>,
    #[serde(default)]
    workspace_registrations: Option<Vec<crate::model::WorkspaceRegistration>>,
    #[serde(default)]
    device_registrations: Option<Vec<crate::model::DeviceRegistration>>,
    #[serde(default)]
    accent_hex: Option<String>,
    #[serde(default)]
    font_size: Option<f32>,
}

/// Which changed file the changes view is showing the diff for. `None`
/// deselects, which is what closing the diff means.
#[derive(Debug, Deserialize)]
struct ChangesSelectPayload {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RetryConnectPayload {
    target_id: String,
}

/// One pane search. An empty `term` clears the search rather than needing its
/// own event, and `step` folds "search this" and "go to the next one" into one
/// path: 0 searches and keeps the current match, +1 and -1 move.
#[derive(Debug, Deserialize)]
struct PaneFindPayload {
    pane_id: String,
    term: String,
    #[serde(default)]
    case_sensitive: bool,
    #[serde(default)]
    whole_word: bool,
    #[serde(default)]
    regex: bool,
    #[serde(default)]
    step: i64,
}

#[derive(Debug, Deserialize)]
struct PaneTextScalePayload {
    pane_id: String,
    direction: String,
}

#[derive(Debug, Deserialize)]
struct PetVisibilityPayload {
    visible: bool,
}

#[derive(Debug, Deserialize)]
struct PetMovePayload {
    /// Already clamped to a visible screen by the shell, which owns the
    /// display geometry; the core only records where the pet ended up.
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
struct PetDragPayload {
    dragging: bool,
}

#[derive(Debug, Deserialize)]
struct PetShortcutPayload {
    /// `null` or blank means the user cleared the binding: nothing is
    /// registered and no shortcut fires (D-14).
    accelerator: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TerminalScrollPayload {
    pane_id: String,
    direction: String,
    lines: u16,
}

#[derive(Debug, Deserialize)]
struct TerminalResizePayload {
    pane_id: String,
    cols: u16,
    rows: u16,
}

enum ValidatedEvent {
    Key(KeyPayload),
    TerminalOutput(TerminalOutputPayload),
    SessionSnapshot(SessionSnapshotPayload),
    Click(ClickPayload),
    FocusPane(FocusPanePayload),
    OpenBrowser(OpenBrowserPayload),
    BrowserStatus(BrowserStatusPayload),
    CreateWorkspace(CreateWorkspacePayload),
    CreateTab(CreateTabPayload),
    FocusCheckout(FocusCheckoutPayload),
    FocusTab(FocusTabPayload),
    FocusDevice(FocusDevicePayload),
    RemoveWorkspace(RemoveWorkspacePayload),
    RegisterDevice(RegisterDevicePayload),
    RemoveDevice(RemoveDevicePayload),
    TestDevice(TestDevicePayload),
    CreatePane(CreatePanePayload),
    ResizePane(ResizePanePayload),
    ToggleZoom(ToggleZoomPayload),
    CloseWorkspace(ConfirmedWorkspacePayload),
    CloseTab(ConfirmedTabPayload),
    ClosePane(ConfirmedPanePayload),
    ForkPane(PaneTargetPayload),
    RemoteControl(RemoteControlPayload),
    RemoteFileList(RemoteFileListPayload),
    FileOpen(FileOpenPayload),
    FileFocus(FileTabPayload),
    FileClose(FileTabPayload),
    FileDraft(FileDraftPayload),
    FileSave(FileSavePayload),
    FileConflict(FileConflictPayload),
    UiStateUpdate(UiStateUpdatePayload),
    RetryConnect(RetryConnectPayload),
    TerminalResize(TerminalResizePayload),
    TerminalScroll(TerminalScrollPayload),
    PaneFind(PaneFindPayload),
    PaneTextScale(PaneTextScalePayload),
    ChangesSelect(ChangesSelectPayload),
    ReconnectPane(FocusPanePayload),
    PetSetVisible(PetVisibilityPayload),
    PetToggleVisible,
    PetMove(PetMovePayload),
    PetDrag(PetDragPayload),
    PetActivity,
    PetShortcutUpdate(PetShortcutPayload),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TerminalSessionLifecycle {
    state: &'static str,
    message: Option<String>,
    generation: u64,
    attempt: u64,
    mode: Option<TerminalSessionMode>,
    exit_category: Option<String>,
    retry_decision: &'static str,
}

impl Default for TerminalSessionLifecycle {
    fn default() -> Self {
        Self {
            state: "idle",
            message: None,
            generation: 0,
            attempt: 0,
            mode: None,
            exit_category: None,
            retry_decision: "automatic_initial",
        }
    }
}

fn terminal_control_request_allowed(state: &str, has_active_session: bool) -> bool {
    !has_active_session
        && !matches!(
            state,
            "starting" | "controlling" | "observing" | "unavailable" | "ended"
        )
}

pub struct Runtime {
    snapshot: Snapshot,
    state_path: PathBuf,
    remote_targets: Vec<crate::model::RemoteTarget>,
    live: Option<LiveContext>,
    remote_controls: HashMap<String, RemoteControlContext>,
    remote_terminals: HashMap<String, RemoteTerminalContext>,
    remote_file_transports: HashMap<String, RusshSftpTransport>,
    remote_control_requests: VecDeque<(String, String)>,
    remote_tab_creations_in_flight: HashSet<(String, String, String, String)>,
    terminal_sessions: HashMap<String, TerminalSession>,
    terminal_session_generations: HashMap<String, u64>,
    terminal_session_lifecycles: HashMap<String, TerminalSessionLifecycle>,
    next_terminal_session_generation: u64,
    next_remote_file_generation: u64,
    terminal_sizes: HashMap<String, (u16, u16)>,
    #[cfg(test)]
    suppress_terminal_session_workers: bool,
    workspace_creations_in_flight: HashSet<String>,
    editor_documents: HashMap<String, EditorDocumentSnapshot>,
    editor_tab_history: Vec<String>,
    worker_context: Option<RuntimeWorkerContext>,
    /// The last moment any agent was working or waiting on the user. The pet
    /// measures idleness from here, so roam and sleep are driven by real
    /// session activity rather than wall-clock uptime.
    pet_active_at_unix_ms: u64,
    pet_waking_until_unix_ms: u64,
    pet_dragging: bool,
    /// First moment each currently-unseen pane became unseen. Memory only by
    /// decision (D-21): after a restart snapshot order decides instead.
    pet_unseen_observed: std::collections::BTreeMap<String, u64>,
    /// True until the first live session snapshot has been ingested. The pane
    /// and checkout ids loaded from disk describe a session that ended, so
    /// they are a restore hint rather than a user selection: the first
    /// snapshot that disagrees with them retargets silently. A selection the
    /// user makes against a live session is authoritative, and its
    /// disappearance stays a reported error.
    restore_hint_pending: bool,
    /// The Herdr workspaces the last session snapshot reported. A catalog
    /// rebuild triggered by a registration change is not a session update, so
    /// it reuses these rather than briefly emptying the navigator.
    last_session_spaces: Vec<workspace::SessionSpace>,
    /// Panes whose fork has been started and not yet answered. `herdr agent
    /// new` blocks until the agent has started, so without this a second
    /// activation during that wait would bill a second session.
    forks_in_flight: HashSet<String>,
    fork_sequence: u64,
    /// The machine's TCP listeners, refreshed on their own window by the
    /// session-sync coordinator. Held here rather than in the snapshot because
    /// what the shell renders is the per-pane attribution, not the raw list.
    listening_ports: crate::model::ListeningPortsSnapshot,
    delta: DeltaState,
}

#[derive(Clone)]
struct RuntimeWorkerContext {
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

/// Revision bookkeeping for the delta snapshot wire. Revisions are stamped
/// lazily at read time by comparing live sections against the last stamped
/// copy, so mutation sites carry no dirty-tracking obligations.
#[derive(Default)]
struct DeltaState {
    revision: u64,
    rest_revision: u64,
    editor_revision: u64,
    changes_revision: u64,
    last_rest: Option<crate::model::RestSections>,
    last_editor: Option<crate::model::EditorSnapshot>,
    last_changes: Option<crate::model::ChangesSnapshot>,
}

impl Runtime {
    pub fn new(options: CoreOptions, environment: environment::EnvironmentReport) -> Self {
        let state_path = PathBuf::from(&options.app_state_path);
        let remote_targets = options.remote_targets.clone();
        let mut snapshot = Snapshot::initial(&options);
        snapshot.status.environment = environment.statuses;
        if !environment.remote_enabled {
            for remote in &mut snapshot.status.remote {
                remote.state = "disabled".to_owned();
                remote.message = Some(
                    "Remote features are disabled because the SSH agent socket is unavailable"
                        .to_owned(),
                );
            }
        }
        let (ui_state, disposition) = persistence::load(&state_path);
        snapshot.ui_state = ui_state;
        snapshot.navigator.devices =
            workspace::devices(&remote_targets, &snapshot.ui_state.device_registrations);
        snapshot.navigator.focused_device_id = Some(
            snapshot
                .ui_state
                .focused_device_id
                .clone()
                .unwrap_or_else(|| workspace::LOCAL_DEVICE_ID.to_owned()),
        );
        snapshot.navigator.focused_checkout_id = snapshot.ui_state.focused_checkout_id.clone();
        snapshot.navigator.workspaces =
            workspace::build_catalog(&snapshot.ui_state.workspace_registrations, &[]);
        let diagnostic = match disposition {
            persistence::LoadDisposition::Loaded => None,
            persistence::LoadDisposition::Missing => Some((
                "ui_state.missing",
                "UI state was not found; safe defaults were loaded",
            )),
            persistence::LoadDisposition::Corrupt => Some((
                "ui_state.corrupt",
                "UI state could not be decoded; safe defaults were loaded",
            )),
        };
        if let Some((kind, message)) = diagnostic {
            eprintln!(
                "{}",
                serde_json::json!({
                    "component": "ui_state",
                    "kind": kind,
                    "message": message,
                    "fallback": "defaults"
                })
            );
            snapshot.status.diagnostics.push(DiagnosticSnapshot {
                kind: kind.to_owned(),
                message: message.to_owned(),
                occurred_at: unix_milliseconds(),
            });
        }
        let mut runtime = Self {
            snapshot,
            state_path,
            remote_targets,
            live: None,
            remote_controls: HashMap::new(),
            remote_terminals: HashMap::new(),
            remote_file_transports: HashMap::new(),
            remote_control_requests: VecDeque::new(),
            remote_tab_creations_in_flight: HashSet::new(),
            terminal_sessions: HashMap::new(),
            terminal_session_generations: HashMap::new(),
            terminal_session_lifecycles: HashMap::new(),
            next_terminal_session_generation: 0,
            next_remote_file_generation: 0,
            terminal_sizes: HashMap::new(),
            #[cfg(test)]
            suppress_terminal_session_workers: false,
            workspace_creations_in_flight: HashSet::new(),
            editor_documents: HashMap::new(),
            editor_tab_history: Vec::new(),
            worker_context: None,
            pet_active_at_unix_ms: unix_milliseconds(),
            pet_waking_until_unix_ms: 0,
            pet_dragging: false,
            pet_unseen_observed: std::collections::BTreeMap::new(),
            restore_hint_pending: true,
            last_session_spaces: Vec::new(),
            forks_in_flight: HashSet::new(),
            fork_sequence: 0,
            listening_ports: crate::model::ListeningPortsSnapshot::default(),
            delta: DeltaState::default(),
        };
        runtime.resync_navigator_focus();
        runtime.apply_persisted_pet_state();
        runtime.refresh_pet();
        runtime
    }

    pub fn install_worker_context(
        &mut self,
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) {
        self.worker_context = Some(RuntimeWorkerContext { runtime, notifier });
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    fn file_tab_id(workspace_id: &str, checkout_id: &str, path: &str) -> String {
        format!("file:{workspace_id}:{checkout_id}:{path}")
    }

    fn activate_file_tab(&mut self, tab_id: &str) -> Result<(), String> {
        if !self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
            return Err(format!("File tab {tab_id} is not open"));
        }
        if let Some(active_id) = self.snapshot.editor.active_tab_id.as_deref()
            && active_id != tab_id
        {
            self.editor_tab_history.retain(|known| known != active_id);
            self.editor_tab_history.push(active_id.to_owned());
        }
        let document = self
            .editor_documents
            .get(tab_id)
            .cloned()
            .ok_or_else(|| format!("File tab {tab_id} has no document state"))?;
        self.snapshot.editor.active_tab_id = Some(tab_id.to_owned());
        self.snapshot.editor.document = Some(document);
        Ok(())
    }

    fn deactivate_file_tab(&mut self) {
        self.snapshot.editor.active_tab_id = None;
        self.snapshot.editor.document = None;
        self.editor_tab_history.clear();
    }

    fn file_tab_matches_focused_context(&self, tab: &FileTabSnapshot) -> bool {
        self.snapshot.navigator.focused_workspace_id.as_deref() == Some(tab.workspace_id.as_str())
            && self.snapshot.navigator.focused_checkout_id.as_deref()
                == Some(tab.checkout_id.as_str())
    }

    fn sync_active_editor_document(&mut self) {
        self.snapshot.editor.document = self
            .snapshot
            .editor
            .active_tab_id
            .as_deref()
            .and_then(|tab_id| self.editor_documents.get(tab_id))
            .cloned();
    }

    fn sync_file_tab_dirty(&mut self, tab_id: &str) {
        let dirty = self
            .editor_documents
            .get(tab_id)
            .is_some_and(|document| document.dirty);
        if let Some(tab) = self
            .snapshot
            .editor
            .tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
        {
            tab.dirty = dirty;
        }
    }

    fn ingest_file_save_result(
        &mut self,
        tab_id: String,
        path: String,
        contents: String,
        editor: EditorDocumentSnapshot,
        result: Result<(), String>,
    ) -> bool {
        let is_current_draft = self.snapshot.editor.tabs.iter().any(|tab| {
            tab.id == tab_id
                && tab.path == path
                && self
                    .editor_documents
                    .get(&tab_id)
                    .and_then(|document| document.contents_utf8.as_deref())
                    == Some(contents.as_str())
        });
        if !is_current_draft {
            self.push_diagnostic(
                "file.save_stale",
                format!("Ignored a completed save for stale draft {path}"),
            );
            return true;
        }
        self.editor_documents.insert(tab_id.clone(), editor);
        self.sync_file_tab_dirty(&tab_id);
        self.sync_active_editor_document();
        match result {
            Ok(()) => {
                self.push_diagnostic("file.save_ready", format!("Saved {path}"));
            }
            Err(message) => self.set_error("file.save_failed", message, true),
        }
        true
    }

    /// Serializes one delta response for the snapshot wire: sections whose
    /// revision passed `have_revision`, plus terminal chunks past
    /// `have_sequence`. Reading is idempotent - the same cursors return the
    /// same delta again - so a caller that failed to apply a response
    /// recovers by re-reading with its unadvanced cursors.
    pub fn snapshot_delta(
        &mut self,
        have_revision: u64,
        have_sequence: u64,
    ) -> Result<Vec<u8>, serde_json::Error> {
        use crate::model::{RestSections, RestWire, SnapshotDeltaWire, TerminalMetaWire};

        if !self
            .delta
            .last_rest
            .as_ref()
            .is_some_and(|rest| rest.matches(&self.snapshot))
        {
            self.delta.revision += 1;
            self.delta.rest_revision = self.delta.revision;
            self.delta.last_rest = Some(RestSections::capture(&self.snapshot));
        }
        if self.delta.last_editor.as_ref() != Some(&self.snapshot.editor) {
            self.delta.revision += 1;
            self.delta.editor_revision = self.delta.revision;
            self.delta.last_editor = Some(self.snapshot.editor.clone());
        }
        if self.delta.last_changes.as_ref() != Some(&self.snapshot.changes) {
            self.delta.revision += 1;
            self.delta.changes_revision = self.delta.revision;
            self.delta.last_changes = Some(self.snapshot.changes.clone());
        }
        // A cursor from the future has no valid meaning in-process; treat it
        // as a fresh reader so the response converges on full state.
        let have_revision = if have_revision > self.delta.revision {
            0
        } else {
            have_revision
        };

        let chunks: Vec<_> = self
            .snapshot
            .terminal
            .chunks
            .iter()
            .filter(|chunk| chunk.sequence > have_sequence)
            .collect();
        let chunks_dropped = match self.snapshot.terminal.chunks.first() {
            Some(oldest) => have_sequence + 1 < oldest.sequence,
            None => have_sequence < self.snapshot.terminal.sequence,
        };

        let wire = SnapshotDeltaWire {
            schema_version: self.snapshot.schema_version,
            revision: self.delta.revision,
            rest: (self.delta.rest_revision > have_revision).then(|| RestWire {
                navigator: &self.snapshot.navigator,
                overlay: &self.snapshot.overlay,
                tab: &self.snapshot.tab,
                connection: &self.snapshot.connection,
                zoomed: &self.snapshot.zoomed,
                focused: &self.snapshot.focused,
                pane_layout: &self.snapshot.pane_layout,
                terminal: TerminalMetaWire {
                    pane_id: &self.snapshot.terminal.pane_id,
                    closed: self.snapshot.terminal.closed,
                    exit_code: self.snapshot.terminal.exit_code,
                    panes: &self.snapshot.terminal.panes,
                },
                ui_state: &self.snapshot.ui_state,
                ime: &self.snapshot.ime,
                status: &self.snapshot.status,
                pet: &self.snapshot.pet,
            }),
            editor: (self.delta.editor_revision > have_revision).then_some(&self.snapshot.editor),
            changes: (self.delta.changes_revision > have_revision)
                .then_some(&self.snapshot.changes),
            find: &self.snapshot.find,
            input_generation: self.snapshot.input_generation,
            terminal_sequence: self.snapshot.terminal.sequence,
            chunks,
            chunks_dropped,
        };
        serde_json::to_vec(&wire)
    }

    pub fn set_live(&mut self, context: LiveContext) {
        self.live = Some(context);
    }

    pub fn install_remote_control(&mut self, context: RemoteControlContext) {
        self.remote_controls
            .insert(context.target_id().to_owned(), context);
    }

    pub fn install_remote_terminal(&mut self, context: RemoteTerminalContext) {
        self.remote_terminals
            .insert(context.target_id().to_owned(), context);
    }

    pub fn install_remote_file_transport(
        &mut self,
        target_id: impl Into<String>,
        transport: RusshSftpTransport,
    ) {
        self.remote_file_transports
            .insert(target_id.into(), transport);
    }

    fn advance_remote_file_generation(&mut self) -> u64 {
        self.next_remote_file_generation = self.next_remote_file_generation.saturating_add(1);
        self.next_remote_file_generation
    }

    fn mark_remote_files_unavailable(
        &mut self,
        status_index: usize,
        root_path: String,
        message: String,
        generation: u64,
    ) {
        self.snapshot.status.remote[status_index].files = RemoteFileListSnapshot {
            root_path: Some(root_path),
            state: "unavailable".to_owned(),
            entries: Vec::new(),
            message: Some(message),
            generation,
        };
    }

    fn request_remote_file_list(&mut self, payload: RemoteFileListPayload) -> bool {
        let target_id = payload.target_id;
        let root_path = payload.root_path;
        let Some(status_index) = self
            .snapshot
            .status
            .remote
            .iter()
            .position(|status| status.target_id == target_id)
        else {
            self.set_error(
                "remote.files.unknown_target",
                format!("Remote file listing requested an unconfigured target {target_id}"),
                false,
            );
            return true;
        };
        if !Path::new(&root_path).is_absolute()
            || root_path.bytes().any(|byte| byte.is_ascii_control())
        {
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(
                status_index,
                root_path.clone(),
                "Remote file root must be an absolute single-line path".to_owned(),
                generation,
            );
            self.set_error(
                "remote.files.invalid_root",
                format!("Remote file root is invalid for target {target_id}"),
                false,
            );
            return true;
        }
        let status = &self.snapshot.status.remote[status_index];
        if status.state != "connected" {
            let message = status
                .message
                .clone()
                .unwrap_or_else(|| format!("Remote target {target_id} is not connected"));
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(status_index, root_path, message, generation);
            return true;
        }
        let root_is_authoritative = status.session.as_ref().is_some_and(|session| {
            session
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .any(|checkout| checkout.path == root_path)
        });
        if !root_is_authoritative {
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(
                status_index,
                root_path.clone(),
                "Remote file root is not part of the authoritative Herdr session".to_owned(),
                generation,
            );
            self.set_error(
                "remote.files.unknown_root",
                format!("Remote file root {root_path} is not open on target {target_id}"),
                false,
            );
            return true;
        }
        if status.files.root_path.as_deref() == Some(root_path.as_str())
            && matches!(status.files.state.as_str(), "loading" | "ready")
        {
            return false;
        }
        let Some(transport) = self.remote_file_transports.get(&target_id).cloned() else {
            let message = format!("Remote target {target_id} has no configured SFTP transport");
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(
                status_index,
                root_path,
                message.clone(),
                generation,
            );
            self.set_error("remote.files.transport_unavailable", message, true);
            return true;
        };
        let Some(context) = self.worker_context.clone() else {
            let message = "The remote file worker is unavailable".to_owned();
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(
                status_index,
                root_path,
                message.clone(),
                generation,
            );
            self.set_error("remote.files.worker_unavailable", message, true);
            return true;
        };

        let generation = self.advance_remote_file_generation();
        self.snapshot.status.remote[status_index].files = RemoteFileListSnapshot {
            root_path: Some(root_path.clone()),
            state: "loading".to_owned(),
            entries: Vec::new(),
            message: None,
            generation,
        };
        self.push_diagnostic(
            "remote.files.requested",
            format!("Listing remote files for {target_id} at {root_path}"),
        );
        let worker_target_id = target_id.clone();
        let worker_root_path = root_path.clone();
        match thread::Builder::new()
            .name(format!("herdr-core-remote-files-{target_id}"))
            .spawn(move || {
                let result = RemoteFileService::new(worker_root_path.clone(), transport)
                    .and_then(|service| service.list(""))
                    .map_err(|error| error.to_string());
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_remote_file_list_result(
                        &worker_target_id,
                        &worker_root_path,
                        generation,
                        result,
                    ),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            }) {
            Ok(_) => true,
            Err(error) => self.ingest_remote_file_list_result(
                &target_id,
                &root_path,
                generation,
                Err(format!("Remote file worker could not be started: {error}")),
            ),
        }
    }

    fn ingest_remote_file_list_result(
        &mut self,
        target_id: &str,
        root_path: &str,
        generation: u64,
        result: Result<Vec<FileEntry>, String>,
    ) -> bool {
        let Some(status_index) = self
            .snapshot
            .status
            .remote
            .iter()
            .position(|status| status.target_id == target_id)
        else {
            self.set_error(
                "remote.files.unknown_target",
                format!("Remote file result named an unconfigured target {target_id}"),
                false,
            );
            return true;
        };
        let files = &self.snapshot.status.remote[status_index].files;
        if files.generation != generation || files.root_path.as_deref() != Some(root_path) {
            self.push_diagnostic(
                "remote.files.stale",
                format!(
                    "Ignored stale remote file result for {target_id} at {root_path} generation {generation}"
                ),
            );
            return true;
        }
        match result {
            Ok(mut entries) => {
                entries.sort_by(|left, right| {
                    let left_is_directory = left.kind == FileKind::Directory;
                    let right_is_directory = right.kind == FileKind::Directory;
                    right_is_directory
                        .cmp(&left_is_directory)
                        .then_with(|| {
                            left.name
                                .to_ascii_lowercase()
                                .cmp(&right.name.to_ascii_lowercase())
                        })
                        .then_with(|| left.path.cmp(&right.path))
                });
                let entry_count = entries.len();
                self.snapshot.status.remote[status_index].files = RemoteFileListSnapshot {
                    root_path: Some(root_path.to_owned()),
                    state: "ready".to_owned(),
                    entries: entries
                        .into_iter()
                        .map(|entry| RemoteFileEntrySnapshot {
                            path: entry.path,
                            name: entry.name,
                            is_directory: entry.kind == FileKind::Directory,
                            size_bytes: entry.size_bytes,
                        })
                        .collect(),
                    message: None,
                    generation,
                };
                self.push_diagnostic(
                    "remote.files.ready",
                    format!("Listed {entry_count} remote files for {target_id} at {root_path}"),
                );
            }
            Err(message) => {
                self.mark_remote_files_unavailable(
                    status_index,
                    root_path.to_owned(),
                    message.clone(),
                    generation,
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "remote_files",
                        "kind": "remote.files_failed",
                        "target": target_id,
                        "root_path": root_path,
                        "generation": generation,
                        "message": message,
                    })
                );
            }
        }
        true
    }

    fn request_remote_control(&mut self, payload: RemoteControlPayload) -> bool {
        let target_id = payload.target_id;
        let request_id = payload.request_id;
        if target_id.trim().is_empty() || request_id.trim().is_empty() {
            self.set_error(
                "remote.control.invalid_request",
                "Remote control requires non-empty target_id and request_id",
                false,
            );
            return true;
        }
        if self
            .remote_control_requests
            .iter()
            .any(|known| known == &(target_id.clone(), request_id.clone()))
        {
            self.push_diagnostic(
                "remote.control.duplicate_ignored",
                format!("Ignored duplicate remote request {request_id} for {target_id}"),
            );
            return true;
        }
        let Some(context) = self.remote_controls.get(&target_id).cloned() else {
            self.set_error(
                "remote.control.unavailable",
                format!("Remote control is unavailable for target {target_id}"),
                true,
            );
            return true;
        };
        let Some(remote) = self
            .snapshot
            .status
            .remote
            .iter()
            .find(|remote| remote.target_id == target_id)
        else {
            self.set_error(
                "remote.control.unknown_target",
                format!("Remote target {target_id} is not configured"),
                false,
            );
            return true;
        };
        if remote.state != "connected" {
            self.set_error(
                "remote.control.not_connected",
                format!(
                    "Remote target {target_id} is {}; no command was sent",
                    remote.state
                ),
                true,
            );
            return true;
        }
        let Some(session) = remote.session.clone() else {
            self.set_error(
                "remote.control.session_missing",
                format!("Remote target {target_id} has no authoritative session projection"),
                true,
            );
            return true;
        };
        let mut source_pane_id = None;
        if let Some(pane_id) = payload.request.pane_id().map(str::to_owned) {
            if pane_id.trim().is_empty() {
                self.set_error(
                    "remote.control.invalid_pane",
                    "Remote pane control requires a non-empty pane_id",
                    false,
                );
                return true;
            }
            let pane_exists = session.workspaces.iter().any(|workspace| {
                workspace.checkouts.iter().any(|checkout| {
                    checkout
                        .tabs
                        .iter()
                        .any(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
                })
            });
            if !pane_exists {
                self.set_error(
                    "remote.control.pane_not_found",
                    format!("Pane {pane_id} does not belong to remote target {target_id}"),
                    false,
                );
                return true;
            }
            source_pane_id = remote_pane_source_id(&target_id, &pane_id).map(str::to_owned);
            if source_pane_id.is_none() {
                self.set_error(
                    "remote.control.invalid_pane_scope",
                    format!("Pane {pane_id} is not scoped to remote target {target_id}"),
                    false,
                );
                return true;
            }
            let needs_confirmation =
                matches!(&payload.request, RemoteControlRequest::ClosePane { .. })
                    && session.agents.iter().any(|agent| {
                        agent.pane_id == pane_id
                            && crate::sidebar::requires_close_confirmation(&agent.state)
                    });
            if needs_confirmation && !payload.request.confirmed() {
                self.set_error(
                    "remote.control.close_confirmation_required",
                    format!(
                        "Pane {pane_id} on {target_id} is working or needs attention; remote close requires confirmed=true"
                    ),
                    false,
                );
                return true;
            }
        }

        let action = match payload.request {
            RemoteControlRequest::FocusPane { .. } => {
                RemoteControlAction::Pane(PaneControlAction::Focus {
                    pane_id: source_pane_id.expect("remote pane source id was validated"),
                })
            }
            RemoteControlRequest::SplitPane { direction, cwd, .. } => {
                RemoteControlAction::Pane(PaneControlAction::Split {
                    pane_id: source_pane_id.expect("remote pane source id was validated"),
                    direction,
                    cwd,
                })
            }
            RemoteControlRequest::TogglePaneZoom { .. } => {
                RemoteControlAction::Pane(PaneControlAction::ToggleZoom {
                    pane_id: source_pane_id.expect("remote pane source id was validated"),
                })
            }
            RemoteControlRequest::ClosePane { .. } => {
                RemoteControlAction::Pane(PaneControlAction::Close {
                    pane_id: source_pane_id.expect("remote pane source id was validated"),
                })
            }
            RemoteControlRequest::FocusWorkspace { workspace_id } => {
                let Some(source_id) = session
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.id == workspace_id)
                    .then(|| remote_workspace_source_id(&target_id, &workspace_id))
                    .flatten()
                else {
                    self.set_error(
                        "remote.control.workspace_not_found",
                        format!(
                            "Workspace {workspace_id} does not belong to remote target {target_id}"
                        ),
                        false,
                    );
                    return true;
                };
                RemoteControlAction::FocusWorkspace {
                    workspace_id: source_id.to_owned(),
                }
            }
            RemoteControlRequest::FocusTab { tab_id } => {
                let exists = !tab_id.trim().is_empty()
                    && session.workspaces.iter().any(|workspace| {
                        workspace.checkouts.iter().any(|checkout| {
                            checkout
                                .tabs
                                .iter()
                                .any(|tab| tab.id.as_deref() == Some(tab_id.as_str()))
                        })
                    });
                if !exists {
                    self.set_error(
                        "remote.control.tab_not_found",
                        format!("Tab {tab_id} does not belong to remote target {target_id}"),
                        false,
                    );
                    return true;
                }
                let Some(source_id) = remote_tab_source_id(&target_id, &tab_id) else {
                    self.set_error(
                        "remote.control.invalid_tab_scope",
                        format!("Tab {tab_id} is not scoped to remote target {target_id}"),
                        false,
                    );
                    return true;
                };
                RemoteControlAction::FocusTab {
                    tab_id: source_id.to_owned(),
                }
            }
            RemoteControlRequest::CreateTab {
                workspace_id,
                cwd,
                label,
            } => {
                let Some(source_id) = session
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.id == workspace_id)
                    .then(|| remote_workspace_source_id(&target_id, &workspace_id))
                    .flatten()
                else {
                    self.set_error(
                        "remote.control.workspace_not_found",
                        format!(
                            "Workspace {workspace_id} does not belong to remote target {target_id}"
                        ),
                        false,
                    );
                    return true;
                };
                if cwd.trim().is_empty() || label.trim().is_empty() {
                    self.set_error(
                        "remote.control.invalid_tab",
                        "Remote tab creation requires non-empty cwd and label",
                        false,
                    );
                    return true;
                }
                RemoteControlAction::CreateTab {
                    workspace_id: source_id.to_owned(),
                    cwd,
                    label,
                }
            }
            RemoteControlRequest::CloseTab { tab_id, confirmed } => {
                let tab = session
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .find(|tab| tab.id.as_deref() == Some(tab_id.as_str()));
                let Some(tab) = tab else {
                    self.set_error(
                        "remote.control.tab_not_found",
                        format!("Tab {tab_id} does not belong to remote target {target_id}"),
                        false,
                    );
                    return true;
                };
                let pane_ids = tab
                    .panes
                    .iter()
                    .map(|pane| pane.id.as_str())
                    .collect::<HashSet<_>>();
                let needs_confirmation = session.agents.iter().any(|agent| {
                    pane_ids.contains(agent.pane_id.as_str())
                        && crate::sidebar::requires_close_confirmation(&agent.state)
                });
                if needs_confirmation && !confirmed {
                    self.set_error(
                        "remote.control.close_confirmation_required",
                        format!(
                            "Tab {tab_id} on {target_id} contains an agent that is working or needs attention; remote close requires confirmed=true"
                        ),
                        false,
                    );
                    return true;
                }
                let Some(source_id) = remote_tab_source_id(&target_id, &tab_id) else {
                    self.set_error(
                        "remote.control.invalid_tab_scope",
                        format!("Tab {tab_id} is not scoped to remote target {target_id}"),
                        false,
                    );
                    return true;
                };
                RemoteControlAction::CloseTab {
                    tab_id: source_id.to_owned(),
                }
            }
        };

        let creation_key = remote_tab_creation_key(&target_id, &action);
        if let Some(key) = creation_key.as_ref()
            && !self.remote_tab_creations_in_flight.insert(key.clone())
        {
            self.push_diagnostic(
                "remote.control.duplicate_tab_ignored",
                format!(
                    "Ignored duplicate in-flight tab.create for workspace {} on {target_id}",
                    key.1
                ),
            );
            return true;
        }
        if self.remote_control_requests.len() == 128 {
            self.remote_control_requests.pop_front();
        }
        self.remote_control_requests
            .push_back((target_id.clone(), request_id.clone()));
        self.push_diagnostic(
            "remote.control.requested",
            format!("Sending {} to {target_id}", action.kind()),
        );
        if let Err(message) = live::spawn_remote_control(context, request_id, action) {
            if let Some(key) = creation_key {
                self.remote_tab_creations_in_flight.remove(&key);
            }
            self.set_error("remote.control.worker_failed", message, true);
        }
        true
    }

    /// Groups the session's working directories under the Herdr workspace that
    /// owns them. Shared with the session-sync coordinator so a catalog
    /// precomputed outside the runtime lock is built from the same inputs.
    ///
    /// Layouts carry the workspace a pane belongs to and `panes` carries its
    /// directory, so the two together give each workspace the set of
    /// repositories it actually occupies without asking git anything.
    pub fn session_spaces(payload: &SessionSnapshotPayload) -> Vec<workspace::SessionSpace> {
        let labels: HashMap<&str, &str> = payload
            .workspaces
            .iter()
            .map(|workspace| (workspace.workspace_id.as_str(), workspace.label.trim()))
            .collect();
        let mut spaces: Vec<workspace::SessionSpace> = Vec::new();
        for layout in &payload.layouts {
            let index = match spaces
                .iter()
                .position(|space| space.id == layout.workspace_id)
            {
                Some(index) => index,
                None => {
                    let label = labels
                        .get(layout.workspace_id.as_str())
                        .copied()
                        .filter(|label| !label.is_empty())
                        .unwrap_or(layout.workspace_id.as_str())
                        .to_owned();
                    spaces.push(workspace::SessionSpace {
                        id: layout.workspace_id.clone(),
                        label,
                        cwds: Vec::new(),
                    });
                    spaces.len() - 1
                }
            };
            for pane in &layout.panes {
                let Some(cwd) = Self::pane_cwd(payload, &pane.pane_id) else {
                    continue;
                };
                if !spaces[index].cwds.contains(&cwd) {
                    spaces[index].cwds.push(cwd);
                }
            }
        }
        spaces
    }

    fn pane_cwd(payload: &SessionSnapshotPayload, pane_id: &str) -> Option<String> {
        payload
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .and_then(|pane| pane.cwd.clone())
            .or_else(|| {
                payload
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id.as_deref().or(agent.id.as_deref()) == Some(pane_id))
                    .and_then(|agent| agent.cwd.clone())
            })
            .map(|cwd| cwd.trim().to_owned())
            // Herdr reports `/` for a pane whose process has exited, which
            // says where the pane is not rather than where it is. Treating it
            // as a directory produced an unnamed checkout row with no panes
            // under it.
            .filter(|cwd| !cwd.is_empty() && cwd != "/")
    }

    /// Rebuilds the navigator from the Herdr workspaces the session reports
    /// and any registration Herdr has no workspace for. Registrations are
    /// durable metadata only: removing one never removes a checkout or ends a
    /// remote process.
    fn reconcile_session_catalog(
        &mut self,
        payload: &SessionSnapshotPayload,
        precomputed: Option<session_sync::PrecomputedCatalog>,
    ) -> bool {
        self.last_session_spaces = Self::session_spaces(payload);
        // The catalog shells out to git, so the sync coordinator builds it before
        // taking the runtime lock; a catalog whose registrations no longer
        // match current state is discarded and rebuilt inline.
        let mut workspaces = match precomputed {
            Some(catalog)
                if catalog.registrations == self.snapshot.ui_state.workspace_registrations =>
            {
                catalog.workspaces
            }
            _ => workspace::build_catalog(
                &self.snapshot.ui_state.workspace_registrations,
                &self.last_session_spaces,
            ),
        };
        Self::apply_workspace_expansion(
            &mut workspaces,
            &self.snapshot.ui_state.collapsed_workspace_ids,
        );
        let projected_agents = project_agents(payload.clone()).agents;
        let listening_ports = self.listening_ports.entries.clone();

        for layout in &payload.layouts {
            // A plain terminal pane is not necessarily represented in the
            // agent list. Its cwd is still authoritative for attaching the
            // live layout to the registered checkout. Falling back to the
            // agent record keeps agent-specific cwd handling intact.
            let context_path = layout.panes.iter().find_map(|pane| {
                payload
                    .panes
                    .iter()
                    .find(|source| source.pane_id == pane.pane_id)
                    .and_then(|source| source.cwd.clone())
                    .filter(|path| !path.trim().is_empty())
                    .or_else(|| {
                        payload
                            .agents
                            .iter()
                            .find(|agent| {
                                agent.pane_id.as_deref().or(agent.id.as_deref())
                                    == Some(pane.pane_id.as_str())
                            })
                            .and_then(|agent| agent.cwd.clone())
                    })
            });
            let Some(workspace_snapshot) = find_workspace_for_context(
                &mut workspaces,
                context_path.as_deref(),
                &layout.workspace_id,
            ) else {
                continue;
            };
            let checkout_index = context_path.as_deref().and_then(|path| {
                workspace_snapshot
                    .checkouts
                    .iter()
                    .position(|checkout| path_is_within_checkout(path, &checkout.path))
            });
            let Some(checkout) = workspace_snapshot
                .checkouts
                .get_mut(checkout_index.unwrap_or(0))
            else {
                continue;
            };
            let panes = layout
                .panes
                .iter()
                .map(|pane| {
                    let agent = projected_agents
                        .iter()
                        .find(|agent| agent.pane_id == pane.pane_id);
                    let source = payload
                        .panes
                        .iter()
                        .find(|source| source.pane_id == pane.pane_id);
                    let cwd = source
                        .and_then(|source| source.cwd.clone())
                        .or_else(|| {
                            payload
                                .agents
                                .iter()
                                .find(|source| {
                                    source.pane_id.as_deref().or(source.id.as_deref())
                                        == Some(pane.pane_id.as_str())
                                })
                                .and_then(|source| source.cwd.clone())
                        })
                        .unwrap_or_else(|| checkout.path.clone());
                    let ports = crate::ports::attributed_ports(&cwd, &listening_ports);
                    PaneSnapshot {
                        id: pane.pane_id.clone(),
                        herdr_label: source.and_then(|source| source.label.clone()),
                        terminal_title: source.and_then(|source| source.terminal_title.clone()),
                        workspace_label: agent.map(|agent| agent.workspace_label.clone()),
                        cwd,
                        state: agent
                            .map(|agent| agent.state.clone())
                            .unwrap_or_else(|| "unknown".to_owned()),
                        summary: agent
                            .map(|agent| agent.summary.clone())
                            .filter(|summary| summary != crate::sidebar::MISSING_SUMMARY),
                        activity_at_unix_ms: agent.and_then(|agent| agent.activity.parse().ok()),
                        fork: pane_fork_snapshot(agent),
                        ports,
                    }
                })
                .collect::<Vec<_>>();
            let tab_label = payload
                .tabs
                .iter()
                .find(|tab| tab.tab_id == layout.tab_id)
                .map(|tab| tab.label.trim())
                .filter(|label| !label.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| layout.tab_id.clone());
            let tab = TabSnapshot {
                id: Some(layout.tab_id.clone()),
                workspace_id: Some(workspace_snapshot.id.clone()),
                checkout_id: Some(checkout.id.clone()),
                label: Some(tab_label),
                empty: panes.is_empty(),
                panes,
            };
            if let Some(existing) = checkout
                .tabs
                .iter_mut()
                .find(|existing| existing.id == tab.id)
            {
                *existing = tab;
            } else {
                checkout.tabs.push(tab);
            }
        }

        let previous = self.snapshot.navigator.clone();
        self.snapshot.navigator.workspaces = workspaces;
        self.snapshot.navigator.devices = workspace::devices(
            &self.remote_targets,
            &self.snapshot.ui_state.device_registrations,
        );
        // An agent belongs to the device whose project holds its pane. The
        // project label is no longer Herdr's workspace label once a
        // registration covers the repository, so labels cannot be the key.
        for device in &mut self.snapshot.navigator.devices {
            device.agent_count = projected_agents
                .iter()
                .filter(|agent| {
                    self.snapshot
                        .navigator
                        .workspaces
                        .iter()
                        .filter(|workspace| workspace.device_id == device.id)
                        .flat_map(|workspace| workspace.checkouts.iter())
                        .flat_map(|checkout| checkout.tabs.iter())
                        .flat_map(|tab| tab.panes.iter())
                        .any(|pane| pane.id == agent.pane_id)
                })
                .count() as u32;
        }
        if self.snapshot.navigator.focused_device_id.is_none() {
            self.snapshot.navigator.focused_device_id = Some(workspace::LOCAL_DEVICE_ID.to_owned());
        }
        self.resync_navigator_focus();
        previous != self.snapshot.navigator
    }

    /// Reconciles the focused checkout, its owning workspace, root path, and
    /// active tab projection after a catalog replacement. Catalog rebuilds
    /// happen from both background sync and event handlers, so this policy
    /// must have one implementation to keep those paths convergent.
    fn resync_navigator_focus(&mut self) {
        let focused_checkout_exists = self.snapshot.navigator.workspaces.iter().any(|workspace| {
            workspace.checkouts.iter().any(|checkout| {
                Some(checkout.id.as_str()) == self.snapshot.navigator.focused_checkout_id.as_deref()
            })
        });
        if !focused_checkout_exists && self.snapshot.ui_state.focused_checkout_id.is_none() {
            self.snapshot.navigator.focused_checkout_id = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .find(|checkout| checkout.exists)
                .map(|checkout| checkout.id.clone());
        }
        let focused = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| {
                        Some(checkout.id.as_str())
                            == self.snapshot.navigator.focused_checkout_id.as_deref()
                    })
                    .map(|checkout| (workspace.id.clone(), checkout.path.clone()))
            });
        self.snapshot.navigator.focused_workspace_id = focused
            .as_ref()
            .map(|(workspace_id, _)| workspace_id.clone());
        self.snapshot.navigator.root_path = focused.map(|(_, path)| path);
        self.sync_active_tab_projection();
    }

    fn sync_active_tab_projection(&mut self) {
        let Some(focused_checkout_id) = self.snapshot.navigator.focused_checkout_id.as_deref()
        else {
            self.snapshot.tab = TabSnapshot {
                id: None,
                workspace_id: None,
                checkout_id: None,
                label: None,
                empty: true,
                panes: Vec::new(),
            };
            return;
        };
        let Some((workspace_id, checkout)) =
            self.snapshot
                .navigator
                .workspaces
                .iter()
                .find_map(|workspace| {
                    workspace
                        .checkouts
                        .iter()
                        .find(|checkout| checkout.id == focused_checkout_id)
                        .map(|checkout| (workspace.id.clone(), checkout))
                })
        else {
            self.snapshot.tab = TabSnapshot {
                id: None,
                workspace_id: None,
                checkout_id: None,
                label: None,
                empty: true,
                panes: Vec::new(),
            };
            return;
        };
        if let Some(tab) = checkout.tabs.first() {
            self.snapshot.tab = tab.clone();
        } else {
            self.snapshot.tab = TabSnapshot {
                id: None,
                workspace_id: Some(workspace_id),
                checkout_id: Some(checkout.id.clone()),
                label: Some("No tabs".to_owned()),
                empty: true,
                panes: Vec::new(),
            };
        }
    }

    /// Applies a live session-sync result: projected agents on success, an
    /// explicit Herdr status on failure. Returns whether the snapshot changed.
    pub fn ingest_session(
        &mut self,
        fetched: Result<SessionSnapshotPayload, SessionFetchError>,
    ) -> bool {
        self.ingest_session_with_catalog(fetched, None)
    }

    /// Applies the authoritative session projection for one configured remote
    /// Herdr target. The last valid session remains visible through a stale or
    /// disconnected interval, while status always names the current failure.
    pub fn ingest_remote_session(
        &mut self,
        target_id: &str,
        fetched: Result<RemoteSessionSnapshot, SessionFetchError>,
    ) -> bool {
        let pane_sets = fetched.as_ref().ok().map(|session| {
            remote_terminal_pane_sets(
                session,
                self.snapshot.navigator.focused_device_id.as_deref() == Some(target_id),
            )
        });
        let Some(status) = self
            .snapshot
            .status
            .remote
            .iter_mut()
            .find(|status| status.target_id == target_id)
        else {
            self.set_error(
                "remote.target_unknown",
                format!("Remote Herdr sync returned an unconfigured target {target_id}"),
                false,
            );
            return true;
        };

        let mut changed = false;
        match fetched {
            Ok(session) => {
                if status.state != "connected" || status.message.is_some() {
                    status.state = "connected".to_owned();
                    status.message = None;
                    changed = true;
                }
                if status.files.root_path.as_deref().is_some_and(|root_path| {
                    !session
                        .workspaces
                        .iter()
                        .flat_map(|workspace| workspace.checkouts.iter())
                        .any(|checkout| checkout.path == root_path)
                }) {
                    status.files = RemoteFileListSnapshot::idle();
                    changed = true;
                }
                if status.session.as_ref() != Some(&session) {
                    status.session = Some(session);
                    changed = true;
                }
            }
            Err(error) => {
                if status.state != error.state()
                    || status.message.as_deref() != Some(error.message())
                {
                    status.state = error.state().to_owned();
                    status.message = Some(error.message().to_owned());
                    changed = true;
                }
            }
        }
        status.last_checked_at_unix_ms = Some(unix_milliseconds());

        let agent_count = status
            .session
            .as_ref()
            .map(|session| session.agents.len())
            .unwrap_or(0)
            .min(u32::MAX as usize) as u32;
        if let Some(device) = self
            .snapshot
            .navigator
            .devices
            .iter_mut()
            .find(|device| device.id == target_id)
        {
            let device_state = if status.state == "connected" {
                "ready"
            } else {
                "unavailable"
            };
            if device.state != device_state || device.agent_count != agent_count {
                device.state = device_state.to_owned();
                device.agent_count = agent_count;
                changed = true;
            }
        }
        if let Some((live_pane_ids, active_pane_ids)) = pane_sets {
            changed |=
                self.reconcile_remote_terminal_panes(target_id, &live_pane_ids, &active_pane_ids);
        }
        changed
    }

    fn reconcile_remote_terminal_panes(
        &mut self,
        target_id: &str,
        live_pane_ids: &HashSet<String>,
        active_pane_ids: &HashSet<String>,
    ) -> bool {
        let target_prefix = format!("remote:{target_id}:pane:");
        let belongs_to_target = |pane_id: &str| pane_id.starts_with(&target_prefix);
        let projected_pane_ids = self
            .snapshot
            .terminal
            .panes
            .iter()
            .filter(|pane| belongs_to_target(&pane.pane_id))
            .map(|pane| pane.pane_id.clone())
            .collect::<HashSet<_>>();
        let mut changed = &projected_pane_ids != live_pane_ids;
        let before_map_entries = self.terminal_sessions.len()
            + self.terminal_session_generations.len()
            + self.terminal_session_lifecycles.len()
            + self.terminal_sizes.len();
        self.terminal_sessions
            .retain(|pane_id, _| !belongs_to_target(pane_id) || active_pane_ids.contains(pane_id));
        self.terminal_session_generations
            .retain(|pane_id, _| !belongs_to_target(pane_id) || active_pane_ids.contains(pane_id));
        self.terminal_session_lifecycles
            .retain(|pane_id, _| !belongs_to_target(pane_id) || active_pane_ids.contains(pane_id));
        self.terminal_sizes
            .retain(|pane_id, _| !belongs_to_target(pane_id) || live_pane_ids.contains(pane_id));
        let after_map_entries = self.terminal_sessions.len()
            + self.terminal_session_generations.len()
            + self.terminal_session_lifecycles.len()
            + self.terminal_sizes.len();
        changed |= before_map_entries != after_map_entries;

        self.snapshot.terminal.panes.retain(|pane| {
            !belongs_to_target(&pane.pane_id) || live_pane_ids.contains(&pane.pane_id)
        });
        let mut pane_ids = live_pane_ids.iter().cloned().collect::<Vec<_>>();
        pane_ids.sort();
        for pane_id in &pane_ids {
            self.ensure_terminal_pane(pane_id);
        }
        let idle_lifecycle = TerminalSessionLifecycle::default();
        for pane in self.snapshot.terminal.panes.iter_mut().filter(|pane| {
            belongs_to_target(&pane.pane_id) && !active_pane_ids.contains(&pane.pane_id)
        }) {
            let idle = TerminalPaneSnapshot {
                pane_id: pane.pane_id.clone(),
                closed: false,
                exit_code: None,
                transport_state: idle_lifecycle.state.to_owned(),
                transport_message: None,
                transport_generation: 0,
                transport_attempt: 0,
                transport_exit_category: None,
                transport_retry_decision: idle_lifecycle.retry_decision.to_owned(),
            };
            if *pane != idle {
                *pane = idle;
                changed = true;
            }
        }
        let mut active_pane_ids = active_pane_ids.iter().cloned().collect::<Vec<_>>();
        active_pane_ids.sort();
        for pane_id in active_pane_ids {
            let current = self
                .terminal_session_lifecycles
                .get(&pane_id)
                .cloned()
                .unwrap_or_default();
            changed |= terminal_control_request_allowed(
                current.state,
                self.terminal_sessions.contains_key(&pane_id),
            );
            self.request_terminal_control(&pane_id);
        }
        changed
    }

    fn reconcile_remote_terminal_selection(&mut self) -> bool {
        let focused_device_id = self.snapshot.navigator.focused_device_id.clone();
        let sessions = self
            .snapshot
            .status
            .remote
            .iter()
            .filter_map(|status| {
                status
                    .session
                    .clone()
                    .map(|session| (status.target_id.clone(), session))
            })
            .collect::<Vec<_>>();
        sessions
            .into_iter()
            .fold(false, |changed, (target_id, session)| {
                let (live_pane_ids, active_pane_ids) = remote_terminal_pane_sets(
                    &session,
                    focused_device_id.as_deref() == Some(target_id.as_str()),
                );
                self.reconcile_remote_terminal_panes(&target_id, &live_pane_ids, &active_pane_ids)
                    | changed
            })
    }

    /// Reconciles the pane and checkout ids loaded from disk against the first
    /// live session, then retires the hint.
    ///
    /// Persisted ids name a session that has already ended, so a pane Herdr no
    /// longer has is the expected case on launch rather than a fault. Dropping
    /// the unusable parts here lets the ordinary resolution below pick the
    /// session's own focus, and keeps `pane.projection_unavailable` meaning
    /// what it says: a pane the user chose against a live session went away.
    fn consume_restore_hint(&mut self, payload: &SessionSnapshotPayload) {
        if !self.restore_hint_pending {
            return;
        }
        self.restore_hint_pending = false;

        let restored_pane_exists = self
            .snapshot
            .ui_state
            .selected_pane_id
            .as_deref()
            .is_some_and(|pane_id| {
                payload
                    .layouts
                    .iter()
                    .any(|layout| layout.panes.iter().any(|pane| pane.pane_id == pane_id))
            });
        if !restored_pane_exists {
            self.snapshot.ui_state.selected_pane_id = None;
            self.snapshot.terminal.pane_id = None;
            self.snapshot.focused.pane_id = None;
        }

        let restored_checkout_exists = self
            .snapshot
            .ui_state
            .focused_checkout_id
            .as_deref()
            .is_some_and(|checkout_id| {
                self.snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .any(|checkout| checkout.id == checkout_id)
            });
        if !restored_checkout_exists {
            self.snapshot.ui_state.focused_checkout_id = None;
            self.snapshot.navigator.focused_checkout_id = None;
            self.resync_navigator_focus();
        }
    }

    /// Like [`Self::ingest_session`], with a workspace catalog the caller
    /// built outside the runtime lock.
    pub fn ingest_session_with_catalog(
        &mut self,
        fetched: Result<SessionSnapshotPayload, SessionFetchError>,
        precomputed: Option<session_sync::PrecomputedCatalog>,
    ) -> bool {
        let previously_projected_pane = self
            .snapshot
            .terminal
            .pane_id
            .as_deref()
            .filter(|pane_id| {
                self.snapshot.pane_layout.as_ref().is_some_and(|layout| {
                    layout
                        .pane_ids()
                        .into_iter()
                        .any(|layout_pane_id| layout_pane_id == *pane_id)
                })
            })
            .map(str::to_owned);
        let previously_projected_tab = self
            .snapshot
            .pane_layout
            .as_ref()
            .map(|layout| layout.tab_id.clone());
        let previously_projected_in_focused_checkout =
            previously_projected_pane.as_deref().is_some_and(|pane_id| {
                self.snapshot
                    .navigator
                    .focused_checkout_id
                    .as_deref()
                    .and_then(|checkout_id| {
                        self.snapshot
                            .navigator
                            .workspaces
                            .iter()
                            .flat_map(|workspace| workspace.checkouts.iter())
                            .find(|checkout| checkout.id == checkout_id)
                    })
                    .is_some_and(|checkout| {
                        checkout
                            .tabs
                            .iter()
                            .flat_map(|tab| tab.panes.iter())
                            .any(|pane| pane.id == pane_id)
                    })
            });
        let live_pane_ids = fetched.as_ref().ok().map(|payload| {
            payload
                .layouts
                .iter()
                .flat_map(|layout| layout.panes.iter())
                .map(|pane| pane.pane_id.clone())
                .collect::<HashSet<_>>()
        });
        if let Some(live_pane_ids) = live_pane_ids.as_ref() {
            let keep =
                |pane_id: &str| pane_id.starts_with("remote:") || live_pane_ids.contains(pane_id);
            self.terminal_sessions.retain(|pane_id, _| keep(pane_id));
            self.terminal_session_generations
                .retain(|pane_id, _| keep(pane_id));
            self.terminal_session_lifecycles
                .retain(|pane_id, _| keep(pane_id));
            self.terminal_sizes.retain(|pane_id, _| keep(pane_id));
        }
        let mut excluded = Vec::new();
        let catalog_changed = fetched
            .as_ref()
            .map(|payload| self.reconcile_session_catalog(payload, precomputed))
            .unwrap_or(false);
        let (state, message, agents, layout, selection_changed) = match fetched {
            Ok(payload) => {
                self.consume_restore_hint(&payload);
                let focused_checkout = self
                    .snapshot
                    .navigator
                    .focused_checkout_id
                    .as_deref()
                    .and_then(|focused_checkout_id| {
                        self.snapshot
                            .navigator
                            .workspaces
                            .iter()
                            .flat_map(|workspace| workspace.checkouts.iter())
                            .find(|checkout| checkout.id == focused_checkout_id)
                    });
                let focused_checkout_pane_ids = focused_checkout
                    .map(|checkout| {
                        checkout
                            .tabs
                            .iter()
                            .flat_map(|tab| tab.panes.iter())
                            .map(|pane| pane.id.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let focused_checkout_pane_set = focused_checkout_pane_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<HashSet<_>>();
                // A `remote:` id names a pane on another machine, which this
                // local session can never hold, so it is not a local selection
                // that has gone missing. Reading it as one is what left the
                // shell stuck on "Selected pane remote:...:pane:w59:p2 is not
                // available for the selected checkout" after a trip to a
                // remote device and back: every local sync tick compared the
                // leftover remote id against local layouts, never matched, and
                // re-raised the same error. Dropping it here means no path
                // that leaves a remote id in the selection can poison local
                // projection, rather than fixing the one navigation that did.
                let selected_pane_id = self
                    .snapshot
                    .terminal
                    .pane_id
                    .clone()
                    .or_else(|| self.snapshot.ui_state.selected_pane_id.clone())
                    .filter(|pane_id| !pane_id.starts_with("remote:"));
                let selected_still_exists = selected_pane_id.as_deref().is_some_and(|pane_id| {
                    payload
                        .layouts
                        .iter()
                        .any(|layout| layout.panes.iter().any(|pane| pane.pane_id == pane_id))
                });
                let selected_pane_missing = selected_pane_id.is_some() && !selected_still_exists;
                let selected_was_projected = previously_projected_in_focused_checkout
                    && selected_pane_id.as_deref() == previously_projected_pane.as_deref();
                let selected_left_focused_checkout =
                    selected_pane_id.as_deref().is_some_and(|pane_id| {
                        focused_checkout.is_some() && !focused_checkout_pane_set.contains(pane_id)
                    });
                // A rendered pane that leaves the selected checkout has
                // completed an expected lifecycle transition. Retarget only
                // inside that checkout, or leave it empty. A pane that was
                // never rendered is still a pending or invalid selection and
                // keeps the explicit projection error below.
                let selected_pane_retired = selected_was_projected
                    && (selected_pane_missing || selected_left_focused_checkout);
                let replacement_pane_id = selected_pane_retired
                    .then(|| {
                        previously_projected_tab
                            .as_deref()
                            .and_then(|tab_id| {
                                payload
                                    .layouts
                                    .iter()
                                    .find(|layout| layout.tab_id == tab_id)
                                    .map(|layout| layout.focused_pane_id.as_str())
                            })
                            .filter(|pane_id| focused_checkout_pane_set.contains(*pane_id))
                            .or_else(|| {
                                payload
                                    .focused_pane_id
                                    .as_deref()
                                    .filter(|pane_id| focused_checkout_pane_set.contains(*pane_id))
                            })
                            .or_else(|| focused_checkout_pane_ids.first().map(String::as_str))
                            .map(str::to_owned)
                    })
                    .flatten();
                let explicit_checkout_missing =
                    self.snapshot.ui_state.focused_checkout_id.is_some()
                        && focused_checkout.is_none();
                // Once the user or persisted state chooses a pane, a session
                // snapshot that omits that workspace must not silently retarget
                // commands to Herdr's unrelated globally focused workspace.
                let target_pane_id = if selected_pane_retired {
                    replacement_pane_id.as_deref()
                } else if selected_pane_missing || explicit_checkout_missing {
                    None
                } else if focused_checkout.is_some() {
                    selected_pane_id
                        .as_deref()
                        .filter(|pane_id| {
                            selected_still_exists && focused_checkout_pane_set.contains(*pane_id)
                        })
                        .or_else(|| {
                            focused_checkout_pane_ids
                                .iter()
                                .find(|pane_id| {
                                    payload.layouts.iter().any(|layout| {
                                        layout
                                            .panes
                                            .iter()
                                            .any(|pane| pane.pane_id == pane_id.as_str())
                                    })
                                })
                                .map(|pane_id| pane_id.as_str())
                        })
                } else if selected_pane_id.is_some() && !selected_still_exists {
                    None
                } else {
                    selected_pane_id
                        .as_deref()
                        .or(payload.focused_pane_id.as_deref())
                        .or_else(|| {
                            payload
                                .layouts
                                .first()
                                .map(|layout| layout.focused_pane_id.as_str())
                        })
                };
                let selected_pane_invalid_for_context = selected_pane_id.is_some()
                    && ((focused_checkout.is_some() && target_pane_id.is_none())
                        || (focused_checkout.is_none()
                            && self.snapshot.ui_state.focused_checkout_id.is_some()));
                let mut selection_changed = false;
                if selected_pane_retired {
                    let retired_pane_id = selected_pane_id.as_deref().unwrap_or("<missing>");
                    self.clear_terminal_projection();
                    self.snapshot.terminal.pane_id = replacement_pane_id.clone();
                    self.snapshot.focused.pane_id = replacement_pane_id.clone();
                    self.snapshot.ui_state.selected_pane_id = replacement_pane_id.clone();
                    if self
                        .snapshot
                        .status
                        .last_error
                        .as_ref()
                        .is_some_and(|error| error.kind == "pane.projection_unavailable")
                    {
                        self.snapshot.status.last_error = None;
                    }
                    let message = replacement_pane_id.as_deref().map_or_else(
                        || {
                            format!(
                                "Retired pane {retired_pane_id}; the selected checkout is now empty"
                            )
                        },
                        |replacement| {
                            format!("Retired pane {retired_pane_id}; selected {replacement}")
                        },
                    );
                    self.push_diagnostic("pane.selection_retired", message);
                    selection_changed = true;
                } else if selected_pane_missing || selected_pane_invalid_for_context {
                    let pane_id = selected_pane_id.as_deref().unwrap_or("<missing>");
                    self.clear_terminal_projection();
                    self.set_error(
                        "pane.projection_unavailable",
                        format!(
                            "Selected pane {pane_id} is not available for the selected checkout; terminal projection is waiting"
                        ),
                        true,
                    );
                }
                let layout = target_pane_id
                    .map(|pane_id| live::project_layout_for_pane(&payload, pane_id))
                    .transpose();
                let projection = project_agents(payload);
                excluded = projection.excluded;
                match layout {
                    Ok(layout) => (
                        "connected",
                        None,
                        Some(projection.agents),
                        layout,
                        selection_changed,
                    ),
                    Err(projection_error) => (
                        "malformed",
                        Some(format!(
                            "Herdr pane layout could not be projected: {projection_error}"
                        )),
                        None,
                        None,
                        selection_changed,
                    ),
                }
            }
            Err(error) => (
                error.state(),
                Some(error.message().to_owned()),
                None,
                None,
                false,
            ),
        };

        // A single unreadable agent record excludes only itself; the
        // exclusion is stated rather than silently folded into the count.
        for exclusion in &excluded {
            let pane_id = exclusion.pane_id.as_deref().unwrap_or("<missing pane id>");
            eprintln!(
                "{}",
                serde_json::json!({
                    "component": "session",
                    "kind": "agent.excluded",
                    "pane_id": pane_id,
                    "source_index": exclusion.source_index,
                    "message": exclusion.reason,
                })
            );
            self.push_diagnostic(
                "agent.excluded",
                format!("Agent {pane_id} was excluded: {}", exclusion.reason),
            );
        }

        let mut changed = catalog_changed || selection_changed || !excluded.is_empty();
        if self.snapshot.status.herdr.state != state
            || self.snapshot.status.herdr.message.as_deref() != message.as_deref()
        {
            self.snapshot.status.herdr.state = state.to_owned();
            self.snapshot.status.herdr.message = message;
            changed = true;
        }
        self.snapshot.status.herdr.last_checked_at_unix_ms = Some(unix_milliseconds());
        if let Some(mut agents) = agents {
            self.place_agents_in_navigator(&mut agents);
            if self.snapshot.navigator.agents != agents {
                self.snapshot.navigator.agents = agents;
                changed = true;
            }
        }
        if let Some(layout) = layout {
            if self.snapshot.terminal.pane_id.is_none() {
                let pane_id = layout.focused_pane_id.clone();
                self.snapshot.terminal.pane_id = Some(pane_id.clone());
                self.snapshot.focused.pane_id = Some(pane_id);
            }
            changed |= self.apply_pane_layout(layout);
        }
        changed | self.refresh_pet()
    }

    /// Applies provider usage that the session-sync coordinator read outside
    /// the runtime mutex. The two fixed rows are revisioned with the rest
    /// snapshot, so an unchanged refresh produces no shell work.
    /// What the changes reader should describe right now, or `None` when the
    /// changes view is not showing and nothing should be read at all. This is
    /// the whole reason the reader never forks `git` on a per-tick path.
    pub fn changes_request(&self) -> Option<crate::changes::ChangesRequest> {
        if !self.snapshot.ui_state.right_panel_visible
            || self.snapshot.ui_state.right_panel_section != RightPanelSection::Changes
        {
            return None;
        }
        let root_path = self.snapshot.navigator.root_path.as_ref()?;
        Some(crate::changes::ChangesRequest {
            root_path: PathBuf::from(root_path),
            selected_path: self.snapshot.changes.selected_path.clone(),
        })
    }

    /// Accepts a projection only while it still describes the checkout the
    /// runtime is asking about, so a slow read against a checkout the operator
    /// has already left cannot overwrite the current one.
    pub fn ingest_changes(&mut self, changes: crate::model::ChangesSnapshot) -> bool {
        let expected = self
            .changes_request()
            .map(|request| request.root_path.to_string_lossy().into_owned());
        if expected != changes.root_path {
            return false;
        }
        if self.snapshot.changes == changes {
            return false;
        }
        self.snapshot.changes = changes;
        true
    }

    pub fn ingest_provider_usage(
        &mut self,
        provider_usage: Vec<crate::model::ProviderUsageSnapshot>,
    ) -> bool {
        if self.snapshot.navigator.provider_usage == provider_usage {
            return false;
        }
        self.snapshot.navigator.provider_usage = provider_usage;
        true
    }

    /// Recomputes the pet's pose, badge row, and attention queue from the
    /// current agent list and connection state. Idempotent: the same inputs
    /// produce the same snapshot and report no change.
    fn refresh_pet(&mut self) -> bool {
        let now = unix_milliseconds();
        let connected = self.snapshot.status.herdr.state == "connected";
        let agents = &self.snapshot.navigator.agents;
        let summary = pet::summarize(agents, connected);
        if summary.error + summary.attention + summary.working > 0 {
            self.pet_active_at_unix_ms = now;
        }
        let idle_ms = now.saturating_sub(self.pet_active_at_unix_ms);
        let waking = now < self.pet_waking_until_unix_ms;
        let ambient = pet::ambient_totals(agents, connected);
        let attention_pane_ids = if connected {
            pet::observe_unseen(&mut self.pet_unseen_observed, agents, now);
            pet::attention_order(&self.snapshot.navigator.agents, &self.pet_unseen_observed)
        } else {
            Vec::new()
        };

        let next = PetSnapshot {
            visible: self.snapshot.ui_state.pet_visible,
            connection: self.snapshot.status.herdr.state.clone(),
            connection_message: self.snapshot.status.herdr.message.clone(),
            pose: pet::pose(summary, idle_ms, waking, connected).to_owned(),
            sleep_phase: pet::sleep_phase_for_idle_ms(idle_ms).as_str().to_owned(),
            roam_allowed: connected && pet::is_roam_allowed(summary, idle_ms, self.pet_dragging),
            badges: PetBadgesSnapshot {
                working: summary.working,
                done: summary.done,
                attention: summary.attention,
                error: summary.error,
                disconnected: summary.disconnected,
                subagents_active: ambient.subagents_active,
                background_running: ambient.background_running,
                background_failed: ambient.background_failed,
            },
            attention_pane_ids,
            origin: self.snapshot.ui_state.pet_origin,
            shortcut: self.snapshot.ui_state.pet_shortcut.clone(),
            shortcut_error: self.snapshot.pet.shortcut_error.clone(),
            theme_id: self.snapshot.pet.theme_id.clone(),
        };
        if self.snapshot.pet == next {
            return false;
        }
        self.snapshot.pet = next;
        true
    }

    /// Applying the same visibility twice converges instead of flapping, so
    /// four surfaces sharing one state can all set it freely.
    fn set_pet_visible(&mut self, visible: bool) -> bool {
        if self.snapshot.ui_state.pet_visible == visible {
            return false;
        }
        self.snapshot.ui_state.pet_visible = visible;
        self.persist_ui_state();
        self.note_pet_activity();
        self.refresh_pet();
        true
    }

    /// Pointer or toggle activity wakes a sleeping pet before the normal
    /// priority resumes.
    fn note_pet_activity(&mut self) {
        let now = unix_milliseconds();
        let idle_ms = now.saturating_sub(self.pet_active_at_unix_ms);
        if pet::sleep_phase_for_idle_ms(idle_ms) != pet::SleepPhase::Awake {
            self.pet_waking_until_unix_ms = now.saturating_add(PET_WAKING_MS);
        }
        self.pet_active_at_unix_ms = now;
    }

    fn apply_persisted_pet_state(&mut self) {
        self.snapshot.pet.visible = self.snapshot.ui_state.pet_visible;
        self.snapshot.pet.origin = self.snapshot.ui_state.pet_origin;
        self.snapshot.pet.shortcut = self.snapshot.ui_state.pet_shortcut.clone();
    }

    /// Saves the current UI state and surfaces a write failure instead of
    /// dropping it.
    fn persist_ui_state(&mut self) {
        if let Err(message) = persistence::save(&self.state_path, &self.snapshot.ui_state) {
            self.set_error("ui_state.save_failed", message, true);
        }
    }

    fn focus_pane(&mut self, pane_id: String) {
        let Some(context) = self.live.as_ref().cloned() else {
            self.set_error(
                "pane.control_unavailable",
                "Pane focus requires a live Herdr connection",
                true,
            );
            return;
        };
        self.push_diagnostic("pane.focus.requested", format!("Focusing pane {pane_id}"));
        if let Err(message) =
            live::spawn_pane_control(context, PaneControlAction::Focus { pane_id })
        {
            self.set_error("pane.focus_worker_failed", message, true);
        }
    }

    /// Puts a pane back on screen after its close did not happen.
    ///
    /// The projection removed it optimistically, and a close that fails
    /// produces no Herdr event, so nothing else would ever contradict that
    /// removal. `Project` asks Herdr for the authoritative layout holding this
    /// pane, which is the same request checkout navigation already uses, so
    /// the correction arrives through the existing projection path rather than
    /// through a saved copy of what the layout used to be.
    fn restore_projection_after_failed_close(&mut self, pane_id: &str) {
        let Some(context) = self.live.as_ref().cloned() else { return };
        if let Err(message) = live::spawn_pane_control(
            context,
            PaneControlAction::Project {
                pane_id: pane_id.to_owned(),
            },
        ) {
            self.push_diagnostic(
                "pane.close_restore_failed",
                format!("Could not re-project pane {pane_id} after a failed close: {message}"),
            );
        }
    }

    fn apply_pane_layout(&mut self, layout: PaneLayoutSnapshot) -> bool {
        let pane_ids = layout
            .pane_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let layout_changed = self.snapshot.pane_layout.as_ref() != Some(&layout);

        let previous = self
            .snapshot
            .terminal
            .panes
            .drain(..)
            .map(|pane| (pane.pane_id.clone(), pane))
            .collect::<HashMap<_, _>>();
        let mut terminal_panes = pane_ids
            .iter()
            .map(|pane_id| {
                previous
                    .get(pane_id)
                    .cloned()
                    .unwrap_or_else(|| self.terminal_pane_snapshot(pane_id))
            })
            .collect::<Vec<_>>();
        let mut remote_panes = previous
            .into_values()
            .filter(|pane| pane.pane_id.starts_with("remote:"))
            .collect::<Vec<_>>();
        remote_panes.sort_by(|left, right| left.pane_id.cmp(&right.pane_id));
        terminal_panes.extend(remote_panes);
        self.snapshot.terminal.panes = terminal_panes;

        // Herdr owns focus and input routing. The shell never keeps a second,
        // hover- or click-local focus value alongside the authoritative layout.
        self.snapshot.terminal.pane_id = Some(layout.focused_pane_id.clone());
        self.snapshot.focused.pane_id = Some(layout.focused_pane_id.clone());
        self.snapshot.zoomed = layout.zoomed.then(|| layout.focused_pane_id.clone());
        self.snapshot.pane_layout = Some(layout);
        if self
            .snapshot
            .status
            .last_error
            .as_ref()
            .is_some_and(|error| error.kind == "pane.projection_unavailable")
        {
            self.snapshot.status.last_error = None;
        }
        self.sync_focused_terminal_projection();

        if self.live.is_some() {
            for pane_id in pane_ids {
                self.request_terminal_control(&pane_id);
            }
        }
        layout_changed
    }

    fn ensure_terminal_pane(&mut self, pane_id: &str) {
        if self
            .snapshot
            .terminal
            .panes
            .iter()
            .any(|pane| pane.pane_id == pane_id)
        {
            return;
        }
        let pane = self.terminal_pane_snapshot(pane_id);
        self.snapshot.terminal.panes.push(pane);
    }

    fn terminal_pane_snapshot(&self, pane_id: &str) -> TerminalPaneSnapshot {
        let lifecycle = self
            .terminal_session_lifecycles
            .get(pane_id)
            .cloned()
            .unwrap_or_default();
        TerminalPaneSnapshot {
            pane_id: pane_id.to_owned(),
            closed: false,
            exit_code: None,
            transport_state: lifecycle.state.to_owned(),
            transport_message: lifecycle.message,
            transport_generation: lifecycle.generation,
            transport_attempt: lifecycle.attempt,
            transport_exit_category: lifecycle.exit_category,
            transport_retry_decision: lifecycle.retry_decision.to_owned(),
        }
    }

    fn sync_transport_projection(&mut self, pane_id: &str) {
        let Some(lifecycle) = self.terminal_session_lifecycles.get(pane_id).cloned() else {
            return;
        };
        if let Some(pane) = self
            .snapshot
            .terminal
            .panes
            .iter_mut()
            .find(|pane| pane.pane_id == pane_id)
        {
            pane.transport_state = lifecycle.state.to_owned();
            pane.transport_message = lifecycle.message;
            pane.transport_generation = lifecycle.generation;
            pane.transport_attempt = lifecycle.attempt;
            pane.transport_exit_category = lifecycle.exit_category;
            pane.transport_retry_decision = lifecycle.retry_decision.to_owned();
        }
    }

    fn sync_focused_terminal_projection(&mut self) {
        let Some(pane_id) = self.snapshot.terminal.pane_id.as_deref() else {
            self.snapshot.terminal.closed = false;
            self.snapshot.terminal.exit_code = None;
            return;
        };
        if let Some(pane) = self
            .snapshot
            .terminal
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
        {
            self.snapshot.terminal.closed = pane.closed;
            self.snapshot.terminal.exit_code = pane.exit_code;
        }
    }

    /// Applies a background pane-control result to the owner-thread snapshot.
    /// The child process is never waited on while the Swift caller holds the
    /// runtime lock; completion arrives through the normal change callback.
    /// Records the outcome of a fork worker.
    ///
    /// `herdr agent new` creates the pane and starts the agent in one atomic
    /// call, so a failure leaves nothing behind and there is no half-made pane
    /// to clean up. The reason it failed is reported rather than swallowed.
    /// Records the machine's listeners and re-attributes every pane to them.
    ///
    /// Panes are re-walked here because ports arrive on their own window rather
    /// than with a session snapshot, so a server that started since the last
    /// topology update would otherwise stay invisible until the topology moved.
    pub fn ingest_listening_ports(&mut self, ports: crate::model::ListeningPortsSnapshot) -> bool {
        if self.listening_ports == ports {
            return false;
        }
        self.listening_ports = ports;
        let entries = self.listening_ports.entries.clone();
        let mut changed = false;
        for workspace in self.snapshot.navigator.workspaces.iter_mut() {
            for checkout in workspace.checkouts.iter_mut() {
                for tab in checkout.tabs.iter_mut() {
                    for pane in tab.panes.iter_mut() {
                        let attributed = crate::ports::attributed_ports(&pane.cwd, &entries);
                        if pane.ports != attributed {
                            pane.ports = attributed;
                            changed = true;
                        }
                    }
                }
            }
        }
        changed
    }

    /// Stores what a pane search found and moves the viewport to the match.
    ///
    /// The scroll goes through the same terminal-control write the wheel uses,
    /// because Herdr owns the pane's history and answers a viewport move with a
    /// fresh frame either way.
    pub fn ingest_pane_find(
        &mut self,
        pane_id: &str,
        result: Result<live::PaneFindOutcome, String>,
    ) -> bool {
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(message) => {
                self.snapshot.find = PaneFindSnapshot {
                    pane_id: Some(pane_id.to_owned()),
                    unavailable_reason: Some(message),
                    ..PaneFindSnapshot::default()
                };
                return true;
            }
        };
        let next = PaneFindSnapshot {
            pane_id: Some(pane_id.to_owned()),
            term: outcome.term,
            index: outcome.index,
            total: outcome.total,
            truncated: outcome.truncated,
            unavailable_reason: None,
        };
        let changed = self.snapshot.find != next;
        self.snapshot.find = next;
        if let Some((direction, lines)) = outcome.scroll {
            let (rows, cols) = self
                .terminal_sizes
                .get(pane_id)
                .copied()
                .unwrap_or((24, 80));
            if let Some(session) = self.terminal_sessions.get_mut(pane_id)
                && session.mode == TerminalSessionMode::Control
                && let Err(message) = session.scroll(&direction, lines, rows, cols)
            {
                self.set_error("terminal.scroll_failed", message, true);
                return true;
            }
        }
        changed
    }

    pub fn ingest_fork_result(
        &mut self,
        parent_pane_id: &str,
        result: Result<String, String>,
        elapsed_ms: u128,
    ) -> bool {
        self.forks_in_flight.remove(parent_pane_id);
        match result {
            Ok(forked_pane_id) => {
                self.push_diagnostic(
                    "pane.fork.created",
                    format!(
                        "Forked pane {parent_pane_id} into {forked_pane_id} in {elapsed_ms}ms"
                    ),
                );
                true
            }
            Err(message) => {
                self.set_error(
                    "pane.fork_failed",
                    format!("Pane {parent_pane_id} could not be forked: {message}"),
                    true,
                );
                true
            }
        }
    }

    pub fn ingest_pane_control_result(
        &mut self,
        action: PaneControlAction,
        result: Result<PaneControlOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        match (action, result) {
            (
                PaneControlAction::Project { pane_id },
                Ok(PaneControlOutcome::Projected { layout }),
            ) => {
                if self.snapshot.terminal.pane_id.as_deref() != Some(pane_id.as_str()) {
                    self.push_diagnostic(
                        "pane.projection.stale",
                        format!("Ignored stale projection for pane {pane_id}"),
                    );
                    return false;
                }
                if !layout.pane_ids().contains(&pane_id.as_str()) {
                    self.set_error(
                        "pane.projection_mismatch",
                        format!("Projected layout does not contain pane {pane_id}"),
                        true,
                    );
                    return true;
                }
                self.push_diagnostic(
                    "pane.projection.ready",
                    format!("Pane {pane_id} projected in {elapsed_ms} ms"),
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "pane_projection",
                        "kind": "pane.projection_ready",
                        "pane_id": pane_id,
                        "duration_ms": elapsed_ms,
                    })
                );
                self.apply_pane_layout(layout);
                true
            }
            (PaneControlAction::Focus { pane_id }, Ok(PaneControlOutcome::Acknowledged { .. })) => {
                self.push_diagnostic(
                    "pane.focus",
                    format!(
                        "Pane {pane_id} focus acknowledged in {elapsed_ms} ms; awaiting authoritative event"
                    ),
                );
                true
            }
            (
                PaneControlAction::Split {
                    pane_id, direction, ..
                },
                Ok(PaneControlOutcome::Acknowledged { created_pane_id }),
            ) => {
                let Some(created_pane_id) = created_pane_id else {
                    self.set_error(
                        "pane.split_invalid_response",
                        "Pane split completed without a created pane id",
                        true,
                    );
                    return true;
                };
                self.push_diagnostic(
                    format!("pane.split.{}", direction.as_str()),
                    format!(
                        "Pane {pane_id} split {} to {created_pane_id} in {elapsed_ms} ms",
                        direction.as_str()
                    ),
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "pane_control",
                        "kind": "pane.split_ready",
                        "pane_id": pane_id,
                        "created_pane_id": created_pane_id,
                        "direction": direction.as_str(),
                        "duration_ms": elapsed_ms,
                    })
                );
                true
            }
            (
                PaneControlAction::Resize {
                    pane_id,
                    direction,
                    amount,
                },
                Ok(PaneControlOutcome::Acknowledged { .. }),
            ) => {
                self.push_diagnostic(
                    "pane.resize",
                    format!(
                        "Pane {pane_id} resize {} by {amount:.3} acknowledged in {elapsed_ms} ms; awaiting authoritative event",
                        direction.as_str()
                    ),
                );
                true
            }
            (
                PaneControlAction::ToggleZoom { pane_id },
                Ok(PaneControlOutcome::Acknowledged { .. }),
            ) => {
                self.push_diagnostic(
                    "pane.zoom_toggled",
                    format!(
                        "Pane {pane_id} zoom acknowledged in {elapsed_ms} ms; awaiting authoritative event"
                    ),
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "pane_control",
                        "kind": "pane.zoom_ready",
                        "pane_id": pane_id,
                        "duration_ms": elapsed_ms,
                    })
                );
                true
            }
            (PaneControlAction::Close { pane_id }, Ok(PaneControlOutcome::Acknowledged { .. })) => {
                self.push_diagnostic(
                    "pane.close",
                    format!(
                        "Pane {pane_id} close acknowledged in {elapsed_ms} ms; awaiting authoritative event"
                    ),
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "pane_control",
                        "kind": "pane.close_ready",
                        "pane_id": pane_id,
                        "duration_ms": elapsed_ms,
                    })
                );
                true
            }
            (PaneControlAction::Project { .. }, Ok(PaneControlOutcome::Acknowledged { .. }))
            | (
                PaneControlAction::Focus { .. }
                | PaneControlAction::Split { .. }
                | PaneControlAction::Resize { .. }
                | PaneControlAction::ToggleZoom { .. }
                | PaneControlAction::Close { .. },
                Ok(PaneControlOutcome::Projected { .. }),
            ) => {
                self.set_error(
                    "pane.control_invalid_outcome",
                    "Pane control returned an outcome for the wrong operation class",
                    false,
                );
                true
            }
            (PaneControlAction::Project { .. }, Err(message)) => {
                self.set_error("pane.projection_failed", message, true);
                true
            }
            (PaneControlAction::Focus { .. }, Err(message)) => {
                self.set_error("pane.focus_failed", message, true);
                true
            }
            (PaneControlAction::Split { .. }, Err(message)) => {
                self.set_error("pane.split_failed", message, true);
                true
            }
            (PaneControlAction::Resize { .. }, Err(message)) => {
                self.set_error("pane.resize_failed", message, true);
                true
            }
            (PaneControlAction::ToggleZoom { .. }, Err(message)) => {
                self.set_error("pane.zoom_failed", message, true);
                true
            }
            (PaneControlAction::Close { pane_id }, Err(message)) => {
                self.restore_projection_after_failed_close(&pane_id);
                self.set_error("pane.close_failed", message, true);
                true
            }
        }
    }

    pub fn ingest_remote_control_result(
        &mut self,
        target_id: &str,
        request_id: &str,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        let action_kind = action.kind();
        if let Some(key) = remote_tab_creation_key(target_id, &action) {
            self.remote_tab_creations_in_flight.remove(&key);
        }
        match result {
            Ok(RemoteControlOutcome::Acknowledged {
                created_tab_id,
                created_pane_id,
            }) => {
                let mut receipt = String::new();
                if let Some(tab_id) = created_tab_id.as_deref() {
                    receipt.push_str(&format!("; created tab {tab_id}"));
                }
                if let Some(pane_id) = created_pane_id.as_deref() {
                    receipt.push_str(&format!("; created pane {pane_id}"));
                }
                self.push_diagnostic(
                    "remote.control.ready",
                    format!(
                        "{action_kind} for {target_id} acknowledged in {elapsed_ms} ms{receipt}; awaiting authoritative event"
                    ),
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "remote_control",
                        "kind": "remote.control.ready",
                        "target": target_id,
                        "request_id": request_id,
                        "action": action_kind,
                        "created_tab_id": created_tab_id,
                        "created_pane_id": created_pane_id,
                        "duration_ms": elapsed_ms,
                    })
                );
            }
            Err(message) => {
                self.set_error(
                    "remote.control.failed",
                    format!("{action_kind} for {target_id} failed: {message}"),
                    true,
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "remote_control",
                        "kind": "remote.control.failed",
                        "target": target_id,
                        "request_id": request_id,
                        "action": action_kind,
                        "message": message,
                        "duration_ms": elapsed_ms,
                    })
                );
            }
        }
        true
    }

    pub fn ingest_local_control_result(
        &mut self,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        let action_kind = action.kind();
        match result {
            Ok(RemoteControlOutcome::Acknowledged {
                created_tab_id,
                created_pane_id,
            }) => {
                if matches!(action, RemoteControlAction::CreateTab { .. })
                    && let Some(pane_id) = created_pane_id.as_ref()
                {
                    // tab.create returns the authoritative root pane before
                    // the ordered event projection catches up. Preserve that
                    // focus intent so the next snapshot cannot retain the old
                    // tab merely because its pane still exists.
                    self.snapshot.terminal.pane_id = Some(pane_id.clone());
                    self.snapshot.focused.surface = Surface::Terminal;
                    self.snapshot.focused.pane_id = Some(pane_id.clone());
                    self.snapshot.ui_state.selected_pane_id = Some(pane_id.clone());
                    self.deactivate_file_tab();
                    self.persist_current_ui_state();
                }
                self.push_diagnostic(
                    "tab.control.ready",
                    format!(
                        "{action_kind} acknowledged in {elapsed_ms} ms; awaiting authoritative Herdr event"
                    ),
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "tab_control",
                        "kind": "tab.control.ready",
                        "action": action_kind,
                        "created_tab_id": created_tab_id,
                        "created_pane_id": created_pane_id,
                        "duration_ms": elapsed_ms,
                    })
                );
            }
            Err(message) => {
                self.set_error(
                    "tab.control.failed",
                    format!("{action_kind} failed: {message}"),
                    true,
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "tab_control",
                        "kind": "tab.control.failed",
                        "action": action_kind,
                        "message": message,
                        "duration_ms": elapsed_ms,
                    })
                );
            }
        }
        true
    }

    /// Appends only the decoded frame bytes when the delivering official
    /// terminal session is still the current generation and mode.
    pub fn ingest_terminal_session_frame(
        &mut self,
        pane_id: &str,
        generation: u64,
        mode: TerminalSessionMode,
        bytes: &[u8],
    ) -> bool {
        if self.terminal_session_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        if self
            .terminal_sessions
            .get(pane_id)
            .is_none_or(|session| session.mode != mode)
        {
            return false;
        }
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(bytes));
        true
    }

    /// Handles a `terminal.closed` envelope or stdout EOF. An owner conflict
    /// falls back exactly once to Herdr's concurrent read-only observer; every
    /// other close ends only the transport, never the authoritative pane.
    pub fn ingest_terminal_session_closed(
        &mut self,
        pane_id: &str,
        generation: u64,
        mode: TerminalSessionMode,
        reason: Option<String>,
    ) -> bool {
        if self.terminal_session_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        if self
            .terminal_sessions
            .get(pane_id)
            .is_none_or(|session| session.mode != mode)
        {
            return false;
        }
        let _ended_session = self.terminal_sessions.remove(pane_id);
        let attempt = self
            .terminal_session_lifecycles
            .get(pane_id)
            .map_or(1, |lifecycle| lifecycle.attempt);
        let category = live::terminal_closed_category(reason.as_deref());
        let message = reason
            .unwrap_or_else(|| format!("Pane {pane_id} terminal {} session ended", mode.as_str()));

        if mode == TerminalSessionMode::Control && category == "owner_conflict" {
            eprintln!(
                "{}",
                serde_json::json!({
                    "component": "terminal_session",
                    "kind": "terminal.control_owner_conflict",
                    "pane_id": pane_id,
                    "generation": generation,
                    "attempt": attempt,
                    "mode": mode.as_str(),
                    "duration_ms": 0,
                    "exit_category": category,
                    "retry_decision": "observe_once",
                })
            );
            self.start_terminal_session(
                pane_id,
                TerminalSessionMode::Observe,
                attempt,
                "observe_once",
                Some(format!(
                    "Another client owns terminal control. Viewing {pane_id} read-only; use Reconnect to try control again."
                )),
            );
            return true;
        }

        self.terminal_session_lifecycles.insert(
            pane_id.to_owned(),
            TerminalSessionLifecycle {
                state: "ended",
                message: Some(message.clone()),
                generation,
                attempt,
                mode: Some(mode),
                exit_category: Some(category.to_owned()),
                retry_decision: "manual",
            },
        );
        self.sync_transport_projection(pane_id);
        let notice = format!("\r\n[{message}]\r\n");
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(notice.as_bytes()));
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "terminal_session",
                "kind": "terminal.session_ended",
                "pane_id": pane_id,
                "generation": generation,
                "attempt": attempt,
                "mode": mode.as_str(),
                "duration_ms": 0,
                "exit_category": category,
                "retry_decision": "manual",
            })
        );
        true
    }

    pub fn ingest_terminal_session_write_failure(
        &mut self,
        pane_id: &str,
        generation: u64,
        message: String,
    ) -> bool {
        if self.terminal_session_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        self.set_error("terminal.write_failed", message.clone(), true);
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "terminal_session",
                "kind": "terminal.control_write_failed",
                "pane_id": pane_id,
                "generation": generation,
                "message": message,
                "retry_decision": "manual",
            })
        );
        true
    }

    fn push_diagnostic(&mut self, kind: impl Into<String>, message: impl Into<String>) {
        self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
            kind: kind.into(),
            message: message.into(),
            occurred_at: unix_milliseconds(),
        });
    }

    pub fn set_error(
        &mut self,
        kind: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
    ) {
        self.snapshot.status.last_error = Some(LastErrorSnapshot {
            kind: kind.into(),
            message: message.into(),
            retryable,
            occurred_at: unix_milliseconds(),
        });
    }

    /// Herdr closes a workspace with its last pane, and a project that exists
    /// only as that Herdr workspace would vanish from the sidebar with it.
    /// The user asked to close a pane, not to forget the project, so the
    /// project is registered at its repository path first. The path-keyed
    /// project id is unchanged by this, and the row stays selectable with
    /// its "start new terminal" control once Herdr's workspace is gone.
    fn retain_project_before_last_pane_closes(&mut self, pane_id: &str) {
        let Some(project) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| {
                workspace.remote_target_id.is_none()
                    && workspace
                        .checkouts
                        .iter()
                        .flat_map(|checkout| checkout.tabs.iter())
                        .flat_map(|tab| tab.panes.iter())
                        .any(|pane| pane.id == pane_id)
            })
        else {
            return;
        };
        let pane_count = project
            .checkouts
            .iter()
            .flat_map(|checkout| checkout.tabs.iter())
            .map(|tab| tab.panes.len())
            .sum::<usize>();
        if project.registered || pane_count != 1 {
            return;
        }
        let registration = match workspace::registration(
            &project.path,
            &project.repo_name,
            &project.device_id,
        ) {
            Ok(registration) => registration,
            Err(message) => {
                self.set_error("workspace.retain_failed", message, false);
                return;
            }
        };
        if self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .any(|existing| existing.id == registration.id)
        {
            return;
        }
        self.push_diagnostic(
            "workspace.retained",
            format!(
                "Registered {} at {} so closing its last pane keeps the project listed",
                registration.label, registration.path
            ),
        );
        self.snapshot
            .ui_state
            .workspace_registrations
            .push(registration);
        // The project id is path-keyed, so registering changes nothing the
        // sidebar shows right now; the sync that follows Herdr's
        // workspace_closed rebuilds the catalog off the runtime lock.
        self.persist_current_ui_state();
    }

    /// Names each agent by the project and checkout its pane sits in, as the
    /// sidebar tree shows them. Herdr's own workspace label ("hide main") is
    /// a launcher artifact that can span two repositories, so it is kept only
    /// for a pane the navigator has not placed.
    fn place_agents_in_navigator(&self, agents: &mut [SidebarAgentSnapshot]) {
        for agent in agents {
            let placed = self.snapshot.navigator.workspaces.iter().find_map(|workspace| {
                workspace.checkouts.iter().find_map(|checkout| {
                    checkout
                        .tabs
                        .iter()
                        .flat_map(|tab| tab.panes.iter())
                        .any(|pane| pane.id == agent.pane_id)
                        .then(|| (workspace.label.clone(), checkout.label.clone()))
                })
            });
            if let Some((workspace_label, checkout_label)) = placed {
                agent.workspace_label = workspace_label;
                agent.checkout_label = Some(checkout_label);
            }
        }
    }

    fn rebuild_catalog(&mut self) {
        let mut workspaces = workspace::build_catalog(
            &self.snapshot.ui_state.workspace_registrations,
            &self.last_session_spaces,
        );
        Self::apply_workspace_expansion(
            &mut workspaces,
            &self.snapshot.ui_state.collapsed_workspace_ids,
        );
        self.snapshot.navigator.workspaces = workspaces;
        self.snapshot.navigator.devices = workspace::devices(
            &self.remote_targets,
            &self.snapshot.ui_state.device_registrations,
        );
        self.resync_navigator_focus();
    }

    fn apply_workspace_expansion(
        workspaces: &mut [crate::model::WorkspaceSnapshot],
        collapsed_workspace_ids: &[String],
    ) {
        let collapsed = collapsed_workspace_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        for workspace in workspaces {
            workspace.expanded = !collapsed.contains(workspace.id.as_str());
        }
    }

    fn persist_current_ui_state(&mut self) {
        self.snapshot.ui_state.focused_device_id =
            self.snapshot.navigator.focused_device_id.clone();
        self.snapshot.ui_state.focused_checkout_id =
            self.snapshot.navigator.focused_checkout_id.clone();
        if let Err(message) = persistence::save(&self.state_path, &self.snapshot.ui_state) {
            self.set_error("ui_state.save_failed", message, true);
        }
    }

    /// Drops the previous checkout's rendered layout before selecting the
    /// next one. Herdr's globally focused pane may belong to another
    /// workspace, so retaining it here would let the next sync update redraw
    /// stale terminal content while the selected checkout has no pane yet.
    fn clear_terminal_projection(&mut self) {
        self.snapshot.pane_layout = None;
        self.snapshot.zoomed = None;
        self.snapshot.terminal.panes.clear();
        self.snapshot.terminal.closed = false;
        self.snapshot.terminal.exit_code = None;
    }

    fn reset_terminal_projection(&mut self, pane_id: Option<String>) {
        self.clear_terminal_projection();
        self.snapshot.terminal.pane_id = pane_id.clone();
        self.snapshot.focused.surface = Surface::Terminal;
        self.snapshot.focused.pane_id = pane_id.clone();
        self.snapshot.ui_state.selected_pane_id = pane_id;
        self.sync_focused_terminal_projection();
    }

    /// A launcher result is a local projection anchor, not a Herdr focus
    /// request. Keep it authoritative over an older terminal pane while the
    /// next event-stream projection catches up, and make the missing layout
    /// visible instead of retaining unrelated same-cwd content.
    fn apply_selected_pane_anchor(&mut self, pane_id: Option<String>) {
        let layout_contains_pane = pane_id.as_deref().is_some_and(|selected_pane_id| {
            self.snapshot.pane_layout.as_ref().is_some_and(|layout| {
                layout
                    .pane_ids()
                    .into_iter()
                    .any(|layout_pane_id| layout_pane_id == selected_pane_id)
            })
        });
        self.snapshot.terminal.pane_id = pane_id.clone();
        self.snapshot.focused.surface = Surface::Terminal;
        self.snapshot.focused.pane_id = pane_id.clone();
        if !layout_contains_pane {
            self.clear_terminal_projection();
            if let Some(pane_id) = pane_id.as_deref() {
                self.set_error(
                    "pane.projection_unavailable",
                    format!(
                        "Selected pane {pane_id} is not present in the Herdr session; terminal projection is waiting"
                    ),
                    true,
                );
            }
        }
        self.sync_focused_terminal_projection();
    }

    fn focus_checkout(&mut self, workspace_id: &str, checkout_id: &str) -> bool {
        let Some((checkout_path, next_pane_id, has_herdr_tab)) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .and_then(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == checkout_id)
                    .map(|checkout| {
                        (
                            checkout.path.clone(),
                            checkout
                                .tabs
                                .iter()
                                .flat_map(|tab| tab.panes.iter())
                                .map(|pane| pane.id.clone())
                                .next(),
                            !checkout.tabs.is_empty(),
                        )
                    })
            })
        else {
            let workspace_exists = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .any(|workspace| workspace.id == workspace_id);
            let (kind, message) = if workspace_exists {
                (
                    "checkout.unknown",
                    format!("Checkout {checkout_id} is not available in {workspace_id}"),
                )
            } else {
                (
                    "checkout.unknown_workspace",
                    format!("Workspace {workspace_id} is not registered"),
                )
            };
            self.set_error(kind, message, false);
            return true;
        };
        self.snapshot.navigator.focused_workspace_id = Some(workspace_id.to_owned());
        self.snapshot.navigator.focused_checkout_id = Some(checkout_id.to_owned());
        self.snapshot.navigator.root_path = Some(checkout_path);
        self.reset_terminal_projection(next_pane_id.clone());
        self.sync_active_tab_projection();
        self.deactivate_file_tab();
        if !has_herdr_tab
            && let Some(file_tab_id) = self
                .snapshot
                .editor
                .tabs
                .iter()
                .rev()
                .find(|tab| tab.workspace_id == workspace_id && tab.checkout_id == checkout_id)
                .map(|tab| tab.id.clone())
            && let Err(message) = self.activate_file_tab(&file_tab_id)
        {
            self.set_error("file.focus_failed", message, false);
        }
        self.persist_current_ui_state();
        if let (Some(context), Some(pane_id)) = (self.live.as_ref().cloned(), next_pane_id)
            && let Err(message) =
                live::spawn_pane_control(context, PaneControlAction::Project { pane_id })
        {
            self.set_error("pane.projection_worker_failed", message, true);
        }
        true
    }

    pub fn ingest_workspace_creation(
        &mut self,
        request_path: &str,
        result: Result<live::WorkspaceCreationOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        self.workspace_creations_in_flight.remove(request_path);
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(message) => {
                self.set_error("workspace.create_failed", message, false);
                return true;
            }
        };
        let catalog_inputs_match =
            self.snapshot.ui_state.workspace_registrations == outcome.base_registrations;
        if catalog_inputs_match {
            self.snapshot.ui_state.workspace_registrations = outcome.registrations;
        } else if !self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .any(|registration| registration.id == outcome.registration.id)
        {
            self.snapshot
                .ui_state
                .workspace_registrations
                .push(outcome.registration.clone());
            self.push_diagnostic(
                "workspace.catalog.refresh_pending",
                "Workspace registrations changed during creation; session sync will refresh the catalog",
            );
        }
        self.snapshot.navigator.focused_device_id = Some(workspace::LOCAL_DEVICE_ID.to_owned());
        self.reconcile_remote_terminal_selection();
        let target_checkout = outcome
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .find(|checkout| checkout.path == outcome.registration.path);
        if let Some(checkout) = target_checkout {
            self.snapshot.navigator.focused_checkout_id = Some(checkout.id.clone());
            self.snapshot.ui_state.focused_checkout_id = Some(checkout.id.clone());
        }
        let target_pane_id = outcome.created_pane_id.clone().or_else(|| {
            target_checkout
                .and_then(|checkout| checkout.tabs.first())
                .and_then(|tab| tab.panes.first())
                .map(|pane| pane.id.clone())
        });
        if catalog_inputs_match {
            self.reset_terminal_projection(target_pane_id);
            self.ingest_session_with_catalog(
                Ok(outcome.session),
                Some(session_sync::PrecomputedCatalog {
                    registrations: self.snapshot.ui_state.workspace_registrations.clone(),
                    workspaces: outcome.workspaces,
                }),
            );
        } else {
            self.push_diagnostic(
                "workspace.session.refresh_pending",
                "Workspace creation completed after registrations changed; session sync will publish the authoritative catalog",
            );
        }
        self.persist_current_ui_state();
        let git_init_failed = outcome.git_init_error.is_some();
        if let Some(message) = outcome.git_init_error.as_ref() {
            self.set_error(
                "workspace.git_init_failed",
                format!("Workspace was registered, but Git initialization failed: {message}"),
                true,
            );
        }
        self.push_diagnostic(
            "workspace.registered",
            format!(
                "Registered workspace {} in {elapsed_ms} ms",
                outcome.registration.path
            ),
        );
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "workspace",
                "kind": "workspace.registered",
                "path": outcome.registration.path,
                "duration_ms": elapsed_ms,
                    "git_init": if git_init_failed { "failed" } else { "complete_or_skipped" },
            })
        );
        true
    }

    pub fn dispatch_json(&mut self, bytes: &[u8]) -> bool {
        let event = match serde_json::from_slice::<EventEnvelope>(bytes) {
            Ok(event) => event,
            Err(_) => {
                self.set_error(
                    "event.invalid_json",
                    "Event JSON could not be decoded",
                    false,
                );
                return true;
            }
        };

        if event.schema_version != SCHEMA_VERSION {
            self.set_error(
                "schema_version.mismatch",
                format!(
                    "Event schema version {} does not match {}",
                    event.schema_version, SCHEMA_VERSION
                ),
                false,
            );
            return true;
        }

        let event = match validate_event(event) {
            Ok(event) => event,
            Err(error) => {
                self.set_error(error.kind, error.message, false);
                return true;
            }
        };

        let cleared_error = self.snapshot.status.last_error.take().is_some();
        self.apply(event) || cleared_error
    }

    fn apply(&mut self, event: ValidatedEvent) -> bool {
        match event {
            ValidatedEvent::Key(payload) => {
                self.snapshot.input_generation = self.snapshot.input_generation.saturating_add(1);
                self.snapshot.focused.surface = Surface::Terminal;
                self.snapshot.focused.pane_id = Some(payload.pane_id.clone());
                self.snapshot.terminal.pane_id = Some(payload.pane_id.clone());
                self.ensure_terminal_pane(&payload.pane_id);
                self.sync_focused_terminal_projection();
                if self.live.is_some()
                    || self.remote_terminals.keys().any(|target_id| {
                        remote_pane_source_id(target_id, &payload.pane_id).is_some()
                    })
                {
                    self.write_terminal_control(&payload.pane_id, &payload.bytes_base64);
                } else {
                    // Fixture mode has no PTY behind the pane; the loopback
                    // echo is the whole byte bridge.
                    self.append_terminal_chunk(payload.pane_id, payload.bytes_base64);
                }
                true
            }
            ValidatedEvent::TerminalOutput(payload) => {
                if self.snapshot.terminal.pane_id.is_none() {
                    self.snapshot.terminal.pane_id = Some(payload.pane_id.clone());
                    self.snapshot.focused.pane_id = Some(payload.pane_id.clone());
                }
                self.ensure_terminal_pane(&payload.pane_id);
                self.append_terminal_chunk(payload.pane_id, payload.bytes_base64);
                true
            }
            ValidatedEvent::SessionSnapshot(payload) => self.ingest_session(Ok(payload)),
            ValidatedEvent::PetSetVisible(payload) => self.set_pet_visible(payload.visible),
            ValidatedEvent::PetToggleVisible => {
                let visible = !self.snapshot.ui_state.pet_visible;
                self.set_pet_visible(visible)
            }
            ValidatedEvent::PetMove(payload) => {
                let origin = PetOriginSnapshot {
                    x: payload.x,
                    y: payload.y,
                };
                if self.snapshot.ui_state.pet_origin == Some(origin) {
                    return false;
                }
                self.snapshot.ui_state.pet_origin = Some(origin);
                self.persist_ui_state();
                self.refresh_pet();
                true
            }
            ValidatedEvent::PetDrag(payload) => {
                if self.pet_dragging == payload.dragging {
                    return false;
                }
                self.pet_dragging = payload.dragging;
                self.note_pet_activity();
                self.refresh_pet()
            }
            ValidatedEvent::PetActivity => {
                self.note_pet_activity();
                self.refresh_pet()
            }
            ValidatedEvent::PetShortcutUpdate(payload) => {
                let accelerator = payload
                    .accelerator
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty());
                let error = payload.error.filter(|value| !value.trim().is_empty());
                let unchanged = self.snapshot.ui_state.pet_shortcut == accelerator
                    && self.snapshot.pet.shortcut_error == error;
                if unchanged {
                    return false;
                }
                self.snapshot.ui_state.pet_shortcut = accelerator;
                self.snapshot.pet.shortcut_error = error;
                self.persist_ui_state();
                self.refresh_pet();
                true
            }
            ValidatedEvent::Click(payload) => {
                let _ = (payload.x, payload.y, payload.button, payload.click_count);
                self.snapshot.focused.surface = payload.surface;
                true
            }
            ValidatedEvent::FocusPane(payload) => {
                self.focus_pane(payload.pane_id);
                true
            }
            ValidatedEvent::ReconnectPane(payload) => {
                let pane_id = payload.pane_id;
                let pane_exists = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .flat_map(|tab| tab.panes.iter())
                    .any(|pane| pane.id == pane_id);
                if !pane_exists {
                    self.set_error(
                        "pane.reconnect_missing",
                        format!("Pane {pane_id} no longer exists"),
                        false,
                    );
                    return true;
                }
                let previous_attempt = self
                    .terminal_session_lifecycles
                    .get(&pane_id)
                    .map_or(0, |lifecycle| lifecycle.attempt);
                let _retired_session = self.terminal_sessions.remove(&pane_id);
                self.terminal_session_lifecycles.insert(
                    pane_id.clone(),
                    TerminalSessionLifecycle {
                        attempt: previous_attempt,
                        retry_decision: "manual",
                        ..TerminalSessionLifecycle::default()
                    },
                );
                self.push_diagnostic(
                    "pane.reconnect.requested",
                    format!("Reconnect requested for pane {pane_id}"),
                );
                self.request_terminal_control(&pane_id);
                true
            }
            ValidatedEvent::OpenBrowser(payload) => {
                self.snapshot.status.chromux.profile = payload.profile;
                let action = chromux::plan_open(&self.snapshot.status.chromux.profile, None, None);
                self.snapshot.status.chromux.state = "parked".to_owned();
                self.snapshot.status.chromux.message = Some(match action {
                    chromux::BrowserAction::Parked(message) => message,
                    _ => "Runtime execution is parked for this approved batch".to_owned(),
                });
                true
            }
            ValidatedEvent::BrowserStatus(payload) => {
                self.snapshot.status.chromux.state = payload.state;
                self.snapshot.status.chromux.profile = payload.profile;
                self.snapshot.status.chromux.current_url = payload.current_url;
                self.snapshot.status.chromux.current_title = payload.current_title;
                self.snapshot.status.chromux.message = payload.message;
                self.snapshot.status.chromux.last_checked_at_unix_ms =
                    Some(payload.last_checked_at_unix_ms);
                true
            }
            ValidatedEvent::RetryConnect(payload) => {
                if let Some(remote) = self
                    .snapshot
                    .status
                    .remote
                    .iter_mut()
                    .find(|remote| remote.target_id == payload.target_id)
                {
                    remote.state = "retry_requested".to_owned();
                    remote.message =
                        Some("Reconnect is waiting for the remote integration task".to_owned());
                    true
                } else {
                    self.set_error(
                        "remote.unknown_target",
                        "Reconnect target is not registered",
                        false,
                    );
                    true
                }
            }
            ValidatedEvent::CreateWorkspace(payload) => {
                if let Some(context) = self.live.as_ref().cloned() {
                    if !self
                        .workspace_creations_in_flight
                        .insert(payload.path.clone())
                    {
                        self.push_diagnostic(
                            "workspace.create.duplicate",
                            format!("Workspace creation is already running for {}", payload.path),
                        );
                        return false;
                    }
                    let path = payload.path.clone();
                    let result = live::spawn_workspace_creation(
                        context,
                        payload.path,
                        payload.label,
                        payload.initialize_git,
                        self.snapshot.ui_state.workspace_registrations.clone(),
                    );
                    if let Err(message) = result {
                        self.workspace_creations_in_flight.remove(&path);
                        self.set_error("workspace.create_worker_failed", message, true);
                    } else {
                        self.push_diagnostic(
                            "workspace.create.requested",
                            format!("Creating workspace from {path}"),
                        );
                    }
                    return true;
                }
                self.set_error(
                    "workspace.control_unavailable",
                    "Workspace creation requires a live Herdr connection so its initial tab and pane can be created",
                    true,
                );
                true
            }
            ValidatedEvent::CreateTab(payload) => {
                let Some(workspace_snapshot) = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == payload.workspace_id)
                else {
                    self.set_error(
                        "tab.unknown_workspace",
                        format!("Workspace {} is not registered", payload.workspace_id),
                        false,
                    );
                    return true;
                };
                let checkout = payload
                    .checkout_id
                    .as_deref()
                    .and_then(|checkout_id| {
                        workspace_snapshot
                            .checkouts
                            .iter()
                            .find(|checkout| checkout.id == checkout_id)
                    })
                    .or_else(|| workspace_snapshot.checkouts.first());
                let Some(checkout) = checkout else {
                    self.set_error("tab.no_checkout", "Workspace has no checkout", false);
                    return true;
                };
                let workspace_id = workspace_snapshot.id.clone();
                let checkout_id = checkout.id.clone();
                let cwd = checkout.path.clone();
                let label = payload.label.trim();
                if label.is_empty() {
                    self.set_error("tab.invalid_label", "Tab label cannot be empty", false);
                    return true;
                }
                // Herdr closes a workspace with its last pane, so a project
                // can be listed with no Herdr workspace behind it. A tab
                // needs one; the shell starts a terminal (which creates the
                // workspace) for a checkout with no panes instead.
                let Some(session_workspace_id) =
                    workspace_snapshot.session_workspace_ids.first().cloned()
                else {
                    self.set_error(
                        "tab.no_live_workspace",
                        format!(
                            "Project {workspace_id} has no Herdr workspace; start a terminal in it first"
                        ),
                        false,
                    );
                    return true;
                };
                let Some(context) = self.live.as_ref().cloned() else {
                    self.set_error(
                        "tab.control_unavailable",
                        "Tab creation requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                self.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
                self.snapshot.navigator.focused_checkout_id = Some(checkout_id);
                self.snapshot.navigator.root_path = Some(cwd.clone());
                self.deactivate_file_tab();
                self.persist_current_ui_state();
                let action = RemoteControlAction::CreateTab {
                    workspace_id: session_workspace_id,
                    cwd,
                    label: label.to_owned(),
                };
                self.push_diagnostic(
                    "tab.create.requested",
                    format!("Creating {}", action.kind()),
                );
                if let Err(message) = live::spawn_local_control(context, action) {
                    self.set_error("tab.create_worker_failed", message, true);
                }
                true
            }
            ValidatedEvent::FocusCheckout(payload) => {
                self.focus_checkout(&payload.workspace_id, &payload.checkout_id)
            }
            ValidatedEvent::FocusTab(payload) => {
                let Some(workspace_snapshot) = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.id == payload.workspace_id)
                else {
                    self.set_error(
                        "tab.unknown_workspace",
                        format!("Workspace {} is not registered", payload.workspace_id),
                        false,
                    );
                    return true;
                };
                let Some(checkout) = workspace_snapshot
                    .checkouts
                    .iter_mut()
                    .find(|checkout| checkout.id == payload.checkout_id)
                else {
                    self.set_error(
                        "tab.unknown_checkout",
                        format!("Checkout {} is not available", payload.checkout_id),
                        false,
                    );
                    return true;
                };
                let Some(index) = checkout
                    .tabs
                    .iter()
                    .position(|tab| tab.id.as_deref() == Some(payload.tab_id.as_str()))
                else {
                    self.set_error(
                        "tab.unknown",
                        format!("Tab {} is not available", payload.tab_id),
                        false,
                    );
                    return true;
                };
                if index != 0 {
                    let tab = checkout.tabs.remove(index);
                    checkout.tabs.insert(0, tab);
                }
                let checkout_path = checkout.path.clone();
                let next_pane_id = checkout
                    .tabs
                    .first()
                    .and_then(|tab| tab.panes.first())
                    .map(|pane| pane.id.clone());
                self.snapshot.navigator.focused_workspace_id = Some(payload.workspace_id);
                self.snapshot.navigator.focused_checkout_id = Some(payload.checkout_id);
                self.snapshot.navigator.root_path = Some(checkout_path);
                self.reset_terminal_projection(next_pane_id);
                self.sync_active_tab_projection();
                self.deactivate_file_tab();
                self.persist_current_ui_state();
                let Some(context) = self.live.as_ref().cloned() else {
                    self.set_error(
                        "tab.control_unavailable",
                        "Tab focus requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                if let Err(message) = live::spawn_local_control(
                    context,
                    RemoteControlAction::FocusTab {
                        tab_id: payload.tab_id,
                    },
                ) {
                    self.set_error("tab.focus_worker_failed", message, true);
                }
                true
            }
            ValidatedEvent::FocusDevice(payload) => {
                if !self
                    .snapshot
                    .navigator
                    .devices
                    .iter()
                    .any(|device| device.id == payload.device_id)
                {
                    self.set_error(
                        "device.unknown",
                        format!("Device {} is not registered", payload.device_id),
                        false,
                    );
                    return true;
                }
                self.snapshot.navigator.focused_device_id = Some(payload.device_id);
                self.deactivate_file_tab();
                self.reconcile_remote_terminal_selection();
                self.persist_current_ui_state();
                true
            }
            ValidatedEvent::RemoveWorkspace(payload) => {
                let before = self.snapshot.ui_state.workspace_registrations.len();
                self.snapshot
                    .ui_state
                    .workspace_registrations
                    .retain(|registration| registration.id != payload.workspace_id);
                if before == self.snapshot.ui_state.workspace_registrations.len() {
                    self.set_error(
                        "workspace.unknown",
                        format!("Workspace {} is not registered", payload.workspace_id),
                        false,
                    );
                    return true;
                }
                self.rebuild_catalog();
                self.persist_current_ui_state();
                self.push_diagnostic(
                    "workspace.unregistered",
                    format!(
                        "Unregistered workspace {} without touching its files",
                        payload.workspace_id
                    ),
                );
                true
            }
            ValidatedEvent::RegisterDevice(payload) => {
                let id = payload.id.trim().to_owned();
                let label = payload.label.trim().to_owned();
                let ssh_alias = payload.ssh_alias.trim().to_owned();
                if id.is_empty() || label.is_empty() || ssh_alias.is_empty() {
                    self.set_error(
                        "device.invalid",
                        "Device id, label, and SSH alias are required",
                        false,
                    );
                    return true;
                }
                if id == workspace::LOCAL_DEVICE_ID
                    || self.remote_targets.iter().any(|target| target.id == id)
                    || self
                        .snapshot
                        .ui_state
                        .device_registrations
                        .iter()
                        .any(|device| device.id == id)
                {
                    self.set_error(
                        "device.duplicate",
                        format!("Device {id} is already registered"),
                        false,
                    );
                    return true;
                }
                self.snapshot.ui_state.device_registrations.push(
                    crate::model::DeviceRegistration {
                        id: id.clone(),
                        label,
                        ssh_alias: Some(ssh_alias.clone()),
                    },
                );
                self.rebuild_catalog();
                self.persist_current_ui_state();
                self.push_diagnostic(
                    "device.registered",
                    format!("Registered SSH device {id} ({ssh_alias})"),
                );
                true
            }
            ValidatedEvent::RemoveDevice(payload) => {
                if payload.device_id == workspace::LOCAL_DEVICE_ID {
                    self.set_error(
                        "device.local_remove_denied",
                        "This Mac cannot be removed",
                        false,
                    );
                    return true;
                }
                let before = self.snapshot.ui_state.device_registrations.len();
                self.snapshot
                    .ui_state
                    .device_registrations
                    .retain(|device| device.id != payload.device_id);
                if before == self.snapshot.ui_state.device_registrations.len() {
                    self.set_error(
                        "device.unknown",
                        format!("Device {} is not registered", payload.device_id),
                        false,
                    );
                    return true;
                }
                self.rebuild_catalog();
                self.persist_current_ui_state();
                self.push_diagnostic(
                    "device.unregistered",
                    format!(
                        "Unregistered device {} without touching its host",
                        payload.device_id
                    ),
                );
                true
            }
            ValidatedEvent::TestDevice(payload) => {
                let known = self
                    .snapshot
                    .navigator
                    .devices
                    .iter()
                    .any(|device| device.id == payload.device_id);
                if !known {
                    self.set_error(
                        "device.unknown",
                        format!("Device {} is not registered", payload.device_id),
                        false,
                    );
                    return true;
                }
                self.push_diagnostic(
                    "device.connection_test_requested",
                    format!(
                        "Connection test requested for {}; SSH credentials remain outside hide",
                        payload.device_id
                    ),
                );
                if let Some(device) = self
                    .snapshot
                    .navigator
                    .devices
                    .iter_mut()
                    .find(|device| device.id == payload.device_id)
                {
                    device.state = "test_requested".to_owned();
                }
                true
            }
            ValidatedEvent::CreatePane(payload) => {
                if payload.command.is_some() {
                    self.set_error(
                        "pane.command_unsupported",
                        "Pane split starts the configured shell; a command cannot be supplied",
                        false,
                    );
                    return true;
                }
                let Some(pane_id) = self.snapshot.terminal.pane_id.clone() else {
                    self.set_error(
                        "pane.no_current_pane",
                        "Select a terminal pane before splitting",
                        false,
                    );
                    return true;
                };
                let _ = payload.tab_id;
                let context = self.live.as_ref().cloned();
                let Some(context) = context else {
                    self.set_error(
                        "pane.control_unavailable",
                        "Pane split requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let direction = payload.direction;
                let action = PaneControlAction::Split {
                    pane_id: pane_id.clone(),
                    direction,
                    cwd: Some(payload.cwd),
                };
                self.push_diagnostic(
                    format!("pane.split.{}.requested", direction.as_str()),
                    format!("Splitting pane {pane_id} {}", direction.as_str()),
                );
                if let Err(message) = live::spawn_pane_control(context, action) {
                    self.set_error("pane.split_worker_failed", message, true);
                }
                true
            }
            ValidatedEvent::ResizePane(payload) => {
                if !payload.amount.is_finite() || !(0.001..=0.5).contains(&payload.amount) {
                    self.set_error(
                        "pane.resize_invalid_amount",
                        "Pane resize amount must be between 0.001 and 0.5",
                        false,
                    );
                    return true;
                }
                let Some(context) = self.live.as_ref().cloned() else {
                    self.set_error(
                        "pane.control_unavailable",
                        "Pane resize requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let pane_id = payload.pane_id;
                let direction = payload.direction;
                self.push_diagnostic(
                    "pane.resize.requested",
                    format!("Resizing pane {pane_id} {}", direction.as_str()),
                );
                if let Err(message) = live::spawn_pane_control(
                    context,
                    PaneControlAction::Resize {
                        pane_id,
                        direction,
                        amount: payload.amount,
                    },
                ) {
                    self.set_error("pane.resize_worker_failed", message, true);
                }
                true
            }
            ValidatedEvent::ToggleZoom(payload) => {
                let context = self.live.as_ref().cloned();
                let Some(context) = context else {
                    self.set_error(
                        "pane.control_unavailable",
                        "Pane zoom requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let pane_id = payload.pane_id;
                self.push_diagnostic(
                    "pane.zoom.requested",
                    format!("Toggling zoom for pane {pane_id}"),
                );
                if let Err(message) =
                    live::spawn_pane_control(context, PaneControlAction::ToggleZoom { pane_id })
                {
                    self.set_error("pane.zoom_worker_failed", message, true);
                }
                true
            }
            ValidatedEvent::CloseWorkspace(payload) => {
                let _ = (payload.workspace_id, payload.confirmed);
                false
            }
            ValidatedEvent::CloseTab(payload) => {
                let tab = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .find(|tab| tab.id.as_deref() == Some(payload.tab_id.as_str()));
                let Some(tab) = tab else {
                    self.set_error(
                        "tab.unknown",
                        format!("Tab {} is not available", payload.tab_id),
                        false,
                    );
                    return true;
                };
                let pane_ids = tab
                    .panes
                    .iter()
                    .map(|pane| pane.id.as_str())
                    .collect::<HashSet<_>>();
                let requires_confirmation = self.snapshot.navigator.agents.iter().any(|agent| {
                    pane_ids.contains(agent.pane_id.as_str())
                        && crate::sidebar::requires_close_confirmation(&agent.state)
                });
                if requires_confirmation && !payload.confirmed {
                    self.set_error(
                        "tab.close_confirmation_required",
                        format!(
                            "Tab {} contains an agent that is working or needs attention; close_tab requires confirmed=true",
                            payload.tab_id
                        ),
                        false,
                    );
                    return true;
                }
                let Some(context) = self.live.as_ref().cloned() else {
                    self.set_error(
                        "tab.control_unavailable",
                        "Tab close requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let tab_id = payload.tab_id;
                self.push_diagnostic("tab.close.requested", format!("Closing tab {tab_id}"));
                if let Err(message) =
                    live::spawn_local_control(context, RemoteControlAction::CloseTab { tab_id })
                {
                    self.set_error("tab.close_worker_failed", message, true);
                }
                true
            }
            ValidatedEvent::ClosePane(payload) => {
                let requires_confirmation = self.snapshot.navigator.agents.iter().any(|agent| {
                    agent.pane_id == payload.pane_id
                        && crate::sidebar::requires_close_confirmation(&agent.state)
                });
                if requires_confirmation && !payload.confirmed {
                    self.set_error(
                        "pane.close_confirmation_required",
                        format!(
                            "Pane {} is working or needs attention; close_pane requires confirmed=true",
                            payload.pane_id
                        ),
                        false,
                    );
                    return true;
                }
                let Some(context) = self.live.as_ref().cloned() else {
                    self.set_error(
                        "pane.control_unavailable",
                        "Pane close requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let pane_id = payload.pane_id;
                self.retain_project_before_last_pane_closes(&pane_id);
                self.push_diagnostic("pane.close.requested", format!("Closing pane {pane_id}"));
                // Take the pane out of the rendered projection now instead of
                // waiting for `pane_closed`, which was measured arriving 167 ms
                // after the request. Only the projection runs ahead: the sync
                // replica still holds the pane, so the authoritative event
                // decodes against a pane that exists and then recomputes this
                // same projection to the same answer. Nothing records that a
                // close is in flight, because nothing has to be reconciled.
                if let Some(layout) = self
                    .snapshot
                    .pane_layout
                    .as_ref()
                    .and_then(|layout| layout.removing(&pane_id))
                {
                    self.apply_pane_layout(layout);
                }
                if let Err(message) = live::spawn_pane_control(
                    context,
                    PaneControlAction::Close {
                        pane_id: pane_id.clone(),
                    },
                ) {
                    self.restore_projection_after_failed_close(&pane_id);
                    self.set_error("pane.close_worker_failed", message, true);
                }
                true
            }
            ValidatedEvent::ForkPane(payload) => {
                let pane_id = payload.pane_id;
                let Some(agent) = self
                    .snapshot
                    .navigator
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == pane_id)
                else {
                    self.set_error(
                        "pane.fork_no_agent",
                        format!("Pane {pane_id} is not running an agent that can be forked"),
                        false,
                    );
                    return true;
                };
                let (Some(agent_kind), Some(session_id)) = (
                    ForkableAgent::parse(&agent.agent_kind),
                    agent.session_id.clone(),
                ) else {
                    self.set_error(
                        "pane.fork_unsupported_agent",
                        format!(
                            "Pane {pane_id} runs {} with no forkable session id",
                            agent.agent_kind
                        ),
                        false,
                    );
                    return true;
                };
                // A fork blocks until the agent has started, which takes long
                // enough for a second click to land. Refusing the second one by
                // name is what keeps one activation from becoming two sessions.
                if !self.forks_in_flight.insert(pane_id.clone()) {
                    self.set_error(
                        "pane.fork_already_running",
                        format!("Pane {pane_id} is already being forked"),
                        false,
                    );
                    return true;
                }
                let Some(context) = self.live.as_ref().cloned() else {
                    self.forks_in_flight.remove(&pane_id);
                    self.set_error(
                        "pane.control_unavailable",
                        "Forking a pane requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let cwd = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .flat_map(|tab| tab.panes.iter())
                    .find(|pane| pane.id == pane_id)
                    .map(|pane| pane.cwd.clone());
                self.fork_sequence += 1;
                // The name is already unique and already sanitized, so it is
                // also the retry identity rather than a second thing to keep
                // unique.
                let name = fork_name(
                    &pane_id,
                    &format!("{}-{}", self.fork_sequence, unix_milliseconds()),
                );
                let request = ForkRequest {
                    parent_pane_id: pane_id.clone(),
                    agent: agent_kind,
                    session_id,
                    cwd,
                    idempotency_key: format!("hide-{name}"),
                    name,
                };
                self.push_diagnostic("pane.fork.requested", format!("Forking pane {pane_id}"));
                if let Err(message) = live::spawn_agent_fork(context, request) {
                    self.forks_in_flight.remove(&pane_id);
                    self.set_error("pane.fork_worker_failed", message, true);
                }
                true
            }
            ValidatedEvent::RemoteControl(payload) => self.request_remote_control(payload),
            ValidatedEvent::RemoteFileList(payload) => self.request_remote_file_list(payload),
            ValidatedEvent::FileOpen(payload) => {
                let context_exists = self.snapshot.navigator.workspaces.iter().any(|workspace| {
                    workspace.id == payload.workspace_id
                        && workspace
                            .checkouts
                            .iter()
                            .any(|checkout| checkout.id == payload.checkout_id)
                });
                let context_is_focused = self.snapshot.navigator.focused_workspace_id.as_deref()
                    == Some(payload.workspace_id.as_str())
                    && self.snapshot.navigator.focused_checkout_id.as_deref()
                        == Some(payload.checkout_id.as_str());
                if !context_exists || !context_is_focused {
                    self.set_error(
                        "file.invalid_context",
                        "A file tab requires the selected workspace and checkout",
                        false,
                    );
                    return true;
                }
                if let Some(tab_id) = self.snapshot.editor.tabs.iter().find_map(|tab| {
                    (tab.workspace_id == payload.workspace_id
                        && tab.checkout_id == payload.checkout_id
                        && tab.path == payload.path)
                        .then(|| tab.id.clone())
                }) {
                    if let Err(message) = self.activate_file_tab(&tab_id) {
                        self.set_error("file.focus_failed", message, false);
                    }
                    self.snapshot.ui_state.selected_path = Some(payload.path);
                    self.persist_current_ui_state();
                    return true;
                }
                match files::open(Path::new(&payload.path)) {
                    Ok(document) => {
                        let tab_id = Self::file_tab_id(
                            &payload.workspace_id,
                            &payload.checkout_id,
                            &payload.path,
                        );
                        self.editor_documents.insert(tab_id.clone(), document);
                        self.snapshot.editor.tabs.push(FileTabSnapshot {
                            id: tab_id.clone(),
                            workspace_id: payload.workspace_id,
                            checkout_id: payload.checkout_id,
                            path: payload.path.clone(),
                            label: Path::new(&payload.path)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .filter(|name| !name.is_empty())
                                .unwrap_or(payload.path.as_str())
                                .to_owned(),
                            dirty: false,
                        });
                        if let Err(message) = self.activate_file_tab(&tab_id) {
                            self.set_error("file.focus_failed", message, false);
                        }
                        self.snapshot.ui_state.selected_path = Some(payload.path);
                    }
                    Err(message) => self.set_error("file.open_failed", message, true),
                }
                self.persist_current_ui_state();
                true
            }
            ValidatedEvent::FileFocus(payload) => {
                let Some(tab) = self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .find(|tab| tab.id == payload.tab_id)
                else {
                    self.set_error(
                        "file.focus_failed",
                        format!("File tab {} is not open", payload.tab_id),
                        false,
                    );
                    return true;
                };
                if !self.file_tab_matches_focused_context(tab) {
                    self.set_error(
                        "file.invalid_context",
                        "A file tab can only be focused in its workspace and checkout",
                        false,
                    );
                    return true;
                }
                match self.activate_file_tab(&payload.tab_id) {
                    Ok(()) => {
                        self.snapshot.ui_state.selected_path = self
                            .snapshot
                            .editor
                            .document
                            .as_ref()
                            .map(|document| document.path.clone());
                    }
                    Err(message) => self.set_error("file.focus_failed", message, false),
                }
                self.persist_current_ui_state();
                true
            }
            ValidatedEvent::FileClose(payload) => {
                let Some(index) = self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .position(|tab| tab.id == payload.tab_id)
                else {
                    self.set_error(
                        "file.close_unknown_tab",
                        format!("File tab {} is not open", payload.tab_id),
                        false,
                    );
                    return true;
                };
                let was_active =
                    self.snapshot.editor.active_tab_id.as_deref() == Some(payload.tab_id.as_str());
                self.snapshot.editor.tabs.remove(index);
                self.editor_documents.remove(&payload.tab_id);
                self.editor_tab_history
                    .retain(|tab_id| tab_id != &payload.tab_id);
                if was_active {
                    self.snapshot.editor.active_tab_id = None;
                    self.snapshot.editor.document = None;
                    while let Some(previous_id) = self.editor_tab_history.pop() {
                        if self
                            .snapshot
                            .editor
                            .tabs
                            .iter()
                            .any(|tab| tab.id == previous_id)
                        {
                            if let Err(message) = self.activate_file_tab(&previous_id) {
                                self.set_error("file.focus_failed", message, false);
                            }
                            break;
                        }
                    }
                }
                self.snapshot.ui_state.selected_path = self
                    .snapshot
                    .editor
                    .document
                    .as_ref()
                    .map(|document| document.path.clone());
                self.persist_current_ui_state();
                true
            }
            ValidatedEvent::FileDraft(payload) => {
                let Some(tab_id) = self.snapshot.editor.active_tab_id.clone() else {
                    self.set_error("file.draft_rejected", "No file tab is active", false);
                    return true;
                };
                let Some(document) = self.editor_documents.get_mut(&tab_id) else {
                    self.set_error(
                        "file.draft_rejected",
                        "The active file tab has no document state",
                        false,
                    );
                    return true;
                };
                match files::update_draft(document, payload.contents_utf8) {
                    Ok(()) => {
                        self.sync_file_tab_dirty(&tab_id);
                        self.sync_active_editor_document();
                    }
                    Err(message) => self.set_error("file.draft_rejected", message, false),
                }
                true
            }
            ValidatedEvent::FileSave(payload) => {
                let Some(tab) = self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .find(|tab| tab.id == payload.tab_id && tab.path == payload.path)
                else {
                    self.set_error("file.save_rejected", "The save target is not open", false);
                    return true;
                };
                let tab_id = tab.id.clone();
                let Some(document) = self.editor_documents.get_mut(&tab_id) else {
                    self.set_error(
                        "file.save_rejected",
                        "The save target has no document state",
                        false,
                    );
                    return true;
                };
                document.contents_utf8 = Some(payload.contents_utf8.clone());
                document.dirty = true;
                self.sync_file_tab_dirty(&tab_id);
                self.sync_active_editor_document();
                let Some(context) = self.worker_context.clone() else {
                    self.set_error(
                        "file.save_worker_unavailable",
                        "The file save worker is unavailable; the draft was preserved",
                        true,
                    );
                    return true;
                };
                let path = payload.path;
                let save_tab_id = tab_id.clone();
                let contents = payload.contents_utf8;
                let expected_modified_at = payload.expected_modified_at_unix_ms;
                let mut editor = self
                    .editor_documents
                    .get(&tab_id)
                    .cloned()
                    .expect("the save document was validated");
                match thread::Builder::new()
                    .name("herdr-core-file-save".to_owned())
                    .spawn(move || {
                        let result = files::save(
                            &mut editor,
                            Path::new(&path),
                            contents.clone(),
                            expected_modified_at,
                        );
                        let Some(runtime) = context.runtime.upgrade() else {
                            return;
                        };
                        let changed = match runtime.lock() {
                            Ok(mut guard) => guard.ingest_file_save_result(
                                save_tab_id,
                                path,
                                contents,
                                editor,
                                result,
                            ),
                            Err(_) => return,
                        };
                        drop(runtime);
                        if changed {
                            context.notifier.notify();
                        }
                    }) {
                    Ok(_) => true,
                    Err(error) => {
                        self.set_error(
                            "file.save_worker_failed",
                            format!("The file save worker could not start: {error}"),
                            true,
                        );
                        true
                    }
                }
            }
            ValidatedEvent::FileConflict(payload) => {
                let Some(tab_id) = self.snapshot.editor.active_tab_id.clone() else {
                    self.set_error("file.conflict_without_tab", "No file tab is active", false);
                    return true;
                };
                let Some(document) = self.editor_documents.get_mut(&tab_id) else {
                    self.set_error(
                        "file.conflict_without_document",
                        "The active file tab has no document state",
                        false,
                    );
                    return true;
                };
                match payload.action.as_str() {
                    "reload" => match files::reload(document) {
                        Ok(()) => {
                            self.sync_file_tab_dirty(&tab_id);
                            self.sync_active_editor_document();
                        }
                        Err(message) => self.set_error("file.reload_failed", message, true),
                    },
                    "keep_editing" => {
                        if let Some(conflict) = document.conflict.as_ref() {
                            document.opened_modified_at_unix_ms =
                                Some(conflict.disk_modified_at_unix_ms);
                        }
                        document.conflict = None;
                        self.sync_active_editor_document();
                    }
                    _ => self.set_error(
                        "file.invalid_conflict_action",
                        "Conflict action must be reload or keep_editing",
                        false,
                    ),
                }
                true
            }
            ValidatedEvent::TerminalResize(payload) => {
                if payload.rows == 0 || payload.cols == 0 {
                    self.set_error(
                        "terminal.invalid_resize",
                        "Terminal dimensions must be positive",
                        false,
                    );
                    return true;
                }
                self.terminal_sizes
                    .insert(payload.pane_id.clone(), (payload.rows, payload.cols));
                if let Some(session) = self.terminal_sessions.get_mut(&payload.pane_id)
                    && session.mode == TerminalSessionMode::Control
                    && let Err(message) = session.resize(payload.rows, payload.cols)
                {
                    self.set_error("terminal.resize_failed", message, true);
                    return true;
                }
                false
            }
            ValidatedEvent::TerminalScroll(payload) => {
                // Herdr owns the pane's history, so the wheel is a request it
                // answers with a fresh frame rather than a local buffer move.
                // A pane another client controls is read-only, not broken, so
                // it simply does not scroll - the same shape as resize.
                let (rows, cols) = self
                    .terminal_sizes
                    .get(&payload.pane_id)
                    .copied()
                    .unwrap_or((24, 80));
                if let Some(session) = self.terminal_sessions.get_mut(&payload.pane_id)
                    && session.mode == TerminalSessionMode::Control
                    && let Err(message) =
                        session.scroll(&payload.direction, payload.lines, rows, cols)
                {
                    self.set_error("terminal.scroll_failed", message, true);
                    return true;
                }
                false
            }
            ValidatedEvent::PaneFind(payload) => {
                if payload.term.is_empty() {
                    if self.snapshot.find == PaneFindSnapshot::default() {
                        return false;
                    }
                    self.snapshot.find = PaneFindSnapshot::default();
                    return true;
                }
                let Some(context) = self.live.as_ref().cloned() else {
                    self.snapshot.find = PaneFindSnapshot {
                        pane_id: Some(payload.pane_id.clone()),
                        term: payload.term.clone(),
                        unavailable_reason: Some(
                            "Searching a pane's history needs a live Herdr connection".to_owned(),
                        ),
                        ..PaneFindSnapshot::default()
                    };
                    return true;
                };
                // A step continues from the match the operator is on, which is
                // only the stored one when it belongs to this pane and term.
                let current_index = if self.snapshot.find.pane_id.as_deref()
                    == Some(payload.pane_id.as_str())
                    && self.snapshot.find.term == payload.term
                {
                    self.snapshot.find.index
                } else {
                    0
                };
                let request = live::PaneFindRequest {
                    pane_id: payload.pane_id.clone(),
                    term: payload.term.clone(),
                    options: crate::find::PaneFindOptions {
                        case_sensitive: payload.case_sensitive,
                        whole_word: payload.whole_word,
                        regex: payload.regex,
                    },
                    step: payload.step,
                    current_index,
                };
                if let Err(message) = live::spawn_pane_find(context, request) {
                    self.set_error("pane.find_worker_failed", message, true);
                }
                false
            }
            ValidatedEvent::PaneTextScale(payload) => {
                let current = self
                    .snapshot
                    .ui_state
                    .pane_text_scales
                    .get(&payload.pane_id)
                    .copied()
                    .unwrap_or(DEFAULT_PANE_TEXT_SCALE);
                let next = match payload.direction.as_str() {
                    "in" => clamp_pane_text_scale(current + PANE_TEXT_SCALE_STEP),
                    "out" => clamp_pane_text_scale(current - PANE_TEXT_SCALE_STEP),
                    "reset" => DEFAULT_PANE_TEXT_SCALE,
                    other => {
                        self.set_error(
                            "pane.text_scale_unknown_direction",
                            format!(
                                "{other} is not a text scale direction; expected in, out, or reset"
                            ),
                            true,
                        );
                        return true;
                    }
                };
                // A pane at the default is absent rather than stored at 1.0,
                // so resetting every pane leaves an empty map rather than a
                // row per pane the user ever touched.
                let changed = if next == DEFAULT_PANE_TEXT_SCALE {
                    self.snapshot
                        .ui_state
                        .pane_text_scales
                        .remove(&payload.pane_id)
                        .is_some()
                } else {
                    self.snapshot
                        .ui_state
                        .pane_text_scales
                        .insert(payload.pane_id.clone(), next)
                        != Some(next)
                };
                if changed {
                    self.persist_ui_state();
                }
                changed
            }
            ValidatedEvent::ChangesSelect(payload) => {
                if self.snapshot.changes.selected_path == payload.path {
                    return false;
                }
                self.snapshot.changes.selected_path = payload.path;
                // The previous file's diff is dropped now rather than left
                // showing under the newly selected file's name until the
                // reader catches up.
                self.snapshot.changes.diff = None;
                true
            }
            ValidatedEvent::UiStateUpdate(payload) => {
                // Pet placement, visibility, and shortcut belong to the pet
                // events; a navigator or keyboard save must not erase them.
                let current = self.snapshot.ui_state.clone();
                self.snapshot.ui_state = UiStateSnapshot {
                    left_sidebar_visible: payload
                        .left_sidebar_visible
                        .unwrap_or(current.left_sidebar_visible),
                    right_panel_visible: payload
                        .right_panel_visible
                        .unwrap_or(current.right_panel_visible),
                    // An unrecognised section name keeps the current one
                    // rather than silently resetting the panel to Explorer.
                    right_panel_section: payload
                        .right_panel_section
                        .as_deref()
                        .and_then(RightPanelSection::parse)
                        .unwrap_or(current.right_panel_section),
                    expanded_paths: payload.expanded_paths,
                    collapsed_workspace_ids: payload.collapsed_workspace_ids,
                    selected_path: payload.selected_path,
                    selected_pane_id: payload.selected_pane_id,
                    shortcut_bindings: payload.shortcut_bindings,
                    pet_visible: self.snapshot.ui_state.pet_visible,
                    pet_origin: self.snapshot.ui_state.pet_origin,
                    pet_shortcut: self.snapshot.ui_state.pet_shortcut.clone(),
                    focused_device_id: payload
                        .focused_device_id
                        .unwrap_or(current.focused_device_id),
                    focused_checkout_id: payload
                        .focused_checkout_id
                        .unwrap_or(current.focused_checkout_id),
                    workspace_registrations: payload
                        .workspace_registrations
                        .unwrap_or(current.workspace_registrations),
                    device_registrations: payload
                        .device_registrations
                        .unwrap_or(current.device_registrations),
                    accent_hex: payload.accent_hex.unwrap_or(current.accent_hex),
                    font_size: payload.font_size.unwrap_or(current.font_size),
                    // The zoom chords own this map; a navigator or keyboard
                    // save must not erase it, for the same reason the pet
                    // fields above are carried through.
                    pane_text_scales: current.pane_text_scales,
                };
                self.apply_selected_pane_anchor(self.snapshot.ui_state.selected_pane_id.clone());
                self.snapshot.navigator.focused_device_id =
                    self.snapshot.ui_state.focused_device_id.clone();
                self.snapshot.navigator.focused_checkout_id =
                    self.snapshot.ui_state.focused_checkout_id.clone();
                self.reconcile_remote_terminal_selection();
                Self::apply_workspace_expansion(
                    &mut self.snapshot.navigator.workspaces,
                    &self.snapshot.ui_state.collapsed_workspace_ids,
                );
                // Session sync owns session-derived temporary workspaces.
                // UI-state persistence must not rebuild from an empty session
                // and erase the catalog that the user is currently viewing.
                match persistence::save(&self.state_path, &self.snapshot.ui_state) {
                    Ok(()) => true,
                    Err(message) => {
                        self.set_error("ui_state.save_failed", message, true);
                        true
                    }
                }
            }
        }
    }

    /// Starts one control attempt. Repeated sync updates are no-ops while any
    /// official control or observer session is starting or active.
    fn request_terminal_control(&mut self, pane_id: &str) {
        let current = self
            .terminal_session_lifecycles
            .get(pane_id)
            .cloned()
            .unwrap_or_default();
        if !terminal_control_request_allowed(
            current.state,
            self.terminal_sessions.contains_key(pane_id),
        ) {
            self.sync_transport_projection(pane_id);
            return;
        }
        let attempt = current.attempt.saturating_add(1);
        self.start_terminal_session(
            pane_id,
            TerminalSessionMode::Control,
            attempt,
            if attempt == 1 {
                "automatic_initial"
            } else {
                "manual"
            },
            None,
        );
    }

    fn start_terminal_session(
        &mut self,
        pane_id: &str,
        mode: TerminalSessionMode,
        attempt: u64,
        retry_decision: &'static str,
        message: Option<String>,
    ) {
        self.next_terminal_session_generation =
            self.next_terminal_session_generation.saturating_add(1);
        let generation = self.next_terminal_session_generation;
        self.terminal_session_generations
            .insert(pane_id.to_owned(), generation);
        let _retired_session = self.terminal_sessions.remove(pane_id);
        self.terminal_session_lifecycles.insert(
            pane_id.to_owned(),
            TerminalSessionLifecycle {
                state: "starting",
                message,
                generation,
                attempt,
                mode: Some(mode),
                exit_category: None,
                retry_decision,
            },
        );
        self.sync_transport_projection(pane_id);
        // Reset only this pane's SwiftTerm grid. The first official frame is
        // a full ANSI frame, while other panes retain their own state.
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(b"\x1bc"));
        self.push_diagnostic(
            "terminal.session_requested",
            format!(
                "Starting terminal {} session for pane {pane_id}",
                mode.as_str()
            ),
        );
        #[cfg(test)]
        if self.suppress_terminal_session_workers {
            self.terminal_sessions.insert(
                pane_id.to_owned(),
                TerminalSession::test_stub(pane_id, generation, mode),
            );
            if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
                lifecycle.state = match mode {
                    TerminalSessionMode::Control => "controlling",
                    TerminalSessionMode::Observe => "observing",
                };
                lifecycle.retry_decision = if mode == TerminalSessionMode::Observe {
                    "manual"
                } else {
                    "none"
                };
            }
            self.sync_transport_projection(pane_id);
            return;
        }
        let context = if pane_id.starts_with("remote:") {
            self.remote_terminals
                .iter()
                .find_map(|(target_id, context)| {
                    remote_pane_source_id(target_id, pane_id).map(|source_pane_id| {
                        TerminalSessionContext::Remote {
                            context: context.clone(),
                            source_pane_id: source_pane_id.to_owned(),
                        }
                    })
                })
        } else {
            self.live
                .as_ref()
                .cloned()
                .map(TerminalSessionContext::Local)
        };
        let Some(context) = context else {
            let message = if pane_id.starts_with("remote:") {
                format!("Pane {pane_id} has no configured remote terminal transport")
            } else {
                "Local terminal sessions require a live Herdr connection".to_owned()
            };
            self.record_terminal_session_failure(
                pane_id,
                generation,
                attempt,
                mode,
                "transport_unavailable",
                &message,
                0,
            );
            self.set_error("terminal.transport_unavailable", message, true);
            return;
        };
        if let Err(message) = live::spawn_terminal_session(
            context,
            pane_id.to_owned(),
            generation,
            mode,
            self.terminal_sizes.get(pane_id).map_or(24, |size| size.0),
            self.terminal_sizes.get(pane_id).map_or(80, |size| size.1),
        ) {
            self.record_terminal_session_failure(
                pane_id,
                generation,
                attempt,
                mode,
                "worker_start_failed",
                &message,
                0,
            );
            let notice = format!(
                "\r\n[Terminal {} session for {pane_id} failed: {message}]\r\n",
                mode.as_str()
            );
            self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(notice.as_bytes()));
            self.set_error("terminal.session_worker_failed", message, true);
        }
    }

    fn record_terminal_session_failure(
        &mut self,
        pane_id: &str,
        generation: u64,
        attempt: u64,
        mode: TerminalSessionMode,
        category: &str,
        message: &str,
        elapsed_ms: u128,
    ) {
        self.terminal_session_lifecycles.insert(
            pane_id.to_owned(),
            TerminalSessionLifecycle {
                state: "unavailable",
                message: Some(message.to_owned()),
                generation,
                attempt,
                mode: Some(mode),
                exit_category: Some(category.to_owned()),
                retry_decision: "manual",
            },
        );
        self.sync_transport_projection(pane_id);
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "terminal_session",
                "kind": "terminal.session_unavailable",
                "pane_id": pane_id,
                "generation": generation,
                "attempt": attempt,
                "mode": mode.as_str(),
                "duration_ms": elapsed_ms,
                "exit_category": category,
                "retry_decision": "manual",
            })
        );
    }

    pub fn ingest_terminal_session_spawn(
        &mut self,
        generation: u64,
        pane_id: &str,
        mode: TerminalSessionMode,
        result: Result<TerminalSession, String>,
        elapsed_ms: u128,
        worker_runtime: Weak<Mutex<Runtime>>,
        notifier: crate::ffi::ChangeNotifier,
    ) -> bool {
        if self.terminal_session_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        if self
            .terminal_session_lifecycles
            .get(pane_id)
            .is_none_or(|lifecycle| lifecycle.mode != Some(mode))
        {
            return false;
        }
        if !pane_id.starts_with("remote:")
            && let Some(layout) = self.snapshot.pane_layout.as_ref()
            && !layout.pane_ids().contains(&pane_id)
        {
            return false;
        }
        match result {
            Ok(session) => {
                self.terminal_sessions.insert(pane_id.to_owned(), session);
                let reader_result = self
                    .terminal_sessions
                    .get_mut(pane_id)
                    .expect("terminal session was just inserted")
                    .start_reader(worker_runtime, notifier);
                if let Err(message) = reader_result {
                    let _failed_session = self.terminal_sessions.remove(pane_id);
                    let attempt = self
                        .terminal_session_lifecycles
                        .get(pane_id)
                        .map_or(1, |lifecycle| lifecycle.attempt);
                    self.record_terminal_session_failure(
                        pane_id,
                        generation,
                        attempt,
                        mode,
                        "reader_start_failed",
                        &message,
                        elapsed_ms,
                    );
                    self.set_error("terminal.session_reader_failed", message, true);
                    return true;
                }
                let attempt = self
                    .terminal_session_lifecycles
                    .get(pane_id)
                    .map_or(1, |lifecycle| lifecycle.attempt);
                let message = self
                    .terminal_session_lifecycles
                    .get(pane_id)
                    .and_then(|lifecycle| lifecycle.message.clone());
                let state = match mode {
                    TerminalSessionMode::Control => "controlling",
                    TerminalSessionMode::Observe => "observing",
                };
                self.terminal_session_lifecycles.insert(
                    pane_id.to_owned(),
                    TerminalSessionLifecycle {
                        state,
                        message,
                        generation,
                        attempt,
                        mode: Some(mode),
                        exit_category: None,
                        retry_decision: if mode == TerminalSessionMode::Observe {
                            "manual"
                        } else {
                            "none"
                        },
                    },
                );
                self.sync_transport_projection(pane_id);
                self.push_diagnostic(
                    "terminal.session_ready",
                    format!(
                        "Pane {pane_id} terminal {} session ready in {elapsed_ms} ms",
                        mode.as_str()
                    ),
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "terminal_session",
                        "kind": "terminal.session_ready",
                        "pane_id": pane_id,
                        "generation": generation,
                        "attempt": attempt,
                        "mode": mode.as_str(),
                        "duration_ms": elapsed_ms,
                        "exit_category": null,
                        "retry_decision": if mode == TerminalSessionMode::Observe { "manual" } else { "none" },
                    })
                );
                true
            }
            Err(message) => {
                let attempt = self
                    .terminal_session_lifecycles
                    .get(pane_id)
                    .map_or(1, |lifecycle| lifecycle.attempt);
                self.record_terminal_session_failure(
                    pane_id,
                    generation,
                    attempt,
                    mode,
                    "spawn_failed",
                    &message,
                    elapsed_ms,
                );
                let notice = format!(
                    "\r\n[Terminal {} session for {pane_id} failed: {message}]\r\n",
                    mode.as_str()
                );
                self.append_terminal_chunk(
                    pane_id.to_owned(),
                    live::encode_base64(notice.as_bytes()),
                );
                self.set_error("terminal.session_failed", message, true);
                true
            }
        }
    }

    /// Routes key bytes only to an official controller. The actual pipe write
    /// runs on the session writer thread, outside the runtime mutex.
    fn write_terminal_control(&mut self, pane_id: &str, bytes_base64: &str) {
        let bytes = match live::decode_base64(bytes_base64) {
            Ok(bytes) => bytes,
            Err(message) => {
                self.set_error("terminal.invalid_input", message, false);
                return;
            }
        };
        match self.terminal_sessions.get(pane_id) {
            Some(session) if session.mode == TerminalSessionMode::Control => {
                if let Err(message) = session.write_bytes(&bytes) {
                    self.set_error("terminal.write_failed", message, true);
                }
            }
            Some(_) => {
                self.set_error(
                    "terminal.read_only",
                    format!(
                        "Pane {pane_id} is read-only because another client owns terminal control; use Reconnect to try again"
                    ),
                    true,
                );
            }
            None => {
                self.set_error(
                    "terminal.unavailable",
                    format!("Pane {pane_id} has no terminal session; use Reconnect to try again"),
                    true,
                );
            }
        }
    }

    fn append_terminal_chunk(&mut self, pane_id: String, bytes_base64: String) {
        self.snapshot.terminal.sequence = self.snapshot.terminal.sequence.saturating_add(1);
        self.snapshot.terminal.chunks.push(TerminalChunk {
            pane_id,
            sequence: self.snapshot.terminal.sequence,
            bytes_base64,
        });
        const RETAINED_TERMINAL_CHUNKS: usize = 512;
        if self.snapshot.terminal.chunks.len() > RETAINED_TERMINAL_CHUNKS {
            let excess = self.snapshot.terminal.chunks.len() - RETAINED_TERMINAL_CHUNKS;
            self.snapshot.terminal.chunks.drain(..excess);
        }
    }
}

fn find_workspace_for_context<'a>(
    workspaces: &'a mut Vec<crate::model::WorkspaceSnapshot>,
    context_path: Option<&str>,
    session_workspace_id: &str,
) -> Option<&'a mut crate::model::WorkspaceSnapshot> {
    let Some(raw_path) = context_path else {
        return workspaces
            .iter()
            .position(|workspace| workspace.id == session_workspace_id)
            .and_then(|index| workspaces.get_mut(index));
    };
    let path = Path::new(raw_path);
    let root = workspace::git_root(path)
        .map(|root| workspace::normalized_for_comparison(&root))
        .unwrap_or_else(|| workspace::normalized_for_comparison(path));
    let normalized = root.clone();
    // A navigator project records the Herdr workspaces occupying it. One
    // Herdr workspace can span two repositories and so two projects, so the
    // pane's directory picks between them. The path comparisons below are the
    // fallback for a registration Herdr has no workspace for, and for a
    // catalog precomputed from a slightly older session.
    let carrying = workspaces
        .iter()
        .enumerate()
        .filter(|(_, workspace)| {
            workspace
                .session_workspace_ids
                .iter()
                .any(|id| id == session_workspace_id)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if let Some(index) = carrying
        .iter()
        .copied()
        .find(|index| {
            workspaces[*index]
                .checkouts
                .iter()
                .any(|checkout| path_is_within_checkout(raw_path, &checkout.path))
        })
        .or_else(|| carrying.first().copied())
    {
        return workspaces.get_mut(index);
    }
    if let Some(index) = workspaces.iter().position(|workspace| {
        workspace.checkouts.iter().any(|checkout| {
            workspace::normalized_for_comparison(Path::new(&checkout.path)) == normalized
        })
    }) {
        return workspaces.get_mut(index);
    }
    if let Some(index) = workspaces.iter().position(|workspace| {
        workspace::normalized_for_comparison(Path::new(&workspace.path)) == normalized
    }) {
        return workspaces.get_mut(index);
    }
    if let Some(index) = workspaces.iter().position(|workspace| {
        let workspace_path = workspace::normalized_for_comparison(Path::new(&workspace.path));
        normalized.starts_with(&format!("{workspace_path}/"))
    }) {
        return workspaces.get_mut(index);
    }
    let mut temporary = workspace::inspect_temporary(Path::new(&root), workspace::LOCAL_DEVICE_ID);
    temporary.session_workspace_ids = vec![session_workspace_id.to_owned()];
    workspaces.push(temporary);
    workspaces.last_mut()
}

fn path_is_within_checkout(path: &str, checkout_path: &str) -> bool {
    let path = PathBuf::from(workspace::normalized_for_comparison(Path::new(path)));
    let checkout_path = PathBuf::from(workspace::normalized_for_comparison(Path::new(
        checkout_path,
    )));
    // `Path::starts_with` compares path components, so `barista` cannot
    // match the checkout component `bar`.
    path.starts_with(checkout_path.as_path())
}

struct EventValidationError {
    kind: &'static str,
    message: String,
}

fn validate_event(event: EventEnvelope) -> Result<ValidatedEvent, EventValidationError> {
    let EventEnvelope { kind, payload, .. } = event;
    let invalid_payload = |kind: &str| EventValidationError {
        kind: "event.invalid_payload",
        message: format!("Event payload for {kind} does not match schema version {SCHEMA_VERSION}"),
    };

    macro_rules! decode {
        ($payload:ty, $variant:ident) => {
            serde_json::from_value::<$payload>(payload)
                .map(ValidatedEvent::$variant)
                .map_err(|_| invalid_payload(&kind))
        };
    }

    match kind.as_str() {
        "key" => decode!(KeyPayload, Key),
        "terminal_output" => decode!(TerminalOutputPayload, TerminalOutput),
        "session_snapshot" => decode!(SessionSnapshotPayload, SessionSnapshot),
        "click" => decode!(ClickPayload, Click),
        "focus_pane" => decode!(FocusPanePayload, FocusPane),
        "open_browser" => decode!(OpenBrowserPayload, OpenBrowser),
        "browser_status" => decode!(BrowserStatusPayload, BrowserStatus),
        "create_workspace" => decode!(CreateWorkspacePayload, CreateWorkspace),
        "create_tab" => decode!(CreateTabPayload, CreateTab),
        "focus_checkout" => decode!(FocusCheckoutPayload, FocusCheckout),
        "focus_tab" => decode!(FocusTabPayload, FocusTab),
        "focus_device" => decode!(FocusDevicePayload, FocusDevice),
        "remove_workspace" => decode!(RemoveWorkspacePayload, RemoveWorkspace),
        "register_device" => decode!(RegisterDevicePayload, RegisterDevice),
        "remove_device" => decode!(RemoveDevicePayload, RemoveDevice),
        "test_device" => decode!(TestDevicePayload, TestDevice),
        "create_pane" => decode!(CreatePanePayload, CreatePane),
        "resize_pane" => decode!(ResizePanePayload, ResizePane),
        "toggle_zoom" => decode!(ToggleZoomPayload, ToggleZoom),
        "close_workspace" => decode!(ConfirmedWorkspacePayload, CloseWorkspace),
        "close_tab" => decode!(ConfirmedTabPayload, CloseTab),
        "close_pane" => decode!(ConfirmedPanePayload, ClosePane),
        "fork_pane" => decode!(PaneTargetPayload, ForkPane),
        "remote_control" => decode!(RemoteControlPayload, RemoteControl),
        "remote_file_list" => decode!(RemoteFileListPayload, RemoteFileList),
        "file_open" => decode!(FileOpenPayload, FileOpen),
        "file_focus" => decode!(FileTabPayload, FileFocus),
        "file_close" => decode!(FileTabPayload, FileClose),
        "file_draft" => decode!(FileDraftPayload, FileDraft),
        "file_save" => decode!(FileSavePayload, FileSave),
        "file_conflict" => decode!(FileConflictPayload, FileConflict),
        "ui_state_update" => decode!(UiStateUpdatePayload, UiStateUpdate),
        "retry_connect" => decode!(RetryConnectPayload, RetryConnect),
        "terminal_resize" => decode!(TerminalResizePayload, TerminalResize),
        "terminal_scroll" => decode!(TerminalScrollPayload, TerminalScroll),
        "pane_find" => decode!(PaneFindPayload, PaneFind),
        "pane_text_scale" => decode!(PaneTextScalePayload, PaneTextScale),
        "changes_select" => decode!(ChangesSelectPayload, ChangesSelect),
        "reconnect_pane" => decode!(FocusPanePayload, ReconnectPane),
        "pet_set_visible" => decode!(PetVisibilityPayload, PetSetVisible),
        "pet_toggle_visible" => Ok(ValidatedEvent::PetToggleVisible),
        "pet_move" => decode!(PetMovePayload, PetMove),
        "pet_drag" => decode!(PetDragPayload, PetDrag),
        "pet_activity" => Ok(ValidatedEvent::PetActivity),
        "pet_shortcut_update" => decode!(PetShortcutPayload, PetShortcutUpdate),
        _ => Err(EventValidationError {
            kind: "event.unknown_kind",
            message: format!("Unknown event kind: {kind}"),
        }),
    }
}

/// Projects the two fork facts the pane header renders from.
///
/// A pane with no agent is neither forkable nor a fork, which is why the
/// default carries both answers rather than the snapshot holding an optional.
pub fn pane_fork_snapshot(agent: Option<&SidebarAgentSnapshot>) -> PaneForkSnapshot {
    let Some(agent) = agent else {
        return PaneForkSnapshot::default();
    };
    PaneForkSnapshot {
        available: is_forkable(Some(agent.agent_kind.as_str()), agent.session_id.as_deref()),
        forked_from_pane_id: agent.spawned_from_pane_id.clone(),
    }
}

pub fn validate_options(options: &CoreOptions) -> Result<(), &'static str> {
    if options.schema_version != SCHEMA_VERSION {
        return Err("options schema version does not match");
    }
    if options.app_state_path.trim().is_empty() {
        return Err("app_state_path must not be empty");
    }
    if options
        .herdr_socket_path
        .as_ref()
        .is_some_and(|path| path.trim().is_empty())
    {
        return Err("herdr_socket_path must be null or non-empty");
    }
    if options
        .herdr_bin_path
        .as_ref()
        .is_some_and(|path| path.trim().is_empty())
    {
        return Err("herdr_bin_path must be null or non-empty");
    }
    for target in &options.remote_targets {
        if target.id.trim().is_empty()
            || target.label.trim().is_empty()
            || target.ssh_alias.trim().is_empty()
            || target.herdr_socket_path.trim().is_empty()
        {
            return Err("remote target fields must not be empty");
        }
        if !Path::new(&target.herdr_socket_path).is_absolute()
            || target
                .herdr_socket_path
                .bytes()
                .any(|byte| byte.is_ascii_control())
        {
            return Err("remote target herdr_socket_path must be absolute and single-line");
        }
    }
    for (index, target) in options.remote_targets.iter().enumerate() {
        if options.remote_targets[index + 1..]
            .iter()
            .any(|candidate| candidate.id == target.id)
        {
            return Err("remote target ids must be unique");
        }
    }
    Ok(())
}

fn unix_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MAX_PANE_TEXT_SCALE, MIN_PANE_TEXT_SCALE};
    use crate::live::SessionFetchError;
    use crate::model::{
        CheckoutSnapshot, DeviceSnapshot, PaneLayoutNodeSnapshot, PaneLayoutSnapshot, PaneSnapshot,
        RemoteSessionSnapshot, RemoteStatusSnapshot, TabSnapshot, TerminalPaneSnapshot,
        WorkspaceRegistration, WorkspaceSnapshot,
    };
    use crate::sidebar::SessionSnapshotPayload;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_RUNTIME_STATE_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn repeated_sync_updates_do_not_start_a_second_terminal_session() {
        assert!(terminal_control_request_allowed("idle", false));
        for state in [
            "starting",
            "controlling",
            "observing",
            "unavailable",
            "ended",
        ] {
            assert!(!terminal_control_request_allowed(state, false), "{state}");
        }
        assert!(!terminal_control_request_allowed("idle", true));
    }

    #[test]
    fn pane_mutation_receipts_never_publish_topology_ahead_of_session_sync() {
        let mut runtime = runtime();
        let pane_id = "w1:p1";
        let layout = PaneLayoutSnapshot {
            workspace_id: "w1".to_owned(),
            tab_id: "w1:t1".to_owned(),
            focused_pane_id: pane_id.to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: pane_id.to_owned(),
            },
        };
        let panes = vec![TerminalPaneSnapshot {
            pane_id: pane_id.to_owned(),
            closed: false,
            ..TerminalPaneSnapshot::default()
        }];
        runtime.snapshot.pane_layout = Some(layout.clone());
        runtime.snapshot.focused.surface = Surface::Terminal;
        runtime.snapshot.focused.pane_id = Some(pane_id.to_owned());
        runtime.snapshot.terminal.pane_id = Some(pane_id.to_owned());
        runtime.snapshot.terminal.panes = panes.clone();
        runtime.snapshot.ui_state.selected_pane_id = Some(pane_id.to_owned());

        let receipts = [
            (
                PaneControlAction::Split {
                    pane_id: pane_id.to_owned(),
                    direction: PaneSplitDirection::Right,
                    cwd: Some("/tmp".to_owned()),
                },
                PaneControlOutcome::Acknowledged {
                    created_pane_id: Some("w1:p2".to_owned()),
                },
            ),
            (
                PaneControlAction::Focus {
                    pane_id: "w1:p2".to_owned(),
                },
                PaneControlOutcome::Acknowledged {
                    created_pane_id: None,
                },
            ),
            (
                PaneControlAction::Resize {
                    pane_id: pane_id.to_owned(),
                    direction: PaneResizeDirection::Right,
                    amount: 0.1,
                },
                PaneControlOutcome::Acknowledged {
                    created_pane_id: None,
                },
            ),
            (
                PaneControlAction::ToggleZoom {
                    pane_id: pane_id.to_owned(),
                },
                PaneControlOutcome::Acknowledged {
                    created_pane_id: None,
                },
            ),
            (
                PaneControlAction::Close {
                    pane_id: pane_id.to_owned(),
                },
                PaneControlOutcome::Acknowledged {
                    created_pane_id: None,
                },
            ),
        ];

        for (action, receipt) in receipts {
            assert!(runtime.ingest_pane_control_result(action, Ok(receipt), 3));
            assert_eq!(runtime.snapshot.pane_layout, Some(layout.clone()));
            assert_eq!(runtime.snapshot.terminal.panes, panes);
            assert_eq!(runtime.snapshot.terminal.pane_id.as_deref(), Some(pane_id));
            assert_eq!(runtime.snapshot.focused.pane_id.as_deref(), Some(pane_id));
            assert_eq!(
                runtime.snapshot.ui_state.selected_pane_id.as_deref(),
                Some(pane_id)
            );
        }
    }

    #[test]
    fn duplicate_inflight_remote_tab_creation_is_observable_and_ignored() {
        let mut runtime = runtime();
        let projected_workspace_id = "remote:mini:workspace:w1";
        let mut remote_workspace = workspace(
            projected_workspace_id,
            "Fixture",
            "/tmp/herdr-ide-remote-tab",
            Vec::new(),
        );
        remote_workspace.remote_target_id = Some("mini".to_owned());
        remote_workspace.device_id = "mini".to_owned();
        runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
            target_id: "mini".to_owned(),
            state: "connected".to_owned(),
            message: None,
            last_checked_at_unix_ms: Some(1),
            session: Some(RemoteSessionSnapshot {
                workspaces: vec![remote_workspace],
                agents: Vec::new(),
                active_tab_ids: Default::default(),
                focused_workspace_id: Some(projected_workspace_id.to_owned()),
                focused_checkout_id: None,
                focused_tab_id: None,
                focused_pane_id: None,
                pane_layouts: Vec::new(),
            }),
            files: RemoteFileListSnapshot::idle(),
        });
        let connector: Arc<dyn crate::herdr_api::ApiConnector> = Arc::new(
            crate::herdr_api::UnixSocketConnector::new("/tmp/herdr-core-never-connect.sock"),
        );
        runtime.install_remote_control(RemoteControlContext::new(
            "mini",
            connector,
            Weak::new(),
            ChangeNotifier::noop(),
        ));
        runtime.remote_tab_creations_in_flight.insert((
            "mini".to_owned(),
            "w1".to_owned(),
            "/tmp/herdr-ide-remote-tab".to_owned(),
            "New tab".to_owned(),
        ));

        assert!(runtime.request_remote_control(RemoteControlPayload {
            target_id: "mini".to_owned(),
            request_id: "request-2".to_owned(),
            request: RemoteControlRequest::CreateTab {
                workspace_id: projected_workspace_id.to_owned(),
                cwd: "/tmp/herdr-ide-remote-tab".to_owned(),
                label: "New tab".to_owned(),
            },
        }));

        let diagnostic = runtime
            .snapshot
            .status
            .diagnostics
            .last()
            .expect("duplicate outcome is visible to the caller");
        assert_eq!(diagnostic.kind, "remote.control.duplicate_tab_ignored");
        assert!(runtime.snapshot.status.last_error.is_none());
    }

    #[test]
    fn local_tab_creation_acknowledgement_preserves_the_created_pane_focus() {
        let mut runtime = runtime();
        runtime.snapshot.terminal.pane_id = Some("w1:p1".to_owned());
        runtime.snapshot.focused.pane_id = Some("w1:p1".to_owned());
        runtime.snapshot.ui_state.selected_pane_id = Some("w1:p1".to_owned());

        assert!(runtime.ingest_local_control_result(
            RemoteControlAction::CreateTab {
                workspace_id: "w1".to_owned(),
                cwd: "/tmp/project".to_owned(),
                label: "2".to_owned(),
            },
            Ok(RemoteControlOutcome::Acknowledged {
                created_tab_id: Some("w1:t2".to_owned()),
                created_pane_id: Some("w1:p2".to_owned()),
            }),
            4,
        ));

        assert_eq!(runtime.snapshot.terminal.pane_id.as_deref(), Some("w1:p2"));
        assert_eq!(runtime.snapshot.focused.pane_id.as_deref(), Some("w1:p2"));
        assert_eq!(
            runtime.snapshot.ui_state.selected_pane_id.as_deref(),
            Some("w1:p2")
        );
    }

    #[test]
    fn remote_session_sync_reconciles_target_scoped_structured_terminals() {
        let mut runtime = runtime();
        runtime.suppress_terminal_session_workers = true;
        runtime.snapshot.navigator.devices.push(DeviceSnapshot {
            id: "mini".to_owned(),
            label: "Mac mini".to_owned(),
            kind: "remote".to_owned(),
            state: "available".to_owned(),
            ssh_alias: Some("mini".to_owned()),
            agent_count: 0,
        });
        runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
            target_id: "mini".to_owned(),
            state: "not_connected".to_owned(),
            message: None,
            last_checked_at_unix_ms: None,
            session: None,
            files: RemoteFileListSnapshot::idle(),
        });
        runtime.snapshot.terminal.panes.push(TerminalPaneSnapshot {
            pane_id: "w-local:p1".to_owned(),
            ..TerminalPaneSnapshot::default()
        });
        let pane_id = "remote:mini:pane:w9:p1";
        let inactive_pane_id = "remote:mini:pane:w9:p2";
        let workspace_id = "remote:mini:workspace:w9";
        let checkout_id = "remote:mini:checkout:w9";
        let active_tab_id = "remote:mini:tab:w9:t1";
        let inactive_tab_id = "remote:mini:tab:w9:t2";
        let mut remote_checkout = checkout(
            workspace_id,
            checkout_id,
            "/tmp/herdr-remote-terminal",
            Some(pane(pane_id, "/tmp/herdr-remote-terminal")),
        );
        remote_checkout.tabs[0].id = Some(active_tab_id.to_owned());
        remote_checkout.tabs.push(TabSnapshot {
            id: Some(inactive_tab_id.to_owned()),
            workspace_id: Some(workspace_id.to_owned()),
            checkout_id: Some(checkout_id.to_owned()),
            label: Some("Inactive".to_owned()),
            empty: false,
            panes: vec![pane(inactive_pane_id, "/tmp/herdr-remote-terminal")],
        });
        let mut remote_workspace = workspace(
            workspace_id,
            "Remote fixture",
            "/tmp/herdr-remote-terminal",
            vec![remote_checkout],
        );
        remote_workspace.remote_target_id = Some("mini".to_owned());
        remote_workspace.device_id = "mini".to_owned();
        let session = RemoteSessionSnapshot {
            workspaces: vec![remote_workspace],
            agents: Vec::new(),
            active_tab_ids: [(workspace_id.to_owned(), active_tab_id.to_owned())]
                .into_iter()
                .collect(),
            focused_workspace_id: Some(workspace_id.to_owned()),
            focused_checkout_id: Some(checkout_id.to_owned()),
            focused_tab_id: Some(active_tab_id.to_owned()),
            focused_pane_id: Some(pane_id.to_owned()),
            pane_layouts: Vec::new(),
        };

        assert!(runtime.ingest_remote_session("mini", Ok(session.clone())));
        assert!(runtime.terminal_sessions.is_empty());
        assert!(
            runtime
                .snapshot
                .terminal
                .panes
                .iter()
                .filter(|pane| pane.pane_id.starts_with("remote:mini:pane:"))
                .all(|pane| pane.transport_state == "idle")
        );

        let focus_remote = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_device",
            "payload": {"device_id": "mini"}
        }))
        .expect("focus remote event");
        assert!(runtime.dispatch_json(&focus_remote));
        assert_eq!(
            runtime.terminal_sessions[pane_id].mode,
            TerminalSessionMode::Control
        );
        assert_eq!(
            runtime.terminal_session_lifecycles[pane_id].state,
            "controlling"
        );
        assert!(
            runtime
                .snapshot
                .terminal
                .panes
                .iter()
                .any(|pane| pane.pane_id == pane_id)
        );
        assert!(!runtime.terminal_sessions.contains_key(inactive_pane_id));
        assert_eq!(
            runtime
                .snapshot
                .terminal
                .panes
                .iter()
                .find(|pane| pane.pane_id == inactive_pane_id)
                .expect("inactive pane remains projected")
                .transport_state,
            "idle"
        );
        assert!(!runtime.ingest_remote_session("mini", Ok(session)));

        let focus_local = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_device",
            "payload": {"device_id": "local"}
        }))
        .expect("focus local event");
        assert!(runtime.dispatch_json(&focus_local));
        assert!(runtime.terminal_sessions.is_empty());
        assert_eq!(
            runtime
                .snapshot
                .terminal
                .panes
                .iter()
                .find(|pane| pane.pane_id == pane_id)
                .expect("inactive target pane remains projected")
                .transport_state,
            "idle"
        );

        assert!(runtime.ingest_remote_session(
            "mini",
            Ok(RemoteSessionSnapshot {
                workspaces: Vec::new(),
                agents: Vec::new(),
                active_tab_ids: Default::default(),
                focused_workspace_id: None,
                focused_checkout_id: None,
                focused_tab_id: None,
                focused_pane_id: None,
                pane_layouts: Vec::new(),
            })
        ));
        assert!(!runtime.terminal_sessions.contains_key(pane_id));
        assert!(
            runtime
                .snapshot
                .terminal
                .panes
                .iter()
                .any(|pane| pane.pane_id == "w-local:p1")
        );
        assert!(
            runtime
                .snapshot
                .terminal
                .panes
                .iter()
                .all(|pane| pane.pane_id != pane_id)
        );
    }

    #[test]
    fn remote_file_results_are_scoped_sorted_and_generation_guarded() {
        let mut runtime = runtime();
        let root_path = "/private/tmp/herdr-remote-files";
        let workspace_id = "remote:mini:workspace:w9";
        let checkout_id = "remote:mini:checkout:w9";
        let mut remote_workspace = workspace(
            workspace_id,
            "Remote files",
            root_path,
            vec![checkout(workspace_id, checkout_id, root_path, None)],
        );
        remote_workspace.remote_target_id = Some("mini".to_owned());
        remote_workspace.device_id = "mini".to_owned();
        runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
            target_id: "mini".to_owned(),
            state: "connected".to_owned(),
            message: None,
            last_checked_at_unix_ms: Some(1),
            session: Some(RemoteSessionSnapshot {
                workspaces: vec![remote_workspace],
                agents: Vec::new(),
                active_tab_ids: Default::default(),
                focused_workspace_id: Some(workspace_id.to_owned()),
                focused_checkout_id: Some(checkout_id.to_owned()),
                focused_tab_id: None,
                focused_pane_id: None,
                pane_layouts: Vec::new(),
            }),
            files: RemoteFileListSnapshot::idle(),
        });

        let request = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "remote_file_list",
            "payload": {"target_id": "mini", "root_path": root_path}
        }))
        .expect("remote file event");
        assert!(runtime.dispatch_json(&request));
        assert_eq!(runtime.snapshot.status.remote[0].files.state, "unavailable");
        assert_eq!(
            runtime
                .snapshot
                .status
                .last_error
                .as_ref()
                .expect("missing SFTP transport is externally visible")
                .kind,
            "remote.files.transport_unavailable"
        );

        runtime.snapshot.status.remote[0].files = RemoteFileListSnapshot {
            root_path: Some(root_path.to_owned()),
            state: "loading".to_owned(),
            entries: Vec::new(),
            message: None,
            generation: 7,
        };
        assert!(runtime.ingest_remote_file_list_result(
            "mini",
            root_path,
            7,
            Ok(vec![
                FileEntry {
                    path: format!("{root_path}/zeta.txt"),
                    name: "zeta.txt".to_owned(),
                    kind: FileKind::File,
                    size_bytes: 4,
                },
                FileEntry {
                    path: format!("{root_path}/Sources"),
                    name: "Sources".to_owned(),
                    kind: FileKind::Directory,
                    size_bytes: 96,
                },
            ]),
        ));
        let files = &runtime.snapshot.status.remote[0].files;
        assert_eq!(files.state, "ready");
        assert_eq!(files.entries[0].name, "Sources");
        assert!(files.entries[0].is_directory);
        assert_eq!(files.entries[1].name, "zeta.txt");

        runtime.snapshot.status.remote[0].files = RemoteFileListSnapshot {
            root_path: Some(root_path.to_owned()),
            state: "loading".to_owned(),
            entries: Vec::new(),
            message: None,
            generation: 8,
        };
        assert!(runtime.ingest_remote_file_list_result("mini", root_path, 7, Ok(Vec::new()),));
        assert_eq!(runtime.snapshot.status.remote[0].files.state, "loading");
        assert_eq!(runtime.snapshot.status.remote[0].files.generation, 8);
        assert_eq!(
            runtime
                .snapshot
                .status
                .diagnostics
                .last()
                .expect("stale result is observable")
                .kind,
            "remote.files.stale"
        );

        runtime.next_remote_file_generation = 8;
        runtime.snapshot.status.remote[0].state = "stale".to_owned();
        assert!(runtime.dispatch_json(&request));
        assert_eq!(runtime.snapshot.status.remote[0].files.state, "unavailable");
        assert_eq!(runtime.snapshot.status.remote[0].files.generation, 9);
        assert!(runtime.ingest_remote_file_list_result("mini", root_path, 8, Ok(Vec::new())));
        assert_eq!(runtime.snapshot.status.remote[0].files.state, "unavailable");
        assert_eq!(runtime.snapshot.status.remote[0].files.generation, 9);
    }

    #[test]
    fn runtime_owner_conflict_observes_ignores_stale_delivery_and_reconnects_once() {
        let mut runtime = runtime();
        runtime.suppress_terminal_session_workers = true;
        runtime.snapshot.navigator.workspaces = vec![workspace(
            "w1",
            "Fixture",
            "/tmp/hide-terminal-session-runtime",
            vec![checkout(
                "w1",
                "checkout-1",
                "/tmp/hide-terminal-session-runtime",
                Some(pane("w1:p1", "/tmp/hide-terminal-session-runtime")),
            )],
        )];
        runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
            pane_id: "w1:p1".to_owned(),
            closed: false,
            ..TerminalPaneSnapshot::default()
        }];
        runtime.next_terminal_session_generation = 40;
        runtime
            .terminal_session_generations
            .insert("w1:p1".to_owned(), 40);
        runtime.terminal_session_lifecycles.insert(
            "w1:p1".to_owned(),
            TerminalSessionLifecycle {
                state: "controlling",
                generation: 40,
                attempt: 1,
                mode: Some(TerminalSessionMode::Control),
                retry_decision: "none",
                ..TerminalSessionLifecycle::default()
            },
        );
        runtime.terminal_sessions.insert(
            "w1:p1".to_owned(),
            TerminalSession::test_stub("w1:p1", 40, TerminalSessionMode::Control),
        );

        let owner_conflict = "terminal attach failed: terminal 42 already has an attached client; retry with --takeover";
        assert!(runtime.ingest_terminal_session_closed(
            "w1:p1",
            40,
            TerminalSessionMode::Control,
            Some(owner_conflict.to_owned()),
        ));
        let observing = runtime
            .terminal_session_lifecycles
            .get("w1:p1")
            .expect("observer lifecycle");
        assert_eq!(observing.state, "observing");
        assert_eq!(observing.mode, Some(TerminalSessionMode::Observe));
        assert_eq!(observing.generation, 41);
        assert_eq!(observing.attempt, 1);
        assert_eq!(runtime.terminal_sessions.len(), 1);
        assert_eq!(
            runtime.terminal_sessions["w1:p1"].mode,
            TerminalSessionMode::Observe
        );
        assert!(
            !runtime.snapshot.terminal.panes[0].closed,
            "transport owner conflict must not close the authoritative pane"
        );

        let chunk_count = runtime.snapshot.terminal.chunks.len();
        assert!(!runtime.ingest_terminal_session_frame(
            "w1:p1",
            40,
            TerminalSessionMode::Control,
            b"stale-generation",
        ));
        assert!(!runtime.ingest_terminal_session_frame(
            "w1:p1",
            41,
            TerminalSessionMode::Control,
            b"stale-mode",
        ));
        assert!(!runtime.ingest_terminal_session_closed(
            "w1:p1",
            40,
            TerminalSessionMode::Control,
            Some(owner_conflict.to_owned()),
        ));
        assert_eq!(runtime.snapshot.terminal.chunks.len(), chunk_count);
        assert_eq!(runtime.next_terminal_session_generation, 41);

        let reconnect = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "reconnect_pane",
            "payload": {"pane_id": "w1:p1"}
        }))
        .expect("reconnect event");
        assert!(runtime.dispatch_json(&reconnect));
        let controlling = runtime
            .terminal_session_lifecycles
            .get("w1:p1")
            .expect("controller lifecycle");
        assert_eq!(controlling.state, "controlling");
        assert_eq!(controlling.mode, Some(TerminalSessionMode::Control));
        assert_eq!(controlling.generation, 42);
        assert_eq!(controlling.attempt, 2);
        assert_eq!(runtime.terminal_sessions.len(), 1);

        runtime.request_terminal_control("w1:p1");
        assert_eq!(runtime.next_terminal_session_generation, 42);
        assert_eq!(
            runtime
                .terminal_session_lifecycles
                .get("w1:p1")
                .expect("same controller")
                .attempt,
            2
        );
    }

    #[test]
    fn workspace_creation_failures_retire_inflight_and_keep_partial_registration_visible() {
        let mut runtime = runtime();
        let missing_path = "/tmp/hide-workspace-missing";
        runtime
            .workspace_creations_in_flight
            .insert(missing_path.to_owned());

        assert!(runtime.ingest_workspace_creation(
            missing_path,
            Err("Workspace path does not exist".to_owned()),
            4,
        ));
        assert!(!runtime.workspace_creations_in_flight.contains(missing_path));
        assert_eq!(
            runtime
                .snapshot
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.as_str()),
            Some("workspace.create_failed")
        );

        let partial_path = "/tmp/hide-workspace-partial";
        let registration = WorkspaceRegistration {
            id: "workspace:partial".to_owned(),
            label: "Partial".to_owned(),
            path: partial_path.to_owned(),
            device_id: workspace::LOCAL_DEVICE_ID.to_owned(),
        };
        runtime
            .workspace_creations_in_flight
            .insert(partial_path.to_owned());

        assert!(
            runtime.ingest_workspace_creation(
                partial_path,
                Ok(live::WorkspaceCreationOutcome {
                    registration: registration.clone(),
                    base_registrations: Vec::new(),
                    registrations: vec![registration.clone()],
                    workspaces: Vec::new(),
                    session: serde_json::from_value(serde_json::json!({
                        "agents": [],
                        "layouts": [],
                    }))
                    .expect("empty session payload"),
                    created_pane_id: None,
                    git_init_error: Some("git init failed explicitly".to_owned()),
                }),
                7,
            )
        );
        assert!(!runtime.workspace_creations_in_flight.contains(partial_path));
        assert_eq!(
            runtime.snapshot.ui_state.workspace_registrations,
            [registration]
        );
        let error = runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .expect("partial failure stays visible");
        assert_eq!(error.kind, "workspace.git_init_failed");
        assert!(error.message.contains("registered"));
        assert!(error.message.contains("git init failed explicitly"));
    }

    fn runtime() -> Runtime {
        let state_id = NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed);
        let options = CoreOptions {
            schema_version: SCHEMA_VERSION,
            herdr_socket_path: Some("/tmp/herdr-core-pet-runtime.sock".to_owned()),
            herdr_bin_path: None,
            remote_targets: Vec::new(),
            app_state_path: std::env::temp_dir()
                .join(format!(
                    "herdr-core-pet-runtime-{}-{}.json",
                    std::process::id(),
                    state_id
                ))
                .to_string_lossy()
                .into_owned(),
        };
        Runtime::new(
            options,
            environment::EnvironmentReport {
                statuses: Vec::new(),
                remote_enabled: false,
                chromux_enabled: false,
                herdr_socket_path_override: None,
                home_path: None,
            },
        )
    }

    /// R11/AC16: the chords move one pane's scale within bounds and reset it,
    /// and the store keeps only the panes the user actually changed.
    #[test]
    fn pane_text_scale_steps_within_bounds_and_leaves_other_panes_alone() {
        let mut runtime = runtime();
        let scale = |pane: &str, direction: &str| {
            serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "kind": "pane_text_scale",
                "payload": {"pane_id": pane, "direction": direction}
            }))
            .expect("pane text scale event")
        };

        assert!(runtime.dispatch_json(&scale("w1:p1", "in")));
        assert_eq!(
            runtime.snapshot().ui_state.pane_text_scales.get("w1:p1"),
            Some(&1.1)
        );
        // A second pane is untouched by the first pane's zoom.
        assert!(!runtime.snapshot().ui_state.pane_text_scales.contains_key("w1:p2"));

        // The upper bound holds however many times it is pressed, and a press
        // that changes nothing reports no change.
        for _ in 0..40 {
            runtime.dispatch_json(&scale("w1:p1", "in"));
        }
        assert_eq!(
            runtime.snapshot().ui_state.pane_text_scales.get("w1:p1"),
            Some(&MAX_PANE_TEXT_SCALE)
        );
        assert!(!runtime.dispatch_json(&scale("w1:p1", "in")));

        for _ in 0..40 {
            runtime.dispatch_json(&scale("w1:p1", "out"));
        }
        assert_eq!(
            runtime.snapshot().ui_state.pane_text_scales.get("w1:p1"),
            Some(&MIN_PANE_TEXT_SCALE)
        );

        // Reset drops the row rather than storing the default.
        assert!(runtime.dispatch_json(&scale("w1:p1", "reset")));
        assert!(runtime.snapshot().ui_state.pane_text_scales.is_empty());
        assert!(!runtime.dispatch_json(&scale("w1:p1", "reset")));

        // An unknown direction is surfaced, not silently ignored.
        assert!(runtime.dispatch_json(&scale("w1:p1", "sideways")));
        assert_eq!(
            runtime.snapshot().status.last_error.as_ref().map(|error| error.kind.clone()),
            Some("pane.text_scale_unknown_direction".to_owned())
        );
    }

    fn working_payload() -> SessionSnapshotPayload {
        serde_json::from_value(serde_json::json!({
            "agents": [{
                "pane_id": "w1:p1",
                "workspace_label": "Fixture",
                "agent": "codex",
                "agent_status": "working",
                "tokens": {"status_working": "\u{25cf}", "sort_rank": "05",
                           "activity": "0000000000001"}
            }],
            "layouts": [{
                "workspace_id": "w1", "tab_id": "t1", "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w1:p1",
                "panes": [{"pane_id": "w1:p1",
                           "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }]
        }))
        .expect("session payload")
    }

    fn pane(id: &str, cwd: &str) -> PaneSnapshot {
        PaneSnapshot {
            id: id.to_owned(),
            herdr_label: None,
            terminal_title: None,
            workspace_label: None,
            cwd: cwd.to_owned(),
            state: "attached".to_owned(),
            summary: None,
            activity_at_unix_ms: None,
            fork: PaneForkSnapshot::default(),
            ports: Vec::new(),
        }
    }

    fn tab(workspace_id: &str, checkout_id: &str, pane: Option<PaneSnapshot>) -> TabSnapshot {
        TabSnapshot {
            id: Some(format!("{checkout_id}:tab")),
            workspace_id: Some(workspace_id.to_owned()),
            checkout_id: Some(checkout_id.to_owned()),
            label: Some("Session".to_owned()),
            empty: pane.is_none(),
            panes: pane.into_iter().collect(),
        }
    }

    fn checkout(
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        pane: Option<PaneSnapshot>,
    ) -> CheckoutSnapshot {
        CheckoutSnapshot {
            id: checkout_id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            label: checkout_id.to_owned(),
            path: path.to_owned(),
            branch: None,
            is_worktree: false,
            exists: true,
            temporary: false,
            tabs: pane
                .clone()
                .map(|pane| vec![tab(workspace_id, checkout_id, Some(pane))])
                .unwrap_or_default(),
        }
    }

    fn workspace(
        id: &str,
        label: &str,
        path: &str,
        checkouts: Vec<CheckoutSnapshot>,
    ) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            id: id.to_owned(),
            label: label.to_owned(),
            path: path.to_owned(),
            remote_target_id: None,
            expanded: true,
            device_id: "local".to_owned(),
            repo_name: label.to_owned(),
            is_git: false,
            default_branch: None,
            registered: true,
            temporary: false,
            session_workspace_ids: Vec::new(),
            checkouts,
        }
    }

    #[test]
    fn ui_state_update_applies_workspace_expansion_without_waiting_for_sync() {
        let mut runtime = runtime();
        runtime.snapshot.navigator.workspaces = vec![workspace(
            "workspace-a",
            "A",
            "/tmp/hide-runtime-a",
            Vec::new(),
        )];
        let collapse = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": [],
                "collapsed_workspace_ids": ["workspace-a"],
                "selected_path": null,
                "selected_pane_id": null,
                "shortcut_bindings": {}
            }
        }))
        .expect("collapse workspace event");

        assert!(runtime.dispatch_json(&collapse));
        assert!(!runtime.snapshot().navigator.workspaces[0].expanded);

        let expand = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": [],
                "collapsed_workspace_ids": [],
                "selected_path": null,
                "selected_pane_id": null,
                "shortcut_bindings": {}
            }
        }))
        .expect("expand workspace event");

        assert!(runtime.dispatch_json(&expand));
        assert!(runtime.snapshot().navigator.workspaces[0].expanded);
    }

    #[test]
    fn focusing_checkout_selects_only_the_target_checkout_pane() {
        let mut runtime = runtime();
        let workspace_a = workspace(
            "workspace-a",
            "A",
            "/tmp/hide-runtime-a",
            vec![checkout(
                "workspace-a",
                "checkout-a",
                "/tmp/hide-runtime-a",
                Some(pane("pane-a", "/tmp/hide-runtime-a")),
            )],
        );
        let workspace_b = workspace(
            "workspace-b",
            "B",
            "/tmp/hide-runtime-b",
            vec![
                checkout(
                    "workspace-b",
                    "checkout-b",
                    "/tmp/hide-runtime-b",
                    Some(pane("pane-b", "/tmp/hide-runtime-b")),
                ),
                checkout(
                    "workspace-b",
                    "checkout-empty",
                    "/tmp/hide-runtime-empty",
                    None,
                ),
            ],
        );
        runtime.snapshot.navigator.workspaces = vec![workspace_a, workspace_b];
        runtime.snapshot.navigator.focused_workspace_id = Some("workspace-a".to_owned());
        runtime.snapshot.navigator.focused_checkout_id = Some("checkout-a".to_owned());
        runtime.snapshot.navigator.root_path = Some("/tmp/hide-runtime-a".to_owned());
        runtime.snapshot.terminal.pane_id = Some("pane-a".to_owned());
        runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
            pane_id: "pane-a".to_owned(),
            closed: false,
            exit_code: None,
            ..TerminalPaneSnapshot::default()
        }];
        runtime.snapshot.ui_state.selected_pane_id = Some("pane-a".to_owned());

        let focus_b = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_checkout",
            "payload": {"workspace_id": "workspace-b", "checkout_id": "checkout-b"}
        }))
        .expect("focus B event");
        assert!(runtime.dispatch_json(&focus_b));
        assert_eq!(
            runtime.snapshot().navigator.focused_workspace_id.as_deref(),
            Some("workspace-b")
        );
        assert_eq!(
            runtime.snapshot().navigator.focused_checkout_id.as_deref(),
            Some("checkout-b")
        );
        assert_eq!(
            runtime.snapshot().navigator.root_path.as_deref(),
            Some("/tmp/hide-runtime-b")
        );
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("pane-b")
        );
        assert_eq!(
            runtime.snapshot().ui_state.selected_pane_id.as_deref(),
            Some("pane-b")
        );
        assert!(runtime.snapshot().terminal.panes.is_empty());

        let focus_empty = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_checkout",
            "payload": {"workspace_id": "workspace-b", "checkout_id": "checkout-empty"}
        }))
        .expect("focus pane-less checkout event");
        assert!(runtime.dispatch_json(&focus_empty));
        assert_eq!(
            runtime.snapshot().navigator.root_path.as_deref(),
            Some("/tmp/hide-runtime-empty")
        );
        assert!(runtime.snapshot().terminal.pane_id.is_none());
        assert!(runtime.snapshot().pane_layout.is_none());
        assert!(runtime.snapshot().terminal.panes.is_empty());
    }

    #[test]
    fn a_plain_terminal_pane_cwd_is_reconciled_into_its_checkout() {
        let mut runtime = runtime();
        let checkout_path = "/private/tmp/hide-registered-checkout";
        runtime.snapshot.ui_state.workspace_registrations = vec![WorkspaceRegistration {
            id: "workspace:registered".to_owned(),
            label: "registered".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        }];
        runtime.rebuild_catalog();
        let checkout_id =
            workspace::checkout_id_for_path("workspace:registered", Path::new(checkout_path));
        runtime.snapshot.navigator.focused_workspace_id = Some("workspace:registered".to_owned());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
        runtime.reset_terminal_projection(None);

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "tabs": [{
                "workspace_id": "herdr-workspace",
                "tab_id": "herdr-workspace:t1",
                "label": "2"
            }],
            "panes": [{"pane_id": "plain:p1", "cwd": checkout_path}],
            "layouts": [{
                "workspace_id": "herdr-workspace",
                "tab_id": "herdr-workspace:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "plain:p1",
                "panes": [{"pane_id": "plain:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }]
        }))
        .expect("plain pane payload");

        // Before the session arrives the registration is the only row.
        assert_eq!(runtime.snapshot.navigator.workspaces.len(), 1);
        assert_eq!(runtime.snapshot.navigator.workspaces[0].checkouts.len(), 1);
        assert!(runtime.ingest_session(Ok(payload)));
        // Once Herdr has a workspace in that directory the registration's
        // row carries it, not a second entry beside it.
        assert_eq!(runtime.snapshot().navigator.workspaces.len(), 1);
        assert_eq!(
            runtime.snapshot().navigator.workspaces[0].session_workspace_ids,
            vec!["herdr-workspace".to_owned()]
        );
        let checkout = runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "workspace:registered")
            .and_then(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == checkout_id)
            })
            .expect("the checkout Herdr occupies");
        assert_eq!(
            checkout.tabs.len(),
            1,
            "plain pane layout should create one checkout tab"
        );
        assert_eq!(checkout.tabs[0].panes[0].id, "plain:p1");
        assert_eq!(checkout.tabs[0].label.as_deref(), Some("2"));
        assert_eq!(checkout.tabs[0].panes[0].cwd, checkout_path);
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("plain:p1")
        );
        assert_eq!(
            runtime
                .snapshot()
                .pane_layout
                .as_ref()
                .map(|layout| layout.focused_pane_id.as_str()),
            Some("plain:p1")
        );
    }

    #[test]
    fn a_pane_in_a_second_directory_projects_into_its_own_project() {
        let mut runtime = runtime();
        let repository_path = "/private/tmp/hide-rebrand/herdr-ide";
        let checkout_path = "/private/tmp/hide-rebrand/worktrees/hide-rebrand";
        // Neither path is a git repository here, so each is its own
        // project: a Herdr workspace spanning two directories is two rows,
        // each keyed by its own path. (A real worktree folds into its main
        // repository's project; `workspace::tests` covers that with git.)
        let spaces = vec![workspace::SessionSpace {
            id: "w3M".to_owned(),
            label: "herdr-ide".to_owned(),
            cwds: vec![repository_path.to_owned(), checkout_path.to_owned()],
        }];
        let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
        let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
        runtime.snapshot.navigator.workspaces = workspace::build_catalog(&[], &spaces);
        runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
        runtime.reset_terminal_projection(None);

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [{"workspace_id": "w3M", "label": "herdr-ide"}],
            "panes": [{"pane_id": "w3M:p1", "cwd": checkout_path}],
            "layouts": [{
                "workspace_id": "w3M",
                "tab_id": "w3M:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w3M:p1",
                "panes": [{"pane_id": "w3M:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }]
        }))
        .expect("second directory pane payload");
        let catalog = session_sync::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: workspace::build_catalog(&[], &spaces),
        };

        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
        assert_eq!(runtime.snapshot().navigator.workspaces.len(), 2);
        let workspace_snapshot = runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .expect("the second directory's project")
            .clone();
        assert_eq!(workspace_snapshot.label, "hide-rebrand");
        assert_eq!(workspace_snapshot.session_workspace_ids, vec!["w3M".to_owned()]);
        assert_eq!(workspace_snapshot.checkouts.len(), 1);
        let checkout = &workspace_snapshot.checkouts[0];
        assert_eq!(checkout.id, checkout_id);
        assert_eq!(checkout.tabs.len(), 1);
        assert_eq!(checkout.tabs[0].panes[0].id, "w3M:p1");
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w3M:p1")
        );
    }

    #[test]
    fn another_workspace_layout_does_not_steal_the_selected_checkout_projection() {
        let mut runtime = runtime();
        let checkout_path = "/private/tmp/hide-selected-checkout";
        let spaces = vec![
            workspace::SessionSpace {
                id: "w3P".to_owned(),
                label: "other".to_owned(),
                cwds: vec![checkout_path.to_owned()],
            },
            workspace::SessionSpace {
                id: "w3Z".to_owned(),
                label: "selected".to_owned(),
                cwds: vec![checkout_path.to_owned()],
            },
        ];
        // Two Herdr workspaces in one directory are one project; the selected
        // pane, not the Herdr workspace id, decides which layout is projected.
        let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
        let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
        runtime.snapshot.navigator.workspaces = workspace::build_catalog(&[], &spaces);
        runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.ui_state.selected_pane_id = Some("w3Z:p1".to_owned());
        runtime.snapshot.terminal.pane_id = Some("w3Z:p1".to_owned());
        runtime.restore_hint_pending = false;

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [
                {"workspace_id": "w3P", "label": "other"},
                {"workspace_id": "w3Z", "label": "selected"}
            ],
            "panes": [
                {"pane_id": "w3P:p1", "cwd": checkout_path},
                {"pane_id": "w3Z:p1", "cwd": checkout_path}
            ],
            "layouts": [
                {
                    "workspace_id": "w3P",
                    "tab_id": "w3P:t1",
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "w3P:p1",
                    "panes": [{"pane_id": "w3P:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                },
                {
                    "workspace_id": "w3Z",
                    "tab_id": "w3Z:t1",
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "w3Z:p1",
                    "panes": [{"pane_id": "w3Z:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                }
            ]
        }))
        .expect("two workspace layout payload");
        let catalog = session_sync::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: workspace::build_catalog(&[], &spaces),
        };
        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));

        let selected_checkout = runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .and_then(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == checkout_id)
            })
            .expect("the selected checkout")
            .clone();
        assert!(selected_checkout.tabs.iter().any(|tab| {
            tab.id.as_deref() == Some("w3Z:t1") && tab.panes.iter().any(|pane| pane.id == "w3Z:p1")
        }));
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w3Z:p1")
        );
        assert_eq!(
            runtime
                .snapshot()
                .pane_layout
                .as_ref()
                .map(|layout| (layout.workspace_id.as_str(), layout.tab_id.as_str())),
            Some(("w3Z", "w3Z:t1"))
        );
    }

    #[test]
    fn an_exited_panes_root_directory_does_not_become_a_checkout() {
        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [{"workspace_id": "w2W", "label": "modakbul"}],
            "panes": [
                {"pane_id": "w2W:p1", "cwd": "/private/tmp/hide-modakbul"},
                {"pane_id": "w2W:pM", "cwd": "/"}
            ],
            "layouts": [{
                "workspace_id": "w2W",
                "tab_id": "w2W:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w2W:p1",
                "panes": [
                    {"pane_id": "w2W:p1", "rect": {"x": 0, "y": 0, "width": 40, "height": 24}},
                    {"pane_id": "w2W:pM", "rect": {"x": 40, "y": 0, "width": 40, "height": 24}}
                ],
                "splits": []
            }]
        }))
        .expect("exited pane payload");

        let spaces = Runtime::session_spaces(&payload);

        assert_eq!(spaces.len(), 1);
        assert_eq!(
            spaces[0].cwds,
            vec!["/private/tmp/hide-modakbul".to_owned()]
        );
    }

    #[test]
    fn a_registration_herdr_already_has_a_workspace_for_is_listed_once() {
        let checkout_path = "/private/tmp/hide-duplicate-registration";
        let spaces = vec![workspace::SessionSpace {
            id: "w41".to_owned(),
            label: "duplicate".to_owned(),
            cwds: vec![checkout_path.to_owned()],
        }];
        let registrations = vec![WorkspaceRegistration {
            id: workspace::workspace_id_for_path(Path::new(checkout_path)),
            label: "Duplicate".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        }];

        let catalog = workspace::build_catalog(&registrations, &spaces);

        // The registration is the row's identity; the Herdr workspace is
        // attached to it rather than replacing it.
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id, registrations[0].id);
        assert_eq!(catalog[0].label, "Duplicate");
        assert!(catalog[0].registered);
        assert_eq!(catalog[0].session_workspace_ids, vec!["w41".to_owned()]);
    }

    /// The user's report: a project that exists only as a Herdr workspace,
    /// one tab, one pane. Closing that pane made Herdr close the workspace,
    /// and the project vanished from the sidebar.
    #[test]
    fn closing_the_last_pane_keeps_an_unregistered_project_listed() {
        let mut runtime = runtime();
        let checkout_path = "/private/tmp/hide-retain-project";
        let spaces = vec![workspace::SessionSpace {
            id: "w5".to_owned(),
            label: "hide main".to_owned(),
            cwds: vec![checkout_path.to_owned()],
        }];
        let project_id = workspace::workspace_id_for_path(Path::new(checkout_path));
        let checkout_id = workspace::checkout_id_for_path(&project_id, Path::new(checkout_path));
        let occupied: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [{"workspace_id": "w5", "label": "hide main"}],
            "tabs": [{"workspace_id": "w5", "tab_id": "w5:t1", "label": "1"}],
            "panes": [{"pane_id": "w5:p1", "cwd": checkout_path}],
            "layouts": [{
                "workspace_id": "w5",
                "tab_id": "w5:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w5:p1",
                "panes": [{"pane_id": "w5:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }]
        }))
        .expect("occupied payload");
        let catalog = session_sync::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: workspace::build_catalog(&[], &spaces),
        };
        runtime.restore_hint_pending = false;
        assert!(runtime.ingest_session_with_catalog(Ok(occupied), Some(catalog)));
        assert!(runtime.focus_checkout(&project_id, &checkout_id));
        assert!(!runtime.snapshot().navigator.workspaces[0].registered);

        runtime.retain_project_before_last_pane_closes("w5:p1");

        let registrations = &runtime.snapshot().ui_state.workspace_registrations;
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].path, checkout_path);
        assert_eq!(registrations[0].id, project_id);
        // Retaining is idempotent: the close path may run again.
        runtime.retain_project_before_last_pane_closes("w5:p1");
        assert_eq!(runtime.snapshot().ui_state.workspace_registrations.len(), 1);

        // Herdr then drops the workspace with the pane.
        let released: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [],
            "tabs": [],
            "panes": [],
            "layouts": []
        }))
        .expect("released payload");
        assert!(runtime.ingest_session(Ok(released)));

        let navigator = &runtime.snapshot().navigator;
        assert_eq!(navigator.workspaces.len(), 1);
        assert_eq!(navigator.workspaces[0].id, project_id);
        assert!(navigator.workspaces[0].registered);
        assert!(navigator.workspaces[0].session_workspace_ids.is_empty());
        assert_eq!(navigator.workspaces[0].checkouts[0].id, checkout_id);
        assert!(navigator.workspaces[0].checkouts[0].tabs.is_empty());
        // The selection survives, so the shell shows this checkout's empty
        // state with its start control rather than "no workspace".
        assert_eq!(navigator.focused_checkout_id.as_deref(), Some(checkout_id.as_str()));
        assert_eq!(navigator.root_path.as_deref(), Some(checkout_path));
    }

    #[test]
    fn a_returned_pane_id_selects_its_layout_when_other_panes_share_the_cwd() {
        let mut runtime = runtime();
        let checkout_path = "/tmp/hide-selected-checkout";
        let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
        let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
        let registration = WorkspaceRegistration {
            id: workspace_id.clone(),
            label: "Selected".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        };
        let selected_workspace = workspace(
            &workspace_id,
            "Selected",
            checkout_path,
            vec![checkout(&workspace_id, &checkout_id, checkout_path, None)],
        );
        runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
        runtime.snapshot.navigator.workspaces = vec![selected_workspace.clone()];
        runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
        runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id.clone());
        runtime.reset_terminal_projection(None);
        runtime.snapshot.terminal.pane_id = Some("w2X:pB".to_owned());
        runtime.snapshot.focused.pane_id = Some("w2X:pB".to_owned());
        runtime.snapshot.pane_layout = Some(PaneLayoutSnapshot {
            workspace_id: "w2X".to_owned(),
            tab_id: "w2X:t1".to_owned(),
            focused_pane_id: "w2X:pB".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w2X:pB".to_owned(),
            },
        });
        runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
            pane_id: "w2X:pB".to_owned(),
            closed: false,
            exit_code: None,
            ..TerminalPaneSnapshot::default()
        }];

        let selected_pane = "w3V:p1";
        let select_pane = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": [],
                "selected_path": null,
                "selected_pane_id": selected_pane,
                "focused_checkout_id": checkout_id,
                "shortcut_bindings": {},
                "accent_hex": "#B9FF66",
                "font_size": 13
            }
        }))
        .expect("selected pane state event");
        assert!(runtime.dispatch_json(&select_pane));
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some(selected_pane)
        );
        assert_eq!(
            runtime.snapshot().focused.pane_id.as_deref(),
            Some(selected_pane)
        );
        assert!(runtime.snapshot().pane_layout.is_none());
        assert!(runtime.snapshot().terminal.panes.is_empty());
        assert_eq!(
            runtime.snapshot().ui_state.focused_checkout_id.as_deref(),
            Some(checkout_id.as_str())
        );
        assert_eq!(
            runtime
                .snapshot()
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.as_str()),
            Some("pane.projection_unavailable")
        );

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [
                {"pane_id": "w2X:pB", "cwd": checkout_path},
                {"pane_id": selected_pane, "cwd": checkout_path}
            ],
            "layouts": [
                {
                    "workspace_id": "w2X",
                    "tab_id": "w2X:t1",
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "w2X:pB",
                    "panes": [{"pane_id": "w2X:pB", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                },
                {
                    "workspace_id": "w3V",
                    "tab_id": "w3V:t1",
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": selected_pane,
                    "panes": [{"pane_id": selected_pane, "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                }
            ]
        }))
        .expect("overlapping cwd session payload");
        let catalog = session_sync::PrecomputedCatalog {
            registrations: vec![registration],
            workspaces: vec![selected_workspace],
        };

        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
        let checkout = runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .and_then(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == checkout_id)
            })
            .expect("selected checkout");
        assert!(checkout.tabs.iter().any(|tab| {
            tab.id.as_deref() == Some("w3V:t1")
                && tab.panes.iter().any(|pane| pane.id == selected_pane)
        }));
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some(selected_pane)
        );
        assert_eq!(
            runtime
                .snapshot()
                .pane_layout
                .as_ref()
                .map(|layout| (layout.workspace_id.as_str(), layout.tab_id.as_str())),
            Some(("w3V", "w3V:t1"))
        );
        assert!(runtime.snapshot().status.last_error.is_none());
    }

    #[test]
    fn a_closed_projected_pane_retargets_to_the_remaining_pane_in_its_checkout() {
        let mut runtime = runtime();
        let checkout_path = "/tmp/hide-closed-selected-pane";
        let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
        let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
        let registration = WorkspaceRegistration {
            id: workspace_id.clone(),
            label: "Closed pane".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        };
        let previous_workspace = workspace(
            &workspace_id,
            "Closed pane",
            checkout_path,
            vec![checkout(
                &workspace_id,
                &checkout_id,
                checkout_path,
                Some(pane("w-close:p1", checkout_path)),
            )],
        );
        let current_workspace = workspace(
            &workspace_id,
            "Closed pane",
            checkout_path,
            vec![checkout(&workspace_id, &checkout_id, checkout_path, None)],
        );
        runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
        runtime.snapshot.navigator.workspaces = vec![previous_workspace];
        runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id);
        runtime.snapshot.ui_state.selected_pane_id = Some("w-close:p1".to_owned());
        runtime.snapshot.terminal.pane_id = Some("w-close:p1".to_owned());
        runtime.snapshot.focused.pane_id = Some("w-close:p1".to_owned());
        runtime.snapshot.pane_layout = Some(PaneLayoutSnapshot {
            workspace_id: "w-close".to_owned(),
            tab_id: "w-close:t1".to_owned(),
            focused_pane_id: "w-close:p1".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w-close:p1".to_owned(),
            },
        });
        runtime.restore_hint_pending = false;

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [{"pane_id": "w-close:p2", "cwd": checkout_path}],
            "focused_pane_id": "w-close:p2",
            "layouts": [{
                "workspace_id": "w-close",
                "tab_id": "w-close:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w-close:p2",
                "panes": [{
                    "pane_id": "w-close:p2",
                    "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                }],
                "splits": []
            }]
        }))
        .expect("remaining pane payload");

        assert!(runtime.ingest_session_with_catalog(
            Ok(payload),
            Some(session_sync::PrecomputedCatalog {
                registrations: vec![registration],
                workspaces: vec![current_workspace],
            }),
        ));
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w-close:p2")
        );
        assert_eq!(
            runtime.snapshot().ui_state.selected_pane_id.as_deref(),
            Some("w-close:p2")
        );
        assert_eq!(
            runtime
                .snapshot()
                .pane_layout
                .as_ref()
                .map(|layout| layout.focused_pane_id.as_str()),
            Some("w-close:p2")
        );
        assert!(runtime.snapshot().status.last_error.is_none());
    }

    #[test]
    fn closing_the_last_projected_pane_leaves_an_empty_checkout_without_an_error() {
        let mut runtime = runtime();
        let checkout_path = "/tmp/hide-last-closed-pane";
        let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
        let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
        let registration = WorkspaceRegistration {
            id: workspace_id.clone(),
            label: "Last pane".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        };
        let previous_workspace = workspace(
            &workspace_id,
            "Last pane",
            checkout_path,
            vec![checkout(
                &workspace_id,
                &checkout_id,
                checkout_path,
                Some(pane("w-last:p1", checkout_path)),
            )],
        );
        let current_workspace = workspace(
            &workspace_id,
            "Last pane",
            checkout_path,
            vec![checkout(&workspace_id, &checkout_id, checkout_path, None)],
        );
        runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
        runtime.snapshot.navigator.workspaces = vec![previous_workspace];
        runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id);
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id);
        runtime.snapshot.ui_state.selected_pane_id = Some("w-last:p1".to_owned());
        runtime.snapshot.terminal.pane_id = Some("w-last:p1".to_owned());
        runtime.snapshot.focused.pane_id = Some("w-last:p1".to_owned());
        runtime.snapshot.pane_layout = Some(PaneLayoutSnapshot {
            workspace_id: "w-last".to_owned(),
            tab_id: "w-last:t1".to_owned(),
            focused_pane_id: "w-last:p1".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w-last:p1".to_owned(),
            },
        });
        runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
            pane_id: "w-last:p1".to_owned(),
            closed: false,
            exit_code: None,
            ..TerminalPaneSnapshot::default()
        }];
        runtime.restore_hint_pending = false;

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [],
            "layouts": []
        }))
        .expect("empty session payload");

        assert!(runtime.ingest_session_with_catalog(
            Ok(payload),
            Some(session_sync::PrecomputedCatalog {
                registrations: vec![registration],
                workspaces: vec![current_workspace],
            }),
        ));
        assert_eq!(runtime.snapshot().terminal.pane_id, None);
        assert_eq!(runtime.snapshot().focused.pane_id, None);
        assert_eq!(runtime.snapshot().ui_state.selected_pane_id, None);
        assert!(runtime.snapshot().pane_layout.is_none());
        assert!(runtime.snapshot().terminal.panes.is_empty());
        assert!(runtime.snapshot().status.last_error.is_none());
    }

    #[test]
    fn a_foreign_stale_projection_is_not_mistaken_for_checkout_pane_retirement() {
        let mut runtime = runtime();
        let checkout_path = "/tmp/hide-foreign-stale-projection";
        let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
        let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
        let registration = WorkspaceRegistration {
            id: workspace_id.clone(),
            label: "Focused checkout".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        };
        let previous_workspace = workspace(
            &workspace_id,
            "Focused checkout",
            checkout_path,
            vec![checkout(
                &workspace_id,
                &checkout_id,
                checkout_path,
                Some(pane("w-focused:p1", checkout_path)),
            )],
        );
        let current_workspace = workspace(
            &workspace_id,
            "Focused checkout",
            checkout_path,
            vec![checkout(
                &workspace_id,
                &checkout_id,
                checkout_path,
                Some(pane("w-focused:p2", checkout_path)),
            )],
        );
        runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
        runtime.snapshot.navigator.workspaces = vec![previous_workspace];
        runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id);
        runtime.snapshot.ui_state.selected_pane_id = Some("w-foreign:p1".to_owned());
        runtime.snapshot.terminal.pane_id = Some("w-foreign:p1".to_owned());
        runtime.snapshot.focused.pane_id = Some("w-foreign:p1".to_owned());
        runtime.snapshot.pane_layout = Some(PaneLayoutSnapshot {
            workspace_id: "w-foreign".to_owned(),
            tab_id: "w-foreign:t1".to_owned(),
            focused_pane_id: "w-foreign:p1".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w-foreign:p1".to_owned(),
            },
        });
        runtime.restore_hint_pending = false;

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [{"pane_id": "w-focused:p2", "cwd": checkout_path}],
            "focused_pane_id": "w-focused:p2",
            "layouts": [{
                "workspace_id": "w-focused",
                "tab_id": "w-focused:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w-focused:p2",
                "panes": [{
                    "pane_id": "w-focused:p2",
                    "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                }],
                "splits": []
            }]
        }))
        .expect("focused checkout payload");

        assert!(runtime.ingest_session_with_catalog(
            Ok(payload),
            Some(session_sync::PrecomputedCatalog {
                registrations: vec![registration],
                workspaces: vec![current_workspace],
            }),
        ));
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w-foreign:p1")
        );
        assert!(runtime.snapshot().pane_layout.is_none());
        assert_eq!(
            runtime
                .snapshot()
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.as_str()),
            Some("pane.projection_unavailable")
        );
    }

    #[test]
    fn a_missing_selected_pane_reports_without_falling_back_to_a_same_cwd_pane() {
        let mut runtime = runtime();
        let checkout_path = "/tmp/hide-missing-selected-pane";
        let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
        let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
        let registration = WorkspaceRegistration {
            id: workspace_id.clone(),
            label: "Missing pane".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        };
        let selected_workspace = workspace(
            &workspace_id,
            "Missing pane",
            checkout_path,
            vec![checkout(&workspace_id, &checkout_id, checkout_path, None)],
        );
        runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
        runtime.snapshot.navigator.workspaces = vec![selected_workspace.clone()];
        runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id);
        runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
        runtime.snapshot.ui_state.selected_pane_id = Some("missing:p1".to_owned());
        runtime.snapshot.terminal.pane_id = Some("missing:p1".to_owned());
        // The user chose this pane against a running session, so it is an
        // authoritative selection rather than a restore hint.
        runtime.restore_hint_pending = false;
        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [{"pane_id": "old:p1", "cwd": checkout_path}],
            "layouts": [{
                "workspace_id": "old-workspace",
                "tab_id": "old-workspace:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "old:p1",
                "panes": [{"pane_id": "old:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }]
        }))
        .expect("missing selected pane payload");
        let catalog = session_sync::PrecomputedCatalog {
            registrations: vec![registration],
            workspaces: vec![selected_workspace],
        };

        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
        assert!(runtime.snapshot().pane_layout.is_none());
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("missing:p1")
        );
        assert_eq!(
            runtime
                .snapshot()
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.as_str()),
            Some("pane.projection_unavailable")
        );
    }

    /// Returning from a remote device left `remote:<target>:pane:<id>` in the
    /// selection, and every local sync tick then compared it against local
    /// layouts, never matched, and re-raised the same projection error. A
    /// remote id names a pane this session can never hold, so it is not a
    /// local selection that has gone missing.
    #[test]
    fn a_remote_pane_left_in_the_selection_does_not_block_local_projection() {
        let mut runtime = runtime();
        let checkout_path = "/tmp/hide-remote-selection-leak";
        let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
        let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
        let registration = WorkspaceRegistration {
            id: workspace_id.clone(),
            label: "Remote selection leak".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        };
        let local_pane = pane("wL:p1", checkout_path);
        let selected_workspace = workspace(
            &workspace_id,
            "Remote selection leak",
            checkout_path,
            vec![checkout(
                &workspace_id,
                &checkout_id,
                checkout_path,
                Some(local_pane),
            )],
        );
        runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
        runtime.snapshot.navigator.workspaces = vec![selected_workspace.clone()];
        runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id);
        runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
        // What a trip to the remote device and back leaves behind.
        runtime.snapshot.ui_state.selected_pane_id = Some("remote:mini:pane:w59:p2".to_owned());
        runtime.snapshot.terminal.pane_id = Some("remote:mini:pane:w59:p2".to_owned());
        runtime.restore_hint_pending = false;
        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [{"pane_id": "wL:p1", "cwd": checkout_path}],
            "layouts": [{
                "workspace_id": "wL",
                "tab_id": "wL:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "wL:p1",
                "panes": [{"pane_id": "wL:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }]
        }))
        .expect("local session payload");
        let catalog = session_sync::PrecomputedCatalog {
            registrations: vec![registration],
            workspaces: vec![selected_workspace],
        };

        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
        assert_eq!(runtime.snapshot().status.last_error, None);
        assert_eq!(
            runtime
                .snapshot()
                .pane_layout
                .as_ref()
                .map(|layout| layout.focused_pane_id.as_str()),
            Some("wL:p1")
        );
    }

    #[test]
    fn a_restored_pane_the_session_no_longer_has_retargets_without_reporting() {
        let mut runtime = runtime();
        // What launch produces: ids read from disk that name the session that
        // ended, against a Herdr session that has since been restarted.
        runtime.snapshot.ui_state.selected_pane_id = Some("wW:p3".to_owned());
        runtime.snapshot.terminal.pane_id = Some("wW:p3".to_owned());
        runtime.snapshot.ui_state.focused_checkout_id = Some("checkout:gone".to_owned());
        runtime.snapshot.navigator.focused_checkout_id = Some("checkout:gone".to_owned());

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [{"pane_id": "w19:p1", "cwd": "/tmp/hide-restored"}],
            "focused_pane_id": "w19:p1",
            "layouts": [{
                "workspace_id": "w19",
                "tab_id": "w19:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w19:p1",
                "panes": [{"pane_id": "w19:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }]
        }))
        .expect("restored pane payload");

        assert!(runtime.ingest_session_with_catalog(
            Ok(payload),
            Some(session_sync::PrecomputedCatalog {
                registrations: Vec::new(),
                workspaces: Vec::new(),
            }),
        ));
        assert_eq!(runtime.snapshot().status.last_error, None);
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w19:p1")
        );
        assert!(runtime.snapshot().pane_layout.is_some());
    }

    #[test]
    fn an_explicit_checkout_waits_without_rendering_stale_projection_when_catalog_is_missing() {
        let mut runtime = runtime();
        let stale_checkout_id = "checkout:selected";
        runtime.snapshot.navigator.focused_checkout_id = Some(stale_checkout_id.to_owned());
        runtime.snapshot.ui_state.focused_checkout_id = Some(stale_checkout_id.to_owned());
        runtime.snapshot.terminal.pane_id = Some("w3P:p1".to_owned());
        runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
            pane_id: "w3P:p1".to_owned(),
            closed: false,
            exit_code: None,
            ..TerminalPaneSnapshot::default()
        }];
        runtime.snapshot.pane_layout = Some(PaneLayoutSnapshot {
            workspace_id: "w3P".to_owned(),
            tab_id: "w3P:t1".to_owned(),
            focused_pane_id: "w3P:p1".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w3P:p1".to_owned(),
            },
        });
        runtime.snapshot.tab = TabSnapshot {
            id: Some("w3P:t1".to_owned()),
            workspace_id: Some("w3P".to_owned()),
            checkout_id: Some(stale_checkout_id.to_owned()),
            label: Some("Old context".to_owned()),
            empty: false,
            panes: vec![pane("w3P:p1", "/tmp/old-context")],
        };
        // The user chose this checkout against a running session, so it is an
        // authoritative selection rather than a restore hint.
        runtime.restore_hint_pending = false;

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [{"pane_id": "w3P:p1", "cwd": "/tmp/old-context"}],
            "layouts": [{
                "workspace_id": "w3P",
                "tab_id": "w3P:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w3P:p1",
                "panes": [{"pane_id": "w3P:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }]
        }))
        .expect("stale projection payload");

        assert!(runtime.ingest_session_with_catalog(
            Ok(payload),
            Some(session_sync::PrecomputedCatalog {
                registrations: Vec::new(),
                workspaces: Vec::new(),
            }),
        ));
        assert!(runtime.snapshot().pane_layout.is_none());
        assert!(runtime.snapshot().terminal.panes.is_empty());
        assert!(runtime.snapshot().tab.empty);
        assert!(runtime.snapshot().tab.panes.is_empty());
        assert_eq!(runtime.snapshot().tab.checkout_id, None);
        assert_eq!(
            runtime
                .snapshot()
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.as_str()),
            Some("pane.projection_unavailable")
        );
    }

    #[test]
    fn checkout_path_matching_uses_component_boundaries() {
        let id = NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed);
        let checkout_path = std::env::temp_dir().join(format!(
            "hide-checkout-boundary-{}-{id}",
            std::process::id()
        ));
        let sibling_path = checkout_path.with_file_name(format!(
            "{}-sibling",
            checkout_path
                .file_name()
                .expect("checkout directory name")
                .to_string_lossy()
        ));
        std::fs::create_dir_all(checkout_path.join("src")).expect("checkout fixture");
        std::fs::create_dir_all(sibling_path.join("src")).expect("sibling fixture");
        let checkout = checkout_path.to_string_lossy();
        let child = checkout_path
            .join("src/main.rs")
            .to_string_lossy()
            .into_owned();
        let sibling_child = sibling_path
            .join("src/main.rs")
            .to_string_lossy()
            .into_owned();

        assert!(path_is_within_checkout(&child, &checkout));
        assert!(!path_is_within_checkout(&sibling_child, &checkout));

        let _ = std::fs::remove_dir_all(checkout_path);
        let _ = std::fs::remove_dir_all(sibling_path);
    }

    #[test]
    fn a_broken_sync_update_keeps_the_last_valid_agents_and_says_it_is_disconnected() {
        let mut runtime = runtime();
        runtime.ingest_session(Ok(working_payload()));
        assert_eq!(runtime.snapshot().navigator.agents.len(), 1);
        assert_eq!(runtime.snapshot().pet.pose, "carrying");
        assert_eq!(runtime.snapshot().pet.badges.working, 1);

        runtime.ingest_session(Err(SessionFetchError::SocketMissing(
            "Herdr socket file does not exist".to_owned(),
        )));
        let down = runtime.snapshot();
        assert_eq!(
            down.navigator.agents.len(),
            1,
            "the last valid agent list is retained rather than blanked"
        );
        assert_eq!(down.pet.pose, "disconnected");
        assert_eq!(down.pet.connection, "socket_missing");
        assert!(
            down.pet.connection_message.is_some(),
            "a missing socket is stated, never a silent idle"
        );
        assert_eq!(
            down.pet.badges.working, 0,
            "a server that stopped answering cannot keep a working badge lit"
        );
        assert_eq!(down.pet.badges.disconnected, 1);

        // The next valid sync update recovers on its own.
        runtime.ingest_session(Ok(working_payload()));
        assert_eq!(runtime.snapshot().pet.pose, "carrying");
        assert_eq!(runtime.snapshot().pet.connection, "connected");
    }

    #[test]
    fn ingesting_the_same_snapshot_twice_reports_no_further_change() {
        let mut runtime = runtime();
        assert!(runtime.ingest_session(Ok(working_payload())));
        assert!(
            !runtime.ingest_session(Ok(working_payload())),
            "an unchanged snapshot must not wake the shell on every refresh"
        );
    }
}
