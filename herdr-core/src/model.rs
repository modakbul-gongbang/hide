use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 2;

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
    pub herdr_socket_path: String,
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
    pub changes: ChangesSnapshot,
    pub find: PaneFindSnapshot,
    pub ui_state: UiStateSnapshot,
    pub ime: ImeSnapshot,
    pub input_generation: u64,
    pub status: StatusSnapshot,
    pub pet: PetSnapshot,
}

/// What a pane search found, over the pane's whole scrollback.
///
/// The count is the reason this crosses the wire at all: the terminal view the
/// shell draws holds only the visible rows, so a counter it computed itself
/// would report what is on screen and call it the total.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PaneFindSnapshot {
    /// Which pane the result belongs to, so a result that lands after the
    /// operator has moved on is ignored rather than shown over another pane.
    pub pane_id: Option<String>,
    pub term: String,
    /// 1-based position of the current match, or 0 when there is none.
    pub index: usize,
    pub total: usize,
    /// Herdr capped the history it returned, so `total` counts what was
    /// searched rather than everything the pane has ever printed.
    pub truncated: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PetSnapshot {
    pub visible: bool,
    /// `connected` while Herdr session sync is healthy; otherwise the sync
    /// failure state, so a missing socket is never a silent idle.
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
pub struct NavigatorSnapshot {
    pub root_path: Option<String>,
    pub focused_device_id: Option<String>,
    pub focused_workspace_id: Option<String>,
    pub focused_checkout_id: Option<String>,
    pub devices: Vec<DeviceSnapshot>,
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub agents: Vec<SidebarAgentSnapshot>,
    pub provider_usage: Vec<ProviderUsageSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProviderUsageSnapshot {
    pub provider: String,
    pub label: String,
    pub window_minutes: u64,
    pub state: String,
    pub used_percent: Option<f64>,
    pub resets_at_unix_seconds: Option<u64>,
    pub message: Option<String>,
    pub last_checked_at_unix_ms: Option<u64>,
}

impl ProviderUsageSnapshot {
    pub fn initial_rows() -> Vec<Self> {
        vec![
            Self::unavailable(
                "claude",
                "Claude Code",
                "Claude Code weekly usage has not been checked yet",
                0,
            ),
            Self::unavailable(
                "codex",
                "Codex",
                "Codex weekly usage has not been checked yet",
                0,
            ),
        ]
    }

    pub fn unavailable(
        provider: impl Into<String>,
        label: impl Into<String>,
        message: impl Into<String>,
        checked_at_unix_ms: u64,
    ) -> Self {
        Self {
            provider: provider.into(),
            label: label.into(),
            window_minutes: 10_080,
            state: "unavailable".to_owned(),
            used_percent: None,
            resets_at_unix_seconds: None,
            message: Some(message.into()),
            last_checked_at_unix_ms: (checked_at_unix_ms > 0).then_some(checked_at_unix_ms),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeviceSnapshot {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub state: String,
    pub ssh_alias: Option<String>,
    pub agent_count: u32,
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
    /// The conversation id this agent is running, kept only when Herdr recorded
    /// the session as an id. A session recorded as a path is dropped here,
    /// because neither agent's fork command takes one.
    pub session_id: Option<String>,
    /// The pane this agent was spawned from, as Herdr's own lineage records it.
    pub spawned_from_pane_id: Option<String>,
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub label: String,
    pub path: String,
    pub remote_target_id: Option<String>,
    pub expanded: bool,
    pub device_id: String,
    pub repo_name: String,
    pub is_git: bool,
    pub default_branch: Option<String>,
    pub registered: bool,
    pub temporary: bool,
    /// The Herdr workspaces whose panes sit in this project, in Herdr order.
    /// Project identity is the repository path, not a Herdr workspace id, so
    /// Herdr dropping its workspace when the last pane closes leaves the row
    /// and the persisted focus in place. Commands that need a Herdr workspace
    /// target the first entry; an empty list means Herdr has none here yet.
    pub session_workspace_ids: Vec<String>,
    pub checkouts: Vec<CheckoutSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CheckoutSnapshot {
    pub id: String,
    pub workspace_id: String,
    pub label: String,
    pub path: String,
    pub branch: Option<String>,
    pub is_worktree: bool,
    pub exists: bool,
    pub temporary: bool,
    pub tabs: Vec<TabSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OverlaySnapshot {
    pub kind: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
    pub actions: Vec<OverlayActionSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OverlayActionSnapshot {
    pub id: String,
    pub label: String,
    pub destructive: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TabSnapshot {
    pub id: Option<String>,
    pub workspace_id: Option<String>,
    pub checkout_id: Option<String>,
    pub label: Option<String>,
    pub empty: bool,
    pub panes: Vec<PaneSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaneSnapshot {
    pub id: String,
    /// The three names a pane can be shown by, in the order the header prefers
    /// them. The core ships the ingredients rather than a chosen title so the
    /// local and remote projections cannot disagree about the ladder, and so
    /// the one that runs it stays testable in the shell.
    pub herdr_label: Option<String>,
    pub terminal_title: Option<String>,
    pub workspace_label: Option<String>,
    pub cwd: String,
    pub state: String,
    pub summary: Option<String>,
    pub activity_at_unix_ms: Option<u64>,
    pub fork: PaneForkSnapshot,
    /// The ports listened on from at or below this pane's working directory.
    pub ports: Vec<u16>,
}

/// The TCP listeners the machine has, with where each was started from.
///
/// Attribution to a pane is not decided here: the reader ships what it saw and
/// the projection applies the rule, so the rule stays testable without a
/// server to point it at.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ListeningPortsSnapshot {
    pub entries: Vec<ListeningPortSnapshot>,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ListeningPortSnapshot {
    pub port: u16,
    pub cwd: String,
}

/// What the pane header needs to know about forking this pane.
///
/// Both facts come from Herdr: whether the pane runs an agent whose own fork
/// command can take its recorded session, and whether this pane is itself the
/// result of such a fork.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PaneForkSnapshot {
    pub available: bool,
    pub forked_from_pane_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
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
    RightPanel,
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TerminalSnapshot {
    pub pane_id: Option<String>,
    pub sequence: u64,
    pub chunks: Vec<TerminalChunk>,
    pub closed: bool,
    pub exit_code: Option<i32>,
    pub panes: Vec<TerminalPaneSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TerminalChunk {
    pub pane_id: String,
    pub sequence: u64,
    pub bytes_base64: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TerminalPaneSnapshot {
    pub pane_id: String,
    pub closed: bool,
    pub exit_code: Option<i32>,
    pub transport_state: String,
    pub transport_message: Option<String>,
    pub transport_generation: u64,
    pub transport_attempt: u64,
    pub transport_exit_category: Option<String>,
    pub transport_retry_decision: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditorSnapshot {
    pub tabs: Vec<FileTabSnapshot>,
    pub active_tab_id: Option<String>,
    pub document: Option<EditorDocumentSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FileTabSnapshot {
    pub id: String,
    pub workspace_id: String,
    pub checkout_id: String,
    pub path: String,
    pub label: String,
    pub dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditorDocumentSnapshot {
    pub path: String,
    pub language: Option<String>,
    pub contents_utf8: Option<String>,
    pub opened_modified_at_unix_ms: Option<u64>,
    pub dirty: bool,
    pub readonly_reason: Option<String>,
    pub conflict: Option<EditorConflictSnapshot>,
}

/// The right panel's two sections. The set is closed: a third section is a
/// product decision, not a value a caller may invent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RightPanelSection {
    #[default]
    Explorer,
    Changes,
}

impl RightPanelSection {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "explorer" => Some(Self::Explorer),
            "changes" => Some(Self::Changes),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct UiStateSnapshot {
    #[serde(default = "default_panel_visible")]
    pub left_sidebar_visible: bool,
    #[serde(default = "default_panel_visible")]
    pub right_panel_visible: bool,
    /// Which of the right panel's two sections is showing. Persisted rather
    /// than held in the view, because hiding the panel tears the view down
    /// and the section has to come back the way it was left.
    #[serde(default)]
    pub right_panel_section: RightPanelSection,
    pub expanded_paths: Vec<String>,
    #[serde(default)]
    pub collapsed_workspace_ids: Vec<String>,
    pub selected_path: Option<String>,
    pub selected_pane_id: Option<String>,
    pub shortcut_bindings: BTreeMap<String, String>,
    pub pet_visible: bool,
    pub pet_origin: Option<PetOriginSnapshot>,
    pub pet_shortcut: Option<String>,
    #[serde(default)]
    pub focused_device_id: Option<String>,
    #[serde(default)]
    pub focused_checkout_id: Option<String>,
    #[serde(default)]
    pub workspace_registrations: Vec<WorkspaceRegistration>,
    #[serde(default)]
    pub device_registrations: Vec<DeviceRegistration>,
    #[serde(default = "default_accent_hex")]
    pub accent_hex: String,
    /// The interface font size, in points, that the Appearance slider sets.
    /// It scales the shell's own chrome - every `hideFont` call site - and
    /// nothing else. A pane's terminal bytes and the editor's code are sized by
    /// `pane_text_scales` instead, so the two never apply to the same text.
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Text scale for one pane's own content, keyed by pane id. A pane at the
    /// default scale is absent rather than present at 1.0, so the map stays
    /// the size of what the user actually changed.
    #[serde(default)]
    pub pane_text_scales: BTreeMap<String, f32>,
}

/// The scale a pane has until the user zooms it.
pub const DEFAULT_PANE_TEXT_SCALE: f32 = 1.0;

/// One press of the zoom chords. Small enough that the range takes several
/// presses to cross, large enough to be visible in one.
pub const PANE_TEXT_SCALE_STEP: f32 = 0.1;

/// The bounds a pane's text scale is clamped to. Below the minimum the terminal
/// is unreadable; above the maximum a standard pane holds too few columns to
/// show a command line without wrapping.
pub const MIN_PANE_TEXT_SCALE: f32 = 0.7;
pub const MAX_PANE_TEXT_SCALE: f32 = 2.0;

/// Rounded to the step so repeated presses cannot drift the stored value off
/// the ladder through float error.
pub fn clamp_pane_text_scale(scale: f32) -> f32 {
    let stepped = (scale / PANE_TEXT_SCALE_STEP).round() * PANE_TEXT_SCALE_STEP;
    stepped.clamp(MIN_PANE_TEXT_SCALE, MAX_PANE_TEXT_SCALE)
}

impl Default for UiStateSnapshot {
    fn default() -> Self {
        Self {
            left_sidebar_visible: true,
            right_panel_visible: true,
            right_panel_section: RightPanelSection::default(),
            expanded_paths: Vec::new(),
            collapsed_workspace_ids: Vec::new(),
            selected_path: None,
            selected_pane_id: None,
            shortcut_bindings: BTreeMap::new(),
            // The pet shows itself on a first run; hiding it is a choice the
            // user makes and the store then remembers (D-09).
            pet_visible: true,
            pet_origin: None,
            pet_shortcut: None,
            focused_device_id: None,
            focused_checkout_id: None,
            workspace_registrations: Vec::new(),
            device_registrations: Vec::new(),
            accent_hex: default_accent_hex(),
            font_size: default_font_size(),
            pane_text_scales: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceRegistration {
    pub id: String,
    pub label: String,
    pub path: String,
    #[serde(default = "default_local_device_id")]
    pub device_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceRegistration {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub ssh_alias: Option<String>,
}

pub(crate) fn default_local_device_id() -> String {
    "local".to_owned()
}

pub(crate) fn default_accent_hex() -> String {
    "#B9FF66".to_owned()
}

pub(crate) fn default_font_size() -> f32 {
    13.0
}

pub(crate) fn default_panel_visible() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditorConflictSnapshot {
    pub disk_modified_at_unix_ms: u64,
    pub opened_modified_at_unix_ms: u64,
}

/// One checkout's Git working-tree state, plus the diff of the file the user
/// selected in the changes view. Produced by [`crate::changes`] outside the
/// runtime mutex and ingested whole.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ChangesSnapshot {
    /// The checkout these entries describe. A view that renders entries under
    /// a different root than it asked about would be lying about whose
    /// changes it is showing, so the root travels with them.
    pub root_path: Option<String>,
    pub entries: Vec<ChangedFileSnapshot>,
    pub selected_path: Option<String>,
    pub diff: Option<ChangedFileDiffSnapshot>,
    /// Why there is nothing to list. Present whenever the reader could not
    /// produce entries, so an empty list is never mistaken for "no changes".
    pub unavailable_reason: Option<String>,
}

/// The four working-tree states this round presents. Git's porcelain codes
/// carry more distinctions than the view uses; [`ChangedFileStatus::from_porcelain`]
/// is the single place they collapse.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangedFileStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
}

impl ChangedFileStatus {
    /// Maps one porcelain v1 `XY` pair onto the presented status. Index and
    /// worktree columns are read together: a file staged as added and then
    /// edited is still an addition to the reader, and a delete on either side
    /// is a delete.
    pub fn from_porcelain(code: &str) -> Self {
        let mut characters = code.chars();
        let index = characters.next().unwrap_or(' ');
        let worktree = characters.next().unwrap_or(' ');
        if index == '?' || worktree == '?' {
            return Self::Untracked;
        }
        if index == 'D' || worktree == 'D' {
            return Self::Deleted;
        }
        if index == 'A' || worktree == 'A' {
            return Self::Added;
        }
        Self::Modified
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Modified => "modified",
            Self::Added => "added",
            Self::Deleted => "deleted",
            Self::Untracked => "untracked",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChangedFileSnapshot {
    /// Absolute, so activating a row needs no second join against the root.
    pub path: String,
    /// Relative to the checkout root, which is what the row shows.
    pub relative_path: String,
    pub status: ChangedFileStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChangedFileDiffSnapshot {
    pub path: String,
    pub text: String,
    /// Set when the diff was cut short, naming the limit that cut it. A
    /// silently truncated diff would read as a complete one.
    pub notice: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImeSnapshot {
    pub marked_text: String,
    pub selected_range: TextRangeSnapshot,
    pub replacement_range: Option<TextRangeSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TextRangeSnapshot {
    pub location: u64,
    pub length: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatusSnapshot {
    pub herdr: ProviderStatusSnapshot,
    pub remote: Vec<RemoteStatusSnapshot>,
    pub chromux: ChromuxStatusSnapshot,
    pub environment: Vec<EnvironmentStatusSnapshot>,
    pub diagnostics: Vec<DiagnosticSnapshot>,
    pub last_error: Option<LastErrorSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentStatusSnapshot {
    pub key: String,
    pub required: bool,
    pub format: String,
    pub state: String,
    pub absent_behavior: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DiagnosticSnapshot {
    pub kind: String,
    pub message: String,
    pub occurred_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProviderStatusSnapshot {
    pub state: String,
    pub socket_path: Option<String>,
    pub message: Option<String>,
    pub last_checked_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RemoteStatusSnapshot {
    pub target_id: String,
    pub state: String,
    pub message: Option<String>,
    pub last_checked_at_unix_ms: Option<u64>,
    pub session: Option<RemoteSessionSnapshot>,
    pub files: RemoteFileListSnapshot,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RemoteFileListSnapshot {
    pub root_path: Option<String>,
    pub state: String,
    pub entries: Vec<RemoteFileEntrySnapshot>,
    pub message: Option<String>,
    pub generation: u64,
}

impl RemoteFileListSnapshot {
    pub fn idle() -> Self {
        Self {
            root_path: None,
            state: "idle".to_owned(),
            entries: Vec::new(),
            message: None,
            generation: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RemoteFileEntrySnapshot {
    pub path: String,
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RemoteSessionSnapshot {
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub agents: Vec<SidebarAgentSnapshot>,
    pub active_tab_ids: BTreeMap<String, String>,
    pub focused_workspace_id: Option<String>,
    pub focused_checkout_id: Option<String>,
    pub focused_tab_id: Option<String>,
    pub focused_pane_id: Option<String>,
    pub pane_layouts: Vec<RemotePaneLayoutSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RemotePaneLayoutSnapshot {
    pub workspace_id: String,
    pub tab_id: String,
    pub focused_pane_id: String,
    pub zoomed: bool,
    pub frames: Vec<RemotePaneLayoutFrame>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RemotePaneLayoutFrame {
    pub pane_id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChromuxStatusSnapshot {
    pub state: String,
    pub profile: String,
    pub current_url: Option<String>,
    pub current_title: Option<String>,
    pub message: Option<String>,
    pub last_checked_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
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
                focused_device_id: None,
                focused_workspace_id: None,
                focused_checkout_id: None,
                devices: vec![DeviceSnapshot {
                    id: "local".to_owned(),
                    label: "This Mac".to_owned(),
                    kind: "local".to_owned(),
                    state: "ready".to_owned(),
                    ssh_alias: None,
                    agent_count: 0,
                }],
                workspaces: Vec::new(),
                agents: Vec::new(),
                provider_usage: ProviderUsageSnapshot::initial_rows(),
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
                checkout_id: None,
                label: None,
                empty: true,
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
                tabs: Vec::new(),
                active_tab_id: None,
                document: None,
            },
            changes: ChangesSnapshot::default(),
            find: PaneFindSnapshot::default(),
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
                        session: None,
                        files: RemoteFileListSnapshot::idle(),
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
            connection_message: Some("Waiting for the first herdr connection attempt".to_owned()),
            pose: "disconnected".to_owned(),
            sleep_phase: "awake".to_owned(),
            roam_allowed: false,
            badges: PetBadgesSnapshot::default(),
            attention_pane_ids: Vec::new(),
            origin: None,
            shortcut: None,
            shortcut_error: None,
            theme_id: "default".to_owned(),
        }
    }
}

/// The sections of [`Snapshot`] that ride the revisioned `rest` channel of
/// the delta wire: everything except the editor and the changes view (each
/// with its own revision), the terminal chunk ring (sequence cursor), and the
/// per-event scalars, which now include find state.
/// Owned copy retained by the runtime to stamp revisions by comparison, so
/// no mutation site needs dirty-tracking discipline.
#[derive(Clone, Debug, PartialEq)]
pub struct RestSections {
    pub navigator: NavigatorSnapshot,
    pub overlay: OverlaySnapshot,
    pub tab: TabSnapshot,
    pub connection: ConnectionSnapshot,
    pub zoomed: Option<String>,
    pub focused: FocusedSnapshot,
    pub pane_layout: Option<PaneLayoutSnapshot>,
    pub terminal_pane_id: Option<String>,
    pub terminal_closed: bool,
    pub terminal_exit_code: Option<i32>,
    pub terminal_panes: Vec<TerminalPaneSnapshot>,
    pub ui_state: UiStateSnapshot,
    pub ime: ImeSnapshot,
    pub status: StatusSnapshot,
    pub pet: PetSnapshot,
}

impl RestSections {
    pub fn capture(snapshot: &Snapshot) -> Self {
        Self {
            navigator: snapshot.navigator.clone(),
            overlay: snapshot.overlay.clone(),
            tab: snapshot.tab.clone(),
            connection: snapshot.connection.clone(),
            zoomed: snapshot.zoomed.clone(),
            focused: snapshot.focused.clone(),
            pane_layout: snapshot.pane_layout.clone(),
            terminal_pane_id: snapshot.terminal.pane_id.clone(),
            terminal_closed: snapshot.terminal.closed,
            terminal_exit_code: snapshot.terminal.exit_code,
            terminal_panes: snapshot.terminal.panes.clone(),
            ui_state: snapshot.ui_state.clone(),
            ime: snapshot.ime.clone(),
            status: snapshot.status.clone(),
            pet: snapshot.pet.clone(),
        }
    }

    /// Field-by-field equality against the live snapshot, so the unchanged
    /// case costs a comparison instead of a clone.
    pub fn matches(&self, snapshot: &Snapshot) -> bool {
        self.navigator == snapshot.navigator
            && self.overlay == snapshot.overlay
            && self.tab == snapshot.tab
            && self.connection == snapshot.connection
            && self.zoomed == snapshot.zoomed
            && self.focused == snapshot.focused
            && self.pane_layout == snapshot.pane_layout
            && self.terminal_pane_id == snapshot.terminal.pane_id
            && self.terminal_closed == snapshot.terminal.closed
            && self.terminal_exit_code == snapshot.terminal.exit_code
            && self.terminal_panes == snapshot.terminal.panes
            && self.ui_state == snapshot.ui_state
            && self.ime == snapshot.ime
            && self.status == snapshot.status
            && self.pet == snapshot.pet
    }
}

/// One delta response on the snapshot wire. `rest`, `editor`, and `changes`
/// are present only when the caller's `have_revision` predates their last
/// change; `chunks` carries only sequences past the caller's cursor. The
/// changes view holds a whole file's diff text, so it is kept off `rest`,
/// which restamps whenever any agent's elapsed time ticks.
#[derive(Serialize)]
pub struct SnapshotDeltaWire<'a> {
    pub schema_version: u32,
    pub revision: u64,
    pub rest: Option<RestWire<'a>>,
    pub editor: Option<&'a EditorSnapshot>,
    pub changes: Option<&'a ChangesSnapshot>,
    /// Find state rides top-level rather than in `rest`, because it changes on
    /// every keystroke while a search is open. In `rest` each keystroke would
    /// restamp that revision and resend the whole navigator, ui state, and pet
    /// sections with it - the wire would be sized by total state instead of by
    /// what changed. Six scalars on every response cost far less.
    pub find: &'a PaneFindSnapshot,
    pub input_generation: u64,
    pub terminal_sequence: u64,
    pub chunks: Vec<&'a TerminalChunk>,
    pub chunks_dropped: bool,
}

#[derive(Serialize)]
pub struct RestWire<'a> {
    pub navigator: &'a NavigatorSnapshot,
    pub overlay: &'a OverlaySnapshot,
    pub tab: &'a TabSnapshot,
    pub connection: &'a ConnectionSnapshot,
    pub zoomed: &'a Option<String>,
    pub focused: &'a FocusedSnapshot,
    pub pane_layout: &'a Option<PaneLayoutSnapshot>,
    pub terminal: TerminalMetaWire<'a>,
    pub ui_state: &'a UiStateSnapshot,
    pub ime: &'a ImeSnapshot,
    pub status: &'a StatusSnapshot,
    pub pet: &'a PetSnapshot,
}

#[derive(Serialize)]
pub struct TerminalMetaWire<'a> {
    pub pane_id: &'a Option<String>,
    pub closed: bool,
    pub exit_code: Option<i32>,
    pub panes: &'a [TerminalPaneSnapshot],
}
