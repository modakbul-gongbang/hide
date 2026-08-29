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
    CoreOptions, DiagnosticSnapshot, LastErrorSnapshot, PaneLayoutSnapshot, SCHEMA_VERSION,
    Snapshot, Surface, TerminalChunk, TerminalPaneSnapshot, UiStateSnapshot,
};
use crate::sidebar::{SessionSnapshotPayload, project_agents};
use crate::{chromux, environment, files, live, persistence};

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
    create_worktree: bool,
}

#[derive(Debug, Deserialize)]
struct CreateTabPayload {
    workspace_id: String,
    label: String,
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
}

#[derive(Debug, Deserialize)]
struct RetryConnectPayload {
    target_id: String,
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
}

pub struct Runtime {
    snapshot: Snapshot,
    state_path: PathBuf,
    live: Option<LiveContext>,
    attaches: HashMap<String, PaneAttach>,
    attach_generations: HashMap<String, u64>,
    next_attach_generation: u64,
    terminal_sizes: HashMap<String, (u16, u16)>,
}

impl Runtime {
    pub fn new(options: CoreOptions, environment: environment::EnvironmentReport) -> Self {
        let state_path = PathBuf::from(&options.app_state_path);
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
        Self {
            snapshot,
            state_path,
            live: None,
            attaches: HashMap::new(),
            attach_generations: HashMap::new(),
            next_attach_generation: 0,
            terminal_sizes: HashMap::new(),
        }
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn set_live(&mut self, context: LiveContext) {
        self.live = Some(context);
    }

    /// Applies a live session poll result: projected agents on success, an
    /// explicit herdr status on failure. Returns whether the snapshot changed.
    pub fn ingest_session(
        &mut self,
        fetched: Result<SessionSnapshotPayload, SessionFetchError>,
    ) -> bool {
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
                match (project_agents(payload), layout) {
                    (Ok(agents), Ok(layout)) => ("connected", None, Some(agents), layout),
                    (Err(projection_error), Ok(layout)) => (
                        "malformed",
                        Some(format!(
                            "Herdr agents could not be projected: {projection_error}"
                        )),
                        None,
                        layout,
                    ),
                    (_, Err(projection_error)) => (
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

        let mut changed = false;
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
        changed
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
                self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: "pane.focus".to_owned(),
                    message: format!("Pane {pane_id} focused in {elapsed_ms} ms"),
                    occurred_at: unix_milliseconds(),
                });
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
                self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: format!("pane.split.{}", direction.as_str()),
                    message: format!(
                        "Pane {pane_id} split {} to {created_pane_id} in {elapsed_ms} ms",
                        direction.as_str()
                    ),
                    occurred_at: unix_milliseconds(),
                });
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
                    self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                        kind: "pane.layout.refresh_pending".to_owned(),
                        message,
                        occurred_at: unix_milliseconds(),
                    });
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
                self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: "pane.zoom_toggled".to_owned(),
                    message: format!(
                        "Pane {pane_id} zoom {} in {elapsed_ms} ms",
                        layout_zoomed
                            .map(|zoomed| if zoomed { "enabled" } else { "disabled" })
                            .unwrap_or("awaiting authoritative layout")
                    ),
                    occurred_at: unix_milliseconds(),
                });
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
                    self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                        kind: "pane.layout.refresh_pending".to_owned(),
                        message,
                        occurred_at: unix_milliseconds(),
                    });
                }
                if let Some(layout) = outcome.layout {
                    self.apply_pane_layout(layout);
                }
                true
            }
            (PaneControlAction::Close { pane_id }, Ok(outcome)) => {
                self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: "pane.close".to_owned(),
                    message: format!("Pane {pane_id} closed in {elapsed_ms} ms"),
                    occurred_at: unix_milliseconds(),
                });
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
                    self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                        kind: "pane.layout.refresh_pending".to_owned(),
                        message,
                        occurred_at: unix_milliseconds(),
                    });
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
            ValidatedEvent::Click(payload) => {
                let _ = (payload.x, payload.y, payload.button, payload.click_count);
                self.snapshot.focused.surface = payload.surface;
                true
            }
            ValidatedEvent::FocusPane(payload) => {
                let Some(context) = self.live.as_ref().cloned() else {
                    self.set_error(
                        "pane.control_unavailable",
                        "Pane focus requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let pane_id = payload.pane_id;
                self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: "pane.focus.requested".to_owned(),
                    message: format!("Focusing pane {pane_id}"),
                    occurred_at: unix_milliseconds(),
                });
                if let Err(message) =
                    live::spawn_pane_control(context, PaneControlAction::Focus { pane_id })
                {
                    self.set_error("pane.focus_worker_failed", message, true);
                }
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
                let _ = (payload.path, payload.label, payload.create_worktree);
                false
            }
            ValidatedEvent::CreateTab(payload) => {
                let _ = (payload.workspace_id, payload.label);
                false
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
                self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: format!("pane.split.{}.requested", direction.as_str()),
                    message: format!("Splitting pane {pane_id} {}", direction.as_str()),
                    occurred_at: unix_milliseconds(),
                });
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
                self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: "pane.zoom.requested".to_owned(),
                    message: format!("Toggling zoom for pane {pane_id}"),
                    occurred_at: unix_milliseconds(),
                });
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
                self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: "pane.close.requested".to_owned(),
                    message: format!("Closing pane {pane_id}"),
                    occurred_at: unix_milliseconds(),
                });
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
                self.snapshot.ui_state = UiStateSnapshot {
                    expanded_paths: payload.expanded_paths,
                    selected_path: payload.selected_path,
                    selected_pane_id: payload.selected_pane_id,
                    shortcut_bindings: payload.shortcut_bindings,
                };
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
        self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
            kind: "pane.attach.requested".to_owned(),
            message: format!("Attaching pane {pane_id}"),
            occurred_at: unix_milliseconds(),
        });
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
                self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: "pane.attach.ready".to_owned(),
                    message: format!("Pane {pane_id} attached in {elapsed_ms} ms"),
                    occurred_at: unix_milliseconds(),
                });
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
