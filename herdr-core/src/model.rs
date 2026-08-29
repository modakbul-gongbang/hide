use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CoreOptions {
    pub schema_version: u32,
    pub herdr_socket_path: Option<String>,
    #[serde(default)]
    pub herdr_bin_path: Option<String>,
    pub remote_targets: Vec<RemoteTarget>,
    pub app_state_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RemoteTarget {
    pub id: String,
    pub label: String,
    pub ssh_alias: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub navigator: NavigatorSnapshot,
    pub overlay: OverlaySnapshot,
    pub tab: TabSnapshot,
    pub connection: ConnectionSnapshot,
    pub zoomed: Option<String>,
    pub focused: FocusedSnapshot,
    pub pane_layout: Option<PaneLayoutSnapshot>,
    pub terminal: TerminalSnapshot,
    pub editor: EditorSnapshot,
    pub ui_state: UiStateSnapshot,
    pub ime: ImeSnapshot,
    pub input_generation: u64,
    pub status: StatusSnapshot,
    pub pet: PetSnapshot,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PetSnapshot {
    pub visible: bool,
    /// `connected` while the herdr session poll is answering; otherwise the
    /// poll's own failure state, so a missing socket is never a silent idle.
    pub connection: String,
    pub connection_message: Option<String>,
    pub pose: String,
    pub sleep_phase: String,
    pub roam_allowed: bool,
    pub badges: PetBadgesSnapshot,
    /// Unseen panes in click order: oldest observation first, snapshot order
    /// as the tie-break.
    pub attention_pane_ids: Vec<String>,
    pub origin: Option<PetOriginSnapshot>,
    pub shortcut: Option<String>,
    pub shortcut_error: Option<String>,
    pub theme_id: String,
    pub last_click: Option<PetClickSnapshot>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PetBadgesSnapshot {
    pub working: usize,
    pub done: usize,
    pub attention: usize,
    pub error: usize,
    pub disconnected: usize,
    pub subagents_active: u32,
    pub background_running: u32,
    pub background_failed: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PetOriginSnapshot {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PetClickSnapshot {
    /// The pane the click jumped to, or `None` when nothing was unseen and
    /// the click only raised the main window.
    pub selected_pane_id: Option<String>,
    pub at_unix_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct NavigatorSnapshot {
    pub root_path: Option<String>,
    pub focused_workspace_id: Option<String>,
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub agents: Vec<SidebarAgentSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SidebarAgentSnapshot {
    pub id: String,
    pub pane_id: String,
    pub workspace_label: String,
    pub agent_kind: String,
    pub state: String,
    pub symbol: String,
    pub summary: String,
    pub elapsed: String,
    pub sort_rank: String,
    pub activity: String,
    pub ambient: Option<AmbientSignal>,
}

/// The only three values this client ever reads out of a pane's optional
/// `ambient` object. Any other key, or a value of the wrong type, is dropped
/// during parsing and never reaches app state, the UI, or logs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AmbientSignal {
    pub subagents_active: u32,
    pub background_running: u32,
    pub background_failed: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub label: String,
    pub path: String,
    pub remote_target_id: Option<String>,
    pub expanded: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct OverlaySnapshot {
    pub kind: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
    pub actions: Vec<OverlayActionSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OverlayActionSnapshot {
    pub id: String,
    pub label: String,
    pub destructive: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TabSnapshot {
    pub id: Option<String>,
    pub workspace_id: Option<String>,
    pub label: Option<String>,
    pub panes: Vec<PaneSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaneSnapshot {
    pub id: String,
    pub label: String,
    pub cwd: String,
    pub state: String,
    pub summary: Option<String>,
    pub activity_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConnectionSnapshot {
    pub kind: String,
    pub state: String,
    pub target_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Sidebar,
    Terminal,
    Workbench,
    Pet,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FocusedSnapshot {
    pub surface: Surface,
    pub pane_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneLayoutDirection {
    Right,
    Down,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaneLayoutSnapshot {
    pub workspace_id: String,
    pub tab_id: String,
    pub focused_pane_id: String,
    pub zoomed: bool,
    pub root: PaneLayoutNodeSnapshot,
}

impl PaneLayoutSnapshot {
    pub fn pane_ids(&self) -> Vec<&str> {
        let mut pane_ids = Vec::new();
        self.root.collect_pane_ids(&mut pane_ids);
        pane_ids
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaneLayoutNodeSnapshot {
    Pane {
        pane_id: String,
    },
    Split {
        direction: PaneLayoutDirection,
        ratio: f32,
        first: Box<PaneLayoutNodeSnapshot>,
        second: Box<PaneLayoutNodeSnapshot>,
    },
}

impl PaneLayoutNodeSnapshot {
    fn collect_pane_ids<'a>(&'a self, pane_ids: &mut Vec<&'a str>) {
        match self {
            Self::Pane { pane_id } => pane_ids.push(pane_id),
            Self::Split { first, second, .. } => {
                first.collect_pane_ids(pane_ids);
                second.collect_pane_ids(pane_ids);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalSnapshot {
    pub pane_id: Option<String>,
    pub sequence: u64,
    pub chunks: Vec<TerminalChunk>,
    pub closed: bool,
    pub exit_code: Option<i32>,
    pub panes: Vec<TerminalPaneSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalChunk {
    pub pane_id: String,
    pub sequence: u64,
    pub bytes_base64: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalPaneSnapshot {
    pub pane_id: String,
    pub closed: bool,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EditorSnapshot {
    pub path: Option<String>,
    pub language: Option<String>,
    pub contents_utf8: Option<String>,
    pub opened_modified_at_unix_ms: Option<u64>,
    pub dirty: bool,
    pub readonly_reason: Option<String>,
    pub conflict: Option<EditorConflictSnapshot>,
    pub diff: Option<DiffSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UiStateSnapshot {
    pub expanded_paths: Vec<String>,
    pub selected_path: Option<String>,
    pub selected_pane_id: Option<String>,
    pub shortcut_bindings: BTreeMap<String, String>,
    pub pet_visible: bool,
    pub pet_origin: Option<PetOriginSnapshot>,
    pub pet_shortcut: Option<String>,
}

impl Default for UiStateSnapshot {
    fn default() -> Self {
        Self {
            expanded_paths: Vec::new(),
            selected_path: None,
            selected_pane_id: None,
            shortcut_bindings: BTreeMap::new(),
            // The pet shows itself on a first run; hiding it is a choice the
            // user makes and the store then remembers (D-09).
            pet_visible: true,
            pet_origin: None,
            pet_shortcut: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct EditorConflictSnapshot {
    pub disk_modified_at_unix_ms: u64,
    pub opened_modified_at_unix_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffSnapshot {
    pub added_lines: Vec<u32>,
    pub removed_lines: Vec<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ImeSnapshot {
    pub marked_text: String,
    pub selected_range: TextRangeSnapshot,
    pub replacement_range: Option<TextRangeSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TextRangeSnapshot {
    pub location: u64,
    pub length: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct StatusSnapshot {
    pub herdr: ProviderStatusSnapshot,
    pub remote: Vec<RemoteStatusSnapshot>,
    pub chromux: ChromuxStatusSnapshot,
    pub environment: Vec<EnvironmentStatusSnapshot>,
    pub diagnostics: Vec<DiagnosticSnapshot>,
    pub last_error: Option<LastErrorSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnvironmentStatusSnapshot {
    pub key: String,
    pub required: bool,
    pub format: String,
    pub state: String,
    pub absent_behavior: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticSnapshot {
    pub kind: String,
    pub message: String,
    pub occurred_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderStatusSnapshot {
    pub state: String,
    pub socket_path: Option<String>,
    pub message: Option<String>,
    pub last_checked_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteStatusSnapshot {
    pub target_id: String,
    pub state: String,
    pub message: Option<String>,
    pub last_checked_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChromuxStatusSnapshot {
    pub state: String,
    pub profile: String,
    pub current_url: Option<String>,
    pub current_title: Option<String>,
    pub message: Option<String>,
    pub last_checked_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LastErrorSnapshot {
    pub kind: String,
    pub message: String,
    pub retryable: bool,
    pub occurred_at: u64,
}

impl Snapshot {
    pub fn initial(options: &CoreOptions) -> Self {
        let herdr_state = if options.herdr_socket_path.is_some() {
            "not_connected"
        } else {
            "unconfigured"
        };
        let herdr_message = if options.herdr_socket_path.is_some() {
            Some("Waiting for the first herdr connection attempt".to_owned())
        } else {
            Some("No herdr socket path was configured".to_owned())
        };

        Self {
            schema_version: SCHEMA_VERSION,
            navigator: NavigatorSnapshot {
                root_path: None,
                focused_workspace_id: None,
                workspaces: Vec::new(),
                agents: Vec::new(),
            },
            overlay: OverlaySnapshot {
                kind: None,
                title: None,
                message: None,
                actions: Vec::new(),
            },
            tab: TabSnapshot {
                id: None,
                workspace_id: None,
                label: None,
                panes: Vec::new(),
            },
            connection: ConnectionSnapshot {
                kind: "local".to_owned(),
                state: "not_connected".to_owned(),
                target_id: None,
            },
            zoomed: None,
            focused: FocusedSnapshot {
                surface: Surface::Terminal,
                pane_id: None,
            },
            pane_layout: None,
            terminal: TerminalSnapshot {
                pane_id: None,
                sequence: 0,
                chunks: Vec::new(),
                closed: false,
                exit_code: None,
                panes: Vec::new(),
            },
            editor: EditorSnapshot {
                path: None,
                language: None,
                contents_utf8: None,
                opened_modified_at_unix_ms: None,
                dirty: false,
                readonly_reason: None,
                conflict: None,
                diff: None,
            },
            ui_state: UiStateSnapshot::default(),
            ime: ImeSnapshot {
                marked_text: String::new(),
                selected_range: TextRangeSnapshot {
                    location: 0,
                    length: 0,
                },
                replacement_range: None,
            },
            input_generation: 0,
            status: StatusSnapshot {
                herdr: ProviderStatusSnapshot {
                    state: herdr_state.to_owned(),
                    socket_path: options.herdr_socket_path.clone(),
                    message: herdr_message,
                    last_checked_at_unix_ms: None,
                },
                remote: options
                    .remote_targets
                    .iter()
                    .map(|target| RemoteStatusSnapshot {
                        target_id: target.id.clone(),
                        state: "not_connected".to_owned(),
                        message: Some("Waiting for the first remote connection attempt".to_owned()),
                        last_checked_at_unix_ms: None,
                    })
                    .collect(),
                chromux: ChromuxStatusSnapshot {
                    state: "not_checked".to_owned(),
                    profile: "default".to_owned(),
                    current_url: None,
                    current_title: None,
                    message: Some("Browser availability has not been checked".to_owned()),
                    last_checked_at_unix_ms: None,
                },
                environment: Vec::new(),
                diagnostics: Vec::new(),
                last_error: None,
            },
            pet: PetSnapshot::initial(),
        }
    }
}

impl PetSnapshot {
    pub fn initial() -> Self {
        Self {
            visible: true,
            connection: "not_connected".to_owned(),
            connection_message: Some(
                "Waiting for the first herdr connection attempt".to_owned(),
            ),
            pose: "disconnected".to_owned(),
            sleep_phase: "awake".to_owned(),
            roam_allowed: false,
            badges: PetBadgesSnapshot::default(),
            attention_pane_ids: Vec::new(),
            origin: None,
            shortcut: None,
            shortcut_error: None,
            theme_id: "default".to_owned(),
            last_click: None,
        }
    }
}
