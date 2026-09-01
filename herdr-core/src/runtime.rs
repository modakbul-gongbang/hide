use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Weak};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::ffi::ChangeNotifier;
use crate::live::{
    LiveContext, PaneControlAction, PaneControlOutcome, PaneResizeDirection, PaneSplitDirection,
    SessionFetchError, TerminalSession, TerminalSessionMode,
};
use crate::model::{
    CoreOptions, DiagnosticSnapshot, LastErrorSnapshot, PaneLayoutSnapshot, PaneSnapshot,
    PetBadgesSnapshot, PetOriginSnapshot, PetSnapshot, SCHEMA_VERSION, Snapshot, Surface,
    TabSnapshot, TerminalChunk, TerminalPaneSnapshot, UiStateSnapshot,
};
use crate::sidebar::{SessionSnapshotPayload, project_agents};
use crate::{chromux, environment, files, live, persistence, pet, workspace};

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
struct FileOpenPayload {
    path: String,
}

#[derive(Debug, Deserialize)]
struct FileSavePayload {
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
struct FileViewerVisibilityPayload {
    visible: bool,
}

#[derive(Debug, Deserialize)]
struct UiStateUpdatePayload {
    #[serde(default)]
    left_sidebar_visible: Option<bool>,
    #[serde(default)]
    right_workbench_visible: Option<bool>,
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

#[derive(Debug, Deserialize)]
struct RetryConnectPayload {
    target_id: String,
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
    FileOpen(FileOpenPayload),
    FileDraft(FileDraftPayload),
    FileSave(FileSavePayload),
    FileConflict(FileConflictPayload),
    FileViewerVisibility(FileViewerVisibilityPayload),
    UiStateUpdate(UiStateUpdatePayload),
    RetryConnect(RetryConnectPayload),
    TerminalResize(TerminalResizePayload),
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
    terminal_sessions: HashMap<String, TerminalSession>,
    terminal_session_generations: HashMap<String, u64>,
    terminal_session_lifecycles: HashMap<String, TerminalSessionLifecycle>,
    next_terminal_session_generation: u64,
    terminal_sizes: HashMap<String, (u16, u16)>,
    #[cfg(test)]
    suppress_terminal_session_workers: bool,
    workspace_creations_in_flight: HashSet<String>,
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
    last_rest: Option<crate::model::RestSections>,
    last_editor: Option<crate::model::EditorSnapshot>,
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
            terminal_sessions: HashMap::new(),
            terminal_session_generations: HashMap::new(),
            terminal_session_lifecycles: HashMap::new(),
            next_terminal_session_generation: 0,
            terminal_sizes: HashMap::new(),
            #[cfg(test)]
            suppress_terminal_session_workers: false,
            workspace_creations_in_flight: HashSet::new(),
            worker_context: None,
            pet_active_at_unix_ms: unix_milliseconds(),
            pet_waking_until_unix_ms: 0,
            pet_dragging: false,
            pet_unseen_observed: std::collections::BTreeMap::new(),
            restore_hint_pending: true,
            last_session_spaces: Vec::new(),
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

