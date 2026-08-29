use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CoreOptions {
    pub schema_version: u32,
    pub herdr_socket_path: Option<String>,
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
    pub terminal: TerminalSnapshot,
    pub editor: EditorSnapshot,
    pub ui_state: UiStateSnapshot,
    pub ime: ImeSnapshot,
    pub input_generation: u64,
    pub status: StatusSnapshot,
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Sidebar,
    Terminal,
    Workbench,
    Pet,
}

#[derive(Clone, Debug, Serialize)]
pub struct FocusedSnapshot {
    pub surface: Surface,
    pub pane_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalSnapshot {
    pub pane_id: Option<String>,
    pub sequence: u64,
    pub chunks: Vec<TerminalChunk>,
    pub closed: bool,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalChunk {
    pub sequence: u64,
    pub bytes_base64: String,
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UiStateSnapshot {
    pub expanded_paths: Vec<String>,
    pub selected_path: Option<String>,
    pub selected_pane_id: Option<String>,
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
            terminal: TerminalSnapshot {
                pane_id: None,
                sequence: 0,
                chunks: Vec::new(),
                closed: false,
                exit_code: None,
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
        }
    }
}
