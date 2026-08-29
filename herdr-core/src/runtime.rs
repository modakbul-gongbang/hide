use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::live::{
    LiveContext, PaneAttach, PaneControlAction, PaneSplitDirection, SessionFetchError,
};
use crate::model::{
    CoreOptions, DiagnosticSnapshot, LastErrorSnapshot, SCHEMA_VERSION, Snapshot, Surface,
    TerminalChunk, UiStateSnapshot,
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
}

#[derive(Debug, Deserialize)]
struct RetryConnectPayload {
    target_id: String,
}

#[derive(Debug, Deserialize)]
struct TerminalResizePayload {
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
    attach: Option<PaneAttach>,
    attach_generation: u64,
    terminal_rows: u16,
    terminal_cols: u16,
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
            attach: None,
            attach_generation: 0,
            terminal_rows: 24,
            terminal_cols: 80,
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
        let (state, message, agents) = match fetched {
            Ok(payload) => match project_agents(payload) {
                Ok(agents) => ("connected", None, Some(agents)),
                Err(projection_error) => (
                    "malformed",
                    Some(format!(
                        "Herdr agents could not be projected: {projection_error}"
                    )),
                    None,
                ),
            },
            Err(error) => (error.state(), Some(error.message().to_owned()), None),
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
        changed
    }

    /// Appends live pane bytes when the delivering attach is still current.
    pub fn ingest_attach_output(&mut self, generation: u64, bytes: &[u8]) -> bool {
        if generation != self.attach_generation {
            return false;
        }
        let pane_id = match self.attach.as_ref() {
            Some(attach) => attach.pane_id.clone(),
            None => return false,
        };
        self.append_terminal_chunk(pane_id, live::encode_base64(bytes));
        true
    }

    /// Marks the terminal closed when the current attach stream ends.
    pub fn ingest_attach_exit(&mut self, generation: u64, message: String) -> bool {
        if generation != self.attach_generation {
            return false;
        }
        let pane_id = match self.attach.as_ref() {
            Some(attach) => attach.pane_id.clone(),
            None => return false,
        };
        self.snapshot.terminal.closed = true;
        let notice = format!("\r\n[{message}]\r\n");
        self.append_terminal_chunk(pane_id, live::encode_base64(notice.as_bytes()));
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
                self.append_terminal_chunk(payload.pane_id, payload.bytes_base64);
                true
            }
            ValidatedEvent::SessionSnapshot(payload) => match project_agents(payload) {
                Ok(agents) => {
                    self.snapshot.navigator.agents = agents;
                    true
                }
                Err(message) => {
                    self.set_error("herdr.invalid_tokens", message, false);
                    true
                }
            },
            ValidatedEvent::Click(payload) => {
                let _ = (payload.x, payload.y, payload.button, payload.click_count);
                self.snapshot.focused.surface = payload.surface;
                true
            }
            ValidatedEvent::FocusPane(payload) => {
                self.snapshot.focused.surface = Surface::Terminal;
                self.snapshot.focused.pane_id = Some(payload.pane_id.clone());
                self.snapshot.terminal.pane_id = Some(payload.pane_id.clone());
                if self.live.is_some() {
                    self.attach_pane(&payload.pane_id);
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
                match live::execute_pane_control(
                    &context,
                    PaneControlAction::Split {
                        pane_id: &pane_id,
                        direction: payload.direction,
                        cwd: Some(&payload.cwd),
                    },
                ) {
                    Ok(()) => {
                        self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                            kind: format!("pane.split.{}", payload.direction.as_str()),
                            message: format!(
                                "Pane {pane_id} split {} successfully",
                                payload.direction.as_str()
                            ),
                            occurred_at: unix_milliseconds(),
                        });
                        true
                    }
                    Err(message) => {
                        self.set_error("pane.split_failed", message, true);
                        true
                    }
                }
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
                match live::execute_pane_control(
                    &context,
                    PaneControlAction::ToggleZoom {
                        pane_id: &payload.pane_id,
                    },
                ) {
                    Ok(()) => {
                        let zoomed = self.snapshot.zoomed.as_deref() == Some(&payload.pane_id);
                        self.snapshot.zoomed = (!zoomed).then_some(payload.pane_id.clone());
                        self.snapshot.status.diagnostics.push(DiagnosticSnapshot {
                            kind: "pane.zoom_toggled".to_owned(),
                            message: format!(
                                "Pane {} zoom is {}",
                                payload.pane_id,
                                if zoomed { "off" } else { "on" }
                            ),
                            occurred_at: unix_milliseconds(),
                        });
                        true
                    }
                    Err(message) => {
                        self.set_error("pane.zoom_failed", message, true);
                        true
                    }
                }
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
                let _ = (payload.pane_id, payload.confirmed);
                false
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
                self.terminal_rows = payload.rows;
                self.terminal_cols = payload.cols;
                if let Some(attach) = self.attach.as_mut()
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

    /// Attaches the terminal to a live pane, replacing any previous attach.
    /// Re-focusing the already attached pane is a no-op so repeated clicks
    /// cannot kill and respawn the transport.
    fn attach_pane(&mut self, pane_id: &str) {
        if self
            .attach
            .as_ref()
            .is_some_and(|attach| attach.pane_id == pane_id && !self.snapshot.terminal.closed)
        {
            return;
        }
        self.attach_generation = self.attach_generation.saturating_add(1);
        self.attach = None;
        self.snapshot.terminal.closed = false;
        self.snapshot.terminal.exit_code = None;
        // Full terminal reset so the previous pane's grid cannot bleed into
        // the new pane; herdr redraws the pane content after attach.
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(b"\x1bc"));
        let context = self
            .live
            .as_ref()
            .cloned()
            .expect("attach_pane is only called with live configured");
        match PaneAttach::spawn(
            &context,
            pane_id,
            self.attach_generation,
            self.terminal_rows,
            self.terminal_cols,
        ) {
            Ok(attach) => {
                self.attach = Some(attach);
            }
            Err(message) => {
                self.snapshot.terminal.closed = true;
                let notice = format!("\r\n[Attach to {pane_id} failed: {message}]\r\n");
                self.append_terminal_chunk(
                    pane_id.to_owned(),
                    live::encode_base64(notice.as_bytes()),
                );
                self.set_error("pane.attach_failed", message, true);
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
        match self.attach.as_mut() {
            Some(attach) if attach.pane_id == pane_id => {
                if let Err(message) = attach.write_bytes(&bytes) {
                    self.set_error("terminal.write_failed", message, true);
                }
            }
            Some(attach) => {
                let message = format!(
                    "Input targeted pane {pane_id} but the terminal is attached to {}",
                    attach.pane_id
                );
                self.set_error("terminal.pane_mismatch", message, false);
            }
            None => {
                self.set_error(
                    "terminal.not_attached",
                    "No pane is attached; select an agent in the sidebar first",
                    true,
                );
            }
        }
    }

    fn append_terminal_chunk(&mut self, pane_id: String, bytes_base64: String) {
        self.snapshot.terminal.sequence = self.snapshot.terminal.sequence.saturating_add(1);
        self.snapshot.terminal.pane_id = Some(pane_id);
        self.snapshot.terminal.chunks.push(TerminalChunk {
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