    fn ingest_file_save_result(
        &mut self,
        path: String,
        contents: String,
        editor: crate::model::EditorSnapshot,
        result: Result<(), String>,
    ) -> bool {
        if self.snapshot.editor.path.as_deref() != Some(path.as_str())
            || self.snapshot.editor.contents_utf8.as_deref() != Some(contents.as_str())
        {
            self.push_diagnostic(
                "file.save_stale",
                format!("Ignored a completed save for stale draft {path}"),
            );
            return true;
        }
        self.snapshot.editor = editor;
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

    /// Groups the session's working directories under the Herdr workspace that
    /// owns them. Shared with the live poller so a catalog precomputed outside
    /// the runtime lock is built from the same inputs.
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
        precomputed: Option<live::PrecomputedCatalog>,
    ) -> bool {
        self.last_session_spaces = Self::session_spaces(payload);
        // The catalog shells out to git, so the poller builds it before
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
                    let cwd = payload
                        .panes
                        .iter()
                        .find(|source| source.pane_id == pane.pane_id)
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
                    PaneSnapshot {
                        id: pane.pane_id.clone(),
                        label: agent
                            .map(|agent| agent.workspace_label.clone())
                            .unwrap_or_else(|| pane.pane_id.clone()),
                        cwd,
                        state: agent
                            .map(|agent| agent.state.clone())
                            .unwrap_or_else(|| "unknown".to_owned()),
                        summary: agent.map(|agent| agent.summary.clone()),
                        activity_at_unix_ms: agent.and_then(|agent| agent.activity.parse().ok()),
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
        for device in &mut self.snapshot.navigator.devices {
            device.agent_count = projected_agents
                .iter()
                .filter(|agent| {
                    self.snapshot
                        .navigator
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.label == agent.workspace_label)
                        .is_some_and(|workspace| workspace.device_id == device.id)
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
    /// happen from both the live poller and event handlers, so this policy
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

    /// Applies a live session poll result: projected agents on success, an
    /// explicit herdr status on failure. Returns whether the snapshot changed.
    pub fn ingest_session(
        &mut self,
        fetched: Result<SessionSnapshotPayload, SessionFetchError>,
    ) -> bool {
        self.ingest_session_with_catalog(fetched, None)
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
        precomputed: Option<live::PrecomputedCatalog>,
    ) -> bool {
        let live_pane_ids = fetched.as_ref().ok().map(|payload| {
            payload
                .layouts
                .iter()
                .flat_map(|layout| layout.panes.iter())
                .map(|pane| pane.pane_id.clone())
                .collect::<HashSet<_>>()
        });
        if let Some(live_pane_ids) = live_pane_ids.as_ref() {
            self.terminal_sessions
                .retain(|pane_id, _| live_pane_ids.contains(pane_id));
            self.terminal_session_generations
                .retain(|pane_id, _| live_pane_ids.contains(pane_id));
            self.terminal_session_lifecycles
                .retain(|pane_id, _| live_pane_ids.contains(pane_id));
            self.terminal_sizes
                .retain(|pane_id, _| live_pane_ids.contains(pane_id));
        }
        let mut excluded = Vec::new();
        let catalog_changed = fetched
            .as_ref()
            .map(|payload| self.reconcile_session_catalog(payload, precomputed))
            .unwrap_or(false);
        let (state, message, agents, layout) = match fetched {
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
                            .collect::<HashSet<_>>()
                    })
                    .unwrap_or_default();
                let selected_pane_id = self
                    .snapshot
                    .terminal
                    .pane_id
                    .clone()
                    .or_else(|| self.snapshot.ui_state.selected_pane_id.clone());
                let selected_still_exists = selected_pane_id.as_deref().is_some_and(|pane_id| {
                    payload
                        .layouts
                        .iter()
                        .any(|layout| layout.panes.iter().any(|pane| pane.pane_id == pane_id))
                });
                let selected_pane_missing = selected_pane_id.is_some() && !selected_still_exists;
                let explicit_checkout_missing =
                    self.snapshot.ui_state.focused_checkout_id.is_some()
                        && focused_checkout.is_none();
                // Once the user or persisted state chooses a pane, a session
                // snapshot that omits that workspace must not silently retarget
                // commands to Herdr's unrelated globally focused workspace.
                let target_pane_id = if selected_pane_missing || explicit_checkout_missing {
                    None
                } else if focused_checkout.is_some() {
                    selected_pane_id
                        .as_deref()
                        .filter(|pane_id| {
                            selected_still_exists && focused_checkout_pane_ids.contains(*pane_id)
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
                if selected_pane_missing || selected_pane_invalid_for_context {
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
                    Ok(layout) => ("connected", None, Some(projection.agents), layout),
                    Err(projection_error) => (
                        "malformed",
                        Some(format!(
                            "Herdr pane layout could not be projected: {projection_error}"
                        )),
                        None,
                        None,
                    ),
                }
            }
            Err(error) => (error.state(), Some(error.message().to_owned()), None, None),
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

        let mut changed = catalog_changed || !excluded.is_empty();
        if self.snapshot.status.herdr.state != state
            || self.snapshot.status.herdr.message.as_deref() != message.as_deref()
        {
            self.snapshot.status.herdr.state = state.to_owned();
            self.snapshot.status.herdr.message = message;
            changed = true;
        }
        self.snapshot.status.herdr.last_checked_at_unix_ms = Some(unix_milliseconds());
        if let Some(agents) = agents
            && self.snapshot.navigator.agents != agents
        {
            self.snapshot.navigator.agents = agents;
            changed = true;
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

    /// Applies provider usage that the live poller read outside the runtime
    /// mutex. The two fixed rows are revisioned with the rest snapshot, so an
    /// unchanged refresh produces no shell work.
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
        self.snapshot.terminal.panes = pane_ids
            .iter()
            .map(|pane_id| {
                previous
                    .get(pane_id)
                    .cloned()
                    .unwrap_or_else(|| self.terminal_pane_snapshot(pane_id))
            })
            .collect();

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
    pub fn ingest_pane_control_result(
        &mut self,
        action: PaneControlAction,
        result: Result<PaneControlOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        match (action, result) {
            (PaneControlAction::Project { pane_id }, Ok(outcome)) => {
                if self.snapshot.terminal.pane_id.as_deref() != Some(pane_id.as_str()) {
                    self.push_diagnostic(
                        "pane.projection.stale",
                        format!("Ignored stale projection for pane {pane_id}"),
                    );
                    return false;
                }
                let Some(layout) = outcome.layout else {
                    self.set_error(
                        "pane.projection_missing_layout",
                        format!("Pane {pane_id} projection returned no layout"),
                        true,
                    );
                    return true;
                };
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
            (PaneControlAction::Focus { pane_id }, Ok(outcome)) => {
                let Some(layout) = outcome.layout else {
                    self.set_error(
                        "pane.focus_missing_layout",
                        format!("Pane {pane_id} focused without an authoritative layout"),
                        true,
                    );
                    return true;
                };
                if layout.focused_pane_id != pane_id {
                    self.set_error(
                        "pane.focus_mismatch",
                        format!(
                            "Requested pane {pane_id}, but Herdr reported focused pane {}",
                            layout.focused_pane_id
                        ),
                        true,
                    );
                    return true;
                }
                self.push_diagnostic(
                    "pane.focus",
                    format!("Pane {pane_id} focused in {elapsed_ms} ms"),
                );
                self.snapshot.ui_state.selected_pane_id = Some(pane_id);
                self.persist_ui_state();
                self.apply_pane_layout(layout);
                true
            }
            (
                PaneControlAction::Split {
                    pane_id, direction, ..
                },
                Ok(outcome),
            ) => {
                let Some(created_pane_id) = outcome.created_pane_id else {
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
                if let Some(message) = outcome.layout_refresh_error {
                    self.push_diagnostic("pane.layout.refresh_pending", message);
                }
                let Some(layout) = outcome.layout else {
                    self.set_error(
                        "pane.layout_refresh_failed",
                        format!(
                            "Pane {created_pane_id} was created, but Herdr did not return its authoritative layout"
                        ),
                        true,
                    );
                    return true;
                };
                if !layout.pane_ids().contains(&created_pane_id.as_str()) {
                    self.set_error(
                        "pane.layout_created_pane_missing",
                        format!(
                            "Authoritative layout does not contain created pane {created_pane_id}"
                        ),
                        true,
                    );
                    return true;
                }
                let authoritative_focus = layout.focused_pane_id.clone();
                self.snapshot.focused.surface = Surface::Terminal;
                self.snapshot.ui_state.selected_pane_id = Some(authoritative_focus);
                self.persist_ui_state();
                self.apply_pane_layout(layout);
                true
            }
            (
                PaneControlAction::Resize {
                    pane_id,
                    direction,
                    amount,
                },
                Ok(outcome),
            ) => {
                let Some(layout) = outcome.layout else {
                    self.set_error(
                        "pane.resize_missing_layout",
                        format!("Pane {pane_id} resized without an authoritative layout"),
                        true,
                    );
                    return true;
                };
                self.push_diagnostic(
                    "pane.resize",
                    format!(
                        "Pane {pane_id} resized {} by {amount:.3} in {elapsed_ms} ms",
                        direction.as_str()
                    ),
                );
                self.apply_pane_layout(layout);
                true
            }
            (PaneControlAction::ToggleZoom { pane_id }, Ok(outcome)) => {
                let layout_zoomed = outcome.layout.as_ref().map(|layout| layout.zoomed);
                self.push_diagnostic(
                    "pane.zoom_toggled",
                    format!(
                        "Pane {pane_id} zoom {} in {elapsed_ms} ms",
                        layout_zoomed
                            .map(|zoomed| if zoomed { "enabled" } else { "disabled" })
                            .unwrap_or("awaiting authoritative layout")
                    ),
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "pane_control",
                        "kind": "pane.zoom_ready",
                        "pane_id": pane_id,
                        "zoomed": layout_zoomed,
                        "duration_ms": elapsed_ms,
                    })
                );
                if let Some(message) = outcome.layout_refresh_error {
                    self.push_diagnostic("pane.layout.refresh_pending", message);
                }
                if let Some(layout) = outcome.layout {
                    self.apply_pane_layout(layout);
                }
                true
            }
            (PaneControlAction::Close { pane_id }, Ok(outcome)) => {
                self.push_diagnostic(
                    "pane.close",
                    format!("Pane {pane_id} closed in {elapsed_ms} ms"),
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

                let _retired_session = self.terminal_sessions.remove(&pane_id);
                self.terminal_session_generations.remove(&pane_id);
                self.terminal_session_lifecycles.remove(&pane_id);
                self.terminal_sizes.remove(&pane_id);
                self.snapshot
                    .terminal
                    .panes
                    .retain(|pane| pane.pane_id != pane_id);

                if let Some(message) = outcome.layout_refresh_error {
                    self.push_diagnostic("pane.layout.refresh_pending", message);
                    if self.snapshot.terminal.pane_id.as_deref() == Some(pane_id.as_str()) {
                        self.snapshot.terminal.pane_id = None;
                        self.snapshot.focused.pane_id = None;
                    }
                } else if let Some(layout) = outcome.layout {
                    let focused_pane_id = layout.focused_pane_id.clone();
                    self.snapshot.focused.surface = Surface::Terminal;
                    self.snapshot.focused.pane_id = Some(focused_pane_id.clone());
                    self.snapshot.terminal.pane_id = Some(focused_pane_id.clone());
                    self.snapshot.ui_state.selected_pane_id = Some(focused_pane_id);
                    self.apply_pane_layout(layout);
                } else {
                    self.snapshot.pane_layout = None;
                    self.snapshot.zoomed = None;
                    self.snapshot.focused.pane_id = None;
                    self.snapshot.terminal.pane_id = None;
                    self.snapshot.terminal.panes.clear();
                    self.snapshot.ui_state.selected_pane_id = None;
                }
                self.persist_ui_state();
                self.sync_focused_terminal_projection();
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
            (PaneControlAction::Close { .. }, Err(message)) => {
                self.set_error("pane.close_failed", message, true);
                true
            }
        }
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
    /// workspace, so retaining it here would let the next poll redraw stale
    /// terminal content while the selected checkout has no pane yet.
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
    /// next live poll catches up, and make the missing layout visible instead
    /// of retaining unrelated same-cwd content.
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
        let Some((checkout_path, next_pane_id)) = self
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
        if self.snapshot.ui_state.workspace_registrations == outcome.base_registrations {
            self.snapshot.ui_state.workspace_registrations = outcome.registrations;
            self.snapshot.navigator.workspaces = outcome.workspaces;
            Self::apply_workspace_expansion(
                &mut self.snapshot.navigator.workspaces,
                &self.snapshot.ui_state.collapsed_workspace_ids,
            );
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
                "Workspace registrations changed during creation; the live poller will refresh the catalog",
            );
        }
        self.snapshot.navigator.focused_device_id = Some(workspace::LOCAL_DEVICE_ID.to_owned());
        if let Some(checkout_id) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == outcome.registration.id)
            .and_then(|workspace| workspace.checkouts.first())
            .map(|checkout| checkout.id.clone())
        {
            self.snapshot.navigator.focused_checkout_id = Some(checkout_id);
        }
        self.resync_navigator_focus();
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
                if self.live.is_some() {
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
                        self.last_session_spaces.clone(),
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
                let registration = match workspace::registration(
                    &payload.path,
                    &payload.label,
                    workspace::LOCAL_DEVICE_ID,
                ) {
                    Ok(registration) => registration,
                    Err(message) => {
                        self.set_error("workspace.invalid", message, false);
                        return true;
                    }
                };
                let path = Path::new(&registration.path);
                if !path.exists() {
                    self.set_error(
                        "workspace.path_missing",
                        format!("Workspace path does not exist: {}", registration.path),
                        false,
                    );
                    return true;
                }
                if payload.initialize_git
                    && let Err(message) = workspace::initialize_git(path)
                {
                    self.set_error("workspace.git_init_failed", message, true);
                }
                if !self
                    .snapshot
                    .ui_state
                    .workspace_registrations
                    .iter()
                    .any(|existing| existing.id == registration.id)
                {
                    self.snapshot
                        .ui_state
                        .workspace_registrations
                        .push(registration.clone());
                }
                self.rebuild_catalog();
                self.snapshot.navigator.focused_device_id =
                    Some(workspace::LOCAL_DEVICE_ID.to_owned());
                if let Some(checkout_id) = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == registration.id)
                    .and_then(|workspace| workspace.checkouts.first())
                    .map(|checkout| checkout.id.clone())
                {
                    self.snapshot.navigator.focused_checkout_id = Some(checkout_id);
                }
                self.resync_navigator_focus();
                self.persist_current_ui_state();
                self.push_diagnostic(
                    "workspace.registered",
                    format!("Registered workspace {}", registration.path),
                );
                true
            }
            ValidatedEvent::CreateTab(payload) => {
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
                let checkout_index = match payload.checkout_id.as_deref() {
                    Some(checkout_id) => {
                        let Some(index) = workspace_snapshot
                            .checkouts
                            .iter()
                            .position(|checkout| checkout.id == checkout_id)
                        else {
                            self.set_error(
                                "tab.unknown_checkout",
                                format!("Checkout {checkout_id} is not available"),
                                false,
                            );
                            return true;
                        };
                        index
                    }
                    None => 0,
                };
                let Some(checkout) = workspace_snapshot.checkouts.get_mut(checkout_index) else {
                    self.set_error("tab.no_checkout", "Workspace has no checkout", false);
                    return true;
                };
                let tab_number = checkout.tabs.len() + 1;
                let tab_id = format!("{}:tab:{tab_number}", checkout.id);
                checkout.tabs.push(TabSnapshot {
                    id: Some(tab_id),
                    workspace_id: Some(workspace_snapshot.id.clone()),
                    checkout_id: Some(checkout.id.clone()),
                    label: Some(if payload.label.trim().is_empty() {
                        format!("Tab {tab_number}")
                    } else {
                        payload.label.trim().to_owned()
                    }),
                    empty: true,
                    panes: Vec::new(),
                });
                self.snapshot.navigator.focused_workspace_id = Some(workspace_snapshot.id.clone());
                self.snapshot.navigator.focused_checkout_id = Some(checkout.id.clone());
                self.snapshot.navigator.root_path = Some(checkout.path.clone());
                self.sync_active_tab_projection();
                self.persist_current_ui_state();
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
                self.persist_current_ui_state();
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
                let _ = (payload.tab_id, payload.confirmed);
                false
            }
            ValidatedEvent::ClosePane(payload) => {
                let requires_confirmation = self.snapshot.navigator.agents.iter().any(|agent| {
                    agent.pane_id == payload.pane_id
                        && matches!(
                            agent.state.as_str(),
                            "working" | "question" | "approval" | "error" | "unseen_completion"
                        )
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
                self.push_diagnostic("pane.close.requested", format!("Closing pane {pane_id}"));
                if let Err(message) =
                    live::spawn_pane_control(context, PaneControlAction::Close { pane_id })
                {
                    self.set_error("pane.close_worker_failed", message, true);
                }
                true
            }
            ValidatedEvent::FileOpen(payload)
                if self.snapshot.editor.path.as_deref() == Some(payload.path.as_str()) =>
            {
                self.snapshot.editor.viewer_visible = true;
                self.snapshot.ui_state.selected_path = Some(payload.path);
                true
            }
            ValidatedEvent::FileOpen(payload) => match files::open(Path::new(&payload.path)) {
                Ok(editor) => {
                    self.snapshot.editor = editor;
                    self.snapshot.ui_state.selected_path = Some(payload.path);
                    true
                }
                Err(message) => {
                    self.set_error("file.open_failed", message, true);
                    true
                }
            },
            ValidatedEvent::FileDraft(payload) => {
                match files::update_draft(&mut self.snapshot.editor, payload.contents_utf8) {
                    Ok(()) => true,
                    Err(message) => {
                        self.set_error("file.draft_rejected", message, false);
                        true
                    }
                }
            }
            ValidatedEvent::FileSave(payload) => {
                if self.snapshot.editor.path.as_deref() == Some(payload.path.as_str()) {
                    self.snapshot.editor.contents_utf8 = Some(payload.contents_utf8.clone());
                    self.snapshot.editor.dirty = true;
                }
                let Some(context) = self.worker_context.clone() else {
                    self.set_error(
                        "file.save_worker_unavailable",
                        "The file save worker is unavailable; the draft was preserved",
                        true,
                    );
                    return true;
                };
                let path = payload.path;
                let contents = payload.contents_utf8;
                let expected_modified_at = payload.expected_modified_at_unix_ms;
                let mut editor = self.snapshot.editor.clone();
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
                            Ok(mut guard) => {
                                guard.ingest_file_save_result(path, contents, editor, result)
                            }
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
            ValidatedEvent::FileConflict(payload) => match payload.action.as_str() {
                "reload" => match files::reload(&mut self.snapshot.editor) {
                    Ok(()) => true,
                    Err(message) => {
                        self.set_error("file.reload_failed", message, true);
                        true
                    }
                },
                "keep_editing" => {
                    if let Some(conflict) = self.snapshot.editor.conflict.as_ref() {
                        self.snapshot.editor.opened_modified_at_unix_ms =
                            Some(conflict.disk_modified_at_unix_ms);
                    }
                    self.snapshot.editor.conflict = None;
                    true
                }
                _ => {
                    self.set_error(
                        "file.invalid_conflict_action",
                        "Conflict action must be reload or keep_editing",
                        false,
                    );
                    true
                }
            },
            ValidatedEvent::FileViewerVisibility(payload) => {
                if payload.visible && self.snapshot.editor.path.is_none() {
                    self.set_error(
                        "file.viewer_without_document",
                        "A file must be selected before the viewer can open",
                        false,
                    );
                    return true;
                }
                self.snapshot.editor.viewer_visible = payload.visible;
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
            ValidatedEvent::UiStateUpdate(payload) => {
                // Pet placement, visibility, and shortcut belong to the pet
                // events; a navigator or keyboard save must not erase them.
                let current = self.snapshot.ui_state.clone();
                self.snapshot.ui_state = UiStateSnapshot {
                    left_sidebar_visible: payload
                        .left_sidebar_visible
                        .unwrap_or(current.left_sidebar_visible),
                    right_workbench_visible: payload
                        .right_workbench_visible
                        .unwrap_or(current.right_workbench_visible),
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
                };
                self.apply_selected_pane_anchor(self.snapshot.ui_state.selected_pane_id.clone());
                self.snapshot.navigator.focused_device_id =
                    self.snapshot.ui_state.focused_device_id.clone();
                self.snapshot.navigator.focused_checkout_id =
                    self.snapshot.ui_state.focused_checkout_id.clone();
                Self::apply_workspace_expansion(
                    &mut self.snapshot.navigator.workspaces,
                    &self.snapshot.ui_state.collapsed_workspace_ids,
                );
                // The live poller owns session-derived temporary workspaces.
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

    /// Starts one control attempt. Repeated polls are no-ops while any
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
        let context = self
            .live
            .as_ref()
            .cloned()
            .expect("terminal sessions are only requested with live configured");
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
        context: &LiveContext,
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
        if let Some(layout) = self.snapshot.pane_layout.as_ref()
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
                    .start_reader(context.runtime.clone(), context.notifier.clone());
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
    // Navigator workspaces carry Herdr's own workspace ids, so the layout
    // names its owner exactly. The path comparisons below are the fallback for
    // a registration Herdr has no workspace for, and for a catalog precomputed
    // from a slightly older session.
    if let Some(index) = workspaces
        .iter()
        .position(|workspace| workspace.id == session_workspace_id)
    {
        return workspaces.get_mut(index);
    }
    let path = Path::new(raw_path);
    let root = workspace::git_root(path)
        .map(|root| workspace::normalized_for_comparison(&root))
        .unwrap_or_else(|| workspace::normalized_for_comparison(path));
    let normalized = root.clone();
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
    workspaces.push(workspace::inspect_temporary(
        Path::new(&root),
        workspace::LOCAL_DEVICE_ID,
    ));
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
        "file_open" => decode!(FileOpenPayload, FileOpen),
        "file_draft" => decode!(FileDraftPayload, FileDraft),
        "file_save" => decode!(FileSavePayload, FileSave),
        "file_conflict" => decode!(FileConflictPayload, FileConflict),
        "file_viewer_visibility" => {
            decode!(FileViewerVisibilityPayload, FileViewerVisibility)
        }
        "ui_state_update" => decode!(UiStateUpdatePayload, UiStateUpdate),
        "retry_connect" => decode!(RetryConnectPayload, RetryConnect),
        "terminal_resize" => decode!(TerminalResizePayload, TerminalResize),
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
        {
            return Err("remote target fields must not be empty");
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
    use crate::live::SessionFetchError;
    use crate::model::{
        CheckoutSnapshot, PaneLayoutNodeSnapshot, PaneLayoutSnapshot, PaneSnapshot, TabSnapshot,
        TerminalPaneSnapshot, WorkspaceRegistration, WorkspaceSnapshot,
    };
    use crate::sidebar::SessionSnapshotPayload;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_RUNTIME_STATE_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn repeated_polls_do_not_start_a_second_terminal_session() {
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

        assert!(runtime.ingest_workspace_creation(
            partial_path,
            Ok(live::WorkspaceCreationOutcome {
                registration: registration.clone(),
                base_registrations: Vec::new(),
                registrations: vec![registration.clone()],
                workspaces: Vec::new(),
                git_init_error: Some("git init failed explicitly".to_owned()),
            }),
            7,
        ));
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
            label: id.to_owned(),
            cwd: cwd.to_owned(),
            state: "attached".to_owned(),
            summary: None,
            activity_at_unix_ms: None,
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
            checkouts,
        }
    }

    #[test]
    fn ui_state_update_applies_workspace_expansion_without_waiting_for_a_poll() {
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
            workspace::checkout_id_for_path("herdr-workspace", Path::new(checkout_path));
        runtime.snapshot.navigator.focused_workspace_id = Some("herdr-workspace".to_owned());
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
        // Once Herdr has a workspace in that directory the row is that
        // workspace, not a second entry beside it.
        assert_eq!(runtime.snapshot().navigator.workspaces.len(), 1);
        let checkout = runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "herdr-workspace")
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
    fn a_worktree_pane_projects_into_its_own_checkout_row() {
        let mut runtime = runtime();
        let repository_path = "/private/tmp/hide-rebrand/herdr-ide";
        let checkout_path = "/private/tmp/hide-rebrand/worktrees/hide-rebrand";
        // Herdr owns the workspace axis, so the navigator workspace is the
        // Herdr workspace and its checkouts are the directories its panes are
        // actually in.
        let spaces = vec![workspace::SessionSpace {
            id: "w3M".to_owned(),
            label: "herdr-ide".to_owned(),
            cwds: vec![repository_path.to_owned(), checkout_path.to_owned()],
        }];
        let checkout_id = workspace::checkout_id_for_path("w3M", Path::new(checkout_path));
        runtime.snapshot.navigator.workspaces = workspace::build_catalog(&[], &spaces);
        runtime.snapshot.navigator.focused_workspace_id = Some("w3M".to_owned());
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
        .expect("worktree pane payload");
        let catalog = live::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: workspace::build_catalog(&[], &spaces),
        };

        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
        let workspace_snapshot = runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "w3M")
            .expect("the Herdr workspace")
            .clone();
        // Only the directories panes occupy, not every worktree the
        // repository has.
        assert_eq!(workspace_snapshot.checkouts.len(), 2);
        let checkout = workspace_snapshot
            .checkouts
            .iter()
            .find(|checkout| checkout.id == checkout_id)
            .expect("the worktree checkout");
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
        let checkout_id = workspace::checkout_id_for_path("w3Z", Path::new(checkout_path));
        runtime.snapshot.navigator.workspaces = workspace::build_catalog(&[], &spaces);
        runtime.snapshot.navigator.focused_workspace_id = Some("w3Z".to_owned());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.ui_state.selected_pane_id = Some("w3Z:p1".to_owned());
        runtime.snapshot.terminal.pane_id = Some("w3Z:p1".to_owned());
        runtime.restore_hint_pending = false;

        // Two Herdr workspaces sit in the same directory, so only the
        // workspace id tells them apart.
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
        let catalog = live::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: workspace::build_catalog(&[], &spaces),
        };
        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));

        let selected_checkout = runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "w3Z")
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

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id, "w41");
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
        let catalog = live::PrecomputedCatalog {
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
        let catalog = live::PrecomputedCatalog {
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
            Some(live::PrecomputedCatalog {
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
            Some(live::PrecomputedCatalog {
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
    fn a_wholly_broken_poll_keeps_the_last_valid_agents_and_says_it_is_disconnected() {
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

        // The next valid poll recovers on its own.
        runtime.ingest_session(Ok(working_payload()));
        assert_eq!(runtime.snapshot().pet.pose, "carrying");
        assert_eq!(runtime.snapshot().pet.connection, "connected");
    }

    #[test]
    fn ingesting_the_same_poll_twice_reports_no_further_change() {
        let mut runtime = runtime();
        assert!(runtime.ingest_session(Ok(working_payload())));
        assert!(
            !runtime.ingest_session(Ok(working_payload())),
            "an unchanged snapshot must not wake the shell every poll"
        );
    }
}
