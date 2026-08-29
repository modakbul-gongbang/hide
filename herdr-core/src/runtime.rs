use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::live::{
    LiveContext, PaneAttach, PaneControlAction, PaneControlOutcome, PaneSplitDirection,
    SessionFetchError,
};
use crate::model::{
    CoreOptions, DiagnosticSnapshot, LastErrorSnapshot, PaneLayoutSnapshot, PetBadgesSnapshot,
    PaneSnapshot, PetClickSnapshot, PetOriginSnapshot, PetSnapshot, SCHEMA_VERSION, Snapshot,
    Surface, TabSnapshot, TerminalChunk, TerminalPaneSnapshot, UiStateSnapshot,
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
    #[serde(alias = "create_worktree")]
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
struct UiStateUpdatePayload {
    expanded_paths: Vec<String>,
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
    #[serde(default)]
    bypass_warnings: Option<bool>,
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
    ToggleZoom(ToggleZoomPayload),
    CloseWorkspace(ConfirmedWorkspacePayload),
    CloseTab(ConfirmedTabPayload),
    ClosePane(ConfirmedPanePayload),
    FileOpen(FileOpenPayload),
    FileDraft(FileDraftPayload),
    FileSave(FileSavePayload),
    FileConflict(FileConflictPayload),
    UiStateUpdate(UiStateUpdatePayload),
    RetryConnect(RetryConnectPayload),
    TerminalResize(TerminalResizePayload),
    PetClick,
    PetSetVisible(PetVisibilityPayload),
    PetToggleVisible,
    PetMove(PetMovePayload),
    PetDrag(PetDragPayload),
    PetActivity,
    PetShortcutUpdate(PetShortcutPayload),
}

pub struct Runtime {
    snapshot: Snapshot,
    state_path: PathBuf,
    remote_targets: Vec<crate::model::RemoteTarget>,
    live: Option<LiveContext>,
    attaches: HashMap<String, PaneAttach>,
    attach_generations: HashMap<String, u64>,
    next_attach_generation: u64,
    terminal_sizes: HashMap<String, (u16, u16)>,
    /// The last moment any agent was working or waiting on the user. The pet
    /// measures idleness from here, so roam and sleep are driven by real
    /// session activity rather than wall-clock uptime.
    pet_active_at_unix_ms: u64,
    pet_waking_until_unix_ms: u64,
    pet_dragging: bool,
    /// First moment each currently-unseen pane became unseen. Memory only by
    /// decision (D-21): after a restart snapshot order decides instead.
    pet_unseen_observed: std::collections::BTreeMap<String, u64>,
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
        snapshot.navigator.root_path = snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .find(|checkout| {
                snapshot.navigator.focused_checkout_id.as_deref() == Some(checkout.id.as_str())
            })
            .map(|checkout| checkout.path.clone())
            .or_else(|| {
                snapshot
                    .navigator
                    .workspaces
                    .first()
                    .and_then(|workspace| workspace.checkouts.first())
                    .map(|checkout| checkout.path.clone())
            });
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
            attaches: HashMap::new(),
            attach_generations: HashMap::new(),
            next_attach_generation: 0,
            terminal_sizes: HashMap::new(),
            pet_active_at_unix_ms: unix_milliseconds(),
            pet_waking_until_unix_ms: 0,
            pet_dragging: false,
            pet_unseen_observed: std::collections::BTreeMap::new(),
        };
        runtime.apply_persisted_pet_state();
        runtime.refresh_pet();
        runtime
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn set_live(&mut self, context: LiveContext) {
        self.live = Some(context);
    }

    /// Rebuilds the navigator from durable registrations and the current
    /// session's pane working directories. Pane directories that are not
    /// registered become clearly marked temporary workspaces for this
    /// session; they are never persisted as registrations.
    fn reconcile_session_catalog(&mut self, payload: &SessionSnapshotPayload) -> bool {
        let temporary_paths = payload
            .panes
            .iter()
            .filter_map(|pane| pane.cwd.clone())
            .chain(payload.agents.iter().filter_map(|agent| agent.cwd.clone()))
            .collect::<Vec<_>>();
        let mut workspaces = workspace::build_catalog(
            &self.snapshot.ui_state.workspace_registrations,
            &temporary_paths,
        );
        let projected_agents = project_agents(payload.clone()).unwrap_or_default();

        for layout in &payload.layouts {
            let context_path = layout
                .panes
                .iter()
                .filter_map(|pane| {
                    payload
                        .agents
                        .iter()
                        .find(|agent| {
                            agent.pane_id.as_deref().or(agent.id.as_deref())
                                == Some(pane.pane_id.as_str())
                        })
                        .and_then(|agent| agent.cwd.clone())
                })
                .next();
            let Some(workspace_snapshot) = find_workspace_for_context(
                &mut workspaces,
                context_path.as_deref(),
                &layout.workspace_id,
            ) else {
                continue;
            };
            let checkout_index = context_path.as_deref().and_then(|path| {
                let normalized = Path::new(path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(path))
                    .to_string_lossy()
                    .trim_end_matches('/')
                    .to_owned();
                workspace_snapshot.checkouts.iter().position(|checkout| {
                    let checkout_path = checkout.path.trim_end_matches('/').to_owned();
                    normalized == checkout_path
                        || normalized.starts_with(&format!("{checkout_path}/"))
                })
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
            let tab = TabSnapshot {
                id: Some(layout.tab_id.clone()),
                workspace_id: Some(workspace_snapshot.id.clone()),
                checkout_id: Some(checkout.id.clone()),
                label: Some("Session".to_owned()),
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
        let focused_checkout_exists = self.snapshot.navigator.workspaces.iter().any(|workspace| {
            workspace.checkouts.iter().any(|checkout| {
                Some(checkout.id.as_str()) == self.snapshot.navigator.focused_checkout_id.as_deref()
            })
        });
        if !focused_checkout_exists {
            self.snapshot.navigator.focused_checkout_id = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .find(|checkout| checkout.exists)
                .map(|checkout| checkout.id.clone());
        }
        self.snapshot.navigator.focused_workspace_id = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| {
                workspace.checkouts.iter().any(|checkout| {
                    Some(checkout.id.as_str())
                        == self.snapshot.navigator.focused_checkout_id.as_deref()
                })
            })
            .map(|workspace| workspace.id.clone());
        self.snapshot.navigator.root_path = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .find(|checkout| {
                Some(checkout.id.as_str()) == self.snapshot.navigator.focused_checkout_id.as_deref()
            })
            .map(|checkout| checkout.path.clone());
        self.sync_active_tab_projection();
        previous != self.snapshot.navigator
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
        let mut excluded = Vec::new();
        let catalog_changed = fetched
            .as_ref()
            .map(|payload| self.reconcile_session_catalog(payload))
            .unwrap_or(false);
        let (state, message, agents, layout) = match fetched {
            Ok(payload) => {
                let selected_pane_id = self.snapshot.terminal.pane_id.as_deref().or(self
                    .snapshot
                    .ui_state
                    .selected_pane_id
                    .as_deref());
                let selected_still_exists = selected_pane_id.is_some_and(|pane_id| {
                    payload
                        .layouts
                        .iter()
                        .any(|layout| layout.panes.iter().any(|pane| pane.pane_id == pane_id))
                });
                // Once the user or persisted state chooses a pane, a session
                // snapshot that omits that workspace must not silently retarget
                // commands to Herdr's unrelated globally focused workspace.
                let target_pane_id = if selected_pane_id.is_some() && !selected_still_exists {
                    None
                } else {
                    selected_pane_id
                        .or(payload.focused_pane_id.as_deref())
                        .or_else(|| {
                            payload
                                .layouts
                                .first()
                                .map(|layout| layout.focused_pane_id.as_str())
                        })
                };
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
            last_click: self.snapshot.pet.last_click.clone(),
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
        let desired = pane_ids.iter().cloned().collect::<HashSet<_>>();
        let layout_changed = self.snapshot.pane_layout.as_ref() != Some(&layout);

        self.attaches.retain(|pane_id, _| desired.contains(pane_id));
        self.attach_generations
            .retain(|pane_id, _| desired.contains(pane_id));
        self.terminal_sizes
            .retain(|pane_id, _| desired.contains(pane_id));

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
                    .unwrap_or_else(|| TerminalPaneSnapshot {
                        pane_id: pane_id.clone(),
                        closed: false,
                        exit_code: None,
                    })
            })
            .collect();

        // Herdr owns focus and input routing. The shell never keeps a second,
        // hover- or click-local focus value alongside the authoritative layout.
        self.snapshot.terminal.pane_id = Some(layout.focused_pane_id.clone());
        self.snapshot.focused.pane_id = Some(layout.focused_pane_id.clone());
        self.snapshot.zoomed = layout.zoomed.then(|| layout.focused_pane_id.clone());
        self.snapshot.pane_layout = Some(layout);
        self.sync_focused_terminal_projection();

        if self.live.is_some() {
            for pane_id in pane_ids {
                self.request_attach(&pane_id);
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
        self.snapshot.terminal.panes.push(TerminalPaneSnapshot {
            pane_id: pane_id.to_owned(),
            closed: false,
            exit_code: None,
        });
    }

    fn set_terminal_closed(&mut self, pane_id: &str, closed: bool) {
        self.ensure_terminal_pane(pane_id);
        if let Some(pane) = self
            .snapshot
            .terminal
            .panes
            .iter_mut()
            .find(|pane| pane.pane_id == pane_id)
        {
            pane.closed = closed;
            if !closed {
                pane.exit_code = None;
            }
        }
        self.sync_focused_terminal_projection();
    }

    fn terminal_is_closed(&self, pane_id: &str) -> bool {
        self.snapshot
            .terminal
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .is_some_and(|pane| pane.closed)
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
                if let Err(message) = persistence::save(&self.state_path, &self.snapshot.ui_state) {
                    self.set_error("ui_state.save_failed", message, true);
                }
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
                if let Err(message) = persistence::save(&self.state_path, &self.snapshot.ui_state) {
                    self.set_error("ui_state.save_failed", message, true);
                }
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

                let _retired_attach = self.attaches.remove(&pane_id);
                self.attach_generations.remove(&pane_id);
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
                if let Err(message) = persistence::save(&self.state_path, &self.snapshot.ui_state) {
                    self.set_error("ui_state.save_failed", message, true);
                }
                self.sync_focused_terminal_projection();
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

    /// Appends live pane bytes when the delivering attach is still current.
    pub fn ingest_attach_output(&mut self, pane_id: &str, generation: u64, bytes: &[u8]) -> bool {
        if self.attach_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        if !self.attaches.contains_key(pane_id) {
            return false;
        }
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(bytes));
        true
    }

    /// Marks the terminal closed when the current attach stream ends.
    pub fn ingest_attach_exit(&mut self, pane_id: &str, generation: u64, message: String) -> bool {
        if self.attach_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        self.set_terminal_closed(pane_id, true);
        let notice = format!("\r\n[{message}]\r\n");
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(notice.as_bytes()));
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

    fn rebuild_catalog(&mut self, temporary_paths: &[String]) {
        self.snapshot.navigator.workspaces = workspace::build_catalog(
            &self.snapshot.ui_state.workspace_registrations,
            temporary_paths,
        );
        self.snapshot.navigator.devices = workspace::devices(
            &self.remote_targets,
            &self.snapshot.ui_state.device_registrations,
        );
        let focused_exists = self.snapshot.navigator.workspaces.iter().any(|workspace| {
            workspace.checkouts.iter().any(|checkout| {
                Some(checkout.id.as_str()) == self.snapshot.navigator.focused_checkout_id.as_deref()
            })
        });
        if !focused_exists {
            self.snapshot.navigator.focused_checkout_id = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .find(|checkout| checkout.exists)
                .map(|checkout| checkout.id.clone());
        }
        self.snapshot.navigator.focused_workspace_id = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| {
                workspace.checkouts.iter().any(|checkout| {
                    Some(checkout.id.as_str())
                        == self.snapshot.navigator.focused_checkout_id.as_deref()
                })
            })
            .map(|workspace| workspace.id.clone());
        self.snapshot.navigator.root_path = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .find(|checkout| {
                Some(checkout.id.as_str()) == self.snapshot.navigator.focused_checkout_id.as_deref()
            })
            .map(|checkout| checkout.path.clone());
        self.sync_active_tab_projection();
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

    fn focus_checkout(&mut self, workspace_id: &str, checkout_id: &str) -> bool {
        let Some(workspace_snapshot) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
        else {
            self.set_error(
                "checkout.unknown_workspace",
                format!("Workspace {workspace_id} is not registered"),
                false,
            );
            return true;
        };
        if !workspace_snapshot
            .checkouts
            .iter()
            .any(|checkout| checkout.id == checkout_id)
        {
            self.set_error(
                "checkout.unknown",
                format!("Checkout {checkout_id} is not available in {workspace_id}"),
                false,
            );
            return true;
        }
        self.snapshot.navigator.focused_workspace_id = Some(workspace_id.to_owned());
        self.snapshot.navigator.focused_checkout_id = Some(checkout_id.to_owned());
        self.snapshot.navigator.root_path = self
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
            })
            .map(|checkout| checkout.path.clone());
        self.sync_active_tab_projection();
        self.persist_current_ui_state();
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
                    self.write_attached(&payload.pane_id, &payload.bytes_base64);
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
            ValidatedEvent::PetClick => {
                // Every toggle surface and the click itself share one state,
                // so the click never has to guess which pane the user meant:
                // the oldest unseen pane wins, and with nothing unseen the
                // shell only raises its window.
                let selected_pane_id = self.snapshot.pet.attention_pane_ids.first().cloned();
                if let Some(pane_id) = selected_pane_id.clone() {
                    self.snapshot.ui_state.selected_pane_id = Some(pane_id.clone());
                    self.persist_ui_state();
                    self.focus_pane(pane_id);
                }
                self.snapshot.pet.last_click = Some(PetClickSnapshot {
                    selected_pane_id,
                    at_unix_ms: unix_milliseconds(),
                });
                true
            }
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
                let checkout_id =
                    workspace::build_catalog(std::slice::from_ref(&registration), &[])
                        .first()
                        .and_then(|workspace| workspace.checkouts.first())
                        .map(|checkout| checkout.id.clone());
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
                self.rebuild_catalog(&[]);
                self.snapshot.navigator.focused_device_id =
                    Some(workspace::LOCAL_DEVICE_ID.to_owned());
                if let Some(checkout_id) = checkout_id.or_else(|| {
                    self.snapshot
                        .navigator
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.id == registration.id)
                        .and_then(|workspace| workspace.checkouts.first())
                        .map(|checkout| checkout.id.clone())
                }) {
                    self.snapshot.navigator.focused_checkout_id = Some(checkout_id);
                }
                self.rebuild_catalog(&[]);
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
                self.snapshot.navigator.focused_workspace_id = Some(payload.workspace_id);
                self.snapshot.navigator.focused_checkout_id = Some(payload.checkout_id);
                self.snapshot.navigator.root_path = Some(checkout.path.clone());
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
                self.rebuild_catalog(&[]);
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
                self.rebuild_catalog(&[]);
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
                self.rebuild_catalog(&[]);
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
                match files::save(
                    &mut self.snapshot.editor,
                    Path::new(&payload.path),
                    payload.contents_utf8,
                    payload.expected_modified_at_unix_ms,
                ) {
                    Ok(()) => true,
                    Err(message) => {
                        self.set_error("file.save_failed", message, true);
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
                if let Some(attach) = self.attaches.get_mut(&payload.pane_id)
                    && let Err(message) = attach.resize(payload.rows, payload.cols)
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
                    expanded_paths: payload.expanded_paths,
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
                    bypass_warnings: payload.bypass_warnings.unwrap_or(current.bypass_warnings),
                };
                self.snapshot.navigator.focused_device_id =
                    self.snapshot.ui_state.focused_device_id.clone();
                self.snapshot.navigator.focused_checkout_id =
                    self.snapshot.ui_state.focused_checkout_id.clone();
                self.rebuild_catalog(&[]);
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

    /// Begins a pane attach without spawning or waiting for a process on the
    /// caller. Re-focusing the active pane is a no-op; a different pane makes
    /// the selection visible immediately and finishes on a worker.
    fn request_attach(&mut self, pane_id: &str) {
        if self.attaches.contains_key(pane_id) && !self.terminal_is_closed(pane_id) {
            return;
        }
        self.next_attach_generation = self.next_attach_generation.saturating_add(1);
        let generation = self.next_attach_generation;
        self.attach_generations
            .insert(pane_id.to_owned(), generation);
        let _retired_attach = self.attaches.remove(pane_id);
        self.set_terminal_closed(pane_id, false);
        // Reset only this pane's SwiftTerm grid; other panes retain their
        // independent terminal state while the replacement attach starts.
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(b"\x1bc"));
        self.push_diagnostic("pane.attach.requested", format!("Attaching pane {pane_id}"));
        let context = self
            .live
            .as_ref()
            .cloned()
            .expect("request_attach is only called with live configured");
        if let Err(message) = live::spawn_pane_attach(
            context,
            pane_id.to_owned(),
            generation,
            self.terminal_sizes.get(pane_id).map_or(24, |size| size.0),
            self.terminal_sizes.get(pane_id).map_or(80, |size| size.1),
        ) {
            self.set_terminal_closed(pane_id, true);
            let notice = format!("\r\n[Attach to {pane_id} failed: {message}]\r\n");
            self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(notice.as_bytes()));
            self.set_error("pane.attach_worker_failed", message, true);
        }
    }

    pub fn ingest_attach_spawn(
        &mut self,
        generation: u64,
        pane_id: &str,
        result: Result<PaneAttach, String>,
        elapsed_ms: u128,
        context: &LiveContext,
    ) -> bool {
        if self.attach_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        if let Some(layout) = self.snapshot.pane_layout.as_ref()
            && !layout.pane_ids().contains(&pane_id)
        {
            return false;
        }
        match result {
            Ok(mut attach) => {
                if let Err(message) =
                    attach.start_reader(context.runtime.clone(), context.notifier.clone())
                {
                    self.set_terminal_closed(pane_id, true);
                    self.set_error("pane.attach_reader_failed", message, true);
                    return true;
                }
                self.attaches.insert(pane_id.to_owned(), attach);
                self.set_terminal_closed(pane_id, false);
                self.push_diagnostic(
                    "pane.attach.ready",
                    format!("Pane {pane_id} attached in {elapsed_ms} ms"),
                );
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "pane_attach",
                        "kind": "pane.attach_ready",
                        "pane_id": pane_id,
                        "generation": generation,
                        "duration_ms": elapsed_ms,
                    })
                );
                true
            }
            Err(message) => {
                self.set_terminal_closed(pane_id, true);
                let notice = format!("\r\n[Attach to {pane_id} failed: {message}]\r\n");
                self.append_terminal_chunk(
                    pane_id.to_owned(),
                    live::encode_base64(notice.as_bytes()),
                );
                self.set_error("pane.attach_failed", message, true);
                true
            }
        }
    }

    /// Routes key bytes to the attached pane's PTY. Failures surface as
    /// explicit errors instead of silently dropping input.
    fn write_attached(&mut self, pane_id: &str, bytes_base64: &str) {
        let bytes = match live::decode_base64(bytes_base64) {
            Ok(bytes) => bytes,
            Err(message) => {
                self.set_error("terminal.invalid_input", message, false);
                return;
            }
        };
        match self.attaches.get_mut(pane_id) {
            Some(attach) => {
                if let Err(message) = attach.write_bytes(&bytes) {
                    self.set_error("terminal.write_failed", message, true);
                }
            }
            None => {
                self.set_error(
                    "terminal.not_attached",
                    format!("Pane {pane_id} is not attached; select or retry that pane"),
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
    if let Some(index) = workspaces
        .iter()
        .position(|workspace| workspace.id == session_workspace_id)
    {
        return workspaces.get_mut(index);
    }
    let Some(raw_path) = context_path else {
        return None;
    };
    let path = Path::new(raw_path);
    let root = workspace::git_root(path)
        .or_else(|| path.canonicalize().ok())
        .unwrap_or_else(|| path.to_path_buf());
    let normalized = root.to_string_lossy().trim_end_matches('/').to_owned();
    if let Some(index) = workspaces.iter().position(|workspace| {
        let workspace_path = workspace.path.trim_end_matches('/');
        normalized == workspace_path
            || normalized.starts_with(&format!("{workspace_path}/"))
            || workspace
                .checkouts
                .iter()
                .any(|checkout| checkout.path == normalized)
    }) {
        return workspaces.get_mut(index);
    }
    workspaces.push(workspace::inspect_temporary(
        &root,
        workspace::LOCAL_DEVICE_ID,
    ));
    workspaces.last_mut()
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
        "toggle_zoom" => decode!(ToggleZoomPayload, ToggleZoom),
        "close_workspace" => decode!(ConfirmedWorkspacePayload, CloseWorkspace),
        "close_tab" => decode!(ConfirmedTabPayload, CloseTab),
        "close_pane" => decode!(ConfirmedPanePayload, ClosePane),
        "file_open" => decode!(FileOpenPayload, FileOpen),
        "file_draft" => decode!(FileDraftPayload, FileDraft),
        "file_save" => decode!(FileSavePayload, FileSave),
        "file_conflict" => decode!(FileConflictPayload, FileConflict),
        "ui_state_update" => decode!(UiStateUpdatePayload, UiStateUpdate),
        "retry_connect" => decode!(RetryConnectPayload, RetryConnect),
        "terminal_resize" => decode!(TerminalResizePayload, TerminalResize),
        "pet_click" => Ok(ValidatedEvent::PetClick),
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
    use crate::sidebar::SessionSnapshotPayload;

    fn runtime() -> Runtime {
        let options = CoreOptions {
            schema_version: SCHEMA_VERSION,
            herdr_socket_path: Some("/tmp/herdr-core-pet-runtime.sock".to_owned()),
            herdr_bin_path: None,
            remote_targets: Vec::new(),
            app_state_path: std::env::temp_dir()
                .join(format!("herdr-core-pet-runtime-{}.json", std::process::id()))
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
