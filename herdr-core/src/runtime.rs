use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::ffi::ChangeNotifier;
use crate::fork::{ForkRequest, ForkableAgent, fork_name, is_forkable};
use crate::live::{
    LiveContext, PaneControlAction, PaneControlOutcome, PaneResizeDirection, PaneSplitDirection,
    RemoteControlAction, RemoteControlContext, RemoteControlOutcome, RemoteTerminalContext,
    SessionFetchError, TerminalSession, TerminalSessionContext, TerminalSessionMode,
};
use crate::model::CheckoutSnapshot;
use crate::model::SidebarAgentSnapshot;
use crate::model::{
    CoreOptions, DEFAULT_PANE_TEXT_SCALE, DiagnosticSnapshot, EditorDocumentSnapshot,
    EditorTabKind, EditorTabSnapshot, LastErrorSnapshot, PANE_TEXT_SCALE_STEP, PaneFindSnapshot,
    PaneForkSnapshot, PaneLayoutSnapshot, PaneSnapshot, PetBadgesSnapshot, PetOriginSnapshot,
    PetSnapshot, RemoteFileEntrySnapshot, RemoteFileListSnapshot, RemoteSessionSnapshot,
    RightPanelSection, SCHEMA_VERSION, Snapshot, StripTabKind, StripTabSnapshot, Surface,
    TabSnapshot, TerminalChunk, TerminalPaneSnapshot, UiStateSnapshot, WorkspaceSnapshot,
    clamp_pane_text_scale,
};
use crate::remote::RusshSftpTransport;
use crate::remote_files::{FileEntry, FileKind, FileService, RemoteFileService};
use crate::sidebar::{ReadRecordScope, SessionSnapshotPayload, project_agents};
use crate::{chromux, environment, files, live, persistence, pet, session_sync, workspace};

/// Places one checkout's tab strip.
///
/// The strip is a sequence of slots. A slot an entry already held it keeps, so
/// a file tab the operator dropped between two Herdr tabs stays where it was
/// put. The Herdr slots are then filled from Herdr's own tab order, because
/// Herdr owns where its tabs sit and a tab it moved has to move here too. A
/// tab that is new to the strip takes a slot at the end, which is where both a
/// newly opened file and a newly created Herdr tab belong.
fn ordered_strip(
    stored: &[String],
    herdr: &[StripTabSnapshot],
    files: &[StripTabSnapshot],
    workspace_of: &BTreeMap<String, String>,
) -> Vec<StripTabSnapshot> {
    let mut by_id = BTreeMap::new();
    for entry in herdr.iter().chain(files.iter()) {
        by_id.insert(entry.id.as_str(), entry.clone());
    }
    // A stored order that names the same tab twice holds one slot, not two, so
    // the Herdr slot count stays equal to the Herdr tab count below.
    let mut placed = Vec::with_capacity(herdr.len() + files.len());
    let mut held = BTreeSet::new();
    for id in stored {
        let Some(entry) = by_id.get(id.as_str()) else {
            continue;
        };
        if held.insert(entry.id.clone()) {
            placed.push(entry.clone());
        }
    }
    placed.extend(
        herdr
            .iter()
            .chain(files.iter())
            .filter(|entry| !held.contains(&entry.id))
            .cloned(),
    );
    // Herdr orders its own workspace's tabs; it has no order that spans two of
    // them. So each Herdr slot is filled from the queue of the workspace that
    // slot already belongs to, which leaves Hide owning how the workspaces and
    // the file tabs interleave. Refilling from one flat queue is what undid
    // every drag in a checkout two Herdr workspaces share.
    let mut queues: BTreeMap<&str, VecDeque<&StripTabSnapshot>> = BTreeMap::new();
    let owner = |entry: &StripTabSnapshot| {
        workspace_of
            .get(&entry.source_id)
            .map_or("", |workspace_id| workspace_id.as_str())
    };
    for entry in herdr {
        queues.entry(owner(entry)).or_default().push_back(entry);
    }
    for entry in &mut placed {
        if entry.kind == StripTabKind::Herdr {
            *entry = queues
                .get_mut(owner(entry))
                .and_then(VecDeque::pop_front)
                .expect("every Herdr slot has a Herdr tab from its own workspace to fill it")
                .clone();
        }
    }
    placed
}

/// One reorder the operator asked for that Herdr has not reported back yet.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingTabMove {
    /// The whole strip order the entry was dropped into, as strip entry ids.
    desired: Vec<String>,
    /// The Herdr workspace the move was asked of. A checkout can hold tabs
    /// from several workspaces, and `tab.move` is indexed inside one of them,
    /// so this is the workspace whose reported order settles the move.
    workspace_id: String,
    /// The moved tab's own workspace's tabs in this checkout, in the order
    /// the operator asked for. That workspace reporting exactly this order is
    /// what commits the arrangement.
    herdr_order: Vec<String>,
    generation: u64,
}

/// Translates a wanted Herdr tab order into the index `tab.move` takes.
///
/// Herdr counts the insertion point in the list it still holds, before the
/// moved tab is taken out of it, so the index is the current position of
/// whichever tab is to end up behind the moved one.
///
/// `workspace_order` is the whole Herdr workspace's tab list, because that is
/// the list Herdr indexes. `desired` is the order the operator asked for in
/// one checkout's strip, which is a subset of it: a workspace's tabs are split
/// across a repository and its worktrees whenever their panes are. Reading the
/// index off the subset instead lands the tab elsewhere as soon as the
/// checkout's tabs do not start at the workspace's first position.
///
/// A tab dropped at the end of its checkout's strip has no successor there, so
/// it goes in front of whichever workspace tab currently follows the checkout's
/// last tab, and at the end of the workspace when nothing follows.
///
/// `None` means the move cannot be expressed as a Herdr move: the named tab is
/// not one of Herdr's, or an anchor it needs is not in the workspace's list.
fn herdr_insert_index(
    workspace_order: &[String],
    desired: &[String],
    moved: &str,
) -> Option<usize> {
    let position = desired.iter().position(|tab_id| tab_id == moved)?;
    let index_of = |wanted: &str| workspace_order.iter().position(|tab_id| tab_id == wanted);
    if let Some(successor) = desired.get(position + 1) {
        return index_of(successor);
    }
    let Some(predecessor) = position
        .checked_sub(1)
        .and_then(|before| desired.get(before))
    else {
        // The checkout has one Herdr tab, so there is nothing to move it past.
        // Its own position is the index that leaves the workspace unchanged.
        return index_of(moved);
    };
    let after_predecessor = index_of(predecessor)? + 1;
    Some(
        workspace_order[after_predecessor..]
            .iter()
            .position(|tab_id| tab_id != moved)
            .map_or(workspace_order.len(), |offset| after_predecessor + offset),
    )
}

/// The folders that have to be open for a revealed path to have a row in the
/// tree: every ancestor between the checkout root and the path, and the path
/// itself when it is a folder.
///
/// The checkout root is left out because the outline always expands its own
/// root row; recording it would put a path in the persisted set that the view
/// never reads.
fn reveal_expansion_paths(checkout_path: &str, path: &str, is_directory: bool) -> Vec<String> {
    let root = Path::new(checkout_path);
    let Ok(relative) = Path::new(path).strip_prefix(root) else {
        return Vec::new();
    };
    let mut expanded = Vec::new();
    let mut current = root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        current.push(component);
        if components.peek().is_none() && !is_directory {
            break;
        }
        expanded.push(current.to_string_lossy().into_owned());
    }
    expanded
}

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
    input_trace: Option<crate::model::TerminalInputTrace>,
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

/// Why the shell asked for a pane focus.
///
/// The read axis needs the two apart. An operator focus is the act the whole
/// read record rests on; a launch restore reinstates the selection the last
/// session ended on, which says nothing about whether the operator has looked
/// at what changed while the app was closed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum PaneFocusOrigin {
    Operator,
    Restore,
}

/// How long a view-state notification may stay unconfirmed before Hide stops
/// treating Herdr's answer as pending.
///
/// The round trip measured on this machine is roughly 130 ms for the request
/// and 170 ms more for the confirming event, so three seconds never trips on
/// a healthy server while still releasing a value quickly when Herdr has
/// stopped answering. The value is kept when the wait expires; only the
/// waiting stops.
const VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS: u64 = 3_000;

/// A view-state change Hide has already made and told Herdr about.
///
/// Hide owns the visible tab and the focused pane, so the value in the
/// snapshot is not a prediction to be undone. This records only that a
/// notification is in flight, which is what tells a Herdr event naming an
/// older value apart from an operator focusing something outside Hide.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingViewFocus {
    /// The checkout the tab belongs to. Empty for a pane focus, which is
    /// identified by its pane id alone.
    scope_id: String,
    /// The tab or pane id Hide asked Herdr to focus.
    target_id: String,
    requested_at_unix_ms: u64,
}

impl PendingViewFocus {
    fn new(scope_id: impl Into<String>, target_id: impl Into<String>) -> Self {
        Self {
            scope_id: scope_id.into(),
            target_id: target_id.into(),
            requested_at_unix_ms: unix_milliseconds(),
        }
    }

    fn expired_at(&self, now_unix_ms: u64) -> bool {
        now_unix_ms.saturating_sub(self.requested_at_unix_ms) >= VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS
    }
}

/// How many diagnostics the snapshot keeps. Newest are kept; the oldest go.
const DIAGNOSTIC_RETENTION: usize = 256;

/// How many tabs keep their panes attached: the one on screen and the four
/// most recently shown.
///
/// Five is the operator's working set, not a memory bound. Time-based expiry
/// was rejected because the core owns no timer and the behaviour could not be
/// proved without one; a visit count is decided by the same events that draw
/// the screen.
const ATTACHED_TAB_LIMIT: usize = 5;

/// What Herdr says about its tabs, kept per Herdr workspace.
///
/// A checkout is keyed by path, so several Herdr workspaces can share one
/// checkout, and each of them names an active tab. Only the focused
/// workspace's active tab is a tab Herdr has focused; the others are each
/// workspace's memory of where it was last, and following one of those moved
/// the canvas away from the tab the operator had just chosen.
#[derive(Clone, Debug, Default, PartialEq)]
struct HerdrTabView {
    /// Herdr's active tab in each Herdr workspace.
    active_tab_by_workspace: BTreeMap<String, String>,
    /// The Herdr workspace that owns each tab.
    workspace_by_tab: BTreeMap<String, String>,
    /// The active tab of Herdr's focused workspace.
    focused_tab_id: Option<String>,
}

impl HerdrTabView {
    fn from_payload(payload: &SessionSnapshotPayload) -> Self {
        let active_tab_by_workspace = payload
            .workspaces
            .iter()
            .filter_map(|workspace| {
                let active_tab_id = workspace.active_tab_id.as_deref()?.trim();
                (!active_tab_id.is_empty())
                    .then(|| (workspace.workspace_id.clone(), active_tab_id.to_owned()))
            })
            .collect::<BTreeMap<_, _>>();
        let mut workspace_by_tab = payload
            .tabs
            .iter()
            .filter(|tab| !tab.workspace_id.trim().is_empty())
            .map(|tab| (tab.tab_id.clone(), tab.workspace_id.clone()))
            .collect::<BTreeMap<_, _>>();
        for layout in &payload.layouts {
            workspace_by_tab
                .entry(layout.tab_id.clone())
                .or_insert_with(|| layout.workspace_id.clone());
        }
        // The focused pane's workspace stands in when the session names no
        // focused workspace, which is the shape older payloads and the test
        // fixtures have.
        let focused_workspace_id = payload
            .focused_workspace_id
            .as_deref()
            .map(str::trim)
            .filter(|workspace_id| !workspace_id.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                let focused_pane_id = payload.focused_pane_id.as_deref()?;
                payload
                    .layouts
                    .iter()
                    .find(|layout| {
                        layout
                            .panes
                            .iter()
                            .any(|pane| pane.pane_id == focused_pane_id)
                    })
                    .map(|layout| layout.workspace_id.clone())
            });
        let focused_tab_id = focused_workspace_id
            .as_deref()
            .and_then(|workspace_id| active_tab_by_workspace.get(workspace_id))
            .cloned();
        Self {
            active_tab_by_workspace,
            workspace_by_tab,
            focused_tab_id,
        }
    }

    /// Whether Herdr shows this tab in the workspace that owns it. That is
    /// what confirms a tab focus Hide asked for: which workspace Herdr's
    /// keyboard is in does not decide it.
    fn is_active_in_its_workspace(&self, tab_id: &str) -> bool {
        self.workspace_by_tab
            .get(tab_id)
            .and_then(|workspace_id| self.active_tab_by_workspace.get(workspace_id))
            .is_some_and(|active_tab_id| active_tab_id == tab_id)
    }
}

/// Which of the two view-state values a pending notification is about.
///
/// The tab and the pane run the same four paths - confirm, follow, refuse,
/// time out - and differ only in the words their records use. Naming those
/// words once is what keeps the paths from drifting apart: the refusal and
/// the timeout had already grown two different reporting shapes for the same
/// event before this existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewFocusSlot {
    Tab,
    Pane,
}

impl ViewFocusSlot {
    const ALL: [Self; 2] = [Self::Tab, Self::Pane];

    fn what(self) -> &'static str {
        match self {
            Self::Tab => "tab",
            Self::Pane => "pane",
        }
    }

    fn id_key(self) -> &'static str {
        match self {
            Self::Tab => "tab_id",
            Self::Pane => "pane_id",
        }
    }

    fn refused_kind(self) -> &'static str {
        match self {
            Self::Tab => "tab.focus.refused",
            Self::Pane => "pane.focus.refused",
        }
    }

    /// What Hide does with the value it kept, said the way the operator would
    /// describe it: a tab is on screen, a pane has the keyboard.
    fn kept_phrase(self) -> &'static str {
        match self {
            Self::Tab => "Hide keeps showing it",
            Self::Pane => "Hide keeps it focused",
        }
    }
}

#[derive(Debug, Deserialize)]
struct FocusPaneRequestPayload {
    pane_id: String,
    origin: PaneFocusOrigin,
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
#[serde(deny_unknown_fields)]
struct GithubRequestPayload {
    workspace_id: String,
    #[serde(default)]
    refresh: bool,
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
struct ReorderTabPayload {
    workspace_id: String,
    checkout_id: String,
    /// The strip entry being moved, by its strip id, not the Herdr tab id:
    /// the strip is what the operator dragged in and it holds both kinds.
    tab_id: String,
    /// Where the entry ends up, as its index in the resulting strip.
    to_index: usize,
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

/// The prefix every pane id belonging to one remote target carries. It is
/// both the terminal-pane ownership test and the read record ledger's scope
/// for that target, so the two cannot disagree about which panes are its own.
fn remote_pane_id_prefix(target_id: &str) -> String {
    format!("remote:{target_id}:pane:")
}

/// Copies each pane's status word and close-confirmation answer from the agent
/// rows that carry the read axis.
///
/// A pane tree is projected separately from the agent rows, by a pass that
/// cannot see the read record ledger and does not know which pane is focused.
/// Left alone it publishes `Done` and demands a close confirmation for every
/// pane the operator has already read. One owner decides the answer; every
/// tree copies it, local and remote alike, because a pane is a pane.
fn sync_pane_status(workspaces: &mut [WorkspaceSnapshot], agents: &[SidebarAgentSnapshot]) -> bool {
    let by_pane = agents
        .iter()
        .map(|agent| {
            (
                agent.pane_id.as_str(),
                (
                    agent.status_label.as_str(),
                    agent.requires_close_confirmation,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut changed = false;
    for pane in workspaces
        .iter_mut()
        .flat_map(|workspace| workspace.checkouts.iter_mut())
        .flat_map(|checkout| checkout.tabs.iter_mut())
        .flat_map(|tab| tab.panes.iter_mut())
    {
        let Some((status_label, requires_close_confirmation)) =
            by_pane.get(pane.id.as_str()).copied()
        else {
            continue;
        };
        if pane.status_label != status_label {
            pane.status_label = status_label.to_owned();
            changed = true;
        }
        if pane.requires_close_confirmation != requires_close_confirmation {
            pane.requires_close_confirmation = requires_close_confirmation;
            changed = true;
        }
    }
    changed |= crate::sidebar::sync_checkout_agent_summaries(workspaces, agents);
    changed |= crate::project_context::sort_projects(workspaces, agents);
    changed
}

/// Drops the text scale of a pane the server no longer reports, on the same
/// pass that drops its read record, so neither map grows forever. Returns
/// whether anything was dropped.
///
/// It runs on every pass that carries a fresh agent list rather than only when
/// a read record moved: an operator who has looked at nothing moves no record,
/// and gating on that left dead panes' zoom in the store forever.
///
/// A pass only drops keys in the namespace it owns, for the same reason read
/// record eviction does: a local sync holds no remote pane list, so unscoped it
/// deleted every remote pane's zoom the moment any local agent changed state.
fn prune_pane_text_scales(
    scales: &mut BTreeMap<String, f32>,
    workspaces: &[WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
    scope: ReadRecordScope<'_>,
) -> bool {
    let live = workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .flat_map(|checkout| checkout.tabs.iter())
        .flat_map(|tab| tab.panes.iter())
        .map(|pane| pane.id.as_str())
        .chain(agents.iter().map(|agent| agent.pane_id.as_str()))
        .collect::<HashSet<_>>();
    let before = scales.len();
    scales.retain(|pane_id, _| !scope.owns(pane_id) || live.contains(pane_id.as_str()));
    scales.len() != before
}

fn remote_pane_source_id<'a>(target_id: &str, projected_id: &'a str) -> Option<&'a str> {
    projected_id
        .strip_prefix(&remote_pane_id_prefix(target_id))
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

/// One clicked path, already resolved on the filesystem by the shell.
///
/// The shell owns filesystem resolution because reading directory entries
/// under the runtime mutex is exactly what the performance guide forbids. The
/// core owns every screen effect the click has, and it owns them together:
/// the checkout switch, the panel, the tree and the editor tab land in one
/// event or the dispatch's fire-and-forget ordering would let the operator
/// see a half-applied reveal.
#[derive(Debug, Deserialize)]
struct RevealPathPayload {
    path: String,
    workspace_id: String,
    checkout_id: String,
    is_directory: bool,
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
struct FileViewPayload {
    tab_id: String,
    markdown_preview: bool,
    wrap: bool,
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
    #[serde(default)]
    collapsed_checkout_ids: Option<Vec<String>>,
    #[serde(default)]
    collapsed_agent_pane_ids: Option<Vec<String>>,
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
    /// The composer's remembered agent and bypass choice, and whether the
    /// Scratch section is open. Each is `None` on a save that did not touch
    /// it, so a navigator save cannot reset the composer and a composer
    /// submission cannot close the Scratch section.
    #[serde(default)]
    last_agent_kind: Option<String>,
    #[serde(default)]
    last_agent_bypass: Option<bool>,
    #[serde(default)]
    scratch_expanded: Option<bool>,
}

/// Which changed file the changes view is showing the diff for. `None`
/// deselects, which is what closing the diff means.
#[derive(Debug, Deserialize)]
struct ChangesSelectPayload {
    /// Whether the row is in the committed group, which decides what its diff
    /// is taken against.
    #[serde(default)]
    committed: bool,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CleanupConfirmPayload {
    id: u64,
    paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GitWorktreeOpenPayload {
    checkout_path: String,
}

#[derive(Debug, Deserialize)]
struct GitWorktreeSetBasePayload {
    repository_root: String,
    branch: String,
}

#[derive(Debug, Deserialize)]
struct CreateScratchChatTabPayload {
    label: String,
}

#[derive(Debug, Deserialize)]
struct CreateWorktreePayload {
    repository_root: String,
    branch: String,
    #[serde(default)]
    base_branch: Option<String>,
    #[serde(default)]
    agent_kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MigrateMainBranchPayload {
    repository_root: String,
    base_branch: String,
}

#[derive(Debug, Deserialize)]
struct TaskOperationAckPayload {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct RemoveWorktreePayload {
    checkout_path: String,
    #[serde(default)]
    delete_branch: bool,
}

#[derive(Debug, Deserialize)]
struct WorktreeRemovalFinishedPayload {
    id: u64,
    removed: bool,
    message: String,
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

/// The editor is one surface, not one per document, so its zoom carries a
/// direction and nothing to key it by.
#[derive(Debug, Deserialize)]
struct EditorTextScalePayload {
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
    #[serde(default)]
    column: Option<u16>,
    #[serde(default)]
    row: Option<u16>,
    #[serde(default)]
    modifiers: u8,
}

#[derive(Debug, Deserialize)]
struct TerminalClickPayload {
    pane_id: String,
    column: u16,
    row: u16,
    modifiers: u8,
}

#[derive(Debug, Deserialize)]
struct TerminalResizePayload {
    pane_id: String,
    cols: u16,
    rows: u16,
    #[serde(default)]
    new_view: bool,
}

enum ValidatedEvent {
    Key(KeyPayload),
    TerminalOutput(TerminalOutputPayload),
    SessionSnapshot(SessionSnapshotPayload),
    Click(ClickPayload),
    FocusPane(FocusPaneRequestPayload),
    OpenBrowser(OpenBrowserPayload),
    BrowserStatus(BrowserStatusPayload),
    CreateWorkspace(CreateWorkspacePayload),
    CreateTab(CreateTabPayload),
    FocusCheckout(FocusCheckoutPayload),
    FocusTab(FocusTabPayload),
    ReorderTab(ReorderTabPayload),
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
    AgentTreeToggle(PaneTargetPayload),
    RemoteControl(RemoteControlPayload),
    RemoteFileList(RemoteFileListPayload),
    FileOpen(FileOpenPayload),
    RevealPath(RevealPathPayload),
    FileFocus(FileTabPayload),
    FileClose(FileTabPayload),
    FileDraft(FileDraftPayload),
    FileView(FileViewPayload),
    FileSave(FileSavePayload),
    FileConflict(FileConflictPayload),
    UiStateUpdate(UiStateUpdatePayload),
    RetryConnect(RetryConnectPayload),
    TerminalResize(TerminalResizePayload),
    TerminalViewport(TerminalResizePayload),
    TerminalScroll(TerminalScrollPayload),
    TerminalClick(TerminalClickPayload),
    PaneFind(PaneFindPayload),
    PaneTextScale(PaneTextScalePayload),
    EditorTextScale(EditorTextScalePayload),
    ChangesSelect(ChangesSelectPayload),
    GitWorktreeOpen(GitWorktreeOpenPayload),
    GitWorktreeSetBase(GitWorktreeSetBasePayload),
    CreateScratchChatTab(CreateScratchChatTabPayload),
    CreateWorktree(CreateWorktreePayload),
    MigrateMainBranch(MigrateMainBranchPayload),
    TaskOperationAck(TaskOperationAckPayload),
    RemoveWorktree(RemoveWorktreePayload),
    WorktreeRemovalFinished(WorktreeRemovalFinishedPayload),
    GithubRequest(GithubRequestPayload),
    /// The card's refresh button, opening the delete confirmation, and a
    /// completed worktree removal. All three say "read again now" about a
    /// different set of readers, and none needs a target: the card is always
    /// the selected checkout, and a removal changes the whole worktree list.
    OverviewSelect(GitWorktreeOpenPayload),
    OverviewChanges(GitWorktreeOpenPayload),
    CleanupReview,
    CleanupConfirm(CleanupConfirmPayload),
    CleanupDismiss,
    CardRefresh,
    CardMeasureDisk,
    ReconnectPane(FocusPanePayload),
    PetSetVisible(PetVisibilityPayload),
    PetToggleVisible,
    PetMove(PetMovePayload),
    PetDrag(PetDragPayload),
    PetActivity,
    PetShortcutUpdate(PetShortcutPayload),
}

/// A file tab that has been read but not yet put on screen.
enum PreparedFileTab {
    /// The file already has a tab; showing it is a focus.
    Open(String),
    /// The file was read and needs a tab of its own.
    Read {
        tab_id: String,
        document: EditorDocumentSnapshot,
    },
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
            "starting" | "controlling" | "observing" | "unavailable" | "ended" | "closing"
        )
}

pub struct Runtime {
    snapshot: Snapshot,
    state_path: PathBuf,
    state_save_pending: bool,
    state_save_active: bool,
    state_save_worker: Option<thread::JoinHandle<()>>,
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
    terminal_recovery: HashMap<String, crate::terminal_recovery::Recovery>,
    next_terminal_session_generation: u64,
    next_remote_file_generation: u64,
    terminal_sizes: HashMap<String, (u16, u16)>,
    terminal_view_sizes: HashMap<String, (u16, u16)>,
    terminal_frames_need_full: HashSet<String>,
    /// Panes whose attach is held until a view reports their size. Herdr
    /// sizes the PTY from the attach, so starting one at a guess costs a
    /// full frame at the wrong size and a second one after the resize.
    panes_awaiting_size: HashSet<String>,
    panes_scrolled_before_size: HashSet<String>,
    /// The tabs that have been on screen, most recent first. An attach lives
    /// for as long as its tab is in this window; every other pane's session is
    /// released. Herdr renders a pane for every attached client, so an attach
    /// nobody is looking at costs a child process here and a render there for
    /// the life of the process.
    recent_visible_tabs: Vec<String>,
    /// Panes Hide has asked Herdr to close. Herdr closes the PTY first, so the
    /// attach child ends before the `pane_closed` event arrives and the pane
    /// is still on screen when its transport reports the close. Projecting
    /// that as a failure is what put "terminal attach ended" on screen for one
    /// frame every time the operator closed a pane.
    panes_closing: HashSet<String>,
    /// Panes that were scrolled before any view reported their size. One
    /// diagnostic answers for the whole wait; a wheel burst against a pane
    /// with no size would otherwise fill the bounded diagnostics list with the
    /// same sentence and push out everything else that happened.
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
    /// The pane the operator chose to look at, and the only pane whose read
    /// record may be raised.
    ///
    /// Herdr's focus is not the operator's attention. A tab carries the pane
    /// it last had focused, so bringing a tab forward makes Herdr report a
    /// focus nobody asked for; a spawned pane takes focus on its own; and a
    /// relaunch inherits whatever focus the arriving session snapshot names.
    /// Counting any of those as a look cleared rows the operator never saw
    /// (one click on a Done row cleared two rows, and a restart cleared the
    /// last unread one). Only a focus Hide itself dispatched on the
    /// operator's behalf arms this, and it is memory only: a launch starts
    /// with the operator having looked at nothing.
    operator_focused_pane_id: Option<String>,
    /// The tab Hide is showing in each checkout it has been asked about.
    ///
    /// Hide owns the visible tab. The navigator is rebuilt from Herdr's
    /// session on every update, so the choice has to live outside it or every
    /// tick would hand the decision back to Herdr.
    visible_tab_ids: BTreeMap<String, String>,
    /// The tab focus Hide has told Herdr about and is still waiting to see
    /// confirmed. Latest request wins; a second switch replaces the first
    /// rather than queueing behind it.
    pending_tab_focus: Option<PendingViewFocus>,
    /// The tab Herdr had focused at the last session update. A follow needs
    /// Herdr's focus to have moved; a focused tab that merely differs from
    /// Hide's, as it does after a notification Herdr never answered, is not
    /// an operator action and is not followed.
    herdr_focused_tab_seen: Option<String>,
    /// The pane focus Hide has told Herdr about and is still waiting to see
    /// confirmed.
    pending_pane_focus: Option<PendingViewFocus>,
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
    /// The catalog and root index most recently accepted from the sync
    /// coordinator, reused when a later precomputation arrives stale so the
    /// reconcile never rebuilds under the runtime lock.
    last_accepted_catalog: Option<Vec<WorkspaceSnapshot>>,
    catalog_roots: workspace::RootIndex,
    /// Where Scratch lives, resolved once when the core is created.
    ///
    /// Held rather than recomputed because every pane in every reconcile is
    /// compared against it, and because one answer per process is what keeps
    /// the projection, the snapshot and the shell's launcher from disagreeing
    /// about which folder is Scratch.
    scratch_root: String,
    /// The strip order each local checkout has, as strip entry ids. It is
    /// memory only by decision: Herdr persists its own tab order and file tabs
    /// do not survive a restart, so there is nothing here worth writing to
    /// disk. Without it a Herdr tab created next to open file tabs would land
    /// in front of them instead of at the end of the strip.
    checkout_tab_order: BTreeMap<String, Vec<String>>,
    /// Every Herdr workspace's whole tab list, in Herdr's order, keyed by the
    /// Herdr workspace id. A checkout holds only the tabs whose panes sit in
    /// its own directory, so one workspace's tabs can be split across a
    /// repository and its worktrees. `tab.move` counts its insertion index in
    /// the workspace's list, not in a checkout's part of it, so the index has
    /// to be read off this list or the tab lands somewhere else.
    herdr_workspace_tab_order: BTreeMap<String, Vec<String>>,
    /// The arrangement a reorder asked for and Herdr has not reported yet,
    /// per checkout. Herdr owns where its own tabs sit, so a drag that moves
    /// one of them is held here rather than written into the strip: an
    /// arrangement written early would show the file slots moved and the
    /// Herdr slots not, and a refusal would have nothing to revert to.
    pending_tab_move: BTreeMap<String, PendingTabMove>,
    /// Numbers each reorder request so a result that a later drag has already
    /// superseded cannot cancel the newer one.
    next_tab_move_generation: u64,
    /// Workspace/active-tab pairs Herdr named that the navigator could not
    /// place, as `<workspace id>/<tab id>`. Session sync reconciles once a
    /// second, so the diagnostic is emitted when the set changes rather than
    /// on every tick.
    unresolved_active_tabs: BTreeSet<String>,
    /// Panes whose fork has been started and not yet answered. `herdr agent
    /// new` blocks until the agent has started, so without this a second
    /// activation during that wait would bill a second session.
    forks_in_flight: HashSet<String>,
    fork_sequence: u64,
    /// The machine's TCP listeners, refreshed on their own window by the
    /// session-sync coordinator. Held here rather than in the snapshot because
    /// what the shell renders is the per-pane attribution, not the raw list.
    listening_ports: crate::model::ListeningPortsSnapshot,
    /// Every open repository's worktrees, refreshed by repository and Herdr
    /// topology changes on the session-sync coordinator. Held here rather than
    /// in the snapshot because the shell renders the checkout rows these
    /// produce, not the raw list.
    worktree_catalog: crate::model::WorktreeCatalogSnapshot,
    /// Every open repository's pull requests, from the operator's own `gh`.
    github: crate::model::GithubSnapshot,
    /// Measurements for the focused project's worktrees. The reader updates
    /// this only while Git is visible or after an explicit refresh.
    disk_usage: Vec<crate::model::DiskUsageSnapshot>,
    /// One counter per local git project, keyed by its navigator path. Opening
    /// the Git section or explicitly refreshing GitHub advances the target
    /// project counter; equal generations reuse the cached answer indefinitely.
    github_generations: HashMap<String, u64>,
    sidebar_github_projects: HashSet<String>,
    /// Bumped when visible Git rows must be measured again: section opening,
    /// explicit refresh, and opening the delete confirmation.
    disk_generation: u64,
    overview_selection: Option<String>,
    cleanup: Option<live::cleanup::CleanupSnapshot>,
    next_cleanup_id: u64,
    /// Bumped when the worktree list itself is known to have changed through a
    /// manual refresh, an in-app removal, or an observed Herdr worktree event.
    worktree_generation: u64,
    /// Identifies one delete handshake across core, Herdr and the shell.
    /// A repeated callback for an older request cannot authorize a newer one.
    next_worktree_removal_id: u64,
    next_task_operation_id: u64,
    delta: DeltaState,
}

#[derive(Clone)]
struct RuntimeWorkerContext {
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

/// Turns a taken delta into the bytes the shell reads.
///
/// It takes the payload and nothing else. That signature is the guarantee the
/// runtime mutex is not held here: there is no runtime in scope to lock. The
/// caller takes a payload under the lock, drops the guard, and calls this.
pub fn serialize_snapshot_delta(
    payload: &crate::model::SnapshotDeltaPayload,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&crate::model::SnapshotDeltaWire::borrow(payload))
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
    /// Reference-counted so a delta can carry the section out of the lock
    /// without copying it. The runtime never mutates one in place: a changed
    /// section becomes a new `Arc`, which leaves any payload already handed
    /// out holding the state it was taken at.
    last_rest: Option<Arc<crate::model::RestSections>>,
    last_editor: Option<Arc<crate::model::EditorSnapshot>>,
    last_changes: Option<Arc<crate::model::ChangesSnapshot>>,
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
        let (ui_state, pane_terminal_sizes, disposition) = persistence::load(&state_path);
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
        snapshot.navigator.workspaces = workspace::build_catalog(
            &snapshot.ui_state.workspace_registrations,
            &[],
            &crate::model::WorktreeCatalogSnapshot::default(),
        );
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
            crate::diagnostic!(serde_json::json!({
                "component": "ui_state",
                "kind": kind,
                "message": message,
                "fallback": "defaults"
            }));
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
            terminal_recovery: HashMap::new(),
            next_terminal_session_generation: 0,
            next_remote_file_generation: 0,
            terminal_view_sizes: HashMap::new(),
            terminal_frames_need_full: HashSet::new(),
            terminal_sizes: pane_terminal_sizes.into_iter().collect(),
            panes_awaiting_size: HashSet::new(),
            panes_scrolled_before_size: HashSet::new(),
            recent_visible_tabs: Vec::new(),
            panes_closing: HashSet::new(),
            #[cfg(test)]
            suppress_terminal_session_workers: false,
            workspace_creations_in_flight: HashSet::new(),
            editor_documents: HashMap::new(),
            editor_tab_history: Vec::new(),
            worker_context: None,
            state_save_pending: false,
            state_save_active: false,
            state_save_worker: None,
            pet_active_at_unix_ms: unix_milliseconds(),
            pet_waking_until_unix_ms: 0,
            pet_dragging: false,
            operator_focused_pane_id: None,
            visible_tab_ids: BTreeMap::new(),
            pending_tab_focus: None,
            herdr_focused_tab_seen: None,
            pending_pane_focus: None,
            pet_unseen_observed: std::collections::BTreeMap::new(),
            restore_hint_pending: true,
            last_session_spaces: Vec::new(),
            last_accepted_catalog: None,
            catalog_roots: workspace::RootIndex::new(),
            scratch_root: crate::scratch::root().to_string_lossy().into_owned(),
            checkout_tab_order: BTreeMap::new(),
            herdr_workspace_tab_order: BTreeMap::new(),
            pending_tab_move: BTreeMap::new(),
            next_tab_move_generation: 0,
            unresolved_active_tabs: BTreeSet::new(),
            forks_in_flight: HashSet::new(),
            fork_sequence: 0,
            listening_ports: crate::model::ListeningPortsSnapshot::default(),
            worktree_catalog: crate::model::WorktreeCatalogSnapshot::default(),
            github: crate::model::GithubSnapshot::default(),
            disk_usage: Vec::new(),
            github_generations: HashMap::new(),
            sidebar_github_projects: HashSet::new(),
            disk_generation: 0,
            overview_selection: None,
            cleanup: None,
            next_cleanup_id: 0,
            worktree_generation: 0,
            next_worktree_removal_id: 0,
            next_task_operation_id: 0,
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

    pub(crate) fn take_state_save_worker(&mut self) -> Option<thread::JoinHandle<()>> {
        self.state_save_worker.take()
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    fn file_tab_id(workspace_id: &str, checkout_id: &str, path: &str) -> String {
        format!("file:{workspace_id}:{checkout_id}:{path}")
    }

    fn activate_editor_tab(&mut self, tab_id: &str) -> Result<(), String> {
        let tab = self
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .cloned()
            .ok_or_else(|| format!("Editor tab {tab_id} is not open"))?;
        let document = match tab.kind {
            EditorTabKind::File => Some(
                self.editor_documents
                    .get(tab_id)
                    .cloned()
                    .ok_or_else(|| format!("File tab {tab_id} has no document state"))?,
            ),
            EditorTabKind::Diff => {
                if tab.diff_committed.is_none() {
                    return Err(format!("Diff tab {tab_id} has no comparison scope"));
                }
                None
            }
        };
        if let Some(active_id) = self.snapshot.editor.active_tab_id.as_deref()
            && active_id != tab_id
        {
            self.editor_tab_history.retain(|known| known != active_id);
            self.editor_tab_history.push(active_id.to_owned());
        }
        self.snapshot.editor.active_tab_id = Some(tab_id.to_owned());
        match tab.kind {
            EditorTabKind::File => {
                self.snapshot.editor.document = document;
                self.snapshot.ui_state.selected_path = Some(tab.path);
            }
            EditorTabKind::Diff => {
                let committed = tab.diff_committed.expect("validated above");
                if self.snapshot.changes.selected_path.as_deref() != Some(tab.path.as_str())
                    || self.snapshot.changes.selected_committed != committed
                {
                    self.snapshot.changes.diff = None;
                }
                self.snapshot.changes.selected_path = Some(tab.path);
                self.snapshot.changes.selected_committed = committed;
                self.snapshot.editor.document = None;
                self.snapshot.ui_state.selected_path = None;
            }
        }
        Ok(())
    }

    fn deactivate_editor_tab(&mut self) {
        self.snapshot.editor.active_tab_id = None;
        self.snapshot.editor.document = None;
        self.editor_tab_history.clear();
    }

    fn sync_active_editor_document(&mut self) {
        self.snapshot.editor.document =
            self.snapshot
                .editor
                .active_tab_id
                .as_deref()
                .and_then(|tab_id| {
                    self.editor_documents.get(tab_id).and_then(|document| {
                        self.snapshot
                            .editor
                            .tabs
                            .iter()
                            .find(|tab| tab.id == tab_id && tab.kind == EditorTabKind::File)
                            .map(|_| document.clone())
                    })
                });
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

    /// Takes one delta response for the snapshot wire: sections whose revision
    /// passed `have_revision`, plus terminal chunks past `have_sequence`.
    /// Reading is idempotent - the same cursors return the same delta again -
    /// so a caller that failed to apply a response recovers by re-reading with
    /// its unadvanced cursors.
    ///
    /// This is the half that needs the runtime, and it is deliberately the
    /// only half: it stamps revisions and copies out what the wire needs, and
    /// `serialize_snapshot_delta` turns that into bytes with the lock already
    /// released. Serializing here would put the whole navigator, ui state and
    /// terminal output through `serde_json` while every attach thread and the
    /// shell's next read wait on the mutex.
    pub fn snapshot_delta_payload(
        &mut self,
        have_revision: u64,
        have_sequence: u64,
    ) -> crate::model::SnapshotDeltaPayload {
        use crate::model::{RestSections, SnapshotDeltaPayload};

        if !self
            .delta
            .last_rest
            .as_ref()
            .is_some_and(|rest| rest.matches(&self.snapshot))
        {
            self.delta.revision += 1;
            self.delta.rest_revision = self.delta.revision;
            self.delta.last_rest = Some(Arc::new(RestSections::capture(&self.snapshot)));
        }
        if self.delta.last_editor.as_deref() != Some(&self.snapshot.editor) {
            self.delta.revision += 1;
            self.delta.editor_revision = self.delta.revision;
            self.delta.last_editor = Some(Arc::new(self.snapshot.editor.clone()));
        }
        if self.delta.last_changes.as_deref() != Some(&self.snapshot.changes) {
            self.delta.revision += 1;
            self.delta.changes_revision = self.delta.revision;
            self.delta.last_changes = Some(Arc::new(self.snapshot.changes.clone()));
        }
        // A cursor from the future has no valid meaning in-process; treat it
        // as a fresh reader so the response converges on full state.
        let have_revision = if have_revision > self.delta.revision {
            0
        } else {
            have_revision
        };

        // Chunks are cloned rather than drained: a caller whose apply failed
        // re-reads with the same cursor and has to get the same bytes back.
        let chunks: Vec<_> = self
            .snapshot
            .terminal
            .chunks
            .iter()
            .filter(|chunk| chunk.sequence > have_sequence)
            .cloned()
            .collect();
        let chunks_dropped = match self.snapshot.terminal.chunks.first() {
            Some(oldest) => have_sequence + 1 < oldest.sequence,
            None => have_sequence < self.snapshot.terminal.sequence,
        };

        SnapshotDeltaPayload {
            schema_version: self.snapshot.schema_version,
            revision: self.delta.revision,
            // The retained copies were compared against the live snapshot
            // above and rebuilt where they differed, so each is the live
            // section and costs a refcount instead of a copy. All three are
            // stamped by that block, so a missing one is a broken invariant
            // and not a section to send as null.
            rest: (self.delta.rest_revision > have_revision).then(|| {
                Arc::clone(
                    self.delta
                        .last_rest
                        .as_ref()
                        .expect("the rest section is stamped before a delta is taken"),
                )
            }),
            editor: (self.delta.editor_revision > have_revision).then(|| {
                Arc::clone(
                    self.delta
                        .last_editor
                        .as_ref()
                        .expect("the editor section is stamped before a delta is taken"),
                )
            }),
            changes: (self.delta.changes_revision > have_revision).then(|| {
                Arc::clone(
                    self.delta
                        .last_changes
                        .as_ref()
                        .expect("the changes section is stamped before a delta is taken"),
                )
            }),
            find: self.snapshot.find.clone(),
            input_generation: self.snapshot.input_generation,
            terminal_sequence: self.snapshot.terminal.sequence,
            chunks,
            chunks_dropped,
        }
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
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_files",
                    "kind": "remote.files_failed",
                    "target": target_id,
                    "root_path": root_path,
                    "generation": generation,
                    "message": message,
                }));
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
                    && session
                        .agents
                        .iter()
                        .any(|agent| agent.pane_id == pane_id && agent.requires_close_confirmation);
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
                    pane_ids.contains(agent.pane_id.as_str()) && agent.requires_close_confirmation
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
    /// Where Scratch lives, for the sync coordinator that builds the catalog
    /// before it takes this lock.
    pub fn scratch_root(&self) -> String {
        self.scratch_root.clone()
    }

    /// The Herdr workspaces holding at least one Scratch pane, in Herdr's own
    /// order.
    ///
    /// Two things need this. A tab whose panes have not reported a directory
    /// yet is placed by its workspace, and a new Scratch tab needs a live
    /// workspace to be created in - the first entry, or none, which is what
    /// makes the first submission create the workspace instead.
    fn scratch_workspace_ids(&self, payload: &SessionSnapshotPayload) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for layout in &payload.layouts {
            if ids.iter().any(|id| id == &layout.workspace_id) {
                continue;
            }
            let holds_scratch = layout.panes.iter().any(|pane| {
                Self::pane_cwd(payload, &pane.pane_id)
                    .is_some_and(|cwd| crate::scratch::contains(&self.scratch_root, &cwd))
            });
            if holds_scratch {
                ids.push(layout.workspace_id.clone());
            }
        }
        ids
    }

    /// The Herdr workspaces and the directories their panes occupy, as the
    /// project catalog sees them.
    ///
    /// A directory inside Scratch is left out here rather than filtered later.
    /// This list is what builds the catalog and resolves repository roots, so
    /// a scratch directory that reached it would earn a project row, a git
    /// fork, and the unregistered-folder fallback that draws an orange
    /// temporary workspace - three leaks from one omission.
    pub fn session_spaces(
        payload: &SessionSnapshotPayload,
        scratch_root: &str,
    ) -> Vec<workspace::SessionSpace> {
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
                if crate::scratch::contains(scratch_root, &cwd) {
                    continue;
                }
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
        self.last_session_spaces = Self::session_spaces(payload, &self.scratch_root);
        // The catalog and the root index shell out to git, so the sync
        // coordinator builds them before taking the runtime lock. A
        // precomputation whose registrations no longer match current state is
        // stale; the last accepted catalog stands in for it and the next
        // publish, a second away, brings a fresh one. It is not rebuilt here:
        // one `git rev-parse` per tab under this lock stalled the main thread
        // and every attach reader for a third of their time (2026-09-06,
        // 18 agents, load 7 to 11). Only a runtime that has never accepted a
        // catalog builds one inline, which is the fixture and test path.
        let (mut workspaces, roots) = match precomputed {
            Some(catalog)
                if catalog.registrations == self.snapshot.ui_state.workspace_registrations =>
            {
                self.last_accepted_catalog = Some(catalog.workspaces.clone());
                self.catalog_roots = catalog.roots.clone();
                (catalog.workspaces, catalog.roots)
            }
            Some(_) if self.last_accepted_catalog.is_some() => {
                self.push_diagnostic(
                    "catalog.precomputed_stale",
                    "The precomputed workspace catalog no longer matches the registrations; the last accepted catalog stands until the next publish".to_owned(),
                );
                (
                    self.last_accepted_catalog
                        .clone()
                        .expect("checked by the match guard"),
                    self.catalog_roots.clone(),
                )
            }
            _ => {
                let workspaces = workspace::build_catalog(
                    &self.snapshot.ui_state.workspace_registrations,
                    &self.last_session_spaces,
                    &self.worktree_catalog,
                );
                let roots = workspace::root_index(&self.last_session_spaces);
                self.last_accepted_catalog = Some(workspaces.clone());
                self.catalog_roots = roots.clone();
                (workspaces, roots)
            }
        };
        let mut unresolved_roots: Vec<String> = Vec::new();
        Self::apply_workspace_expansion(
            &mut workspaces,
            &self.snapshot.ui_state.collapsed_workspace_ids,
        );
        let projected_agents = project_agents(payload.clone()).agents;
        let listening_ports = self.listening_ports.entries.clone();

        // Every workspace's whole tab list, in Herdr's order, before any of it
        // is split across checkouts. A tab whose layout has not arrived yet is
        // in it, because Herdr counts it when it indexes a move. Rebuilt whole
        // each reconcile so a closed workspace leaves no stale order behind.
        self.herdr_workspace_tab_order = payload.tabs.iter().fold(
            BTreeMap::<String, Vec<String>>::new(),
            |mut order, session_tab| {
                order
                    .entry(session_tab.workspace_id.clone())
                    .or_default()
                    .push(session_tab.tab_id.clone());
                order
            },
        );

        // Herdr's tab order is the navigator's tab order. A layout is the
        // per-tab detail looked up by tab id, never what decides where a tab
        // sits: layouts arrive in the order each tab was first drawn, so a tab
        // Herdr moved kept its original place forever and a tab that redrew
        // never moved back.
        let mut placed_tabs: BTreeMap<&str, usize> = BTreeMap::new();
        // Herdr's own labels, kept per checkout while they are still raw. The
        // snapshot's tabs carry the formatted form, so the free number has to
        // be taken here or read back out of display text later.
        let mut raw_tab_labels: BTreeMap<String, Vec<String>> = BTreeMap::new();
        // Which Herdr workspaces hold Scratch panes. A tab whose panes report
        // no directory at all still belongs to Scratch when its workspace
        // does, which is the state a pane is in for the moment between Herdr
        // creating it and reporting where it runs.
        let scratch_workspace_ids = self.scratch_workspace_ids(payload);
        let mut scratch_tabs: Vec<crate::model::ScratchTabSnapshot> = Vec::new();
        for session_tab in &payload.tabs {
            let Some(layout) = payload
                .layouts
                .iter()
                .find(|layout| layout.tab_id == session_tab.tab_id)
            else {
                continue;
            };
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
            // Scratch is decided before a project is looked for, so a
            // scratch pane never reaches the placement that would give it a
            // project row or an unregistered-folder fallback.
            let in_scratch = match context_path.as_deref() {
                Some(path) => crate::scratch::contains(&self.scratch_root, path),
                None => scratch_workspace_ids
                    .iter()
                    .any(|id| id == &layout.workspace_id),
            };
            if in_scratch {
                let panes = project_layout_panes(
                    layout,
                    payload,
                    &projected_agents,
                    &listening_ports,
                    &self.scratch_root,
                );
                let title = panes.iter().find_map(|pane| {
                    projected_agents
                        .iter()
                        .find(|agent| agent.pane_id == pane.id)
                        .and_then(|agent| agent.chat_title.clone())
                });
                scratch_tabs.push(crate::model::ScratchTabSnapshot {
                    id: session_tab.tab_id.clone(),
                    label: crate::model::display_tab_label(&session_tab.label, &session_tab.tab_id),
                    title,
                    panes,
                });
                continue;
            }
            let Some(workspace_snapshot) = find_workspace_for_context(
                &mut workspaces,
                context_path.as_deref(),
                &layout.workspace_id,
                &roots,
                &mut unresolved_roots,
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
            let panes = project_layout_panes(
                layout,
                payload,
                &projected_agents,
                &listening_ports,
                &checkout.path,
            );
            let tab = TabSnapshot {
                id: Some(session_tab.tab_id.clone()),
                workspace_id: Some(workspace_snapshot.id.clone()),
                checkout_id: Some(checkout.id.clone()),
                label: Some(crate::model::display_tab_label(
                    &session_tab.label,
                    &session_tab.tab_id,
                )),
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
            *placed_tabs.entry(layout.workspace_id.as_str()).or_default() += 1;
            raw_tab_labels
                .entry(checkout.id.clone())
                .or_default()
                .push(session_tab.label.clone());
        }

        // A worktree earns its row from git, not from a pane, so which rows
        // have a terminal is only known once the tabs are attached.
        for workspace in &mut workspaces {
            for checkout in &mut workspace.checkouts {
                checkout.has_panes = checkout.tabs.iter().any(|tab| !tab.panes.is_empty());
            }
        }

        for workspace in &mut workspaces {
            for checkout in &mut workspace.checkouts {
                checkout.next_tab_label = crate::model::next_tab_label(
                    raw_tab_labels
                        .get(&checkout.id)
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .map(String::as_str),
                );
            }
        }

        // Herdr names one active tab per workspace. Hide owns which tab is
        // visible, so that name is what Hide reconciles against rather than
        // what it obeys: it confirms a switch Hide made, or it is an operator
        // focusing a tab outside Hide and Hide follows it and says so.
        // A checkout is keyed by path, so two Herdr workspaces at one path
        // land in one checkout with one active tab each. The view is kept
        // per Herdr workspace for that reason: folding it to one tab per
        // checkout let the last workspace in payload order overwrite the
        // others, and a tab focus on any other workspace was then never
        // confirmed and always followed back.
        if !unresolved_roots.is_empty() {
            unresolved_roots.sort();
            unresolved_roots.dedup();
            self.push_diagnostic(
                "catalog.root_unresolved",
                format!(
                    "{} pane director{} placed by path alone because the root index did not carry {}: {}",
                    unresolved_roots.len(),
                    if unresolved_roots.len() == 1 { "y was" } else { "ies were" },
                    if unresolved_roots.len() == 1 { "it" } else { "them" },
                    unresolved_roots.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
                ),
            );
        }
        let mut unresolved_active_tabs = BTreeSet::new();
        let herdr_tabs = HerdrTabView::from_payload(payload);
        for session_workspace in &payload.workspaces {
            let Some(active_tab_id) = session_workspace
                .active_tab_id
                .as_deref()
                .map(str::trim)
                .filter(|active_tab_id| !active_tab_id.is_empty())
            else {
                continue;
            };
            let resolved = workspaces.iter().any(|workspace| {
                workspace.checkouts.iter().any(|checkout| {
                    checkout
                        .tabs
                        .iter()
                        .any(|tab| tab.id.as_deref() == Some(active_tab_id))
                })
            });
            // A workspace none of whose tabs reached the navigator is not a
            // contradiction, only a workspace outside every registered
            // checkout. An active tab missing from a workspace that did place
            // tabs is the state worth reporting.
            if !resolved
                && placed_tabs
                    .get(session_workspace.workspace_id.as_str())
                    .is_some_and(|placed| *placed > 0)
            {
                unresolved_active_tabs.insert(format!(
                    "{}/{active_tab_id}",
                    session_workspace.workspace_id
                ));
            }
        }
        self.report_unresolved_active_tabs(unresolved_active_tabs);
        self.reconcile_visible_tabs(&mut workspaces, &herdr_tabs);

        let previous = self.snapshot.navigator.clone();
        let previous_card = self.snapshot.card.clone();
        crate::sidebar::sync_checkout_agent_summaries(
            &mut workspaces,
            &self.snapshot.navigator.agents,
        );
        crate::project_context::sort_projects(&mut workspaces, &projected_agents);
        self.snapshot.navigator.workspaces = workspaces;
        self.snapshot.navigator.scratch = crate::model::ScratchSnapshot {
            id: crate::scratch::NODE_ID.to_owned(),
            label: crate::scratch::LABEL.to_owned(),
            path: self.scratch_root.clone(),
            expanded: self.snapshot.ui_state.scratch_expanded,
            session_workspace_ids: scratch_workspace_ids,
            tabs: scratch_tabs,
        };
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
        self.rebuild_tab_strips();
        previous != self.snapshot.navigator || previous_card != self.snapshot.card
    }

    /// Puts one strip entry at a new place in its checkout's strip.
    ///
    /// The two kinds of entry have different owners. A file tab's slot is
    /// Hide's, so a move that only rearranges file slots is committed here and
    /// nothing is sent to Herdr. The relative order of Herdr's tabs is Herdr's,
    /// so a move that changes it is a request: the arrangement is held until
    /// Herdr reports the order it actually has, and a refusal leaves the strip
    /// on the order Herdr last reported.
    fn reorder_tab(&mut self, payload: ReorderTabPayload) -> bool {
        let Some(workspace) = self
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
        if workspace.remote_target_id.is_some() {
            self.set_error(
                "tab.reorder_remote",
                "A remote target's tab order is Herdr's alone and cannot be rearranged here",
                false,
            );
            return true;
        }
        let Some(checkout) = workspace
            .checkouts
            .iter()
            .find(|checkout| checkout.id == payload.checkout_id)
        else {
            self.set_error(
                "tab.unknown_checkout",
                format!("Checkout {} is not available", payload.checkout_id),
                false,
            );
            return true;
        };
        let strip = checkout.strip.clone();
        let Some(from) = strip.iter().position(|entry| entry.id == payload.tab_id) else {
            self.set_error(
                "tab.reorder_unknown",
                format!("Tab {} is not in this checkout's strip", payload.tab_id),
                false,
            );
            return true;
        };
        if payload.to_index >= strip.len() {
            self.set_error(
                "tab.reorder_out_of_range",
                format!(
                    "Position {} is past the end of a strip of {}",
                    payload.to_index,
                    strip.len()
                ),
                false,
            );
            return true;
        }

        let mut desired = strip.clone();
        let moved = desired.remove(from);
        desired.insert(payload.to_index, moved.clone());
        let desired_ids = desired
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        let herdr_ids = |entries: &[StripTabSnapshot]| {
            entries
                .iter()
                .filter(|entry| entry.kind == StripTabKind::Herdr)
                .map(|entry| entry.source_id.clone())
                .collect::<Vec<_>>()
        };
        let current_herdr = herdr_ids(&strip);
        let desired_herdr = herdr_ids(&desired);

        if current_herdr == desired_herdr {
            // Only slots Hide owns changed, so Herdr has nothing to do and the
            // arrangement is the operator's the moment they drop it.
            self.pending_tab_move.remove(&payload.checkout_id);
            self.checkout_tab_order
                .insert(payload.checkout_id.clone(), desired_ids);
            self.rebuild_tab_strips();
            return true;
        }

        // Herdr indexes a move inside the workspace that owns the tab, so the
        // drag - not the checkout - decides whether Herdr hears about it. What
        // matters is the moved tab's own workspace subsequence: a drag that
        // only steps over tabs belonging to another Herdr workspace changes
        // nothing Herdr can see, and the strip settles locally.
        //
        // Deciding this per checkout is what refused every drag in a checkout
        // whose tabs come from two Herdr workspaces, which is the ordinary
        // arrangement for a repository opened twice.
        let Some((moved_workspace_id, workspace_order)) = self
            .herdr_workspace_tab_order
            .iter()
            .find(|(_, order)| order.contains(&moved.source_id))
            .map(|(workspace_id, order)| (workspace_id.clone(), order.clone()))
        else {
            self.set_error(
                "tab.reorder_inconsistent",
                format!(
                    "Tab {} cannot be placed there: Herdr does not list it in any workspace",
                    moved.source_id
                ),
                false,
            );
            return true;
        };
        let owned = |entries: &[String]| {
            entries
                .iter()
                .filter(|tab_id| workspace_order.contains(tab_id))
                .cloned()
                .collect::<Vec<_>>()
        };
        let current_owned = owned(&current_herdr);
        let desired_owned = owned(&desired_herdr);
        if current_owned == desired_owned {
            // The moved tab kept its place among its own workspace's tabs, so
            // only slots Hide arranges changed and the drop is final now.
            self.pending_tab_move.remove(&payload.checkout_id);
            self.checkout_tab_order
                .insert(payload.checkout_id.clone(), desired_ids);
            self.rebuild_tab_strips();
            return true;
        }

        let Some(insert_index) =
            herdr_insert_index(&workspace_order, &desired_owned, &moved.source_id)
        else {
            self.set_error(
                "tab.reorder_inconsistent",
                format!(
                    "Tab {} cannot be placed there: it is not one of Herdr's tabs in this checkout",
                    moved.source_id
                ),
                false,
            );
            return true;
        };
        let Some(context) = self.live.as_ref().cloned() else {
            self.set_error(
                "tab.control_unavailable",
                "Moving a Herdr tab requires a live Herdr connection",
                true,
            );
            return true;
        };
        let generation = self.next_tab_move_generation;
        self.next_tab_move_generation += 1;
        self.pending_tab_move.insert(
            payload.checkout_id.clone(),
            PendingTabMove {
                desired: desired_ids,
                workspace_id: moved_workspace_id,
                // The order asked for is the moved tab's own workspace's, the
                // same subsequence the request was indexed in. Holding the
                // checkout's mixed order here would wait for an interleaving
                // Herdr never reports once a checkout draws tabs from two
                // workspaces, and the drag would snap back and stay back.
                herdr_order: desired_owned.clone(),
                generation,
            },
        );
        self.push_diagnostic(
            "tab.move.requested",
            format!(
                "Asking Herdr to insert tab {} at {insert_index} in its own workspace",
                moved.source_id
            ),
        );
        if let Err(message) = live::spawn_local_control(
            context,
            RemoteControlAction::MoveTab {
                checkout_id: payload.checkout_id.clone(),
                tab_id: moved.source_id,
                insert_index,
                // Herdr answers with its own workspace's tabs, so the order to
                // check the answer against is the moved tab's workspace
                // subsequence, never the checkout's mixed order.
                expected_order: desired_owned,
                generation,
            },
        ) {
            self.pending_tab_move.remove(&payload.checkout_id);
            self.set_error("tab.move_worker_failed", message, true);
        }
        true
    }

    /// Drops a held reorder and says why, so a refused move is never a strip
    /// that silently stayed where it was.
    fn abandon_tab_move(&mut self, checkout_id: &str, generation: u64, reason: String) -> bool {
        // A result from a drag a later drag has replaced must not cancel the
        // newer one.
        if self
            .pending_tab_move
            .get(checkout_id)
            .is_none_or(|pending| pending.generation != generation)
        {
            return false;
        }
        self.pending_tab_move.remove(checkout_id);
        self.set_error("tab.move_refused", reason, true);
        self.rebuild_tab_strips();
        true
    }

    /// Rewrites every local checkout's tab strip from the Herdr and editor
    /// tabs it currently holds.
    ///
    /// A remote checkout keeps the strip its own projection built: the remote
    /// context browses Herdr's tabs and has no file tabs to mix in.
    fn rebuild_tab_strips(&mut self) {
        let editor_tabs = &self.snapshot.editor.tabs;
        let order = &mut self.checkout_tab_order;
        let pending = &mut self.pending_tab_move;
        // Which Herdr workspace each tab belongs to, so a strip slot is
        // refilled from that workspace's order rather than from a flat one.
        let owners = &self
            .herdr_workspace_tab_order
            .iter()
            .flat_map(|(workspace_id, tab_ids)| {
                tab_ids
                    .iter()
                    .map(move |tab_id| (tab_id.clone(), workspace_id.clone()))
            })
            .collect::<BTreeMap<String, String>>();
        let mut live_checkouts = BTreeSet::new();
        // Checkouts whose held arrangement became unreachable. The diagnostic
        // is pushed after the loop, which is where the snapshot is free again.
        let mut dropped_moves = Vec::new();
        for workspace in &mut self.snapshot.navigator.workspaces {
            if workspace.remote_target_id.is_some() {
                continue;
            }
            for checkout in &mut workspace.checkouts {
                live_checkouts.insert(checkout.id.clone());
                let herdr = StripTabSnapshot::from_herdr_tabs(&checkout.tabs);
                let editor = editor_tabs
                    .iter()
                    .filter(|tab| {
                        tab.workspace_id == checkout.workspace_id && tab.checkout_id == checkout.id
                    })
                    .map(|tab| match tab.kind {
                        EditorTabKind::File => {
                            StripTabSnapshot::file(tab.id.clone(), tab.label.clone())
                        }
                        EditorTabKind::Diff => {
                            StripTabSnapshot::diff(tab.id.clone(), tab.label.clone())
                        }
                    })
                    .collect::<Vec<_>>();
                let stored = order.entry(checkout.id.clone()).or_default();
                // A held reorder lands the moment Herdr reports the order it
                // asked for, whichever path carried it: the `tab_moved` event,
                // a move Herdr had already made, or a move made from the TUI.
                // A held reorder whose tabs are no longer the checkout's tabs
                // can never be reported, so it is dropped rather than kept
                // waiting for an order that cannot arrive.
                if let Some(held) = pending.get(&checkout.id) {
                    // Only the tabs the move was asked about: the workspace
                    // it named, as this checkout currently holds them.
                    let live_owned = herdr
                        .iter()
                        .map(|entry| &entry.source_id)
                        .filter(|tab_id| owners.get(*tab_id) == Some(&held.workspace_id))
                        .cloned()
                        .collect::<Vec<_>>();
                    if live_owned.iter().cloned().collect::<BTreeSet<_>>()
                        != held.herdr_order.iter().cloned().collect::<BTreeSet<_>>()
                    {
                        pending.remove(&checkout.id);
                        dropped_moves.push(checkout.id.clone());
                    } else if live_owned == held.herdr_order {
                        *stored = held.desired.clone();
                        pending.remove(&checkout.id);
                    }
                }
                checkout.strip = ordered_strip(stored, &herdr, &editor, owners);
                *stored = checkout
                    .strip
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect();
            }
        }
        order.retain(|checkout_id, _| live_checkouts.contains(checkout_id));
        pending.retain(|checkout_id, _| live_checkouts.contains(checkout_id));
        for checkout_id in dropped_moves {
            self.push_diagnostic(
                "tab.move.dropped",
                format!(
                    "The tab arrangement held for {checkout_id} was abandoned: its tabs changed before Herdr reported the order"
                ),
            );
        }
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
        // Every catalog rebuild and every focus change lands here, so this is
        // the one place the pull-request badges and the summary card have to
        // be re-derived from. Doing it at each call site is how the row and
        // the card would come to disagree.
        self.apply_pull_requests();
        self.refresh_worktree_projection();
    }

    /// Decides which tab each checkout shows, given what Herdr says is active
    /// and what Hide has already chosen.
    ///
    /// Hide owns the visible tab, so Herdr's name is read four ways:
    /// it agrees with Hide and confirms a switch in flight; it disagrees while
    /// Hide's notification is still unconfirmed, and Hide keeps its own value
    /// until Herdr answers; it disagrees with nothing in flight, which is an
    /// operator focusing that tab outside Hide, so Hide follows it and reports
    /// the tab and where the change came from; or Herdr names no tab in this
    /// checkout, and Hide keeps showing what it was showing.
    ///
    /// A checkout that has tabs always ends with one of them visible. Leaving
    /// a sibling checkout of a split workspace without an active tab is what
    /// made its canvas draw the empty-checkout state over real panes.
    fn reconcile_visible_tabs(
        &mut self,
        workspaces: &mut [WorkspaceSnapshot],
        herdr: &HerdrTabView,
    ) {
        let mut followed: Vec<(String, String, String)> = Vec::new();
        let mut follow_pane: Option<String> = None;
        let mut confirmed_pending = false;
        // Herdr's focus moved since the last update. Only then is its focused
        // tab an action to follow; an unchanged focus that differs from
        // Hide's tab is the state a timed-out notification leaves behind, and
        // Hide keeps its value through that.
        let herdr_focus_moved = herdr.focused_tab_id != self.herdr_focused_tab_seen;
        self.herdr_focused_tab_seen = herdr.focused_tab_id.clone();
        let selected_pane_id = self.snapshot.terminal.pane_id.clone();
        // The catalog is rebuilt whole on every pass, so a checkout absent
        // from it is gone rather than momentarily missing. Keeping its tab
        // would grow this map for the life of the process.
        let live_checkout_ids = workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .map(|checkout| checkout.id.clone())
            .collect::<HashSet<_>>();
        self.visible_tab_ids
            .retain(|checkout_id, _| live_checkout_ids.contains(checkout_id));
        for workspace in workspaces.iter_mut() {
            for checkout in workspace.checkouts.iter_mut() {
                let has_tab = |tab_id: &str| {
                    checkout
                        .tabs
                        .iter()
                        .any(|tab| tab.id.as_deref() == Some(tab_id))
                };
                let hide_tab = self
                    .visible_tab_ids
                    .get(&checkout.id)
                    .filter(|tab_id| has_tab(tab_id))
                    .cloned();
                let herdr_tab = herdr
                    .focused_tab_id
                    .as_deref()
                    .filter(|tab_id| herdr_focus_moved && has_tab(tab_id))
                    .map(str::to_owned);
                let pending_tab = self
                    .pending_tab_focus
                    .as_ref()
                    .filter(|pending| pending.scope_id == checkout.id)
                    .map(|pending| pending.target_id.clone());
                // A tab focus is confirmed by the workspace that owns the
                // tab showing it, whichever workspace Herdr's keyboard is in.
                if pending_tab
                    .as_deref()
                    .is_some_and(|tab_id| herdr.is_active_in_its_workspace(tab_id))
                {
                    confirmed_pending = true;
                }
                // The tab holding the selected pane, when it is in this
                // checkout. With no tab of its own yet, Hide shows the tab the
                // keyboard is in rather than one Herdr remembers, so a restore
                // draws the layout it attaches.
                let selected_tab = selected_pane_id.as_deref().and_then(|pane_id| {
                    checkout
                        .tabs
                        .iter()
                        .find(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
                        .and_then(|tab| tab.id.clone())
                });
                // A pending tab Herdr has not listed yet (one just created)
                // keeps its claim on the checkout instead of being replaced
                // by whichever tab is drawn while it arrives.
                let pending_tab_unlisted = pending_tab
                    .as_deref()
                    .is_some_and(|tab_id| !has_tab(tab_id));
                let visible = match (hide_tab, herdr_tab) {
                    (Some(hide_tab), Some(herdr_tab)) if hide_tab == herdr_tab => Some(hide_tab),
                    (Some(hide_tab), Some(herdr_tab)) => {
                        if pending_tab.as_deref() == Some(hide_tab.as_str()) {
                            Some(hide_tab)
                        } else {
                            // Following the tab has to bring the keyboard with
                            // it. Leaving the projection on the tab that just
                            // stopped being visible parks the focus ring and
                            // the first responder on a pane nobody can see.
                            if self.snapshot.navigator.focused_checkout_id.as_deref()
                                == Some(checkout.id.as_str())
                            {
                                let first_pane_id = checkout
                                    .tabs
                                    .iter()
                                    .find(|tab| tab.id.as_deref() == Some(herdr_tab.as_str()))
                                    .and_then(|tab| tab.panes.first())
                                    .map(|pane| pane.id.clone());
                                follow_pane = self.tab_focus_pane_id(&herdr_tab, first_pane_id);
                            }
                            followed.push((checkout.id.clone(), hide_tab, herdr_tab.clone()));
                            Some(herdr_tab)
                        }
                    }
                    (Some(hide_tab), None) => Some(hide_tab),
                    // With no value of its own yet, Hide takes the first of:
                    // the tab it asked for, the tab holding the keyboard,
                    // the tab Herdr has focused, a tab Herdr shows in any of
                    // the checkout's workspaces, the first tab.
                    (None, herdr_tab) => pending_tab
                        .clone()
                        .filter(|tab_id| has_tab(tab_id))
                        .or(selected_tab)
                        .or(herdr_tab)
                        .or_else(|| {
                            checkout
                                .tabs
                                .iter()
                                .filter_map(|tab| tab.id.as_deref())
                                .find(|tab_id| herdr.is_active_in_its_workspace(tab_id))
                                .map(str::to_owned)
                        })
                        .or_else(|| checkout.tabs.first().and_then(|tab| tab.id.clone())),
                };
                match visible {
                    Some(tab_id) => {
                        if !pending_tab_unlisted {
                            self.visible_tab_ids
                                .insert(checkout.id.clone(), tab_id.clone());
                        }
                        checkout.active_tab_id = Some(tab_id);
                    }
                    None => {
                        if !pending_tab_unlisted {
                            self.visible_tab_ids.remove(&checkout.id);
                        }
                        checkout.active_tab_id = None;
                    }
                }
            }
        }
        if confirmed_pending {
            self.pending_tab_focus = None;
        }
        if let Some(pane_id) = follow_pane {
            self.select_terminal_pane(Some(pane_id));
            // Herdr moved the keyboard, not the operator. The read record
            // follows only a focus the operator made in Hide.
            self.operator_focused_pane_id = None;
        }
        for (checkout_id, hide_tab, herdr_tab) in followed {
            crate::diagnostic!(serde_json::json!({
                "component": "view_state",
                "kind": "tab.focus.followed",
                "checkout_id": checkout_id,
                "from_tab_id": hide_tab,
                "to_tab_id": herdr_tab,
                "origin": "herdr",
            }));
            self.push_diagnostic(
                "tab.focus.followed",
                format!(
                    "Herdr focused tab {herdr_tab} in {checkout_id}; Hide was showing {hide_tab}"
                ),
            );
        }
    }

    /// The pane that becoming visible should put the keyboard on: the one the
    /// operator last had in that tab, and its first pane before it has ever
    /// been visited.
    fn tab_focus_pane_id(&self, tab_id: &str, first_pane_id: Option<String>) -> Option<String> {
        self.snapshot
            .pane_layouts
            .iter()
            .find(|layout| layout.tab_id == tab_id)
            .map(|layout| layout.focused_pane_id.clone())
            .or(first_pane_id)
    }

    /// Keeps the focused checkout drawing the tab that holds the keyboard.
    ///
    /// The visible tab and the selected pane are two core-owned values with
    /// one invariant between them: the selected pane lies in the visible tab
    /// of the focused checkout. A tab action moves the pane into the tab; a
    /// pane action, a restore or a retirement moves the tab to the pane,
    /// here. Without it the canvas drew one tab while the pane attached was
    /// in another, and the operator saw five terminals with nothing in them.
    fn align_visible_tab_with_selected_pane(&mut self) -> bool {
        let Some(pane_id) = self.snapshot.terminal.pane_id.clone() else {
            return false;
        };
        let Some(checkout_id) = self.snapshot.navigator.focused_checkout_id.clone() else {
            return false;
        };
        let Some(checkout) = self
            .snapshot
            .navigator
            .workspaces
            .iter_mut()
            .flat_map(|workspace| workspace.checkouts.iter_mut())
            .find(|checkout| checkout.id == checkout_id)
        else {
            return false;
        };
        let Some(tab_id) = checkout
            .tabs
            .iter()
            .find(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
            .and_then(|tab| tab.id.clone())
        else {
            return false;
        };
        if checkout.active_tab_id.as_deref() == Some(tab_id.as_str())
            && self.visible_tab_ids.get(&checkout_id) == Some(&tab_id)
        {
            return false;
        }
        let from_tab_id = checkout.active_tab_id.replace(tab_id.clone());
        self.visible_tab_ids
            .insert(checkout_id.clone(), tab_id.clone());
        self.sync_active_tab_projection();
        crate::diagnostic!(serde_json::json!({
            "component": "view_state",
            "kind": "tab.visible_aligned",
            "checkout_id": checkout_id,
            "from_tab_id": from_tab_id,
            "to_tab_id": tab_id,
            "pane_id": pane_id,
        }));
        self.push_diagnostic(
            "tab.visible_aligned",
            format!("Tab {tab_id} is visible because it holds the selected pane {pane_id}"),
        );
        true
    }

    /// Stops waiting on a view-state notification Herdr never answered.
    ///
    /// The value Hide chose is kept: the operator's tab and pane are Hide's,
    /// and a silent Herdr is a reason to report, not a reason to move the
    /// screen out from under them. Dropping the wait is what lets the next
    /// Herdr event be read as an external focus rather than as a late answer.
    fn expire_pending_view_focus(&mut self, now_unix_ms: u64) -> bool {
        let mut expired = Vec::new();
        for slot in ViewFocusSlot::ALL {
            let pending = self.pending_view_focus(slot);
            if let Some(pending) = pending.as_ref()
                && pending.expired_at(now_unix_ms)
            {
                expired.push((slot.what(), pending.target_id.clone()));
                *self.pending_view_focus_mut(slot) = None;
            }
        }
        let changed = !expired.is_empty();
        for (what, target_id) in expired {
            crate::diagnostic!(serde_json::json!({
                "component": "view_state",
                "kind": "view_focus.timed_out",
                "what": what,
                "target_id": target_id,
                "timeout_ms": VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS,
            }));
            self.push_diagnostic(
                "view_focus.timed_out",
                format!(
                    "Herdr did not confirm {what} focus {target_id} within {VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS} ms; Hide keeps it"
                ),
            );
        }
        changed
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
        // The visible tab is looked up by the id Hide holds for this checkout.
        // The first tab is not a stand-in for a missing one: reading position
        // as focus is what made a tab move look like a focus change. A
        // checkout that has tabs always names one, so the only tabless case
        // left is a checkout with no tabs at all.
        let active = checkout.active_tab_id.as_deref().and_then(|active_tab_id| {
            checkout
                .tabs
                .iter()
                .find(|tab| tab.id.as_deref() == Some(active_tab_id))
        });
        if let Some(tab) = active {
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
        mut fetched: Result<RemoteSessionSnapshot, SessionFetchError>,
    ) -> bool {
        // A remote projection is built off the runtime, so it cannot see the
        // read ledger; without this every stopped remote pane published `Done`
        // and demanded a close confirmation. Hide never focuses a remote pane,
        // so the remote server's own focus is the read signal.
        let mut read_changed = false;
        if let Ok(session) = fetched.as_mut() {
            read_changed = self.apply_remote_read_state(target_id, session);
        }
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

        let mut changed = read_changed;
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
        let target_prefix = remote_pane_id_prefix(target_id);
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
        // A pane the remote no longer lists loses everything; a pane it lists
        // but does not run a session for keeps its sizes and loses the session.
        changed |= self.retain_terminal_pane_state(|pane_id| {
            !belongs_to_target(pane_id) || live_pane_ids.contains(pane_id)
        });
        changed |= self.retain_terminal_session_state(|pane_id| {
            !belongs_to_target(pane_id) || active_pane_ids.contains(pane_id)
        });

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
                transport_last_attempt_at_unix_ms: None,
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

    /// Removes every piece of terminal state for panes that no longer exist.
    /// Keeping this list in one place prevents a newly added pane-keyed cache
    /// from surviving retirement and being inherited if Herdr reuses an id.
    fn retain_terminal_pane_state(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let before = self.terminal_state_len();
        self.retain_terminal_session_state(&keep);
        self.terminal_sizes.retain(|pane_id, _| keep(pane_id));
        self.terminal_view_sizes.retain(|pane_id, _| keep(pane_id));
        self.panes_closing.retain(|pane_id| keep(pane_id));
        before != self.terminal_state_len()
    }

    /// Removes the state of a pane's terminal session while the pane itself,
    /// and so its sizes, stays known.
    fn retain_terminal_session_state(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let before = self.terminal_state_len();
        self.terminal_sessions.retain(|pane_id, _| keep(pane_id));
        self.terminal_session_generations
            .retain(|pane_id, _| keep(pane_id));
        self.terminal_session_lifecycles
            .retain(|pane_id, _| keep(pane_id));
        self.terminal_recovery.retain(|pane_id, _| keep(pane_id));
        self.terminal_frames_need_full
            .retain(|pane_id| keep(pane_id));
        self.panes_awaiting_size.retain(|pane_id| keep(pane_id));
        self.panes_scrolled_before_size
            .retain(|pane_id| keep(pane_id));
        before != self.terminal_state_len()
    }

    /// Every pane-keyed terminal map, counted together so a retain pass can
    /// report whether it removed anything.
    fn terminal_state_len(&self) -> usize {
        self.terminal_sessions.len()
            + self.terminal_session_generations.len()
            + self.terminal_session_lifecycles.len()
            + self.terminal_recovery.len()
            + self.terminal_sizes.len()
            + self.terminal_view_sizes.len()
            + self.terminal_frames_need_full.len()
            + self.panes_awaiting_size.len()
            + self.panes_scrolled_before_size.len()
            + self.panes_closing.len()
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
        // The session update is this runtime's only regular tick, so it is
        // also where a notification Herdr never answered stops being pending.
        // Doing it first lets this same update be read as an external focus
        // rather than as a late answer to a request that has gone quiet.
        let timed_out = self.expire_pending_view_focus(unix_milliseconds());
        let previously_projected_pane = self
            .snapshot
            .terminal
            .pane_id
            .as_deref()
            .filter(|pane_id| self.layout_holding_pane(pane_id).is_some())
            .map(str::to_owned);
        let previously_projected_tab = self
            .active_pane_layout()
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
        // Scratch is in no checkout, so the flag above can never be true for
        // one of its panes. This is captured here for the same reason that one
        // is: the catalog reconciliation below replaces the Scratch node, and
        // after it nothing records which space held a pane that has gone.
        let previously_selected_in_scratch = self
            .snapshot
            .terminal
            .pane_id
            .as_deref()
            .or(self.snapshot.ui_state.selected_pane_id.as_deref())
            .is_some_and(|pane_id| {
                self.snapshot
                    .navigator
                    .scratch
                    .tabs
                    .iter()
                    .any(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
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
            self.retain_terminal_pane_state(keep);
        }
        let mut excluded = Vec::new();
        let mut rejected_layouts: Vec<(String, String)> = Vec::new();
        let catalog_changed = fetched
            .as_ref()
            .map(|payload| self.reconcile_session_catalog(payload, precomputed))
            .unwrap_or(false);
        let (state, message, agents, layouts, layout, selection_changed) = match fetched {
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
                // Scratch belongs to no checkout, so the rule above cannot see
                // one of its panes leave. Closing a Scratch tab left the
                // selection on a pane that no longer existed, which raised the
                // projection error below on every tick and left every later
                // command reading as though no space were focused: ⌘T answered
                // "create or register a workspace" while a Scratch tab was on
                // screen. A Scratch pane that goes is the same expected
                // transition, and it retargets inside Scratch for the same
                // reason a checkout's retargets inside itself.
                let scratch_pane_retired = previously_selected_in_scratch && selected_pane_missing;
                let selected_pane_retired = scratch_pane_retired
                    || (selected_was_projected
                        && (selected_pane_missing || selected_left_focused_checkout));
                let replacement_pane_id = if scratch_pane_retired {
                    let scratch_workspaces = self.scratch_workspace_ids(&payload);
                    let scratch_panes: Vec<&str> = payload
                        .layouts
                        .iter()
                        .filter(|layout| {
                            scratch_workspaces
                                .iter()
                                .any(|id| id == &layout.workspace_id)
                        })
                        .flat_map(|layout| layout.panes.iter().map(|pane| pane.pane_id.as_str()))
                        .collect();
                    payload
                        .focused_pane_id
                        .as_deref()
                        .filter(|pane_id| scratch_panes.contains(pane_id))
                        .or_else(|| scratch_panes.first().copied())
                        .map(str::to_owned)
                } else {
                    selected_pane_retired
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
                                    payload.focused_pane_id.as_deref().filter(|pane_id| {
                                        focused_checkout_pane_set.contains(*pane_id)
                                    })
                                })
                                .or_else(|| {
                                    // A close moves Herdr's keyboard to another
                                    // tab, and the pane focus for it can arrive
                                    // after this snapshot; the tab Herdr names now
                                    // is where the operator is looking, not the
                                    // checkout's first pane.
                                    HerdrTabView::from_payload(&payload)
                                        .focused_tab_id
                                        .and_then(|tab_id| {
                                            payload
                                                .layouts
                                                .iter()
                                                .find(|layout| layout.tab_id == tab_id)
                                        })
                                        .map(|layout| layout.focused_pane_id.as_str())
                                        .filter(|pane_id| {
                                            focused_checkout_pane_set.contains(*pane_id)
                                        })
                                })
                                .or_else(|| focused_checkout_pane_ids.first().map(String::as_str))
                                .map(str::to_owned)
                        })
                        .flatten()
                };
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
                    // With no selection, the keyboard lands in the tab the
                    // checkout is showing, on that tab's remembered pane, so
                    // the tab drawn is the tab attached. The first pane of the
                    // checkout is for a checkout that shows no tab yet.
                    let visible_tab_pane_id = self
                        .snapshot
                        .ui_state
                        .focused_checkout_id
                        .as_deref()
                        .and_then(|checkout_id| self.visible_tab_ids.get(checkout_id))
                        .and_then(|tab_id| {
                            payload
                                .layouts
                                .iter()
                                .find(|layout| &layout.tab_id == tab_id)
                        })
                        .map(|layout| layout.focused_pane_id.as_str())
                        .filter(|pane_id| focused_checkout_pane_set.contains(*pane_id));
                    selected_pane_id
                        .as_deref()
                        .filter(|pane_id| {
                            selected_still_exists && focused_checkout_pane_set.contains(*pane_id)
                        })
                        .or(visible_tab_pane_id)
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
                let (layouts, rejected) = live::project_layouts(&payload);
                rejected_layouts = rejected;
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
                        layouts,
                        layout,
                        selection_changed,
                    ),
                    Err(projection_error) => (
                        "malformed",
                        Some(format!(
                            "Herdr pane layout could not be projected: {projection_error}"
                        )),
                        None,
                        layouts,
                        None,
                        selection_changed,
                    ),
                }
            }
            Err(error) => (
                error.state(),
                Some(error.message().to_owned()),
                None,
                Vec::new(),
                None,
                false,
            ),
        };

        // A single unreadable agent record excludes only itself; the
        // exclusion is stated rather than silently folded into the count.
        for exclusion in &excluded {
            let pane_id = exclusion.pane_id.as_deref().unwrap_or("<missing pane id>");
            crate::diagnostic!(serde_json::json!({
                "component": "session",
                "kind": "agent.excluded",
                "pane_id": pane_id,
                "source_index": exclusion.source_index,
                "message": exclusion.reason,
            }));
            self.push_diagnostic(
                "agent.excluded",
                format!("Agent {pane_id} was excluded: {}", exclusion.reason),
            );
        }

        let mut changed = catalog_changed || selection_changed || timed_out || !excluded.is_empty();
        if self.snapshot.status.herdr.state != state
            || self.snapshot.status.herdr.message.as_deref() != message.as_deref()
        {
            self.snapshot.status.herdr.state = state.to_owned();
            self.snapshot.status.herdr.message = message;
            changed = true;
        }
        if let Some(mut agents) = agents {
            self.place_agents_in_navigator(&mut agents);
            changed |= self.apply_pane_read_state(&mut agents, ReadRecordScope::Local);
            if crate::sidebar::prune_lineage_collapse(
                &mut self.snapshot.ui_state.collapsed_agent_pane_ids,
                &agents,
                ReadRecordScope::Local,
            ) {
                self.persist_ui_state();
                changed = true;
            }
            crate::sidebar::apply_lineage(
                &mut agents,
                &self.snapshot.navigator.workspaces,
                &self.snapshot.ui_state.collapsed_agent_pane_ids,
            );
            if self.snapshot.navigator.agents != agents {
                self.snapshot.navigator.agents = agents;
                changed = true;
            }
        }
        for (tab_id, reason) in &rejected_layouts {
            crate::diagnostic!(serde_json::json!({
                "component": "session",
                "kind": "layout.excluded",
                "tab_id": tab_id,
                "message": reason,
            }));
            self.push_diagnostic(
                "layout.excluded",
                format!("Tab {tab_id} has no drawable layout: {reason}"),
            );
        }
        changed |= self.store_pane_layouts(layouts);
        if let Some(layout) = layout {
            if self.snapshot.terminal.pane_id.is_none() {
                let pane_id = layout.focused_pane_id.clone();
                self.snapshot.terminal.pane_id = Some(pane_id.clone());
                self.snapshot.focused.pane_id = Some(pane_id);
            }
            changed |= self.apply_pane_layout(layout);
        }
        changed |= self.refresh_worktree_projection();
        changed |= self.align_visible_tab_with_selected_pane();
        changed |= self.track_visible_tab_attachments();
        changed | self.refresh_pet()
    }

    /// Applies provider usage that the session-sync coordinator read outside
    /// the runtime mutex. The two fixed rows are revisioned with the rest
    /// snapshot, so an unchanged refresh produces no shell work.
    /// What the changes reader should describe right now, or `None` when the
    /// changes view and no diff tab are showing and nothing should be read at
    /// all. This is the whole reason the reader never forks `git` on a
    /// per-tick path.
    pub fn changes_request(&self) -> Option<crate::changes::ChangesRequest> {
        let changes_list_visible = self.snapshot.ui_state.right_panel_visible
            && self.snapshot.ui_state.right_panel_section == RightPanelSection::Changes;
        let active_diff = self
            .snapshot
            .editor
            .active_tab_id
            .as_deref()
            .and_then(|tab_id| {
                self.snapshot
                    .editor
                    .tabs
                    .iter()
                    .find(|tab| tab.id == tab_id && tab.kind == EditorTabKind::Diff)
            });
        if !changes_list_visible && active_diff.is_none() {
            return None;
        }
        let root_path = self.snapshot.navigator.root_path.as_ref()?;
        let selected_path = active_diff
            .map(|tab| tab.path.clone())
            .or_else(|| self.snapshot.changes.selected_path.clone());
        let selected_committed = active_diff
            .and_then(|tab| tab.diff_committed)
            .unwrap_or(self.snapshot.changes.selected_committed);
        Some(crate::changes::ChangesRequest {
            root_path: PathBuf::from(root_path),
            selected_path,
            selected_committed,
            // The base comes from the checkout row, so the committed group and
            // the card's `↑A ↓B` are measured against the same branch.
            base_branch: self
                .focused_local_checkout()
                .and_then(|(_, checkout)| checkout.base_branch.clone()),
        })
    }

    /// Which repositories to list worktrees for, and what base branch each
    /// branch is measured against.
    ///
    /// The bases come from the pull-request answer, so the first worktree read
    /// compares against the repository default and a later one compares
    /// against each pull request's own base. That is a changed request, which
    /// is always due, so the correction arrives without a second trigger.
    pub fn worktrees_request(&self) -> crate::worktrees::WorktreeRequest {
        let projects = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none() && workspace.is_git)
            .map(|workspace| crate::worktrees::WorktreeProjectRequest {
                root_path: PathBuf::from(&workspace.path),
                base_override: self
                    .snapshot
                    .ui_state
                    .project_base_branches
                    .get(&workspace.path)
                    .cloned(),
                bases: self
                    .github
                    .project(&workspace.path)
                    .map(|project| {
                        project
                            .pull_requests
                            .iter()
                            .map(|pull_request| {
                                (
                                    pull_request.head_branch.clone(),
                                    pull_request.base_branch.clone(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
        crate::worktrees::WorktreeRequest {
            overview_root: (self.snapshot.ui_state.right_panel_visible
                && self.snapshot.ui_state.right_panel_section == RightPanelSection::Overview)
                .then(|| {
                    self.focused_local_checkout()
                        .map(|(w, _)| PathBuf::from(&w.path))
                })
                .flatten(),
            projects,
            generation: self.worktree_generation,
        }
    }

    pub fn ingest_worktrees(&mut self, catalog: crate::model::WorktreeCatalogSnapshot) -> bool {
        let changed = self.worktree_catalog != catalog || self.snapshot.git_worktrees_loading;
        self.worktree_catalog = catalog;
        self.snapshot.git_worktrees_loading = false;
        self.refresh_worktree_projection();
        changed
    }

    pub(crate) fn cleanup_current_path(&self) -> Option<String> {
        self.focused_local_checkout()
            .map(|(_, checkout)| checkout.path.clone())
    }

    pub(crate) fn ingest_cleanup(&mut self, answer: live::cleanup::CleanupSnapshot) -> bool {
        if self
            .cleanup
            .as_ref()
            .is_none_or(|current| current.id != answer.id)
        {
            return false;
        }
        let removed = answer
            .rows
            .iter()
            .any(|row| row.result.as_deref() == Some("removed"));
        self.cleanup = Some(answer);
        if removed {
            self.refresh_worktrees();
            self.remeasure_disk();
        }
        self.refresh_worktree_projection();
        true
    }

    fn review_cleanup(&mut self) -> bool {
        if self
            .cleanup
            .as_ref()
            .is_some_and(|r| matches!(r.phase.as_str(), "loading" | "removing"))
        {
            return false;
        }
        let Some((workspace, checkout)) = self.focused_local_checkout() else {
            return false;
        };
        let root = workspace.path.clone();
        let current = checkout.path.clone();
        self.next_cleanup_id = self.next_cleanup_id.wrapping_add(1).max(1);
        let mut review = live::cleanup::CleanupSnapshot {
            id: self.next_cleanup_id,
            repository_root: root,
            phase: "loading".into(),
            ..Default::default()
        };
        self.cleanup = Some(review.clone());
        let started = self.live.clone().ok_or_else(|| "A live Herdr connection is required to verify worktree usage. Connect and review again.".into())
            .and_then(|context| live::cleanup::spawn(context, review.clone(), None, current));
        if let Err(message) = started {
            review.phase = "failed".into();
            review.message = Some(message);
            self.cleanup = Some(review);
        }
        self.refresh_worktree_projection();
        true
    }

    fn confirm_cleanup(&mut self, payload: CleanupConfirmPayload) -> bool {
        let Some(review) = self
            .cleanup
            .clone()
            .filter(|r| r.id == payload.id && r.phase == "review")
        else {
            return false;
        };
        let paths: Vec<_> = payload
            .paths
            .into_iter()
            .filter(|path| {
                review
                    .rows
                    .iter()
                    .any(|row| row.path == *path && row.exclusion.is_none())
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if paths.is_empty() {
            return false;
        }
        let Some(current) = self.cleanup_current_path() else {
            return false;
        };
        self.cleanup.as_mut().unwrap().phase = "removing".into();
        let started = self
            .live
            .clone()
            .ok_or_else(|| {
                "The Herdr connection is unavailable. Reconnect and review again.".into()
            })
            .and_then(|context| {
                live::cleanup::spawn(context, review.clone(), Some(paths), current)
            });
        if let Err(message) = started {
            let active = self.cleanup.as_mut().unwrap();
            active.phase = "failed".into();
            active.message = Some(message);
        }
        self.refresh_worktree_projection();
        true
    }

    /// Combines subprocess answers with live pane and agent state, then sends
    /// that one model to the sidebar, Git section, and summary card.
    /// No filesystem or socket work occurs here.
    fn refresh_worktree_projection(&mut self) -> bool {
        let before_catalog = self.worktree_catalog.clone();
        let before_navigator = self.snapshot.navigator.clone();
        let before_git = self.snapshot.git_worktrees.clone();
        let before_remote = self.snapshot.git_worktrees_remote;

        let pane_rows = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .map(|checkout| {
                (
                    checkout.path.clone(),
                    checkout
                        .tabs
                        .iter()
                        .flat_map(|tab| tab.panes.iter())
                        .map(|pane| pane.id.clone())
                        .collect::<HashSet<_>>(),
                )
            })
            .collect::<HashMap<_, _>>();
        let running = self
            .snapshot
            .navigator
            .agents
            .iter()
            .filter(|agent| agent.activity == "working")
            .map(|agent| agent.pane_id.as_str())
            .collect::<HashSet<_>>();
        let requested_github: HashSet<_> = self
            .github_request()
            .projects
            .iter()
            .map(|p| p.root.to_string_lossy().into_owned())
            .collect();
        let github = self.github.clone();
        let disk_usage = self.disk_usage.clone();

        for project in &mut self.worktree_catalog.projects {
            let github_project = github.project(&project.root_path);
            project.github = github_project.map(|g| g.status.clone()).unwrap_or_else(|| {
                crate::model::GithubStatusSnapshot {
                    loading: requested_github.contains(&project.root_path),
                    ..Default::default()
                }
            });
            project.pull_requests = github_project
                .map(|g| g.pull_requests.clone())
                .unwrap_or_default();
            project.pull_request_window = crate::github::PULL_REQUEST_LIMIT.into();
            for worktree in &mut project.worktrees {
                let panes = pane_rows.get(&worktree.path);
                worktree.pane_count = panes.map_or(0, HashSet::len);
                worktree.running_agent_count = panes.map_or(0, |pane_ids| {
                    pane_ids
                        .iter()
                        .filter(|pane_id| running.contains(pane_id.as_str()))
                        .count()
                });
                worktree.disk = disk_usage
                    .iter()
                    .find(|disk| disk.path.as_deref() == Some(worktree.path.as_str()))
                    .cloned()
                    .unwrap_or_default();
                worktree.github = github_project
                    .map(|project| project.status.clone())
                    .unwrap_or_default();
                worktree.pull_request = worktree.branch.as_deref().and_then(|branch| {
                    github_project?
                        .pull_requests
                        .iter()
                        .find(|pull_request| pull_request.head_branch == branch)
                        .cloned()
                });
                worktree.deletion_gate = crate::worktrees::deletion_gate(
                    worktree,
                    worktree.branch == project.base_branch && project.base_branch.is_some(),
                    false,
                    worktree.pane_count,
                    worktree.running_agent_count,
                );
            }
            project.shared_git_disk = disk_usage
                .iter()
                .find(|d| d.path.is_some() && d.path == project.shared_git_path)
                .cloned()
                .unwrap_or_default();
            let components: Vec<_> = project
                .worktrees
                .iter()
                .map(|w| &w.disk)
                .chain(std::iter::once(&project.shared_git_disk))
                .collect();
            project.disk_total_bytes = project
                .shared_git_path
                .as_ref()
                .and_then(|_| components.iter().map(|d| d.total_bytes).sum());
            let confirmed: Vec<_> = components.iter().filter_map(|d| d.total_bytes).collect();
            project.disk_confirmed_bytes = (!confirmed.is_empty()).then(|| confirmed.iter().sum());
            project.linked_disk_bytes = project
                .worktrees
                .iter()
                .filter(|w| !w.is_main)
                .map(|w| w.disk.total_bytes)
                .sum();
            project.disk_unavailable_reason = components
                .iter()
                .find_map(|d| d.unavailable_reason.clone())
                .or_else(|| {
                    project
                        .shared_git_path
                        .is_none()
                        .then(|| "Shared Git directory is unavailable. Refresh Overview.".into())
                });
            project.worktrees.sort_by(|left, right| {
                right
                    .is_main
                    .cmp(&left.is_main)
                    .then_with(|| (right.pane_count > 0).cmp(&(left.pane_count > 0)))
                    .then_with(|| {
                        right
                            .last_commit_unix_seconds
                            .unwrap_or(0)
                            .cmp(&left.last_commit_unix_seconds.unwrap_or(0))
                    })
                    .then_with(|| left.path.cmp(&right.path))
            });
        }

        for workspace in &mut self.snapshot.navigator.workspaces {
            if workspace.remote_target_id.is_some() {
                continue;
            }
            workspace::apply_worktrees(workspace, &self.worktree_catalog);
        }
        crate::project_context::sort_projects(
            &mut self.snapshot.navigator.workspaces,
            &self.snapshot.navigator.agents,
        );

        let focused_project = self
            .snapshot
            .navigator
            .focused_checkout_id
            .as_deref()
            .and_then(|focused_id| {
                self.snapshot.navigator.workspaces.iter().find(|workspace| {
                    workspace
                        .checkouts
                        .iter()
                        .any(|checkout| checkout.id == focused_id)
                })
            });
        self.snapshot.git_worktrees_remote =
            focused_project.is_some_and(|workspace| workspace.remote_target_id.is_some());
        self.snapshot.git_worktrees = focused_project
            .filter(|workspace| workspace.remote_target_id.is_none())
            .and_then(|workspace| self.worktree_catalog.project(&workspace.path))
            .cloned();
        if let Some(project) = self.snapshot.git_worktrees.as_mut() {
            project.cleanup = self
                .cleanup
                .as_ref()
                .filter(|review| review.repository_root == project.root_path)
                .cloned();
        }
        self.refresh_card();
        before_catalog != self.worktree_catalog
            || before_navigator != self.snapshot.navigator
            || before_git != self.snapshot.git_worktrees
            || before_remote != self.snapshot.git_worktrees_remote
    }

    /// The worktree catalog the coordinator should build the next projection
    /// from. Held by the runtime so a rebuild triggered from anywhere uses the
    /// same worktrees the last read produced.
    pub fn worktree_catalog(&self) -> crate::model::WorktreeCatalogSnapshot {
        self.worktree_catalog.clone()
    }

    /// Which repositories to look pull requests up for. Remote projects are
    /// out of scope, and a plain folder has no repository to ask about.
    pub fn github_request(&self) -> crate::github::GithubRequest {
        if !self.github_lookup_requested() && self.sidebar_github_projects.is_empty() {
            return crate::github::GithubRequest::default();
        }
        crate::github::GithubRequest {
            projects: self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .filter(|workspace| workspace.remote_target_id.is_none() && workspace.is_git)
                .filter(|workspace| {
                    (self.github_lookup_requested()
                        && (self.snapshot.ui_state.right_panel_section == RightPanelSection::Git
                            || self
                                .focused_local_checkout()
                                .is_some_and(|(focused, _)| focused.path == workspace.path)))
                        || self.sidebar_github_projects.contains(&workspace.path)
                })
                .map(|workspace| crate::github::GithubProjectRequest {
                    root: PathBuf::from(&workspace.path),
                    generation: self
                        .github_generations
                        .get(&workspace.path)
                        .copied()
                        .unwrap_or(0),
                })
                .collect(),
        }
    }

    fn github_lookup_requested(&self) -> bool {
        self.snapshot.ui_state.right_panel_visible
            && matches!(
                self.snapshot.ui_state.right_panel_section,
                RightPanelSection::Git | RightPanelSection::Overview
            )
    }

    pub fn ingest_github(&mut self, github: crate::model::GithubSnapshot) -> bool {
        // A failed lookup must not erase the answer it failed to replace: the
        // card shows the previous pull requests with `as of` beside them, so a
        // stale project keeps its results and only its status changes.
        let mut merged = github;
        for project in &mut merged.projects {
            if (project.status.stale || project.status.unavailable_reason.is_some())
                && let Some(previous) = self.github.project(&project.root_path)
                && !previous.pull_requests.is_empty()
            {
                project.status.stale = true;
                project.pull_requests = previous.pull_requests.clone();
                project.status.last_success_at_unix_ms = previous.status.last_success_at_unix_ms;
            }
        }
        if self.github == merged {
            return false;
        }
        self.github = merged;
        self.apply_pull_requests();
        self.refresh_worktree_projection();
        true
    }

    /// Records that one project's pull requests must be read again.
    ///
    /// Repeated calls before the read happens are one refresh, not several:
    /// the counter is the request, and an unchanged request is not re-read.
    pub fn refresh_pull_requests(&mut self, project_path: &str) {
        let generation = self
            .github_generations
            .entry(project_path.to_owned())
            .or_insert(0);
        *generation = generation.wrapping_add(1);
        if let Some(cached) = self
            .github
            .projects
            .iter_mut()
            .find(|p| p.root_path == project_path)
        {
            cached.status.loading = true;
        }
    }

    pub fn refresh_worktrees(&mut self) {
        self.worktree_generation = self.worktree_generation.wrapping_add(1);
        self.snapshot.git_worktrees_loading = true;
    }

    fn open_git_worktree(&mut self, checkout_path: String) -> bool {
        let target = self.worktree_catalog.projects.iter().find_map(|project| {
            project
                .worktrees
                .iter()
                .find(|worktree| worktree.path == checkout_path)
                .map(|worktree| (project.root_path.clone(), worktree.clone()))
        });
        let Some((repository_root, worktree)) = target else {
            self.set_error(
                "worktree.open_unknown",
                format!("Worktree is no longer listed: {checkout_path}"),
                true,
            );
            self.refresh_worktrees();
            return true;
        };
        if worktree.missing {
            self.ingest_worktree_open_result(
                checkout_path,
                Err("Worktree is missing on disk".to_owned()),
            );
            return true;
        }
        let newest_pane = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .filter(|checkout| checkout.path == checkout_path)
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .max_by_key(|pane| pane.activity_at_unix_ms.unwrap_or(0))
            .map(|pane| pane.id.clone());
        let Some(context) = self.live.as_ref().cloned() else {
            self.ingest_worktree_open_result(
                checkout_path,
                Err("Opening a worktree needs a live Herdr connection".to_owned()),
            );
            return true;
        };
        if let Err(message) =
            live::spawn_worktree_open(context, checkout_path.clone(), repository_root, newest_pane)
        {
            self.ingest_worktree_open_result(checkout_path, Err(message));
        }
        true
    }

    fn set_git_worktree_base(&mut self, project_path: String, branch: String) -> bool {
        let listed = self
            .worktree_catalog
            .project(&project_path)
            .is_some_and(|project| {
                project
                    .worktrees
                    .iter()
                    .any(|worktree| worktree.branch.as_deref() == Some(branch.as_str()))
            });
        if !listed {
            self.set_error(
                "worktree.base_unavailable",
                format!("Branch {branch} is no longer checked out in this repository"),
                true,
            );
            self.refresh_worktrees();
            return true;
        }
        if self
            .snapshot
            .ui_state
            .project_base_branches
            .insert(project_path, branch)
            .is_some()
        {
            // Replacing and inserting both persist below; the return value is
            // deliberately not used as the change detector because the same
            // branch is an idempotent no-op at the reader boundary.
        }
        self.persist_ui_state();
        self.refresh_worktrees();
        true
    }

    fn remove_git_worktree(&mut self, payload: RemoveWorktreePayload) -> bool {
        if let Some(removal) = self.snapshot.worktree_removal.as_ref()
            && matches!(removal.phase.as_str(), "closing" | "ready")
        {
            if removal.checkout_path == payload.checkout_path {
                return false;
            }
            self.set_error(
                "worktree.remove_busy",
                format!(
                    "Finish removing {} before deleting another worktree",
                    removal.checkout_path
                ),
                true,
            );
            return true;
        }
        let target = self.worktree_catalog.projects.iter().find_map(|project| {
            project
                .worktrees
                .iter()
                .find(|worktree| worktree.path == payload.checkout_path)
                .map(|worktree| {
                    (
                        project.root_path.clone(),
                        project.base_branch.clone(),
                        worktree.clone(),
                    )
                })
        });
        let Some((repository_root, protected_base_branch, worktree)) = target else {
            self.set_error(
                "worktree.remove_unknown",
                format!("Worktree is no longer listed: {}", payload.checkout_path),
                true,
            );
            self.refresh_worktrees();
            return true;
        };
        if let Some(reason) = worktree.deletion_gate.blocked_reason.as_ref() {
            self.set_error("worktree.remove_blocked", reason.clone(), true);
            return true;
        }
        let pane_ids = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .filter(|checkout| checkout.path == payload.checkout_path)
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>();
        self.next_worktree_removal_id = self.next_worktree_removal_id.wrapping_add(1).max(1);
        let id = self.next_worktree_removal_id;
        self.snapshot.worktree_removal = Some(crate::model::WorktreeRemovalSnapshot {
            id,
            repository_root,
            checkout_path: payload.checkout_path.clone(),
            expected_head_sha: worktree.head_sha.clone(),
            expected_branch: worktree.branch.clone(),
            protected_base_branch,
            branch: worktree.branch,
            delete_branch: payload.delete_branch && worktree.deletion_gate.can_delete_branch,
            phase: "closing".to_owned(),
            message: None,
        });
        eprintln!(
            "{}",
            serde_json::json!({
                "component":"worktree_removal",
                "stage":"close_requested",
                "id":id,
                "path":payload.checkout_path,
                "pane_ids":pane_ids,
            })
        );
        let Some(context) = self.live.as_ref().cloned() else {
            return self.ingest_worktree_close_result(
                id,
                Err("Deleting a worktree needs a live Herdr connection".to_owned()),
            );
        };
        if let Err(message) =
            live::spawn_worktree_close(context, id, payload.checkout_path, pane_ids)
        {
            self.ingest_worktree_close_result(id, Err(message));
        }
        true
    }

    pub fn ingest_worktree_close_result(&mut self, id: u64, result: Result<(), String>) -> bool {
        let Some(active) = self.snapshot.worktree_removal.as_ref() else {
            return false;
        };
        if active.id != id || active.phase != "closing" {
            return false;
        }
        let mut result = result;
        if result.is_ok() {
            let current = self
                .worktree_catalog
                .projects
                .iter()
                .flat_map(|project| &project.worktrees)
                .find(|worktree| worktree.path == active.checkout_path);
            let identity_changed = current.is_none_or(|worktree| {
                worktree.head_sha != active.expected_head_sha
                    || worktree.branch != active.expected_branch
                    || worktree.deletion_gate.blocked_reason.is_some()
            });
            let pane_reappeared = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| &workspace.checkouts)
                .any(|checkout| checkout.path == active.checkout_path && checkout.has_panes);
            if identity_changed || pane_reappeared {
                result = Err(if pane_reappeared {
                    "A pane appeared in the worktree while deletion was being confirmed".to_owned()
                } else {
                    "The worktree identity or deletion gate changed while panes were closing"
                        .to_owned()
                });
            }
        }
        let removal = self.snapshot.worktree_removal.as_mut().unwrap();
        match result {
            Ok(()) => {
                removal.phase = "ready".to_owned();
                removal.message = None;
            }
            Err(message) => {
                removal.phase = "failed".to_owned();
                removal.message = Some(message);
            }
        }
        true
    }

    pub fn ingest_worktree_open_result(
        &mut self,
        checkout_path: String,
        result: Result<(), String>,
    ) -> bool {
        let message = result.err();
        let mut changed = false;
        for project in &mut self.worktree_catalog.projects {
            if let Some(worktree) = project
                .worktrees
                .iter_mut()
                .find(|worktree| worktree.path == checkout_path)
                && worktree.open_error != message
            {
                worktree.open_error = message.clone();
                changed = true;
            }
        }
        for workspace in &mut self.snapshot.navigator.workspaces {
            for checkout in &mut workspace.checkouts {
                if checkout.path == checkout_path
                    && let Some(worktree) = checkout.worktree.as_mut()
                    && worktree.open_error != message
                {
                    worktree.open_error = message.clone();
                    changed = true;
                }
            }
        }
        if message.is_some() {
            self.refresh_worktrees();
        }
        changed
    }

    fn finish_worktree_removal(&mut self, payload: WorktreeRemovalFinishedPayload) -> bool {
        let Some(removal) = self.snapshot.worktree_removal.as_mut() else {
            self.set_error(
                "worktree.remove_result_without_request",
                "A worktree removal result arrived without an active request",
                false,
            );
            return true;
        };
        if removal.id != payload.id || removal.phase != "ready" {
            self.set_error(
                "worktree.remove_result_stale",
                format!(
                    "Worktree removal result {} is not the active ready request",
                    payload.id
                ),
                false,
            );
            return true;
        }
        removal.phase = if payload.removed {
            "finished"
        } else {
            "failed"
        }
        .to_owned();
        removal.message = Some(payload.message);
        if payload.removed {
            self.refresh_worktrees();
        }
        true
    }

    /// Worktrees whose size should be measured. An empty request while Git is
    /// hidden is intentional: idle sidebar projection must never launch du.
    pub fn disk_request(&self) -> crate::disk::DiskRequest {
        let paths = if self.snapshot.ui_state.right_panel_visible
            && matches!(
                self.snapshot.ui_state.right_panel_section,
                RightPanelSection::Git | RightPanelSection::Overview
            ) {
            self.focused_local_checkout()
                .and_then(|(workspace, _)| self.worktree_catalog.project(&workspace.path))
                .map(|project| {
                    let mut paths: Vec<_> = project
                        .worktrees
                        .iter()
                        .map(|w| PathBuf::from(&w.path))
                        .collect();
                    if let Some(shared) = &project.shared_git_path {
                        paths.push(PathBuf::from(shared));
                    }
                    paths
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        crate::disk::DiskRequest {
            paths,
            generation: self.disk_generation,
        }
    }

    pub fn ingest_disk_usage(&mut self, disk: Vec<crate::model::DiskUsageSnapshot>) -> bool {
        if self.disk_usage == disk {
            return false;
        }
        self.disk_usage = disk;
        self.refresh_worktree_projection();
        true
    }

    pub fn remeasure_disk(&mut self) {
        self.disk_generation = self.disk_generation.wrapping_add(1);
        self.refresh_card();
    }

    /// The selected checkout, when it is one this machine owns. A remote
    /// checkout has no card: remote worktree management is out of scope, so
    /// nothing here describes one.
    fn focused_local_checkout(&self) -> Option<(&WorkspaceSnapshot, &CheckoutSnapshot)> {
        let focused_checkout_id = self.snapshot.navigator.focused_checkout_id.as_deref()?;
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none())
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == focused_checkout_id)
                    .map(|checkout| (workspace, checkout))
            })
    }

    /// Puts each branch's pull request on the checkout row that shows it.
    ///
    /// The row badge and the card read the same value from the same place, so
    /// the two cannot disagree about what a branch's pull request is.
    fn apply_pull_requests(&mut self) -> bool {
        let mut changed = false;
        let github = self.github.clone();
        let git_requested = self.github_lookup_requested();
        for workspace in self.snapshot.navigator.workspaces.iter_mut() {
            let project = github.project(&workspace.path);
            let status = project
                .map(|project| project.status.clone())
                .unwrap_or_else(|| crate::model::GithubStatusSnapshot {
                    loading: workspace.is_git
                        && workspace.remote_target_id.is_none()
                        && (self.sidebar_github_projects.contains(&workspace.path)
                            || git_requested),
                    ..Default::default()
                });
            for checkout in workspace.checkouts.iter_mut() {
                if checkout.github != status {
                    checkout.github = status.clone();
                    changed = true;
                }
                let pull_request = checkout.branch.as_deref().and_then(|branch| {
                    project?
                        .pull_requests
                        .iter()
                        .find(|pull_request| pull_request.head_branch == branch)
                        .cloned()
                });
                if checkout.pull_request != pull_request {
                    checkout.pull_request = pull_request;
                    changed = true;
                }
            }
        }
        changed
    }

    /// Rebuilds the summary card from what the readers have answered.
    ///
    /// Everything per-checkout already lives on the checkout row; what is
    /// assembled here is the repository's `gh` health, the one disk
    /// measurement, and whether this worktree may be removed.
    pub fn refresh_card(&mut self) -> bool {
        let card = self.projected_card();
        if self.snapshot.card == card {
            return false;
        }
        let retired = self
            .snapshot
            .card
            .panes
            .iter()
            .filter(|old| {
                !card.panes.iter().any(|new| {
                    new.pane_id == old.pane_id && new.parent_pane_id == old.parent_pane_id
                })
            })
            .count();
        if retired > 0 {
            crate::diagnostic!(serde_json::json!({
                "component": "checkout_context", "kind": "context.retired", "count": retired,
            }));
        }
        self.snapshot.card = card;
        true
    }

    fn projected_card(&self) -> crate::model::CheckoutCardSnapshot {
        let Some((workspace, checkout)) = self.focused_local_checkout() else {
            return crate::model::CheckoutCardSnapshot::default();
        };
        let github = if workspace.is_git {
            self.github
                .project(&workspace.path)
                .map(|project| project.status.clone())
                // An absent answer is loading only while the Git section
                // requests it. Explorer also renders this card but starts
                // no lookup, so absence there must not imply work in flight.
                .unwrap_or(crate::model::GithubStatusSnapshot {
                    loading: self.github_lookup_requested(),
                    ..crate::model::GithubStatusSnapshot::default()
                })
        } else {
            crate::model::GithubStatusSnapshot::default()
        };
        let disk = self
            .disk_usage
            .iter()
            .find(|disk| disk.path.as_deref() == Some(checkout.path.as_str()))
            .cloned()
            .unwrap_or_default();
        let disk_measuring = checkout.is_worktree && disk.path.is_none();
        crate::model::CheckoutCardSnapshot {
            inspected_checkout_path: self
                .overview_selection
                .as_ref()
                .filter(|path| workspace.checkouts.iter().any(|c| &c.path == *path))
                .cloned()
                .or_else(|| Some(checkout.path.clone())),
            checkout_id: Some(checkout.id.clone()),
            panes: crate::project_context::checkout_panes(
                checkout,
                &self.snapshot.navigator.workspaces,
                &self.snapshot.navigator.agents,
            ),
            github,
            disk,
            disk_measuring,
            deletion_gate: checkout
                .worktree
                .as_ref()
                .map(|worktree| worktree.deletion_gate.clone()),
        }
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
        if summary.needs_you + summary.working > 0 {
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
                needs_you: summary.needs_you,
                done: summary.done,
                working: summary.working,
                seen: summary.seen,
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

    /// Raises the operator-focused pane's read record and sets the read axis
    /// on every row, then persists the record when it actually moved.
    ///
    /// This is the only place the read axis is decided, and the pane it reads
    /// is the one the operator chose, not the one Herdr reports focused.
    /// Herdr marks every pane in a tab seen the moment the tab is focused, so
    /// three finished agents side by side would clear together; Hide keeps its
    /// own pane-level record instead and never derives unread from Herdr's
    /// `done` or `idle`, nor from a focus it merely inherited.
    ///
    /// The record moves on a real state change or an operator focus, not on
    /// every tick, so the save this triggers is not a per-tick disk write.
    fn apply_pane_read_state(
        &mut self,
        agents: &mut [SidebarAgentSnapshot],
        scope: ReadRecordScope<'_>,
    ) -> bool {
        let focused = self.operator_focused_pane_id.clone();
        let changes = crate::sidebar::apply_read_state(
            agents,
            &mut self.snapshot.ui_state.pane_read_records,
            focused.as_deref(),
            scope,
        );
        let synced = self.sync_pane_status_from_agents(agents);
        let pruned = prune_pane_text_scales(
            &mut self.snapshot.ui_state.pane_text_scales,
            &self.snapshot.navigator.workspaces,
            agents,
            scope,
        );
        if changes.is_empty() && !pruned {
            return synced;
        }
        self.record_read_record_changes(&changes);
        true
    }

    /// Applies the read axis to one remote target's agent rows.
    ///
    /// A pane is a pane: a remote row earns its read record the same way a
    /// local one does, from the focus its own server reports, because Hide
    /// never focuses a remote pane itself. Eviction is scoped to this target's
    /// pane id prefix, so a local sync cannot drop what this pass wrote and
    /// this pass cannot drop another target's records.
    ///
    /// The remote pane tree arrives freshly projected on every sync, with no
    /// read axis applied, so it is synced whether or not the ledger moved.
    fn apply_remote_read_state(
        &mut self,
        target_id: &str,
        session: &mut RemoteSessionSnapshot,
    ) -> bool {
        let focused = session.focused_pane_id.clone();
        let prefix = remote_pane_id_prefix(target_id);
        let changes = crate::sidebar::apply_read_state(
            &mut session.agents,
            &mut self.snapshot.ui_state.pane_read_records,
            focused.as_deref(),
            ReadRecordScope::Remote(&prefix),
        );
        let lineage_pruned = crate::sidebar::prune_lineage_collapse(
            &mut self.snapshot.ui_state.collapsed_agent_pane_ids,
            &session.agents,
            ReadRecordScope::Remote(&prefix),
        );
        crate::sidebar::apply_lineage(
            &mut session.agents,
            &session.workspaces,
            &self.snapshot.ui_state.collapsed_agent_pane_ids,
        );
        if lineage_pruned {
            self.persist_ui_state();
        }
        let synced = sync_pane_status(&mut session.workspaces, &session.agents);
        let pruned = prune_pane_text_scales(
            &mut self.snapshot.ui_state.pane_text_scales,
            &session.workspaces,
            &session.agents,
            ReadRecordScope::Remote(&prefix),
        );
        if changes.is_empty() && !pruned && !lineage_pruned {
            return synced;
        }
        self.record_read_record_changes(&changes);
        true
    }

    /// Logs each read record move and saves the ledger.
    ///
    /// The record moves on a real state change, a focus move, or a pane going
    /// away, not on every tick, so the save this triggers is not a per-tick
    /// disk write.
    fn record_read_record_changes(&mut self, changes: &[crate::sidebar::ReadRecordChange]) {
        for change in changes {
            crate::diagnostic!(serde_json::json!({
                "component": "session",
                "kind": "pane.read_record",
                "pane_id": change.pane_id,
                "evicted": change.evicted,
                "state_change_seq": change.record.state_change_seq,
                "demand": change.record.demand,
                "activity": change.record.activity,
            }));
        }
        self.persist_ui_state();
    }

    /// Copies each local pane's status word and close-confirmation answer from
    /// the agent rows that just had the read axis applied.
    fn sync_pane_status_from_agents(&mut self, agents: &[SidebarAgentSnapshot]) -> bool {
        sync_pane_status(&mut self.snapshot.navigator.workspaces, agents)
    }

    /// Steps one text scale by a direction the shell sent, or names the
    /// direction it could not read and answers `None`.
    fn stepped_text_scale(&mut self, current: f32, direction: &str) -> Option<f32> {
        match direction {
            "in" => Some(clamp_pane_text_scale(current + PANE_TEXT_SCALE_STEP)),
            "out" => Some(clamp_pane_text_scale(current - PANE_TEXT_SCALE_STEP)),
            "reset" => Some(DEFAULT_PANE_TEXT_SCALE),
            other => {
                self.set_error(
                    "pane.text_scale_unknown_direction",
                    format!("{other} is not a text scale direction; expected in, out, or reset"),
                    true,
                );
                None
            }
        }
    }

    /// Saves the current UI state and surfaces a write failure instead of
    /// dropping it.
    /// Writes the operator's UI state and the pane sizes the next launch
    /// attaches with. The sizes are not part of the UI state the shell draws,
    /// so they are collected here rather than carried on the snapshot.
    fn write_ui_state(&mut self) -> Result<(), String> {
        let Some(context) = self.worker_context.clone() else {
            // Standalone runtimes have no shared mutex or worker context.
            return persistence::save(
                &self.state_path,
                &self.snapshot.ui_state,
                &self
                    .terminal_sizes
                    .iter()
                    .map(|(id, size)| (id.clone(), *size))
                    .collect(),
            );
        };
        self.state_save_pending = true;
        if self.state_save_active {
            return Ok(());
        }
        self.state_save_active = true;
        match thread::Builder::new()
            .name("hide-state-save".into())
            .spawn(move || {
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                loop {
                    let (path, state, sizes) = {
                        let mut guard = runtime.lock().unwrap_or_else(|e| e.into_inner());
                        if !guard.state_save_pending {
                            guard.state_save_active = false;
                            return;
                        }
                        guard.state_save_pending = false;
                        (
                            guard.state_path.clone(),
                            guard.snapshot.ui_state.clone(),
                            guard
                                .terminal_sizes
                                .iter()
                                .map(|(id, size)| (id.clone(), *size))
                                .collect(),
                        )
                    };
                    // The existing save function serializes and writes outside the
                    // runtime mutex. One pending flag coalesces newer UI state.
                    if let Err(message) = persistence::save(&path, &state, &sizes) {
                        runtime.lock().unwrap_or_else(|e| e.into_inner()).set_error(
                            "ui_state.save_failed",
                            message,
                            true,
                        );
                        context.notifier.notify();
                    }
                }
            }) {
            Ok(worker) => {
                self.state_save_worker = Some(worker);
                Ok(())
            }
            Err(error) => {
                self.state_save_active = false;
                Err(format!("UI state save worker could not start: {error}"))
            }
        }
    }

    fn persist_ui_state(&mut self) {
        if let Err(message) = self.write_ui_state() {
            self.set_error("ui_state.save_failed", message, true);
        }
    }

    /// Moves the keyboard focus to a pane and tells Herdr afterwards.
    ///
    /// Hide owns the focused pane, so the focus ring and the first responder
    /// move on this frame rather than on Herdr's confirming event. What Herdr
    /// still owns is which panes exist and how they are split; this only says
    /// which of them has the keyboard.
    ///
    /// For an operator focus the pane also becomes the one the read record
    /// follows. The record is raised here rather than when the resulting
    /// layout lands, so one click on a Done row clears that row inside the
    /// same dispatch. A focus that never reaches Herdr arms nothing.
    fn focus_pane(&mut self, pane_id: String, origin: PaneFocusOrigin) {
        let already_focused = self.snapshot.focused.pane_id.as_deref() == Some(pane_id.as_str());
        self.snapshot.terminal.pane_id = Some(pane_id.clone());
        self.snapshot.focused.surface = Surface::Terminal;
        self.snapshot.focused.pane_id = Some(pane_id.clone());
        // The persisted selection follows the ring. The shell echoes this
        // field back on every UI-state save, and a stale value there put the
        // keyboard back on the previous pane when the sidebar was toggled.
        self.snapshot.ui_state.selected_pane_id = Some(pane_id.clone());
        self.sync_focused_terminal_projection();
        // A pane in a tab the checkout is not showing brings its tab forward.
        self.align_visible_tab_with_selected_pane();
        let Some(context) = self.live.as_ref().cloned() else {
            self.set_error(
                "pane.control_unavailable",
                "Pane focus requires a live Herdr connection",
                true,
            );
            return;
        };
        // Rule 11: focusing the pane that already has the keyboard, with
        // nothing in flight, converges without a second notification. The
        // look itself still counts, so only the notification is skipped.
        // Settled means Herdr's own layout agrees too. A checkout coming
        // forward selects a pane locally without telling Herdr, and skipping
        // the notification then let Herdr's next layout take the focus back.
        let herdr_agrees = self
            .layout_holding_pane(&pane_id)
            .is_none_or(|layout| layout.focused_pane_id == pane_id);
        let notify = !already_focused
            || !herdr_agrees
            || !self.view_focus_settled_on(ViewFocusSlot::Pane, &pane_id);
        if notify {
            self.push_diagnostic("pane.focus.requested", format!("Focusing pane {pane_id}"));
            if let Err(message) = live::spawn_pane_control(
                context,
                PaneControlAction::Focus {
                    pane_id: pane_id.clone(),
                },
            ) {
                self.set_error("pane.focus_worker_failed", message, true);
                return;
            }
            // Latest request wins, so a second click while the first is
            // unconfirmed cannot be pulled back by Herdr's answer to the
            // first.
            self.pending_pane_focus = Some(PendingViewFocus::new(String::new(), pane_id.clone()));
        }
        if origin == PaneFocusOrigin::Restore {
            return;
        }
        self.operator_focused_pane_id = Some(pane_id);
        self.refresh_pane_read_state();
    }

    /// The layout of the tab that holds this pane. Every tab in the session
    /// has one, so this answers for a pane in any tab, not only the visible
    /// one.
    fn layout_holding_pane(&self, pane_id: &str) -> Option<&PaneLayoutSnapshot> {
        self.snapshot
            .pane_layouts
            .iter()
            .find(|layout| layout.pane_ids().contains(&pane_id))
    }

    /// The layout being drawn: the one holding the selected pane.
    fn active_pane_layout(&self) -> Option<&PaneLayoutSnapshot> {
        self.snapshot.active_pane_layout()
    }

    /// Replaces the session's layouts wholesale from one session projection.
    ///
    /// Herdr sends the whole session on every topology update, so a
    /// `layout_updated` for one tab arrives as a payload in which only that
    /// tab's entry differs; comparing the projected vector is what keeps the
    /// other tabs' entries and the revision they ride untouched.
    fn store_pane_layouts(&mut self, mut layouts: Vec<PaneLayoutSnapshot>) -> bool {
        // Sorted by tab id, because Herdr's own order for the layouts array
        // carries no meaning - the tab list is what orders tabs - and a
        // reshuffle of it would otherwise restamp the revisioned section and
        // resend the whole navigator with it.
        layouts.sort_by(|left, right| left.tab_id.cmp(&right.tab_id));
        if self.snapshot.pane_layouts == layouts {
            return false;
        }
        self.snapshot.pane_layouts = layouts;
        true
    }

    fn apply_pane_layout(&mut self, layout: PaneLayoutSnapshot) -> bool {
        let pane_ids = layout
            .pane_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let layout_changed = self
            .snapshot
            .pane_layouts
            .iter()
            .find(|stored| stored.tab_id == layout.tab_id)
            != Some(&layout);
        // The rendered projection is compared on its own, because an
        // unchanged layout no longer implies an unchanged projection. Layouts
        // now survive a tab switch, so this runs for a tab whose geometry
        // Herdr never altered while the panes it puts on the canvas still
        // change. Returning the layout comparison alone would then withhold
        // the notification for a canvas that did change.
        let previous_pane_ids = self
            .snapshot
            .terminal
            .panes
            .iter()
            .map(|pane| pane.pane_id.clone())
            .collect::<Vec<_>>();
        let previous_selected = self.snapshot.terminal.pane_id.clone();
        let previous_zoomed = self.snapshot.zoomed.clone();

        // A pane a visited tab left behind keeps its projection entry. The
        // terminal view the shell holds open for that tab reads its transport
        // state from here, and dropping the entry on every switch is what
        // dropped the attach with it and made the tab come back empty. Only a
        // pane that has left the session goes, and the session is the union
        // of every tab's layout.
        let arriving_tab_id = layout.tab_id.clone();
        match self
            .snapshot
            .pane_layouts
            .iter_mut()
            .find(|stored| stored.tab_id == arriving_tab_id)
        {
            Some(stored) => *stored = layout.clone(),
            None => {
                self.snapshot.pane_layouts.push(layout.clone());
                self.snapshot
                    .pane_layouts
                    .sort_by(|left, right| left.tab_id.cmp(&right.tab_id));
            }
        }
        let session_pane_ids = self
            .snapshot
            .pane_layouts
            .iter()
            .flat_map(|stored| stored.pane_ids())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        self.snapshot.terminal.panes.retain(|pane| {
            pane.pane_id.starts_with("remote:") || session_pane_ids.contains(&pane.pane_id)
        });
        for pane_id in &pane_ids {
            self.ensure_terminal_pane(pane_id);
        }

        // Hide owns the focused pane. An arriving layout confirms the focus
        // Hide notified Herdr about, or - with nothing in flight - it is a
        // focus made outside Hide and Hide follows it and says so. While a
        // notification is unconfirmed the layout's geometry is taken and its
        // focus is not, so the operator's click is not undone by the frame
        // that was already on its way.
        let previous_focus = self.snapshot.focused.pane_id.clone();
        let pending_pane = self
            .pending_pane_focus
            .as_ref()
            .map(|pending| pending.target_id.clone());
        let arriving_confirms_pending_tab = self
            .pending_tab_focus
            .as_ref()
            .is_some_and(|pending| pending.target_id == arriving_tab_id);
        let adopt_focus = match pending_pane.as_deref() {
            Some(pending) if pending == layout.focused_pane_id => {
                self.pending_pane_focus = None;
                true
            }
            Some(_) => false,
            None => true,
        };
        if adopt_focus {
            if previous_focus.as_deref() != Some(layout.focused_pane_id.as_str())
                && pending_pane.is_none()
                && !arriving_confirms_pending_tab
            {
                self.report_followed_pane_focus(previous_focus.as_deref(), &layout.focused_pane_id);
            }
            self.snapshot.terminal.pane_id = Some(layout.focused_pane_id.clone());
            self.snapshot.focused.pane_id = Some(layout.focused_pane_id.clone());
            self.release_operator_focus_if_moved(&previous_focus, &layout.focused_pane_id);
            // Zoom is Herdr's, and its subject is the focused pane. While
            // Hide keeps a focus Herdr has not confirmed, taking the zoom
            // would hide the pane the operator just clicked behind another.
            self.snapshot.zoomed = layout.zoomed.then(|| layout.focused_pane_id.clone());
        }
        let mut notice_cleared = false;
        if self
            .snapshot
            .status
            .last_error
            .as_ref()
            .is_some_and(|error| error.kind == "pane.projection_unavailable")
        {
            self.snapshot.status.last_error = None;
            notice_cleared = true;
        }
        self.sync_focused_terminal_projection();

        if self.live.is_some() {
            for pane_id in pane_ids {
                self.request_terminal_control(&pane_id);
            }
        }
        let projection_changed = notice_cleared
            || previous_selected != self.snapshot.terminal.pane_id
            || previous_focus != self.snapshot.focused.pane_id
            || previous_zoomed != self.snapshot.zoomed
            || previous_pane_ids
                != self
                    .snapshot
                    .terminal
                    .panes
                    .iter()
                    .map(|pane| pane.pane_id.clone())
                    .collect::<Vec<_>>();
        layout_changed || projection_changed
    }

    /// Ends the wait on a view-state focus Herdr refused, keeping the value
    /// Hide chose and reporting the refusal.
    ///
    /// A refusal that names some other target is not this wait's answer and
    /// is left alone, so a late refusal for a tab the operator has already
    /// moved on from cannot end the wait on the current one.
    fn clear_refused_view_focus(&mut self, slot: ViewFocusSlot, target_id: &str, message: &str) {
        if self
            .pending_view_focus(slot)
            .as_ref()
            .is_none_or(|pending| pending.target_id != target_id)
        {
            return;
        }
        *self.pending_view_focus_mut(slot) = None;
        let what = slot.what();
        crate::diagnostic!(serde_json::json!({
            "component": "view_state",
            "kind": slot.refused_kind(),
            slot.id_key(): target_id,
            "message": message,
        }));
        self.push_diagnostic(
            slot.refused_kind(),
            format!(
                "Herdr refused {what} focus {target_id}: {message}; {}",
                slot.kept_phrase()
            ),
        );
    }

    /// Rule 11: whether repeating this view-state change would converge on
    /// what the core already holds, so Herdr needs no second notification.
    /// True when nothing is in flight for the slot, or what is in flight is
    /// this very target.
    fn view_focus_settled_on(&self, slot: ViewFocusSlot, target_id: &str) -> bool {
        self.pending_view_focus(slot)
            .as_ref()
            .is_none_or(|pending| pending.target_id == target_id)
    }

    fn pending_view_focus(&self, slot: ViewFocusSlot) -> &Option<PendingViewFocus> {
        match slot {
            ViewFocusSlot::Tab => &self.pending_tab_focus,
            ViewFocusSlot::Pane => &self.pending_pane_focus,
        }
    }

    fn pending_view_focus_mut(&mut self, slot: ViewFocusSlot) -> &mut Option<PendingViewFocus> {
        match slot {
            ViewFocusSlot::Tab => &mut self.pending_tab_focus,
            ViewFocusSlot::Pane => &mut self.pending_pane_focus,
        }
    }

    /// Reports that Hide moved its keyboard focus to follow a pane focus made
    /// outside it.
    ///
    /// Rule 9: the record names the panes and where the change came from, and
    /// carries nothing about what is in them.
    fn report_followed_pane_focus(&mut self, previous: Option<&str>, arriving: &str) {
        let from = previous.unwrap_or("<none>").to_owned();
        crate::diagnostic!(serde_json::json!({
            "component": "view_state",
            "kind": "pane.focus.followed",
            "from_pane_id": from,
            "to_pane_id": arriving,
            "origin": "herdr",
        }));
        self.push_diagnostic(
            "pane.focus.followed",
            format!("Herdr focused pane {arriving}; Hide was on {from}"),
        );
    }

    /// Drops the operator focus once Herdr moves focus off the pane the
    /// operator chose.
    ///
    /// The pane has to have been focused before it can be moved away from.
    /// Requiring that is what lets a requested focus survive the stale layout
    /// a tab brings forward on its way: bringing a checkout forward to reach
    /// its pane makes Herdr report that tab's remembered pane first, and
    /// clearing on that would leave the clicked row unread.
    fn release_operator_focus_if_moved(&mut self, previous: &Option<String>, arriving: &str) {
        let Some(operator) = self.operator_focused_pane_id.as_deref() else {
            return;
        };
        if arriving == operator || previous.as_deref() != Some(operator) {
            return;
        }
        self.push_diagnostic(
            "pane.read_focus.released",
            format!("Herdr moved focus from {operator} to {arriving}"),
        );
        self.operator_focused_pane_id = None;
    }

    /// Re-runs the read axis over the agents already in the snapshot, for the
    /// moment the operator picks a pane without a new agent list arriving.
    fn refresh_pane_read_state(&mut self) -> bool {
        let before = self.snapshot.navigator.agents.clone();
        let mut agents = std::mem::take(&mut self.snapshot.navigator.agents);
        self.apply_pane_read_state(&mut agents, ReadRecordScope::Retain);
        let changed = before != agents;
        self.snapshot.navigator.agents = agents;
        changed
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
            transport_last_attempt_at_unix_ms: self
                .terminal_recovery
                .get(pane_id)
                .and_then(|r| r.last_attempt_at_unix_ms),
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
            pane.transport_last_attempt_at_unix_ms = self
                .terminal_recovery
                .get(pane_id)
                .and_then(|r| r.last_attempt_at_unix_ms);
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
            if let Some(said) = self.scroll_withheld_for_missing_size(pane_id) {
                return said || changed;
            }
            let lines = i32::from(lines) * if direction == "up" { 1 } else { -1 };
            if let Some(session) = self.terminal_sessions.get_mut(pane_id)
                && session.mode == TerminalSessionMode::Control
                && let Err(message) = session.scroll(live::ScrollRequest {
                    lines,
                    ..Default::default()
                })
            {
                self.set_error("terminal.scroll_failed", message, true);
                return true;
            }
        }
        changed
    }

    /// Whether a scroll must be withheld because the pane reported no size,
    /// and whether saying so changed the snapshot.
    ///
    /// `None` means the pane can be scrolled. The attach is held back until
    /// the same size arrives, so a pane without one has nothing to write to,
    /// and the guessed 24x80 that used to stand in only ever resized the PTY
    /// to a grid it was not running at. Every scroll producer goes through
    /// here so the wait is said once per pane rather than once per producer.
    fn scroll_withheld_for_missing_size(&mut self, pane_id: &str) -> Option<bool> {
        if self.terminal_sizes.contains_key(pane_id) {
            return None;
        }
        if self.panes_scrolled_before_size.insert(pane_id.to_owned()) {
            self.push_diagnostic(
                "terminal.scroll_deferred",
                format!(
                    "Pane {pane_id} was scrolled before its view reported a size; nothing was sent"
                ),
            );
            return Some(true);
        }
        Some(false)
    }

    pub fn ingest_fork_result(
        &mut self,
        parent_pane_id: &str,
        result: Result<String, String>,
        elapsed_ms: u128,
    ) -> bool {
        self.forks_in_flight.remove(parent_pane_id);
        // Either outcome leaves the process, because a fork that produced no
        // pane and no message is the report the operator brought: a modal
        // appeared and nothing else happened.
        match result {
            Ok(forked_pane_id) => {
                self.push_diagnostic(
                    "pane.fork.created",
                    format!("Forked pane {parent_pane_id} into {forked_pane_id} in {elapsed_ms}ms"),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_fork",
                    "kind": "pane.fork.created",
                    "pane_id": parent_pane_id,
                    "forked_pane_id": forked_pane_id,
                    "duration_ms": elapsed_ms,
                }));
                true
            }
            Err(message) => {
                self.set_error(
                    "pane.fork_failed",
                    format!("Pane {parent_pane_id} could not be forked: {message}"),
                    true,
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_fork",
                    "kind": "pane.fork_failed",
                    "pane_id": parent_pane_id,
                    "message": message,
                    "duration_ms": elapsed_ms,
                }));
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
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_projection",
                    "kind": "pane.projection_ready",
                    "pane_id": pane_id,
                    "duration_ms": elapsed_ms,
                }));
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
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_control",
                    "kind": "pane.split_ready",
                    "pane_id": pane_id,
                    "created_pane_id": created_pane_id,
                    "direction": direction.as_str(),
                    "duration_ms": elapsed_ms,
                }));
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
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_control",
                    "kind": "pane.zoom_ready",
                    "pane_id": pane_id,
                    "duration_ms": elapsed_ms,
                }));
                true
            }
            (PaneControlAction::Close { pane_id }, Ok(PaneControlOutcome::Acknowledged { .. })) => {
                self.push_diagnostic(
                    "pane.close",
                    format!(
                        "Pane {pane_id} close acknowledged in {elapsed_ms} ms; awaiting authoritative event"
                    ),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_control",
                    "kind": "pane.close_ready",
                    "pane_id": pane_id,
                    "duration_ms": elapsed_ms,
                }));
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
            (PaneControlAction::Focus { pane_id }, Err(message)) => {
                // Hide keeps the pane it focused. The refusal is reported and
                // the wait ends, so the next Herdr event naming another pane
                // is read as the authority it is rather than as a late answer.
                self.clear_refused_view_focus(ViewFocusSlot::Pane, &pane_id, &message);
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
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_control",
                    "kind": "remote.control.ready",
                    "target": target_id,
                    "request_id": request_id,
                    "action": action_kind,
                    "created_tab_id": created_tab_id,
                    "created_pane_id": created_pane_id,
                    "duration_ms": elapsed_ms,
                }));
            }
            // A remote target owns its tab order; `spawn_remote_control`
            // refuses the only action that reports one back.
            Ok(RemoteControlOutcome::TabsOrdered { .. }) => {
                self.set_error(
                    "remote.control.failed",
                    format!("{action_kind} for {target_id} returned a tab order remotely"),
                    false,
                );
            }
            Err(message) => {
                self.set_error(
                    "remote.control.failed",
                    format!("{action_kind} for {target_id} failed: {message}"),
                    true,
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_control",
                    "kind": "remote.control.failed",
                    "target": target_id,
                    "request_id": request_id,
                    "action": action_kind,
                    "message": message,
                    "duration_ms": elapsed_ms,
                }));
            }
        }
        true
    }

    /// Opens a terminal tab in Scratch.
    ///
    /// Nothing about it waits for a project: the folder and the workspace are
    /// both created on demand by the worker, so the first `⌘T` in Scratch
    /// works with no earlier setup.
    fn create_scratch_tab(&mut self, label: &str) -> bool {
        if label.is_empty() {
            self.set_error("tab.invalid_label", "Tab label cannot be empty", false);
            return true;
        }
        let Some(context) = self.live.as_ref().cloned() else {
            self.set_error(
                "tab.control_unavailable",
                "Tab creation requires a live Herdr connection",
                true,
            );
            return true;
        };
        let request = live::ScratchTabRequest {
            root: self.scratch_root.clone(),
            workspace_id: self
                .snapshot
                .navigator
                .scratch
                .session_workspace_ids
                .first()
                .cloned(),
            label: label.to_owned(),
        };
        self.push_diagnostic(
            "scratch.tab.requested",
            format!(
                "Creating a Scratch tab in {}",
                request
                    .workspace_id
                    .as_deref()
                    .unwrap_or("a new Herdr workspace")
            ),
        );
        if let Err(message) = live::spawn_scratch_tab_creation(context, request) {
            self.set_error("scratch.tab_worker_failed", message, true);
        }
        true
    }

    /// What the Scratch tab worker found.
    ///
    /// A failure carries the step that failed, because "the folder could not
    /// be created" and "Herdr refused the tab" are different problems with
    /// different fixes and one message for both would hide which happened.
    pub fn ingest_scratch_tab_result(
        &mut self,
        result: Result<String, String>,
        elapsed_ms: u128,
    ) -> bool {
        match result {
            Ok(pane_id) => {
                self.snapshot.terminal.pane_id = Some(pane_id.clone());
                self.snapshot.focused.surface = Surface::Terminal;
                self.snapshot.focused.pane_id = Some(pane_id.clone());
                self.snapshot.ui_state.selected_pane_id = Some(pane_id.clone());
                self.deactivate_editor_tab();
                self.persist_current_ui_state();
                self.push_diagnostic(
                    "scratch.tab.ready",
                    format!("Scratch tab created in {elapsed_ms} ms as pane {pane_id}"),
                );
            }
            Err(message) => {
                self.set_error(
                    "scratch.tab.failed",
                    format!("Scratch tab failed: {message}"),
                    true,
                );
            }
        }
        true
    }

    fn begin_task_operation(
        &mut self,
        kind: &str,
        repository_root: Option<String>,
        branch: Option<String>,
        base_branch: Option<String>,
        agent_kind: Option<String>,
    ) -> Result<u64, String> {
        if self
            .snapshot
            .task_operation
            .as_ref()
            .is_some_and(|operation| operation.phase == "working")
        {
            return Err("Another task operation is still running".into());
        }
        self.next_task_operation_id = self.next_task_operation_id.wrapping_add(1).max(1);
        let id = self.next_task_operation_id;
        self.snapshot.task_operation = Some(crate::model::TaskOperationSnapshot {
            id,
            kind: kind.to_owned(),
            phase: "working".into(),
            repository_root,
            branch,
            base_branch,
            path: None,
            pane_id: None,
            agent_kind,
            message: None,
        });
        Ok(id)
    }

    fn create_scratch_chat_tab(&mut self, label: String) -> bool {
        let label = label.trim();
        if label.is_empty() {
            self.set_error(
                "scratch_chat.invalid_label",
                "Tab label cannot be empty",
                false,
            );
            return true;
        }
        let id = match self.begin_task_operation("scratch_chat_tab", None, None, None, None) {
            Ok(id) => id,
            Err(message) => {
                self.set_error("task_operation.busy", message, true);
                return true;
            }
        };
        let Some(context) = self.live.as_ref().cloned() else {
            return self.ingest_task_operation_result(
                id,
                Err("create tab: a live Herdr connection is required".into()),
            );
        };
        let request = live::ScratchTabRequest {
            root: self.scratch_root.clone(),
            workspace_id: self
                .snapshot
                .navigator
                .scratch
                .session_workspace_ids
                .first()
                .cloned(),
            label: label.to_owned(),
        };
        if let Err(message) = live::spawn_scratch_chat_tab_creation(context, id, request) {
            return self.ingest_task_operation_result(id, Err(message));
        }
        true
    }

    fn create_project_worktree(&mut self, payload: CreateWorktreePayload) -> bool {
        let branch = payload.branch.trim().to_owned();
        if branch.is_empty() {
            self.set_error(
                "worktree.create_invalid_branch",
                "Branch is required",
                false,
            );
            return true;
        }
        let has_branch = self
            .worktree_catalog
            .project(&payload.repository_root)
            .is_some_and(|project| {
                project
                    .worktrees
                    .iter()
                    .any(|row| row.branch.is_some() && row.head_sha.is_some())
            });
        if !has_branch {
            self.set_error(
                "worktree.create_without_branches",
                "Create the repository's first branch before creating a worktree",
                true,
            );
            return true;
        }
        let id = match self.begin_task_operation(
            "worktree_create",
            Some(payload.repository_root.clone()),
            Some(branch.clone()),
            payload.base_branch.clone(),
            payload.agent_kind.clone(),
        ) {
            Ok(id) => id,
            Err(message) => {
                self.set_error("task_operation.busy", message, true);
                return true;
            }
        };
        let request = live::WorktreeTaskRequest {
            id,
            repository_root: payload.repository_root,
            branch,
            base_branch: payload.base_branch,
            agent_kind: payload.agent_kind,
            focus: true,
        };
        let Some(context) = self.live.as_ref().cloned() else {
            return self.ingest_task_operation_result(
                id,
                Err("create worktree: a live Herdr connection is required".into()),
            );
        };
        if let Err(message) = live::spawn_worktree_create(context, request) {
            return self.ingest_task_operation_result(id, Err(message));
        }
        true
    }

    fn migrate_main_branch(&mut self, payload: MigrateMainBranchPayload) -> bool {
        let Some(project) = self.worktree_catalog.project(&payload.repository_root) else {
            self.set_error(
                "branch_migrate.unknown_project",
                "Repository status is unavailable",
                true,
            );
            return true;
        };
        let Some(main) = project.worktrees.iter().find(|row| row.is_main) else {
            self.set_error(
                "branch_migrate.missing_main",
                "The main worktree is unavailable",
                true,
            );
            return true;
        };
        if main.dirty {
            self.set_error(
                "branch_migrate.dirty",
                "Commit or discard uncommitted changes before moving the branch",
                true,
            );
            return true;
        }
        let branch = main.branch.clone();
        let id = match self.begin_task_operation(
            "branch_migrate",
            Some(payload.repository_root.clone()),
            branch.clone(),
            Some(payload.base_branch.clone()),
            None,
        ) {
            Ok(id) => id,
            Err(message) => {
                self.set_error("task_operation.busy", message, true);
                return true;
            }
        };
        let request = live::WorktreeTaskRequest {
            id,
            repository_root: payload.repository_root,
            branch: branch.unwrap_or_default(),
            base_branch: Some(payload.base_branch),
            agent_kind: None,
            focus: false,
        };
        let Some(context) = self.live.as_ref().cloned() else {
            return self.ingest_task_operation_result(
                id,
                Err("move branch: a live Herdr connection is required".into()),
            );
        };
        if let Err(message) = live::spawn_branch_migration(context, request) {
            return self.ingest_task_operation_result(id, Err(message));
        }
        true
    }

    pub fn ingest_task_operation_result(
        &mut self,
        id: u64,
        result: Result<live::WorktreeTaskOutcome, String>,
    ) -> bool {
        let Some(operation) = self.snapshot.task_operation.as_mut() else {
            return false;
        };
        if operation.id != id || operation.phase != "working" {
            return false;
        }
        let should_focus = operation.kind != "branch_migrate";
        match result {
            Ok(outcome) => {
                operation.phase = "ready".into();
                operation.path = Some(outcome.path);
                operation.pane_id = Some(outcome.pane_id.clone());
                if should_focus {
                    self.snapshot.terminal.pane_id = Some(outcome.pane_id.clone());
                    self.snapshot.focused.surface = Surface::Terminal;
                    self.snapshot.focused.pane_id = Some(outcome.pane_id.clone());
                    self.snapshot.ui_state.selected_pane_id = Some(outcome.pane_id);
                }
                self.refresh_worktrees();
            }
            Err(message) => {
                operation.phase = "failed".into();
                operation.message = Some(message);
                self.refresh_worktrees();
            }
        }
        true
    }

    fn acknowledge_task_operation(&mut self, id: u64) -> bool {
        if self
            .snapshot
            .task_operation
            .as_ref()
            .is_some_and(|operation| operation.id == id && operation.phase != "working")
        {
            self.snapshot.task_operation = None;
            true
        } else {
            false
        }
    }

    pub fn ingest_local_control_result(
        &mut self,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        let action_kind = action.kind();
        if let RemoteControlAction::MoveTab {
            checkout_id,
            tab_id,
            expected_order,
            generation,
            ..
        } = &action
        {
            return self.ingest_tab_move_result(
                checkout_id,
                tab_id,
                expected_order,
                *generation,
                result,
                elapsed_ms,
            );
        }
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
                    // The tab was created with focus, so it is Hide's visible
                    // tab from this acknowledgment and Herdr's `tab_focused`
                    // is its confirmation. Without this the strip's active
                    // mark stayed where it was until the operator clicked the
                    // new tab a second time.
                    if let (Some(tab_id), Some(checkout_id)) = (
                        created_tab_id.as_ref(),
                        self.snapshot.navigator.focused_checkout_id.clone(),
                    ) {
                        self.visible_tab_ids
                            .insert(checkout_id.clone(), tab_id.clone());
                        self.pending_tab_focus =
                            Some(PendingViewFocus::new(checkout_id, tab_id.clone()));
                    }
                    self.deactivate_editor_tab();
                    self.persist_current_ui_state();
                }
                self.push_diagnostic(
                    "tab.control.ready",
                    format!(
                        "{action_kind} acknowledged in {elapsed_ms} ms; awaiting authoritative Herdr event"
                    ),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "tab_control",
                    "kind": "tab.control.ready",
                    "action": action_kind,
                    "created_tab_id": created_tab_id,
                    "created_pane_id": created_pane_id,
                    "duration_ms": elapsed_ms,
                }));
            }
            Err(message) => {
                // Hide keeps the tab it made visible. The refusal is reported
                // and the wait ends, so the next Herdr event naming another
                // tab is read as an external focus rather than a late answer.
                if let RemoteControlAction::FocusTab { tab_id } = &action {
                    self.clear_refused_view_focus(ViewFocusSlot::Tab, tab_id, &message);
                }
                self.set_error(
                    "tab.control.failed",
                    format!("{action_kind} failed: {message}"),
                    true,
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "tab_control",
                    "kind": "tab.control.failed",
                    "action": action_kind,
                    "message": message,
                    "duration_ms": elapsed_ms,
                }));
            }
            // `tab.move` is the only action that reports a tab order and it
            // is handled above, before this match.
            Ok(RemoteControlOutcome::TabsOrdered { .. }) => {
                self.set_error(
                    "tab.control.failed",
                    format!("{action_kind} returned a tab order it was not asked for"),
                    false,
                );
            }
        }
        true
    }

    /// Reads what Herdr did with a requested tab move.
    ///
    /// Herdr answers with the workspace's tab list in its new order, so the
    /// answer says whether the move landed as asked without waiting for the
    /// event. An answer that matches leaves the arrangement held until the
    /// order reaches the navigator; anything else drops it and says so, so a
    /// refused or differently-placed move is never a strip that quietly
    /// stayed where it was.
    fn ingest_tab_move_result(
        &mut self,
        checkout_id: &str,
        tab_id: &str,
        expected_order: &[String],
        generation: u64,
        result: Result<RemoteControlOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        let outcome = match result {
            Ok(RemoteControlOutcome::TabsOrdered { tab_ids }) => Ok(tab_ids),
            Ok(RemoteControlOutcome::Acknowledged { .. }) => {
                Err("tab.move did not report the resulting tab order".to_owned())
            }
            Err(message) => Err(message),
        };
        match outcome {
            Ok(tab_ids) => {
                // The response lists the whole workspace, which can hold tabs
                // from sibling checkouts. Only the order of this checkout's
                // tabs was asked for, so only that is checked.
                let placed = tab_ids
                    .iter()
                    .filter(|candidate| expected_order.contains(candidate))
                    .cloned()
                    .collect::<Vec<_>>();
                if placed == expected_order {
                    self.push_diagnostic(
                        "tab.move.ready",
                        format!(
                            "Herdr placed tab {tab_id} as asked in {elapsed_ms} ms; awaiting the ordered event"
                        ),
                    );
                    crate::diagnostic!(serde_json::json!({
                        "component": "tab_control",
                        "kind": "tab.move.ready",
                        "checkout_id": checkout_id,
                        "tab_id": tab_id,
                        "order": placed,
                        "duration_ms": elapsed_ms,
                    }));
                    return true;
                }
                crate::diagnostic!(serde_json::json!({
                    "component": "tab_control",
                    "kind": "tab.move.diverged",
                    "checkout_id": checkout_id,
                    "tab_id": tab_id,
                    "requested": expected_order,
                    "placed": placed,
                    "duration_ms": elapsed_ms,
                }));
                self.abandon_tab_move(
                    checkout_id,
                    generation,
                    format!("Herdr put tab {tab_id} somewhere else; the strip follows Herdr"),
                );
                true
            }
            Err(message) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "tab_control",
                    "kind": "tab.move.failed",
                    "checkout_id": checkout_id,
                    "tab_id": tab_id,
                    "requested": expected_order,
                    "message": message,
                    "duration_ms": elapsed_ms,
                }));
                self.abandon_tab_move(
                    checkout_id,
                    generation,
                    format!("Herdr refused to move tab {tab_id}: {message}"),
                );
                true
            }
        }
    }

    /// Appends only the decoded frame bytes when the delivering official
    /// terminal session is still the current generation and mode.
    /// None retires the reader; Some(false) keeps reading a held frame without
    /// publishing it. Skipping a foreign grid must not terminate observation.
    pub fn ingest_terminal_session_frame(
        &mut self,
        pane_id: &str,
        generation: u64,
        mode: TerminalSessionMode,
        bytes: &[u8],
        frame: crate::model::TerminalFrame,
    ) -> Option<bool> {
        if self.terminal_session_generations.get(pane_id) != Some(&generation)
            || self
                .terminal_sessions
                .get(pane_id)
                .is_none_or(|session| session.mode != mode)
        {
            return None;
        }
        let expected = self
            .terminal_view_sizes
            .get(pane_id)
            .or_else(|| self.terminal_sizes.get(pane_id))
            .copied();
        if expected != Some((frame.height, frame.width)) {
            self.terminal_frames_need_full.insert(pane_id.to_owned());
            // Logged per held frame to the file and stderr sink only. A push
            // into the snapshot's diagnostics would restamp the revisioned
            // rest section on every frame of a mismatch burst.
            crate::diagnostic!(serde_json::json!({
                "component": "terminal", "kind": "terminal.frame_geometry_mismatch",
                "pane_id": pane_id, "frame": [frame.width, frame.height],
                "expected": expected.map(|(height, width)| [width, height]),
            }));
            return Some(false);
        }
        if self.terminal_frames_need_full.contains(pane_id) && !frame.full {
            return Some(false);
        }
        // Preserve the last valid view during a retry. Reset the parser only
        // as part of the replacement full frame, so no empty canvas is exposed.
        let reset_bytes = self
            .terminal_frames_need_full
            .remove(pane_id)
            .then(|| [b"\x1bc".as_slice(), bytes].concat());
        let bytes = reset_bytes.as_deref().unwrap_or(bytes);
        if mode == TerminalSessionMode::Control {
            if let Some(recovery) = self.terminal_recovery.remove(pane_id) {
                crate::diagnostic!(serde_json::json!({
                    "kind": "terminal.control_frame_ready", "pane_id": pane_id,
                    "generation": generation, "occurred_at": unix_milliseconds(),
                    "retries": recovery.retries,
                    "last_attempt_at_unix_ms": recovery.last_attempt_at_unix_ms,
                    "rows": frame.height, "cols": frame.width,
                }));
            }
            if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
                lifecycle.message = None;
                lifecycle.retry_decision = "none";
            }
            self.sync_transport_projection(pane_id);
        }
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(bytes));
        self.snapshot
            .terminal
            .chunks
            .last_mut()
            .expect("just appended frame")
            .frame = Some(frame);
        Some(true)
    }

    /// Handles a `terminal.closed` envelope or stdout EOF. An owner conflict
    /// falls back exactly once to Herdr's concurrent read-only observer; every
    /// other close ends only the transport, never the authoritative pane.
    /// Whether this pane is on its way out: Hide asked Herdr to close it, or
    /// Herdr has already stopped listing it in any tab's layout.
    fn pane_is_going_away(&self, pane_id: &str) -> bool {
        if self.panes_closing.contains(pane_id) {
            return true;
        }
        // An empty layout list is a session that has not arrived, not a pane
        // that left one.
        !self.snapshot.pane_layouts.is_empty() && self.layout_holding_pane(pane_id).is_none()
    }

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
            crate::diagnostic!(serde_json::json!({
                "component": "terminal_session",
                "kind": "terminal.control_owner_conflict",
                "pane_id": pane_id,
                "generation": generation,
                "attempt": attempt,
                "mode": mode.as_str(),
                "duration_ms": 0,
                "exit_category": category,
                "retry_decision": self.terminal_retry_decision(pane_id, "observe_once"),
            }));
            self.schedule_terminal_recovery(
                pane_id,
                "Another client owns terminal control; viewing read-only".to_owned(),
            );
            let retry_message = self.terminal_recovery.get(pane_id).map(|r| r.message());
            self.start_terminal_session(
                pane_id,
                TerminalSessionMode::Observe,
                attempt,
                "observe_once",
                retry_message,
            );
            return true;
        }

        // A pane that is going away, either because Hide asked or because
        // Herdr has already stopped reporting it, ends its transport as a
        // consequence of the close. It is not a failure, so nothing is drawn
        // over the pane's last frame and no notice is appended to it: the pane
        // keeps what it was showing until it is removed. Every other reason
        // still reports itself.
        if self.pane_is_going_away(pane_id) {
            self.panes_closing.remove(pane_id);
            self.terminal_session_lifecycles.insert(
                pane_id.to_owned(),
                TerminalSessionLifecycle {
                    state: "closing",
                    message: None,
                    generation,
                    attempt,
                    mode: Some(mode),
                    exit_category: Some(category.to_owned()),
                    retry_decision: "none",
                },
            );
            self.sync_transport_projection(pane_id);
            crate::diagnostic!(serde_json::json!({
                "component": "terminal_session",
                "kind": "terminal.session_closed_with_pane",
                "pane_id": pane_id,
                "generation": generation,
                "attempt": attempt,
                "mode": mode.as_str(),
                "duration_ms": 0,
                "exit_category": category,
                "retry_decision": "none",
            }));
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
        self.schedule_terminal_recovery(pane_id, message.clone());
        self.sync_transport_projection(pane_id);
        let notice = format!("\r\n[{message}]\r\n");
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(notice.as_bytes()));
        crate::diagnostic!(serde_json::json!({
            "component": "terminal_session",
            "kind": "terminal.session_ended",
            "pane_id": pane_id,
            "generation": generation,
            "attempt": attempt,
            "mode": mode.as_str(),
            "duration_ms": 0,
            "exit_category": category,
            "retry_decision": self.terminal_retry_decision(pane_id, "manual"),
        }));
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
        if !pane_id.starts_with("remote:") {
            self.terminal_sessions.remove(pane_id);
            if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
                lifecycle.state = "unavailable";
            }
            self.schedule_terminal_recovery(pane_id, message.clone());
            self.sync_transport_projection(pane_id);
        }
        crate::diagnostic!(serde_json::json!({
            "component": "terminal_session",
            "kind": "terminal.control_write_failed",
            "pane_id": pane_id,
            "generation": generation,
            "message": message,
            "retry_decision": self.terminal_retry_decision(pane_id, "manual"),
        }));
        true
    }

    /// Reports the workspaces whose Herdr-named active tab the navigator
    /// could not place. Reconcile runs once a second, so only a change in the
    /// set is worth a diagnostic; repeating it every tick would grow the
    /// status section without saying anything new.
    fn report_unresolved_active_tabs(&mut self, current: BTreeSet<String>) {
        let added = current
            .difference(&self.unresolved_active_tabs)
            .cloned()
            .collect::<Vec<_>>();
        for entry in &added {
            let (workspace_id, tab_id) = entry
                .split_once('/')
                .expect("unresolved active tab entries carry both ids");
            self.push_diagnostic(
                "tab.active_unresolved",
                format!(
                    "Herdr workspace {workspace_id} reports active tab {tab_id}, which is not in any checkout's tab list"
                ),
            );
        }
        self.unresolved_active_tabs = current;
    }

    fn push_diagnostic(&mut self, kind: impl Into<String>, message: impl Into<String>) {
        let diagnostic = DiagnosticSnapshot {
            kind: kind.into(),
            message: message.into(),
            occurred_at: unix_milliseconds(),
        };
        crate::diagnostic!(serde_json::json!({
            "kind": diagnostic.kind, "message": diagnostic.message, "occurred_at": diagnostic.occurred_at
        }));
        self.snapshot.status.diagnostics.push(diagnostic);
        // The list rides the revisioned rest section, so it is bounded: a
        // fault that repeats every few seconds otherwise grows what every
        // rest re-send carries for the life of the process.
        let excess = self
            .snapshot
            .status
            .diagnostics
            .len()
            .saturating_sub(DIAGNOSTIC_RETENTION);
        if excess > 0 {
            self.snapshot.status.diagnostics.drain(..excess);
        }
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
        let Some(project) = self.snapshot.navigator.workspaces.iter().find(|workspace| {
            workspace.remote_target_id.is_none()
                && workspace
                    .checkouts
                    .iter()
                    .flat_map(|checkout| checkout.tabs.iter())
                    .flat_map(|tab| tab.panes.iter())
                    .any(|pane| pane.id == pane_id)
        }) else {
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
        let registration =
            match workspace::registration(&project.path, &project.repo_name, &project.device_id) {
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
            let placed = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .find_map(|workspace| {
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

    fn refresh_agent_lineage(&mut self) {
        crate::sidebar::apply_lineage(
            &mut self.snapshot.navigator.agents,
            &self.snapshot.navigator.workspaces,
            &self.snapshot.ui_state.collapsed_agent_pane_ids,
        );
        for remote in &mut self.snapshot.status.remote {
            if let Some(session) = &mut remote.session {
                crate::sidebar::apply_lineage(
                    &mut session.agents,
                    &session.workspaces,
                    &self.snapshot.ui_state.collapsed_agent_pane_ids,
                );
            }
        }
    }

    fn rebuild_catalog(&mut self) {
        let mut workspaces = workspace::build_catalog(
            &self.snapshot.ui_state.workspace_registrations,
            &self.last_session_spaces,
            &self.worktree_catalog,
        );
        self.last_accepted_catalog = Some(workspaces.clone());
        self.catalog_roots = workspace::root_index(&self.last_session_spaces);
        Self::apply_workspace_expansion(
            &mut workspaces,
            &self.snapshot.ui_state.collapsed_workspace_ids,
        );
        crate::sidebar::sync_checkout_agent_summaries(
            &mut workspaces,
            &self.snapshot.navigator.agents,
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
        self.persist_ui_state();
    }

    /// Drops the rendered terminal state before selecting a pane in another
    /// checkout. Herdr's globally focused pane may belong to another
    /// workspace, so retaining the attach set here would let the next sync
    /// update redraw stale terminal content while the selected checkout has
    /// no pane yet.
    ///
    /// The layouts are not dropped. They describe every tab in the session,
    /// they are Herdr's and not this selection's, and emptying them to mark a
    /// selection in progress is what made the canvas pass through a blank
    /// frame on the way to the tab the operator asked for.
    fn clear_terminal_projection(&mut self) {
        self.snapshot.zoomed = None;
        self.snapshot.terminal.panes.clear();
        self.snapshot.terminal.closed = false;
        self.snapshot.terminal.exit_code = None;
    }

    /// Points the terminal projection at another pane without discarding
    /// anything Herdr has said.
    ///
    /// Zoom is re-read from that pane's own layout rather than carried over,
    /// so leaving a zoomed tab does not leak its zoom into the next one.
    fn select_terminal_pane(&mut self, pane_id: Option<String>) {
        let zoomed = pane_id
            .as_deref()
            .and_then(|pane_id| self.layout_holding_pane(pane_id))
            .and_then(|layout| layout.zoomed.then(|| layout.focused_pane_id.clone()));
        let pane_ids = pane_id
            .as_deref()
            .and_then(|pane_id| self.layout_holding_pane(pane_id))
            .map(|layout| {
                layout
                    .pane_ids()
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.snapshot.terminal.pane_id = pane_id.clone();
        self.snapshot.focused.surface = Surface::Terminal;
        self.snapshot.focused.pane_id = pane_id.clone();
        self.snapshot.ui_state.selected_pane_id = pane_id;
        self.snapshot.zoomed = zoomed;
        for pane_id in pane_ids {
            self.ensure_terminal_pane(&pane_id);
        }
        self.sync_focused_terminal_projection();
    }

    fn reset_terminal_projection(&mut self, pane_id: Option<String>) {
        self.clear_terminal_projection();
        self.select_terminal_pane(pane_id);
    }

    /// A launcher result is a local projection anchor, not a Herdr focus
    /// request. Keep it authoritative over an older terminal pane while the
    /// next event-stream projection catches up, and make the missing layout
    /// visible instead of retaining unrelated same-cwd content.
    fn apply_selected_pane_anchor(&mut self, pane_id: Option<String>) {
        let layout_contains_pane = pane_id
            .as_deref()
            .is_some_and(|selected_pane_id| self.layout_holding_pane(selected_pane_id).is_some());
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

    /// Opens or focuses the file tab for one path in a checkout Hide is
    /// already showing. The caller owns the context check and the persistence,
    /// because a reveal has already made that decision by the time it gets
    /// here and would otherwise make it twice.
    /// The tab the operator is looking at: the focused checkout's visible tab.
    ///
    /// Another checkout's visible tab is that checkout's memory, not a tab on
    /// screen, so it does not renew an attach.
    fn focused_visible_tab_id(&self) -> Option<String> {
        self.snapshot
            .navigator
            .focused_checkout_id
            .as_deref()
            .and_then(|checkout_id| self.visible_tab_ids.get(checkout_id))
            .cloned()
    }

    /// Records that a tab was on screen and releases whatever fell out of the
    /// window that leaves.
    fn track_visible_tab_attachments(&mut self) -> bool {
        let known_tabs = self
            .snapshot
            .pane_layouts
            .iter()
            .map(|layout| layout.tab_id.clone())
            .collect::<HashSet<_>>();
        // A tab Herdr no longer reports cannot come back, so holding its slot
        // would shrink the window for the tabs that can.
        self.recent_visible_tabs
            .retain(|tab_id| known_tabs.contains(tab_id));
        if let Some(tab_id) = self.focused_visible_tab_id()
            && self.recent_visible_tabs.first() != Some(&tab_id)
        {
            self.recent_visible_tabs.retain(|held| held != &tab_id);
            self.recent_visible_tabs.insert(0, tab_id);
        }
        self.recent_visible_tabs.truncate(ATTACHED_TAB_LIMIT);
        self.release_sessions_outside_attach_window()
    }

    /// Ends the terminal session of every pane whose tab has left the attach
    /// window.
    ///
    /// The pane keeps its projection entry, carrying `released`, because the
    /// sidebar and the pane header read their state from there and a missing
    /// entry would read as a failure rather than as a pane nobody is watching.
    /// The shell drops the canvas and the buffered bytes on that state, so the
    /// tab redraws from Herdr's own frame when it is next shown.
    fn release_sessions_outside_attach_window(&mut self) -> bool {
        let window = self
            .recent_visible_tabs
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let attached = self
            .snapshot
            .pane_layouts
            .iter()
            .filter(|layout| window.contains(&layout.tab_id))
            .flat_map(|layout| layout.pane_ids())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        // A pane no layout claims is not a pane that left the window; the
        // session reconcile owns those and drops them with the session.
        let placed = self
            .snapshot
            .pane_layouts
            .iter()
            .flat_map(|layout| layout.pane_ids())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let releasing = self
            .terminal_session_lifecycles
            .iter()
            .filter(|(_, lifecycle)| lifecycle.state != "released")
            .map(|(pane_id, _)| pane_id)
            .filter(|pane_id| !pane_id.starts_with("remote:"))
            .filter(|pane_id| placed.contains(*pane_id) && !attached.contains(*pane_id))
            .cloned()
            .collect::<Vec<_>>();
        if releasing.is_empty() {
            return false;
        }
        for pane_id in releasing {
            let _released_session = self.terminal_sessions.remove(&pane_id);
            self.panes_awaiting_size.remove(&pane_id);
            self.terminal_recovery.remove(&pane_id);
            let attempt = self
                .terminal_session_lifecycles
                .get(&pane_id)
                .map_or(0, |lifecycle| lifecycle.attempt);
            let generation = self
                .terminal_session_generations
                .get(&pane_id)
                .copied()
                .unwrap_or_default();
            self.terminal_session_lifecycles.insert(
                pane_id.clone(),
                TerminalSessionLifecycle {
                    state: "released",
                    message: Some(format!(
                        "Pane {pane_id} was detached after its tab left the last {ATTACHED_TAB_LIMIT} shown"
                    )),
                    generation,
                    attempt,
                    mode: None,
                    exit_category: None,
                    retry_decision: "on_next_visit",
                },
            );
            self.sync_transport_projection(&pane_id);
            self.push_diagnostic(
                "terminal.session_released",
                format!("Released the terminal session for pane {pane_id}"),
            );
            crate::diagnostic!(serde_json::json!({
                "component": "terminal_session",
                "kind": "terminal.session_released",
                "pane_id": pane_id,
                "generation": generation,
                "attempt": attempt,
                "retry_decision": "on_next_visit",
            }));
        }
        true
    }

    /// Reads what a file tab needs without putting anything on screen.
    ///
    /// The read is the only fallible part of opening a file, so it is done on
    /// its own: a caller that changes other state can then read first and
    /// change nothing when the file cannot be read.
    fn prepare_file_tab(
        &self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
    ) -> Result<PreparedFileTab, String> {
        if let Some(tab_id) = self.snapshot.editor.tabs.iter().find_map(|tab| {
            (tab.kind == EditorTabKind::File
                && tab.workspace_id == workspace_id
                && tab.checkout_id == checkout_id
                && tab.path == path)
                .then(|| tab.id.clone())
        }) {
            return Ok(PreparedFileTab::Open(tab_id));
        }
        files::open(Path::new(path)).map(|document| PreparedFileTab::Read {
            tab_id: Self::file_tab_id(workspace_id, checkout_id, path),
            document,
        })
    }

    /// Puts a prepared file tab on screen. Nothing here can fail on the file.
    fn show_file_tab(
        &mut self,
        prepared: PreparedFileTab,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
    ) {
        let tab_id = match prepared {
            PreparedFileTab::Open(tab_id) => tab_id,
            PreparedFileTab::Read { tab_id, document } => {
                self.editor_documents.insert(tab_id.clone(), document);
                self.snapshot.editor.tabs.push(EditorTabSnapshot {
                    id: tab_id.clone(),
                    workspace_id: workspace_id.to_owned(),
                    checkout_id: checkout_id.to_owned(),
                    path: path.to_owned(),
                    label: Path::new(path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .filter(|name| !name.is_empty())
                        .unwrap_or(path)
                        .to_owned(),
                    kind: EditorTabKind::File,
                    diff_committed: None,
                    markdown_preview: true,
                    wrap: false,
                    dirty: false,
                });
                // A new file tab takes a slot at the end of the strip.
                self.rebuild_tab_strips();
                tab_id
            }
        };
        if let Err(message) = self.activate_editor_tab(&tab_id) {
            self.set_error("file.focus_failed", message, false);
        }
        self.snapshot.ui_state.selected_path = Some(path.to_owned());
    }

    fn open_file_tab(&mut self, workspace_id: &str, checkout_id: &str, path: &str) {
        match self.prepare_file_tab(workspace_id, checkout_id, path) {
            Ok(prepared) => self.show_file_tab(prepared, workspace_id, checkout_id, path),
            Err(message) => self.set_error("file.open_failed", message, true),
        }
    }

    fn diff_tab_id(workspace_id: &str, checkout_id: &str, path: &str, committed: bool) -> String {
        let scope = if committed { "committed" } else { "working" };
        format!("diff:{workspace_id}:{checkout_id}:{scope}:{path}")
    }

    /// Opens one Changes row in the central editor strip.
    ///
    /// The right panel remains the list and the tab owns the reading surface,
    /// so closing the panel cannot make an open diff disappear.
    fn show_diff_tab(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        committed: bool,
    ) {
        let tab_id = Self::diff_tab_id(workspace_id, checkout_id, path, committed);
        if !self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
            let name = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or(path);
            let scope = if committed {
                "branch diff"
            } else {
                "working diff"
            };
            self.snapshot.editor.tabs.push(EditorTabSnapshot {
                id: tab_id.clone(),
                workspace_id: workspace_id.to_owned(),
                checkout_id: checkout_id.to_owned(),
                path: path.to_owned(),
                label: format!("{name} ({scope})"),
                kind: EditorTabKind::Diff,
                diff_committed: Some(committed),
                markdown_preview: true,
                wrap: false,
                dirty: false,
            });
            self.rebuild_tab_strips();
        }
        if let Err(message) = self.activate_editor_tab(&tab_id) {
            self.set_error("diff.focus_failed", message, false);
        }
    }

    /// Everything one clicked path changes on screen, decided in one event.
    ///
    /// The checkout comes forward, the right panel opens on Explorer, the tree
    /// expands every ancestor and selects the path, and a file also takes an
    /// editor tab. They are one event because the shell's dispatch is
    /// fire-and-forget: sent as separate events the operator would watch the
    /// checkout switch, then the panel appear, then the tree move, and a
    /// refusal partway would leave the screen in a state nobody asked for.
    fn reveal_path(&mut self, payload: RevealPathPayload) -> bool {
        let Some(checkout_path) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == payload.workspace_id)
            .and_then(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == payload.checkout_id)
                    .map(|checkout| checkout.path.clone())
            })
        else {
            self.set_error(
                "reveal.unknown_checkout",
                format!(
                    "Checkout {} is not registered, so {} was not revealed",
                    payload.checkout_id, payload.path
                ),
                false,
            );
            return true;
        };
        // The file is read before anything moves. Reading is the only part of
        // a reveal that can fail, and a reveal that settles the whole screen
        // at once must not leave the checkout focused and the tree expanded
        // around a document that never arrived.
        let prepared = if payload.is_directory {
            None
        } else {
            match self.prepare_file_tab(&payload.workspace_id, &payload.checkout_id, &payload.path)
            {
                Ok(prepared) => Some(prepared),
                Err(message) => {
                    self.set_error("file.open_failed", message, true);
                    return true;
                }
            }
        };
        if self.snapshot.navigator.focused_checkout_id.as_deref()
            != Some(payload.checkout_id.as_str())
        {
            self.focus_checkout(&payload.workspace_id, &payload.checkout_id);
        }
        self.snapshot.ui_state.right_panel_visible = true;
        self.snapshot.ui_state.right_panel_section = RightPanelSection::Explorer;
        for expanded in reveal_expansion_paths(&checkout_path, &payload.path, payload.is_directory)
        {
            if !self.snapshot.ui_state.expanded_paths.contains(&expanded) {
                self.snapshot.ui_state.expanded_paths.push(expanded);
            }
        }
        self.snapshot.ui_state.selected_path = Some(payload.path.clone());
        if let Some(prepared) = prepared {
            self.show_file_tab(
                prepared,
                &payload.workspace_id,
                &payload.checkout_id,
                &payload.path,
            );
        }
        self.push_diagnostic(
            "path.revealed",
            format!(
                "Revealed {} {} in checkout {}",
                if payload.is_directory {
                    "folder"
                } else {
                    "file"
                },
                payload.path,
                payload.checkout_id
            ),
        );
        self.persist_current_ui_state();
        true
    }

    fn focus_checkout(&mut self, workspace_id: &str, checkout_id: &str) -> bool {
        // The checkout comes forward on the tab it was showing, with the
        // keyboard on the pane the operator last had there. Its first pane
        // is only for a checkout Hide has never shown.
        let visible_tab_id = self.visible_tab_ids.get(checkout_id).cloned();
        let Some((checkout_path, first_pane_id, has_herdr_tab)) = self
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
                        let visible_tab = visible_tab_id.as_deref().and_then(|tab_id| {
                            checkout
                                .tabs
                                .iter()
                                .find(|tab| tab.id.as_deref() == Some(tab_id))
                        });
                        let first_pane_id = visible_tab
                            .and_then(|tab| tab.panes.first())
                            .or_else(|| {
                                checkout.tabs.iter().flat_map(|tab| tab.panes.iter()).next()
                            })
                            .map(|pane| pane.id.clone());
                        (
                            checkout.path.clone(),
                            first_pane_id,
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
        let next_pane_id = match visible_tab_id.as_deref() {
            Some(tab_id) => self.tab_focus_pane_id(tab_id, first_pane_id),
            None => first_pane_id,
        };
        self.snapshot.navigator.focused_workspace_id = Some(workspace_id.to_owned());
        self.snapshot.navigator.focused_checkout_id = Some(checkout_id.to_owned());
        self.snapshot.navigator.root_path = Some(checkout_path);
        // Selecting a checkout measures it, and selecting the one already
        // selected measures it again - that is the card's cheapest refresh
        // for a number that moves whenever a build runs (R8).
        self.remeasure_disk();
        self.reset_terminal_projection(next_pane_id.clone());
        self.sync_active_tab_projection();
        self.align_visible_tab_with_selected_pane();
        // The operator chose a checkout, not a pane: the record stops
        // following the pane just left, and nothing is raised for the pane
        // the checkout came forward on. A sidebar row click dispatches this
        // and then a pane focus, and raising the remembered pane here for
        // that one frame cleared a question nobody had read.
        self.operator_focused_pane_id = None;
        self.refresh_pane_read_state();
        self.deactivate_editor_tab();
        if !has_herdr_tab
            && let Some(file_tab_id) = self
                .snapshot
                .editor
                .tabs
                .iter()
                .rev()
                .find(|tab| tab.workspace_id == workspace_id && tab.checkout_id == checkout_id)
                .map(|tab| tab.id.clone())
            && let Err(message) = self.activate_editor_tab(&file_tab_id)
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
                    roots: self.catalog_roots.clone(),
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
        crate::diagnostic!(serde_json::json!({
            "component": "workspace",
            "kind": "workspace.registered",
            "path": outcome.registration.path,
            "duration_ms": elapsed_ms,
                "git_init": if git_init_failed { "failed" } else { "complete_or_skipped" },
        }));
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
        let changed = self.apply(event) || cleared_error;
        // Every event that can move the visible tab funnels through here, so
        // the attach window is maintained once rather than at each of the four
        // places a tab becomes visible.
        let released = self.track_visible_tab_attachments();
        changed || released
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
                    self.write_terminal_control(
                        &payload.pane_id,
                        &payload.bytes_base64,
                        payload.input_trace,
                    );
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
                self.focus_pane(payload.pane_id, payload.origin);
                true
            }
            ValidatedEvent::ReconnectPane(payload) => {
                self.terminal_recovery.remove(&payload.pane_id);
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
                // Scratch is not a project, so it is answered before the
                // project lookup rather than by pretending to be one.
                if payload.workspace_id == crate::scratch::NODE_ID {
                    return self.create_scratch_tab(payload.label.trim());
                }
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
                // The new tab goes next to the tab the operator is looking
                // at, in that tab's Herdr workspace: a checkout can hold tabs
                // from several. Herdr closes a workspace with its last pane,
                // so a project can be listed with no Herdr workspace behind
                // it. A tab needs one; the shell starts a terminal (which
                // creates the workspace) for a checkout with no panes instead.
                let visible_tab_workspace_id = self
                    .visible_tab_ids
                    .get(&checkout_id)
                    .and_then(|tab_id| {
                        self.snapshot
                            .pane_layouts
                            .iter()
                            .find(|layout| &layout.tab_id == tab_id)
                    })
                    .map(|layout| layout.workspace_id.clone())
                    .filter(|id| workspace_snapshot.session_workspace_ids.contains(id));
                let Some(session_workspace_id) = visible_tab_workspace_id
                    .or_else(|| workspace_snapshot.session_workspace_ids.first().cloned())
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
                self.deactivate_editor_tab();
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
                let checkout_path = checkout.path.clone();
                // Hide owns the visible tab, so the active mark moves here,
                // on the frame the operator asked for it, and Herdr is told
                // afterwards. Focusing a tab still does not reorder the
                // strip: Herdr owns the order, and moving the tab here made
                // every switch look like a reorder until the next catalog
                // rebuild undid it.
                let already_visible = checkout.active_tab_id.as_deref() == Some(&payload.tab_id);
                checkout.active_tab_id = Some(payload.tab_id.clone());
                let first_pane_id = checkout.tabs[index]
                    .panes
                    .first()
                    .map(|pane| pane.id.clone());
                // Return to the pane the operator last had in that tab. Its
                // layout is already here, so the tab's own focused pane is
                // known without asking Herdr for it.
                let next_pane_id = self.tab_focus_pane_id(&payload.tab_id, first_pane_id);
                self.snapshot.navigator.focused_workspace_id = Some(payload.workspace_id);
                self.snapshot.navigator.focused_checkout_id = Some(payload.checkout_id.clone());
                self.snapshot.navigator.root_path = Some(checkout_path);
                self.visible_tab_ids
                    .insert(payload.checkout_id.clone(), payload.tab_id.clone());
                // Nothing is cleared here. The tab being selected already has
                // its layout in the snapshot, so the canvas draws it on this
                // frame instead of showing an empty canvas until Herdr
                // answers.
                self.select_terminal_pane(next_pane_id.clone());
                self.sync_active_tab_projection();
                // The operator chose this tab, so its pane now holds Hide's
                // keyboard and the read record follows it. Leaving the record
                // on the pane of the tab just left kept marking that pane read
                // while nobody was looking at it.
                self.operator_focused_pane_id = next_pane_id;
                self.refresh_pane_read_state();
                self.deactivate_editor_tab();
                self.persist_current_ui_state();
                // A first visit attaches the tab's panes now. Waiting for the
                // next session update to do it left the canvas empty until
                // Herdr happened to emit something, up to the catalog window.
                if self.live.is_some() {
                    let pane_ids = self
                        .snapshot
                        .pane_layouts
                        .iter()
                        .find(|layout| layout.tab_id == payload.tab_id)
                        .map(|layout| {
                            layout
                                .pane_ids()
                                .into_iter()
                                .map(str::to_owned)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for pane_id in pane_ids {
                        self.request_terminal_control(&pane_id);
                    }
                }
                // Rule 11: reaching for the tab already showing, with nothing
                // in flight, converges on the state it is already in and
                // sends Herdr no second notification.
                if already_visible
                    && self.view_focus_settled_on(ViewFocusSlot::Tab, &payload.tab_id)
                {
                    return true;
                }
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
                        tab_id: payload.tab_id.clone(),
                    },
                ) {
                    self.set_error("tab.focus_worker_failed", message, true);
                    return true;
                }
                // Latest request wins. A second switch while the first is
                // unconfirmed replaces it, so Herdr's answer to the first
                // cannot pull the canvas back off the tab the operator is
                // now on.
                self.pending_tab_focus =
                    Some(PendingViewFocus::new(payload.checkout_id, payload.tab_id));
                true
            }
            ValidatedEvent::ReorderTab(payload) => self.reorder_tab(payload),
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
                self.deactivate_editor_tab();
                self.reconcile_remote_terminal_selection();
                self.persist_current_ui_state();
                true
            }
            ValidatedEvent::RemoveWorkspace(payload) => {
                // Removing a registration never closes Herdr workspaces. An
                // occupied project would immediately return through discovery.
                if self.snapshot.navigator.workspaces.iter().any(|workspace| {
                    workspace.id == payload.workspace_id
                        && (!workspace.session_workspace_ids.is_empty()
                            || workspace
                                .checkouts
                                .iter()
                                .any(|checkout| checkout.has_panes))
                }) {
                    self.set_error(
                        "workspace.registration_in_use",
                        "This project is still open in Herdr. Close or move its panes and workspaces, then remove the registration again. Files and worktrees are kept.",
                        true,
                    );
                    crate::diagnostic!(serde_json::json!({
                        "component": "registration", "kind": "remove.in_use",
                        "workspace_id": payload.workspace_id,
                    }));
                    return true;
                }
                let before = self.snapshot.ui_state.workspace_registrations.len();
                self.snapshot
                    .ui_state
                    .workspace_registrations
                    .retain(|registration| registration.id != payload.workspace_id);
                if before == self.snapshot.ui_state.workspace_registrations.len() {
                    return false;
                }
                // Retire the accepted projection directly. Rebuilding the
                // filesystem catalog here ran git while holding the mutex.
                self.snapshot
                    .navigator
                    .workspaces
                    .retain(|workspace| workspace.id != payload.workspace_id);
                if let Some(catalog) = &mut self.last_accepted_catalog {
                    catalog.retain(|workspace| workspace.id != payload.workspace_id);
                }
                self.snapshot
                    .ui_state
                    .collapsed_workspace_ids
                    .retain(|id| id != &payload.workspace_id);
                self.resync_navigator_focus();
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
                    pane_ids.contains(agent.pane_id.as_str()) && agent.requires_close_confirmation
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
                    agent.pane_id == payload.pane_id && agent.requires_close_confirmation
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
                self.panes_closing.insert(pane_id.clone());
                self.push_diagnostic("pane.close.requested", format!("Closing pane {pane_id}"));
                if let Err(message) =
                    live::spawn_pane_control(context, PaneControlAction::Close { pane_id })
                {
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
                // unique. It is used as the idempotency key unchanged: a
                // `hide-` prefix on the key used to make a second string that
                // nothing kept inside Herdr's name rule, and the key is what
                // the CLI puts in its request id, so a key that broke the rule
                // was the value the operator saw refused.
                let name = fork_name(
                    &pane_id,
                    &format!("{}-{}", self.fork_sequence, unix_milliseconds()),
                );
                let request = ForkRequest {
                    parent_pane_id: pane_id.clone(),
                    agent: agent_kind,
                    session_id,
                    cwd,
                    idempotency_key: name.clone(),
                    name,
                };
                self.push_diagnostic("pane.fork.requested", format!("Forking pane {pane_id}"));
                if let Err(message) = live::spawn_agent_fork(context, request) {
                    // A worker that never started is a fork that failed, and
                    // the operator is owed the same notice and the same
                    // stderr diagnostic either way.
                    self.ingest_fork_result(&pane_id, Err(message), 0);
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
                self.open_file_tab(&payload.workspace_id, &payload.checkout_id, &payload.path);
                self.persist_current_ui_state();
                true
            }
            ValidatedEvent::RevealPath(payload) => self.reveal_path(payload),
            ValidatedEvent::FileFocus(payload) => {
                let Some(tab) = self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .find(|tab| tab.id == payload.tab_id)
                    .cloned()
                else {
                    self.set_error(
                        "file.focus_failed",
                        format!("File tab {} is not open", payload.tab_id),
                        false,
                    );
                    return true;
                };
                let Some(checkout) = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == tab.workspace_id)
                    .and_then(|workspace| {
                        workspace
                            .checkouts
                            .iter()
                            .find(|checkout| checkout.id == tab.checkout_id)
                    })
                    .cloned()
                else {
                    self.set_error("editor.invalid_context", "The editor tab's project or checkout is no longer available. Selection was kept.", false);
                    return true;
                };
                // Validate the document/diff before moving any selection. One
                // event restores the surface and its context, including MRU
                // visits across checkouts; no intermediate terminal frame.
                if let Err(message) = self.activate_editor_tab(&payload.tab_id) {
                    self.set_error("editor.focus_failed", message, false);
                    return true;
                }
                let pane_id = checkout.active_tab_id.as_deref().and_then(|id| {
                    let first = checkout
                        .tabs
                        .iter()
                        .find(|tab| tab.id.as_deref() == Some(id))
                        .and_then(|tab| tab.panes.first())
                        .map(|pane| pane.id.clone());
                    self.tab_focus_pane_id(id, first)
                });
                self.snapshot.navigator.focused_workspace_id = Some(tab.workspace_id);
                self.snapshot.navigator.focused_checkout_id = Some(tab.checkout_id);
                self.snapshot.navigator.root_path = Some(checkout.path);
                self.select_terminal_pane(pane_id);
                self.operator_focused_pane_id = None;
                self.refresh_pane_read_state();
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
                let closed_tab = self.snapshot.editor.tabs.remove(index);
                self.rebuild_tab_strips();
                self.editor_documents.remove(&payload.tab_id);
                self.editor_tab_history
                    .retain(|tab_id| tab_id != &payload.tab_id);
                if was_active {
                    self.snapshot.editor.active_tab_id = None;
                    self.snapshot.editor.document = None;
                    if closed_tab.kind == EditorTabKind::Diff {
                        self.snapshot.changes.selected_path = None;
                        self.snapshot.changes.diff = None;
                    }
                    while let Some(previous_id) = self.editor_tab_history.pop() {
                        if self
                            .snapshot
                            .editor
                            .tabs
                            .iter()
                            .any(|tab| tab.id == previous_id)
                        {
                            if let Err(message) = self.activate_editor_tab(&previous_id) {
                                self.set_error("editor.focus_failed", message, false);
                            }
                            break;
                        }
                    }
                }
                self.persist_current_ui_state();
                true
            }
            ValidatedEvent::FileView(payload) => {
                let Some(tab) = self
                    .snapshot
                    .editor
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.id == payload.tab_id && tab.kind == EditorTabKind::File)
                else {
                    self.set_error(
                        "file.view_rejected",
                        "The file tab is no longer open",
                        false,
                    );
                    return true;
                };
                if tab.markdown_preview == payload.markdown_preview && tab.wrap == payload.wrap {
                    return false;
                }
                tab.markdown_preview = payload.markdown_preview;
                tab.wrap = payload.wrap;
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
            ValidatedEvent::TerminalClick(payload) => {
                // D8 explicitly chooses Herdr's detected agent as the policy
                // boundary until its frame protocol carries mouse mode.
                // Do not infer it from titles, output, or a session id.
                let agent = self
                    .snapshot
                    .navigator
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == payload.pane_id);
                let report = agent.is_some_and(|agent| agent.agent_kind == "claude");
                crate::diagnostic!(serde_json::json!({
                    "component": "terminal", "kind": "terminal.click_routed",
                    "pane_id": payload.pane_id,
                    "basis": if agent.is_some() { "herdr.agent.list" } else { "not_detected" },
                    "agent_kind": agent.map(|agent| agent.agent_kind.as_str()),
                    "route": if report { "sgr_mouse" } else { "local_selection" },
                    "column": payload.column, "row": payload.row,
                }));
                if report {
                    // SGR uses one-based cells, uppercase M for press and
                    // lowercase m for release. No newline or Enter is sent.
                    let column = u32::from(payload.column) + 1;
                    let row = u32::from(payload.row) + 1;
                    // The shell sends crossterm's bitset, the one Herdr's
                    // terminal.scroll takes. SGR has bits for shift, alt and
                    // control only, so command (bit 8) is dropped here on
                    // purpose: a TUI has no way to receive it.
                    let flags = ((payload.modifiers & 1) * 4)
                        | ((payload.modifiers & 2) * 8)
                        | ((payload.modifiers & 4) * 2);
                    let bytes =
                        format!("\x1b[<{flags};{column};{row}M\x1b[<{flags};{column};{row}m");
                    self.write_terminal_control(
                        &payload.pane_id,
                        &live::encode_base64(bytes.as_bytes()),
                        None,
                    );
                    return self.snapshot.status.last_error.is_some();
                }
                false
            }
            ValidatedEvent::TerminalScroll(payload) => {
                if !matches!(payload.direction.as_str(), "up" | "down") || payload.lines == 0 {
                    self.set_error(
                        "terminal.invalid_scroll",
                        "Scroll needs a direction and positive line count",
                        false,
                    );
                    return true;
                }
                // Herdr owns the pane's history, so the wheel is a request it
                // answers with a fresh frame rather than a local buffer move.
                // A pane another client controls is read-only, not broken, so
                // it simply does not scroll - the same shape as resize.
                if let Some(said) = self.scroll_withheld_for_missing_size(&payload.pane_id) {
                    return said;
                }
                let lines =
                    i32::from(payload.lines) * if payload.direction == "up" { 1 } else { -1 };
                if let Some(session) = self.terminal_sessions.get_mut(&payload.pane_id)
                    && session.mode == TerminalSessionMode::Control
                    && let Err(message) = session.scroll(live::ScrollRequest {
                        lines,
                        column: payload.column,
                        row: payload.row,
                        modifiers: payload.modifiers,
                    })
                {
                    self.set_error("terminal.scroll_failed", message, true);
                    return true;
                }
                false
            }
            ValidatedEvent::TerminalViewport(payload) => {
                let size = (payload.rows, payload.cols);
                let changed = self
                    .terminal_view_sizes
                    .insert(payload.pane_id.clone(), size)
                    != Some(size);
                if changed || payload.new_view {
                    self.terminal_frames_need_full.insert(payload.pane_id);
                }
                false
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
                let size = (payload.rows, payload.cols);
                self.panes_scrolled_before_size.remove(&payload.pane_id);
                let previous = self.terminal_sizes.insert(payload.pane_id.clone(), size);
                // A view reporting the size the pane is already running at is
                // the common case right after an attach. Sending it on would
                // make Herdr answer with a second full frame for a size that
                // never changed.
                // A pane's first size is what the next launch attaches with,
                // so it is written now. Later sizes ride the next UI-state
                // save rather than putting a file write in the middle of a
                // window drag.
                if previous.is_none() {
                    self.persist_ui_state();
                }
                if self.panes_awaiting_size.remove(&payload.pane_id) {
                    self.terminal_recovery.remove(&payload.pane_id);
                    self.terminal_session_lifecycles
                        .entry(payload.pane_id.clone())
                        .or_default()
                        .state = "idle";
                    self.request_terminal_control(&payload.pane_id);
                    return true;
                }
                if previous == Some(size)
                    && !self.terminal_frames_need_full.contains(&payload.pane_id)
                {
                    return false;
                }
                crate::diagnostic!(
                    serde_json::json!({"kind":"terminal.resize_settled", "pane_id":payload.pane_id, "rows":payload.rows, "cols":payload.cols})
                );
                if self.terminal_sessions.contains_key(&payload.pane_id) {
                    if let Some(session) = self.terminal_sessions.get_mut(&payload.pane_id)
                        && session.mode == TerminalSessionMode::Control
                        && let Err(message) = session.resize(payload.rows, payload.cols)
                    {
                        self.set_error("terminal.resize_failed", message, true);
                        return true;
                    }
                    return false;
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
                let Some(next) = self.stepped_text_scale(current, &payload.direction) else {
                    return true;
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
            ValidatedEvent::GithubRequest(payload) => {
                let project = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .find(|workspace| {
                        workspace.id == payload.workspace_id
                            && workspace.is_git
                            && workspace.remote_target_id.is_none()
                    })
                    .map(|workspace| workspace.path.clone());
                let Some(project) = project else {
                    self.set_error(
                        "github.invalid_project",
                        "GitHub lookup requires a registered local Git project".to_owned(),
                        false,
                    );
                    return true;
                };
                let first = self.sidebar_github_projects.insert(project.clone());
                if payload.refresh {
                    self.refresh_pull_requests(&project);
                    if let Some(cached) = self
                        .github
                        .projects
                        .iter_mut()
                        .find(|cached| cached.root_path == project)
                    {
                        cached.status.loading = true;
                    }
                }
                if first || payload.refresh {
                    self.apply_pull_requests();
                }
                first || payload.refresh
            }
            ValidatedEvent::CleanupReview => self.review_cleanup(),
            ValidatedEvent::CleanupConfirm(payload) => self.confirm_cleanup(payload),
            ValidatedEvent::CleanupDismiss => {
                if self.cleanup.as_ref().is_none_or(|r| r.phase == "removing") {
                    return false;
                }
                self.cleanup = None;
                self.refresh_worktree_projection();
                true
            }
            ValidatedEvent::OverviewChanges(payload) => {
                if self
                    .focused_local_checkout()
                    .is_none_or(|(_, checkout)| checkout.path != payload.checkout_path)
                {
                    return false;
                }
                self.snapshot.ui_state.right_panel_visible = true;
                self.snapshot.ui_state.right_panel_section = RightPanelSection::Changes;
                self.persist_ui_state();
                true
            }
            ValidatedEvent::OverviewSelect(payload) => {
                let valid = self.focused_local_checkout().is_some_and(|(project, _)| {
                    project
                        .checkouts
                        .iter()
                        .any(|c| c.path == payload.checkout_path)
                });
                if !valid || self.overview_selection.as_deref() == Some(&payload.checkout_path) {
                    return false;
                }
                self.overview_selection = Some(payload.checkout_path);
                self.refresh_card()
            }
            ValidatedEvent::CardRefresh => {
                // The card's one refresh button re-reads both the remote
                // answer and the local counts, because the operator pressing
                // it means "this is out of date", not "gh is out of date".
                if let Some((workspace, _)) = self.focused_local_checkout() {
                    let project = workspace.path.clone();
                    self.refresh_pull_requests(&project);
                }
                self.refresh_worktrees();
                self.remeasure_disk();
                true
            }
            ValidatedEvent::CardMeasureDisk => {
                // The delete confirmation states the size it is about to
                // delete, so it is measured when the dialog opens rather than
                // shown from whenever the card last looked.
                self.remeasure_disk();
                true
            }
            ValidatedEvent::GitWorktreeOpen(payload) => {
                self.open_git_worktree(payload.checkout_path)
            }
            ValidatedEvent::GitWorktreeSetBase(payload) => {
                self.set_git_worktree_base(payload.repository_root, payload.branch)
            }
            ValidatedEvent::CreateScratchChatTab(payload) => {
                self.create_scratch_chat_tab(payload.label)
            }
            ValidatedEvent::CreateWorktree(payload) => self.create_project_worktree(payload),
            ValidatedEvent::MigrateMainBranch(payload) => self.migrate_main_branch(payload),
            ValidatedEvent::TaskOperationAck(payload) => {
                self.acknowledge_task_operation(payload.id)
            }
            ValidatedEvent::RemoveWorktree(payload) => self.remove_git_worktree(payload),
            ValidatedEvent::WorktreeRemovalFinished(payload) => {
                self.finish_worktree_removal(payload)
            }
            ValidatedEvent::EditorTextScale(payload) => {
                let current = self.snapshot.ui_state.editor_text_scale;
                let Some(next) = self.stepped_text_scale(current, &payload.direction) else {
                    return true;
                };
                if next == current {
                    return false;
                }
                self.snapshot.ui_state.editor_text_scale = next;
                self.persist_ui_state();
                true
            }
            ValidatedEvent::ChangesSelect(payload) => {
                let Some(path) = payload.path else {
                    if self.snapshot.changes.selected_path.is_none() {
                        return false;
                    }
                    self.snapshot.changes.selected_path = None;
                    self.snapshot.changes.diff = None;
                    return true;
                };
                let entries = if payload.committed {
                    &self.snapshot.changes.committed
                } else {
                    &self.snapshot.changes.entries
                };
                if !entries.iter().any(|entry| entry.path == path) {
                    self.set_error(
                        "diff.selection_unavailable",
                        format!("{path} is no longer in the selected Changes group"),
                        false,
                    );
                    return true;
                }
                let Some(workspace_id) = self.snapshot.navigator.focused_workspace_id.clone()
                else {
                    self.set_error(
                        "diff.invalid_context",
                        "A diff tab requires the selected workspace and checkout",
                        false,
                    );
                    return true;
                };
                let Some(checkout_id) = self.snapshot.navigator.focused_checkout_id.clone() else {
                    self.set_error(
                        "diff.invalid_context",
                        "A diff tab requires the selected workspace and checkout",
                        false,
                    );
                    return true;
                };
                let tab_id =
                    Self::diff_tab_id(&workspace_id, &checkout_id, &path, payload.committed);
                if self.snapshot.editor.active_tab_id.as_deref() == Some(tab_id.as_str()) {
                    return false;
                }
                self.show_diff_tab(&workspace_id, &checkout_id, &path, payload.committed);
                self.persist_current_ui_state();
                true
            }
            ValidatedEvent::AgentTreeToggle(payload) => {
                let exists = self
                    .snapshot
                    .navigator
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == payload.pane_id)
                    || self
                        .snapshot
                        .status
                        .remote
                        .iter()
                        .filter_map(|remote| remote.session.as_ref())
                        .any(|session| {
                            session
                                .agents
                                .iter()
                                .any(|agent| agent.pane_id == payload.pane_id)
                        });
                if !exists {
                    self.set_error(
                        "agent_tree.parent_unavailable",
                        format!("Agent pane {} is no longer available", payload.pane_id),
                        true,
                    );
                    return true;
                }
                let collapsed = &mut self.snapshot.ui_state.collapsed_agent_pane_ids;
                if collapsed.contains(&payload.pane_id) {
                    collapsed.retain(|pane| pane != &payload.pane_id);
                } else {
                    collapsed.push(payload.pane_id);
                }
                self.refresh_agent_lineage();
                self.persist_ui_state();
                true
            }
            ValidatedEvent::UiStateUpdate(payload) => {
                // Pet placement, visibility, and shortcut belong to the pet
                // events; a navigator or keyboard save must not erase them.
                let current = self.snapshot.ui_state.clone();
                let git_was_visible = current.right_panel_visible
                    && matches!(
                        current.right_panel_section,
                        RightPanelSection::Git | RightPanelSection::Overview
                    );
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
                    collapsed_checkout_ids: payload
                        .collapsed_checkout_ids
                        .unwrap_or(current.collapsed_checkout_ids),
                    project_base_branches: current.project_base_branches,
                    collapsed_agent_pane_ids: payload
                        .collapsed_agent_pane_ids
                        .unwrap_or(current.collapsed_agent_pane_ids),
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
                    editor_text_scale: current.editor_text_scale,
                    pane_read_records: current.pane_read_records,
                    last_agent_kind: payload.last_agent_kind.unwrap_or(current.last_agent_kind),
                    last_agent_bypass: payload
                        .last_agent_bypass
                        .unwrap_or(current.last_agent_bypass),
                    scratch_expanded: payload.scratch_expanded.unwrap_or(current.scratch_expanded),
                };
                self.snapshot.navigator.scratch.expanded = self.snapshot.ui_state.scratch_expanded;
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
                self.refresh_agent_lineage();
                let git_is_visible = self.snapshot.ui_state.right_panel_visible
                    && matches!(
                        self.snapshot.ui_state.right_panel_section,
                        RightPanelSection::Git | RightPanelSection::Overview
                    );
                if git_is_visible && !git_was_visible {
                    if let Some((workspace, _)) = self.focused_local_checkout() {
                        let project = workspace.path.clone();
                        self.refresh_pull_requests(&project);
                    }
                    self.refresh_worktrees();
                    self.remeasure_disk();
                }
                if git_is_visible != git_was_visible {
                    self.refresh_card();
                }
                // Session sync owns session-derived temporary workspaces.
                // UI-state persistence must not rebuild from an empty session
                // and erase the catalog that the user is currently viewing.
                match self.write_ui_state() {
                    Ok(()) => true,
                    Err(message) => {
                        self.set_error("ui_state.save_failed", message, true);
                        true
                    }
                }
            }
        }
    }

    fn terminal_retry_decision(&self, pane_id: &str, fallback: &'static str) -> &'static str {
        self.terminal_recovery
            .get(pane_id)
            .map_or(fallback, |r| r.decision())
    }

    fn schedule_terminal_recovery(&mut self, pane_id: &str, reason: String) {
        // Remote reconnect policy is owned by its existing transport.
        if pane_id.starts_with("remote:") {
            return;
        }
        let recovery = self
            .terminal_recovery
            .entry(pane_id.to_owned())
            .or_insert_with(|| {
                crate::terminal_recovery::Recovery::new(Instant::now(), reason.clone())
            });
        recovery.reason = reason;
        if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
            lifecycle.message = Some(recovery.message());
            lifecycle.retry_decision = recovery.decision();
        }
    }

    pub(crate) fn maintain_terminals(&mut self, now: Instant) -> bool {
        let visible = self
            .focused_visible_tab_id()
            .and_then(|tab_id| {
                self.snapshot
                    .pane_layouts
                    .iter()
                    .find(|layout| layout.tab_id == tab_id)
                    .map(|layout| {
                        layout
                            .pane_ids()
                            .into_iter()
                            .map(str::to_owned)
                            .collect::<HashSet<_>>()
                    })
            })
            .unwrap_or_default();
        let mut changed = false;
        for pane_id in &visible {
            if self
                .terminal_session_lifecycles
                .get(pane_id)
                .is_some_and(|lifecycle| lifecycle.state == "released")
            {
                self.request_terminal_control(pane_id);
                changed = true;
            }
        }
        let due = self
            .terminal_recovery
            .iter()
            .filter(|(pane_id, recovery)| {
                visible.contains(*pane_id) && recovery.due.is_some_and(|due| now >= due)
            })
            .map(|(pane_id, _)| pane_id.clone())
            .collect::<Vec<_>>();
        for pane_id in due {
            if self.pane_is_going_away(&pane_id) {
                self.terminal_recovery.remove(&pane_id);
                continue;
            }
            let retry = self
                .terminal_recovery
                .get_mut(&pane_id)
                .expect("collected recovery")
                .advance(now);
            let message = self.terminal_recovery[&pane_id].message();
            if retry && self.terminal_sizes.contains_key(&pane_id) {
                let attempt = self
                    .terminal_session_lifecycles
                    .get(&pane_id)
                    .map_or(1, |l| l.attempt + 1);
                self.start_terminal_session(
                    &pane_id,
                    TerminalSessionMode::Control,
                    attempt,
                    "automatic_bounded",
                    Some(message.clone()),
                );
            } else if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(&pane_id) {
                lifecycle.message = Some(message.clone());
                lifecycle.retry_decision = if retry { "automatic_bounded" } else { "manual" };
                if !retry && lifecycle.state != "observing" {
                    lifecycle.state = "unavailable";
                    self.terminal_sessions.remove(&pane_id);
                    // A late spawn cannot revive an exhausted attempt.
                    self.terminal_session_generations.remove(&pane_id);
                }
                self.sync_transport_projection(&pane_id);
            }
            self.push_diagnostic(
                if retry {
                    "terminal.retrying"
                } else {
                    "terminal.retries_exhausted"
                },
                format!("Pane {pane_id}: {message}"),
            );
            changed = true;
        }
        changed
    }

    /// Starts one control attempt. Repeated sync updates are no-ops while any
    /// official control or observer session is starting or active.
    fn request_terminal_control(&mut self, pane_id: &str) {
        let native_content = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .find(|pane| pane.id == pane_id)
            .is_some_and(|pane| !pane.content.is_terminal());
        if native_content {
            // A host may report its browser identity after the first layout.
            // Release any early PTY attachment rather than holding invisible
            // terminal control behind the native content surface.
            self.terminal_sessions.remove(pane_id);
            self.terminal_session_lifecycles.remove(pane_id);
            self.terminal_sizes.remove(pane_id);
            self.panes_awaiting_size.remove(pane_id);
            self.terminal_recovery.remove(pane_id);
            return;
        }
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
        // Herdr sizes the PTY from the attach, so attaching before a view has
        // reported a size costs a full frame at a guessed size and a second
        // one after the resize. The pane's own view reports within a frame of
        // the layout arriving, and the resize handler starts the attach then.
        if !self.terminal_sizes.contains_key(pane_id) {
            if self.panes_awaiting_size.insert(pane_id.to_owned()) {
                self.push_diagnostic(
                    "terminal.attach_deferred",
                    format!(
                        "Pane {pane_id} is waiting for its view to report a size before attaching"
                    ),
                );
            }
            self.terminal_session_lifecycles
                .entry(pane_id.to_owned())
                .or_default()
                .state = "waiting_size";
            self.schedule_terminal_recovery(
                pane_id,
                "Waiting for the pane view to report its size".to_owned(),
            );
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

    fn terminal_session_context(&self, pane_id: &str) -> Option<TerminalSessionContext> {
        if pane_id.starts_with("remote:") {
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
        }
    }

    fn start_terminal_session(
        &mut self,
        pane_id: &str,
        mode: TerminalSessionMode,
        attempt: u64,
        retry_decision: &'static str,
        message: Option<String>,
    ) {
        self.terminal_frames_need_full.insert(pane_id.to_owned());
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
        if mode == TerminalSessionMode::Control {
            self.schedule_terminal_recovery(
                pane_id,
                "Waiting for the first terminal frame".to_owned(),
            );
            if let Some(recovery) = self.terminal_recovery.get_mut(pane_id) {
                recovery.last_attempt_at_unix_ms = Some(unix_milliseconds());
            }
        }
        let decision = self.terminal_retry_decision(pane_id, retry_decision);
        if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
            lifecycle.retry_decision = decision;
        }
        self.sync_transport_projection(pane_id);
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
            }
            self.sync_transport_projection(pane_id);
            return;
        }
        let context = self.terminal_session_context(pane_id);
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
        // Herdr sizes the PTY from the attach, so there is no honest size to
        // send when no view has reported one. Every path here has one:
        // `request_terminal_control` holds a pane back until its view reports,
        // and an observe session only follows a control session that already
        // had a size. A pane that arrives here without one is a routing bug,
        // and saying so beats attaching at a guess and hiding it.
        let Some((rows, cols)) = self.terminal_sizes.get(pane_id).copied() else {
            let message =
                format!("Pane {pane_id} has no reported terminal size, so it cannot be attached");
            self.record_terminal_session_failure(
                pane_id,
                generation,
                attempt,
                mode,
                "size_unknown",
                &message,
                0,
            );
            self.set_error("terminal.size_unknown", message, true);
            return;
        };
        if let Err(message) =
            live::spawn_terminal_session(context, pane_id.to_owned(), generation, mode, rows, cols)
        {
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

    // This keeps the lifecycle and diagnostic fields adjacent at the one
    // failure boundary instead of splitting a correlated event into builders.
    #[allow(clippy::too_many_arguments)]
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
        self.schedule_terminal_recovery(pane_id, format!("Terminal start refused: {message}"));
        self.sync_transport_projection(pane_id);
        crate::diagnostic!(serde_json::json!({
            "component": "terminal_session",
            "kind": "terminal.session_unavailable",
            "pane_id": pane_id,
            "generation": generation,
            "attempt": attempt,
            "mode": mode.as_str(),
            "duration_ms": elapsed_ms,
            "exit_category": category,
            "retry_decision": self.terminal_retry_decision(pane_id, "manual"),
        }));
    }

    // The worker callback supplies each correlation field independently; a
    // wrapper would only move this boundary without reducing its inputs.
    #[allow(clippy::too_many_arguments)]
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
            && !self.snapshot.pane_layouts.is_empty()
            && self.layout_holding_pane(pane_id).is_none()
        {
            return false;
        }
        match result {
            Ok(session) => {
                if mode == TerminalSessionMode::Control
                    && let Some((rows, cols)) = self.terminal_sizes.get(pane_id).copied()
                    && let Err(message) = session.resize(rows, cols)
                {
                    self.set_error("terminal.resize_after_attach_failed", message, true);
                }
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
                        retry_decision: self.terminal_retry_decision(
                            pane_id,
                            if mode == TerminalSessionMode::Observe {
                                "manual"
                            } else {
                                "none"
                            },
                        ),
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
                crate::diagnostic!(serde_json::json!({
                    "component": "terminal_session",
                    "kind": "terminal.session_ready",
                    "pane_id": pane_id,
                    "generation": generation,
                    "attempt": attempt,
                    "mode": mode.as_str(),
                    "duration_ms": elapsed_ms,
                    "exit_category": null,
                    "retry_decision": self.terminal_retry_decision(pane_id, if mode == TerminalSessionMode::Observe { "manual" } else { "none" }),
                    "last_attempt_at_unix_ms": self.terminal_recovery.get(pane_id).and_then(|r| r.last_attempt_at_unix_ms),
                }));
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
    fn write_terminal_control(
        &mut self,
        pane_id: &str,
        bytes_base64: &str,
        trace: Option<crate::model::TerminalInputTrace>,
    ) {
        let bytes = match live::decode_base64(bytes_base64) {
            Ok(bytes) => bytes,
            Err(message) => {
                self.set_error("terminal.invalid_input", message, false);
                return;
            }
        };
        match self.terminal_sessions.get(pane_id) {
            Some(session) if session.mode == TerminalSessionMode::Control => {
                if let Err(message) = session.write_bytes(&bytes, trace) {
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

    pub(crate) fn ingest_terminal_input_sent(
        &mut self,
        pane_id: &str,
        generation: u64,
        sent: crate::model::TerminalInputSent,
    ) -> bool {
        if self.terminal_session_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        self.append_terminal_chunk(pane_id.to_owned(), String::new());
        self.snapshot
            .terminal
            .chunks
            .last_mut()
            .expect("just appended input trace")
            .input_sent = Some(sent);
        true
    }

    fn append_terminal_chunk(&mut self, pane_id: String, bytes_base64: String) {
        self.snapshot.terminal.sequence = self.snapshot.terminal.sequence.saturating_add(1);
        self.snapshot.terminal.chunks.push(TerminalChunk {
            pane_id,
            sequence: self.snapshot.terminal.sequence,
            bytes_base64,
            frame: None,
            input_sent: None,
        });
        const RETAINED_TERMINAL_CHUNKS: usize = 512;
        if self.snapshot.terminal.chunks.len() > RETAINED_TERMINAL_CHUNKS {
            let excess = self.snapshot.terminal.chunks.len() - RETAINED_TERMINAL_CHUNKS;
            self.snapshot.terminal.chunks.drain(..excess);
        }
    }
}

/// One tab's panes, as the navigator draws them.
///
/// Lifted out of the placement loop so a Scratch tab and a project tab are
/// projected by the same code: a pane row that differed between the two
/// sections would be a second definition of what a pane is.
fn project_layout_panes(
    layout: &crate::sidebar::SessionLayoutPayload,
    payload: &SessionSnapshotPayload,
    projected_agents: &[SidebarAgentSnapshot],
    listening_ports: &[crate::model::ListeningPortSnapshot],
    fallback_cwd: &str,
) -> Vec<PaneSnapshot> {
    layout
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
                .unwrap_or_else(|| fallback_cwd.to_owned());
            let ports = crate::ports::attributed_ports(&cwd, listening_ports);
            PaneSnapshot {
                id: pane.pane_id.clone(),
                content: source
                    .map(|source| {
                        crate::pane_content::PaneContent::from_tokens(&source.tokens, false)
                    })
                    .unwrap_or_default(),
                herdr_label: source.and_then(|source| source.label.clone()),
                terminal_title: source.and_then(|source| source.terminal_title.clone()),
                workspace_label: agent.map(|agent| agent.workspace_label.clone()),
                cwd,
                // This projection cannot see the read record ledger, so both
                // read-dependent values are refilled from the navigator's
                // agent rows once those are final; see
                // `sync_pane_status_from_agents`.
                status_label: agent
                    .map(|agent| agent.status_label.clone())
                    .unwrap_or_else(|| "Unknown".to_owned()),
                requires_close_confirmation: agent
                    .is_some_and(|agent| agent.requires_close_confirmation),
                summary: agent
                    .map(|agent| agent.summary.clone())
                    .filter(|summary| summary != crate::sidebar::MISSING_SUMMARY),
                activity_at_unix_ms: agent.and_then(|agent| agent.last_activity.parse().ok()),
                fork: pane_fork_snapshot(agent),
                ports,
            }
        })
        .collect()
}

fn find_workspace_for_context<'a>(
    workspaces: &'a mut Vec<crate::model::WorkspaceSnapshot>,
    context_path: Option<&str>,
    session_workspace_id: &str,
    roots: &workspace::RootIndex,
    unresolved_roots: &mut Vec<String>,
) -> Option<&'a mut crate::model::WorkspaceSnapshot> {
    let Some(raw_path) = context_path else {
        return workspaces
            .iter()
            .position(|workspace| workspace.id == session_workspace_id)
            .and_then(|index| workspaces.get_mut(index));
    };
    let path = Path::new(raw_path);
    // The root was resolved outside the runtime lock; a directory the index
    // does not carry is placed by its own path and reported, never by asking
    // git from here.
    let root = roots.get(raw_path).cloned().unwrap_or_else(|| {
        unresolved_roots.push(raw_path.to_owned());
        workspace::normalized_for_comparison(path)
    });
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
        "focus_pane" => decode!(FocusPaneRequestPayload, FocusPane),
        "open_browser" => decode!(OpenBrowserPayload, OpenBrowser),
        "browser_status" => decode!(BrowserStatusPayload, BrowserStatus),
        "create_workspace" => decode!(CreateWorkspacePayload, CreateWorkspace),
        "create_tab" => decode!(CreateTabPayload, CreateTab),
        "focus_checkout" => decode!(FocusCheckoutPayload, FocusCheckout),
        "focus_tab" => decode!(FocusTabPayload, FocusTab),
        "reorder_tab" => decode!(ReorderTabPayload, ReorderTab),
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
        "agent_tree_toggle" => decode!(PaneTargetPayload, AgentTreeToggle),
        "remote_control" => decode!(RemoteControlPayload, RemoteControl),
        "remote_file_list" => decode!(RemoteFileListPayload, RemoteFileList),
        "file_open" => decode!(FileOpenPayload, FileOpen),
        "reveal_path" => decode!(RevealPathPayload, RevealPath),
        "file_focus" => decode!(FileTabPayload, FileFocus),
        "file_close" => decode!(FileTabPayload, FileClose),
        "file_draft" => decode!(FileDraftPayload, FileDraft),
        "file_view" => decode!(FileViewPayload, FileView),
        "file_save" => decode!(FileSavePayload, FileSave),
        "file_conflict" => decode!(FileConflictPayload, FileConflict),
        "ui_state_update" => decode!(UiStateUpdatePayload, UiStateUpdate),
        "retry_connect" => decode!(RetryConnectPayload, RetryConnect),
        "terminal_resize" => decode!(TerminalResizePayload, TerminalResize),
        "terminal_viewport" => decode!(TerminalResizePayload, TerminalViewport),
        "terminal_scroll" => decode!(TerminalScrollPayload, TerminalScroll),
        "terminal_click" => decode!(TerminalClickPayload, TerminalClick),
        "pane_find" => decode!(PaneFindPayload, PaneFind),
        "pane_text_scale" => decode!(PaneTextScalePayload, PaneTextScale),
        "editor_text_scale" => decode!(EditorTextScalePayload, EditorTextScale),
        "changes_select" => decode!(ChangesSelectPayload, ChangesSelect),
        "git_worktree_open" => decode!(GitWorktreeOpenPayload, GitWorktreeOpen),
        "git_worktree_set_base" => decode!(GitWorktreeSetBasePayload, GitWorktreeSetBase),
        "create_scratch_chat_tab" => {
            decode!(CreateScratchChatTabPayload, CreateScratchChatTab)
        }
        "create_worktree" => decode!(CreateWorktreePayload, CreateWorktree),
        "migrate_main_branch" => decode!(MigrateMainBranchPayload, MigrateMainBranch),
        "task_operation_ack" => decode!(TaskOperationAckPayload, TaskOperationAck),
        "remove_worktree" => decode!(RemoveWorktreePayload, RemoveWorktree),
        "worktree_removal_finished" => {
            decode!(WorktreeRemovalFinishedPayload, WorktreeRemovalFinished)
        }
        "github_request" => decode!(GithubRequestPayload, GithubRequest),
        "cleanup_review" => Ok(ValidatedEvent::CleanupReview),
        "cleanup_confirm" => decode!(CleanupConfirmPayload, CleanupConfirm),
        "cleanup_dismiss" => Ok(ValidatedEvent::CleanupDismiss),
        "overview_changes" => decode!(GitWorktreeOpenPayload, OverviewChanges),
        "overview_select" => decode!(GitWorktreeOpenPayload, OverviewSelect),
        "card_refresh" => Ok(ValidatedEvent::CardRefresh),
        "card_measure_disk" => Ok(ValidatedEvent::CardMeasureDisk),
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

    /// The catalog before the worktree reader has answered.
    /// Reconciling a session with a precomputed catalog runs no git at all.
    ///
    /// Placing each tab into a checkout used to resolve the pane directory's
    /// repository root with `git rev-parse` while the runtime lock was held,
    /// once per tab per publish. The root index arrives with the catalog
    /// instead, so the lock is never held across a subprocess.
    #[test]
    fn reconciling_with_a_precomputed_catalog_runs_no_git() {
        let base = workspace::temp_base_outside_any_repository();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let roots: Vec<std::path::PathBuf> = (0..3)
            .map(|index| base.join(format!("hide-no-git-reconcile-{stamp}-{index}")))
            .collect();
        for root in &roots {
            std::fs::create_dir_all(root).expect("fixture directory");
        }
        let cwds: Vec<String> = roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect();
        let tabs: Vec<serde_json::Value> = (0..3)
            .map(|index| serde_json::json!({"workspace_id": "w1", "tab_id": format!("w1:t{index}"), "label": ""}))
            .collect();
        let panes: Vec<serde_json::Value> = (0..3)
            .map(|index| serde_json::json!({"pane_id": format!("w1:p{index}"), "cwd": cwds[index]}))
            .collect();
        let layouts: Vec<serde_json::Value> = (0..3)
            .map(|index| serde_json::json!({
                "workspace_id": "w1",
                "tab_id": format!("w1:t{index}"),
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": format!("w1:p{index}"),
                "panes": [{"pane_id": format!("w1:p{index}"), "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }))
            .collect();
        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [{"workspace_id": "w1", "label": "three"}],
            "panes": panes,
            "tabs": tabs,
            "layouts": layouts,
        }))
        .expect("three-tab payload");
        let spaces = Runtime::session_spaces(&payload, "");
        let catalog = session_sync::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: workspace::build_catalog(&[], &spaces, &no_worktrees()),
            roots: workspace::root_index(&spaces),
        };
        let mut runtime = runtime();

        let before = workspace::git_calls_on_this_thread();
        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
        let after = workspace::git_calls_on_this_thread();

        assert_eq!(
            after - before,
            0,
            "the reconcile ran git under the runtime lock"
        );
        let placed: usize = runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .map(|checkout| checkout.tabs.len())
            .sum();
        assert_eq!(placed, 3, "every tab landed in a checkout without git");
        assert!(
            !runtime
                .snapshot()
                .status
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == "catalog.root_unresolved"),
            "the root index carried every pane directory"
        );
        for root in &roots {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    /// A precomputed catalog whose registrations went stale between the
    /// coordinator's read and the reconcile is not rebuilt under the lock: the
    /// last accepted catalog stands, and a diagnostic says so.
    #[test]
    fn a_stale_precomputed_catalog_keeps_the_last_accepted_one() {
        let base = workspace::temp_base_outside_any_repository();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = base.join(format!("hide-stale-catalog-{stamp}"));
        std::fs::create_dir_all(&root).expect("fixture directory");
        let cwd = root.to_string_lossy().into_owned();
        let payload = || -> SessionSnapshotPayload {
            serde_json::from_value(serde_json::json!({
                "agents": [],
                "workspaces": [{"workspace_id": "w1", "label": "one"}],
                "panes": [{"pane_id": "w1:p1", "cwd": cwd}],
                "tabs": [{"workspace_id": "w1", "tab_id": "w1:t1", "label": ""}],
                "layouts": [{
                    "workspace_id": "w1",
                    "tab_id": "w1:t1",
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "w1:p1",
                    "panes": [{"pane_id": "w1:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                }]
            }))
            .expect("one-tab payload")
        };
        let spaces = Runtime::session_spaces(&payload(), "");
        let fresh = session_sync::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: workspace::build_catalog(&[], &spaces, &no_worktrees()),
            roots: workspace::root_index(&spaces),
        };
        let mut runtime = runtime();
        assert!(runtime.ingest_session_with_catalog(Ok(payload()), Some(fresh)));
        let accepted = runtime.snapshot().navigator.workspaces.clone();
        assert!(!accepted.is_empty(), "the first catalog was accepted");

        let stale = session_sync::PrecomputedCatalog {
            registrations: vec![WorkspaceRegistration {
                id: "workspace:stale".to_owned(),
                label: "stale".to_owned(),
                path: cwd.clone(),
                device_id: workspace::LOCAL_DEVICE_ID.to_owned(),
            }],
            workspaces: Vec::new(),
            roots: workspace::RootIndex::new(),
        };
        let before = workspace::git_calls_on_this_thread();
        runtime.ingest_session_with_catalog(Ok(payload()), Some(stale));
        let after = workspace::git_calls_on_this_thread();

        assert_eq!(
            after - before,
            0,
            "a stale catalog was rebuilt under the runtime lock"
        );
        assert_eq!(
            runtime.snapshot().navigator.workspaces,
            accepted,
            "the last accepted catalog stands until the next publish"
        );
        assert!(
            runtime
                .snapshot()
                .status
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == "catalog.precomputed_stale"),
            "the stale catalog was reported"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn no_worktrees() -> crate::model::WorktreeCatalogSnapshot {
        crate::model::WorktreeCatalogSnapshot::default()
    }

    #[test]
    fn worktree_rows_sort_main_then_open_then_commit_time() {
        use crate::model::{ProjectWorktreesSnapshot, WorktreeCatalogSnapshot, WorktreeSnapshot};

        let mut runtime = runtime();
        let mut project = workspace(
            "workspace-1",
            "hide",
            "/repo",
            vec![
                checkout("workspace-1", "main", "/repo", None),
                checkout(
                    "workspace-1",
                    "open",
                    "/repo.worktrees/open",
                    Some(pane("w1:p1", "/repo.worktrees/open")),
                ),
            ],
        );
        project.is_git = true;
        runtime.snapshot.navigator.workspaces = vec![project];
        runtime.snapshot.navigator.focused_checkout_id = Some("main".to_owned());
        let row = |path: &str, branch: &str, is_main: bool, committed_at: u64| WorktreeSnapshot {
            path: path.to_owned(),
            branch: Some(branch.to_owned()),
            is_main,
            last_commit_unix_seconds: Some(committed_at),
            ..WorktreeSnapshot::default()
        };

        runtime.ingest_worktrees(WorktreeCatalogSnapshot {
            projects: vec![ProjectWorktreesSnapshot {
                root_path: "/repo".to_owned(),
                worktrees: vec![
                    row("/repo.worktrees/old", "old", false, 20),
                    row("/repo.worktrees/recent", "recent", false, 30),
                    row("/repo.worktrees/open", "open", false, 10),
                    row("/repo", "main", true, 1),
                ],
                ..ProjectWorktreesSnapshot::default()
            }],
        });

        let ordered = runtime
            .snapshot()
            .git_worktrees
            .as_ref()
            .expect("focused local project")
            .worktrees
            .iter()
            .map(|worktree| worktree.branch.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ordered, ["main", "open", "recent", "old"]);
        assert_eq!(
            runtime
                .snapshot()
                .git_worktrees
                .as_ref()
                .unwrap()
                .worktrees
                .iter()
                .filter(|worktree| worktree.is_main)
                .count(),
            1,
            "bare registrations never become rows"
        );
    }

    /// A settled worktree with nothing in the way, ready for the Remove
    /// button's rules to be applied to it.
    fn settled_worktree(badge: crate::model::PullRequestBadge) -> CheckoutSnapshot {
        let pull_request = crate::model::PullRequestSnapshot {
            title: "Fixture pull request".into(),
            checks: crate::model::PullRequestChecks::Unknown,
            number: 7,
            head_branch: "feature".to_owned(),
            base_branch: "main".to_owned(),
            url: "https://example.invalid/pull/7".to_owned(),
            badge,
            review: None,
            is_draft: false,
            merged_at_unix_ms: None,
            updated_at_unix_ms: None,
        };
        CheckoutSnapshot {
            id: "checkout-feature".to_owned(),
            workspace_id: "workspace-1".to_owned(),
            label: "feature".to_owned(),
            path: "/tmp/hide/feature".to_owned(),
            branch: Some("feature".to_owned()),
            is_worktree: true,
            exists: true,
            pull_request: Some(pull_request.clone()),
            worktree: Some(crate::model::WorktreeSnapshot {
                path: "/tmp/hide/feature".to_owned(),
                branch: Some("feature".to_owned()),
                pull_request: Some(pull_request),
                deletion_gate: crate::model::WorktreeDeletionGateSnapshot {
                    button_label: "Delete worktree…".to_owned(),
                    can_delete_branch: true,
                    ..crate::model::WorktreeDeletionGateSnapshot::default()
                },
                ..crate::model::WorktreeSnapshot::default()
            }),
            ..CheckoutSnapshot::default()
        }
    }

    fn card_for(
        runtime: &mut Runtime,
        checkout: CheckoutSnapshot,
    ) -> crate::model::CheckoutCardSnapshot {
        let checkout_id = checkout.id.clone();
        runtime.snapshot.navigator.workspaces = vec![workspace(
            "workspace-1",
            "hide",
            "/tmp/hide",
            vec![checkout],
        )];
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id);
        runtime.refresh_card();
        runtime.snapshot.card.clone()
    }

    #[test]
    fn sidebar_github_request_is_scoped_idempotent_and_does_not_move_focus() {
        let mut runtime = runtime();
        let mut project = workspace(
            "workspace-1",
            "hide",
            "/tmp/hide",
            vec![settled_worktree(crate::model::PullRequestBadge::Open)],
        );
        project.is_git = true;
        let mut other = workspace("workspace-2", "other", "/tmp/other", Vec::new());
        other.is_git = true;
        runtime.snapshot.navigator.workspaces = vec![project, other];
        let focus = runtime.snapshot.focused.clone();
        let event = |refresh| {
            serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION, "kind": "github_request",
                "payload": {"workspace_id":"workspace-1", "refresh":refresh}
            }))
            .unwrap()
        };
        assert!(runtime.dispatch_json(&event(false)));
        assert!(!runtime.dispatch_json(&event(false)));
        assert_eq!(runtime.snapshot.focused, focus);
        assert_eq!(runtime.github_request().projects.len(), 1);
        assert_eq!(
            runtime.github_request().projects[0].root,
            PathBuf::from("/tmp/hide")
        );
        assert_eq!(runtime.github_request().projects[0].generation, 0);
        assert!(
            runtime.snapshot.navigator.workspaces[0].checkouts[0]
                .github
                .loading
        );
        assert!(runtime.dispatch_json(&event(true)));
        assert_eq!(runtime.github_request().projects[0].generation, 1);
    }

    /// Without sidebar requests, pull requests are absent until Git is visible.
    /// Once visible, one request per project is stable until header refresh.
    #[test]
    fn pull_requests_are_scoped_to_overview_and_visible_git_section() {
        let mut runtime = runtime();
        let mut hide = workspace(
            "workspace-1",
            "hide",
            "/tmp/hide",
            vec![settled_worktree(crate::model::PullRequestBadge::Open)],
        );
        hide.is_git = true;
        runtime.snapshot.navigator.focused_checkout_id = Some(hide.checkouts[0].id.clone());
        let mut other = workspace("workspace-2", "other", "/tmp/other", Vec::new());
        other.is_git = true;
        runtime.snapshot.navigator.workspaces = vec![hide, other];
        let generations = |runtime: &Runtime| -> Vec<(String, u64)> {
            runtime
                .github_request()
                .projects
                .into_iter()
                .map(|project| {
                    (
                        project.root.to_string_lossy().into_owned(),
                        project.generation,
                    )
                })
                .collect()
        };
        assert_eq!(
            generations(&runtime),
            vec![("/tmp/hide".to_owned(), 0)],
            "Overview reads only its project"
        );
        runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Explorer;
        assert!(generations(&runtime).is_empty());
        assert!(!runtime.projected_card().github.loading);
        runtime.snapshot.ui_state.right_panel_visible = true;
        runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Git;
        assert!(runtime.projected_card().github.loading);
        assert_eq!(
            generations(&runtime),
            vec![("/tmp/hide".to_owned(), 0), ("/tmp/other".to_owned(), 0)]
        );
        runtime.refresh_pull_requests("/tmp/hide");
        assert_eq!(
            generations(&runtime),
            vec![("/tmp/hide".to_owned(), 1), ("/tmp/other".to_owned(), 0)]
        );
        runtime.refresh_card();
        assert!(runtime.snapshot.card.github.loading);
        let leave_git = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": [],
                "right_panel_section": "explorer",
                "focused_checkout_id": runtime.snapshot.navigator.focused_checkout_id,
            }
        }))
        .expect("leave Git event");
        assert!(runtime.dispatch_json(&leave_git));
        assert!(!runtime.snapshot.card.github.loading);
        runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Git;
        runtime.snapshot.ui_state.right_panel_visible = false;
        assert!(!runtime.projected_card().github.loading);
    }

    /// The card consumes the same deletion gate as the sidebar and Git list.
    /// It must not derive an older, card-only rule from pull-request state.
    #[test]
    fn the_card_projects_the_worktrees_shared_deletion_gate() {
        use crate::model::PullRequestBadge;
        let mut runtime = runtime();
        let mut checkout = settled_worktree(PullRequestBadge::Open);
        let gate = checkout.worktree.as_mut().expect("worktree");
        gate.deletion_gate.blocked_reason =
            Some("Commit or discard uncommitted changes first".to_owned());
        gate.deletion_gate.warnings = vec!["not pushed".to_owned()];
        gate.deletion_gate.button_label = "Close 2 panes and delete".to_owned();

        assert_eq!(
            card_for(&mut runtime, checkout).deletion_gate,
            Some(crate::model::WorktreeDeletionGateSnapshot {
                blocked_reason: Some("Commit or discard uncommitted changes first".to_owned()),
                warnings: vec!["not pushed".to_owned()],
                button_label: "Close 2 panes and delete".to_owned(),
                can_delete_branch: true,
            })
        );
    }

    /// The size shown must be this checkout's. Until the measurement for the
    /// selected path arrives, the card says it is measuring rather than
    /// showing the previous checkout's number.
    #[test]
    fn the_card_says_it_is_measuring_until_this_checkouts_size_arrives() {
        use crate::model::PullRequestBadge;
        let mut runtime = runtime();
        let card = card_for(&mut runtime, settled_worktree(PullRequestBadge::Merged));
        assert!(card.disk_measuring, "nothing has been measured yet");

        runtime.disk_usage = vec![crate::model::DiskUsageSnapshot {
            path: Some("/tmp/hide/somewhere-else".to_owned()),
            total_bytes: Some(4096),
            ..crate::model::DiskUsageSnapshot::default()
        }];
        let card = card_for(&mut runtime, settled_worktree(PullRequestBadge::Merged));
        assert!(
            card.disk_measuring,
            "another checkout's size is not this one's"
        );

        runtime.disk_usage = vec![crate::model::DiskUsageSnapshot {
            path: Some("/tmp/hide/feature".to_owned()),
            total_bytes: Some(4096),
            ..crate::model::DiskUsageSnapshot::default()
        }];
        let card = card_for(&mut runtime, settled_worktree(PullRequestBadge::Merged));
        assert!(!card.disk_measuring);
        assert_eq!(card.disk.total_bytes, Some(4096));
    }

    /// A failed lookup must not erase the pull requests it failed to replace:
    /// the card shows the previous ones with how old they are, which is a
    /// different thing from showing none.
    #[test]
    fn a_failed_lookup_keeps_the_pull_requests_it_could_not_refresh() {
        use crate::model::{
            GithubProjectSnapshot, GithubSnapshot, GithubStatusSnapshot, PullRequestBadge,
            PullRequestSnapshot,
        };
        let mut runtime = runtime();
        let pull_request = PullRequestSnapshot {
            title: "Fixture pull request".into(),
            checks: crate::model::PullRequestChecks::Unknown,
            number: 7,
            head_branch: "feature".to_owned(),
            base_branch: "main".to_owned(),
            url: "https://example.invalid/pull/7".to_owned(),
            badge: PullRequestBadge::Open,
            review: None,
            is_draft: false,
            merged_at_unix_ms: None,
            updated_at_unix_ms: None,
        };
        runtime.ingest_github(GithubSnapshot {
            projects: vec![GithubProjectSnapshot {
                root_path: "/tmp/hide".to_owned(),
                status: GithubStatusSnapshot {
                    available: true,
                    last_success_at_unix_ms: Some(1_000),
                    ..GithubStatusSnapshot::default()
                },
                pull_requests: vec![pull_request.clone()],
            }],
        });

        runtime.ingest_github(GithubSnapshot {
            projects: vec![GithubProjectSnapshot {
                root_path: "/tmp/hide".to_owned(),
                status: GithubStatusSnapshot {
                    available: true,
                    stale: true,
                    unavailable_reason: Some("gh pr list: network unreachable".to_owned()),
                    ..GithubStatusSnapshot::default()
                },
                ..GithubProjectSnapshot::default()
            }],
        });

        let project = runtime
            .github
            .project("/tmp/hide")
            .expect("the project survives");
        assert_eq!(project.pull_requests, vec![pull_request.clone()]);
        assert!(project.status.stale);
        assert_eq!(project.status.last_success_at_unix_ms, Some(1_000));
        runtime.ingest_github(GithubSnapshot {
            projects: vec![GithubProjectSnapshot {
                root_path: "/tmp/hide".to_owned(),
                status: GithubStatusSnapshot {
                    available: false,
                    unavailable_reason: Some("gh is not logged in".to_owned()),
                    ..Default::default()
                },
                ..Default::default()
            }],
        });
        let project = runtime.github.project("/tmp/hide").unwrap();
        assert_eq!(project.pull_requests, vec![pull_request]);
        assert!(project.status.stale);
        assert!(!project.status.available);
        assert_eq!(project.status.last_success_at_unix_ms, Some(1_000));
    }
    use crate::live::SessionFetchError;
    use crate::model::{
        CheckoutSnapshot, DeviceSnapshot, PaneLayoutNodeSnapshot, PaneLayoutSnapshot, PaneSnapshot,
        RemoteSessionSnapshot, RemoteStatusSnapshot, TabSnapshot, TerminalPaneSnapshot,
        WorkspaceRegistration, WorkspaceSnapshot,
    };
    use crate::model::{MAX_PANE_TEXT_SCALE, MIN_PANE_TEXT_SCALE};
    use crate::sidebar::SessionSnapshotPayload;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_RUNTIME_STATE_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn a_foreign_grid_is_held_until_a_matching_full_frame_arrives() {
        let mut runtime = runtime();
        let pane = "w-grid:p1";
        runtime.terminal_sessions.insert(
            pane.to_owned(),
            TerminalSession::test_stub(pane, 1, TerminalSessionMode::Observe),
        );
        runtime
            .terminal_session_generations
            .insert(pane.to_owned(), 1);
        runtime
            .terminal_view_sizes
            .insert(pane.to_owned(), (45, 115));
        let before = runtime.snapshot.terminal.sequence;
        for width in [84, 2] {
            assert_eq!(
                runtime.ingest_terminal_session_frame(
                    pane,
                    1,
                    TerminalSessionMode::Observe,
                    b"foreign",
                    crate::model::TerminalFrame {
                        width,
                        height: 9,
                        full: true
                    }
                ),
                Some(false)
            );
        }
        assert_eq!(runtime.snapshot.terminal.sequence, before);
        assert_eq!(
            runtime.ingest_terminal_session_frame(
                pane,
                1,
                TerminalSessionMode::Observe,
                b"partial",
                crate::model::TerminalFrame {
                    width: 115,
                    height: 45,
                    full: false
                }
            ),
            Some(false)
        );
        assert_eq!(
            runtime.ingest_terminal_session_frame(
                pane,
                1,
                TerminalSessionMode::Observe,
                b"matching",
                crate::model::TerminalFrame {
                    width: 115,
                    height: 45,
                    full: true
                }
            ),
            Some(true)
        );
        assert_eq!(runtime.snapshot.terminal.sequence, before + 1);
        assert_eq!(
            runtime
                .snapshot
                .terminal
                .chunks
                .last()
                .unwrap()
                .frame
                .as_ref()
                .unwrap()
                .width,
            115
        );
    }

    #[test]
    fn retry_preserves_the_canvas_until_a_matching_replacement_frame() {
        let mut runtime = runtime();
        runtime.suppress_terminal_session_workers = true;
        let pane = "w-retry:p1";
        runtime.terminal_sizes.insert(pane.into(), (24, 80));
        runtime.append_terminal_chunk(pane.into(), live::encode_base64(b"last valid screen"));
        let before = runtime.snapshot.terminal.sequence;
        runtime.start_terminal_session(
            pane,
            TerminalSessionMode::Control,
            2,
            "automatic_bounded",
            None,
        );
        assert_eq!(
            runtime.snapshot.terminal.sequence, before,
            "retry must not blank the canvas"
        );
        let recovery = &runtime.terminal_recovery[pane];
        assert!(recovery.last_attempt_at_unix_ms.is_some());
        assert_eq!(
            runtime.terminal_session_lifecycles[pane].retry_decision,
            "automatic_bounded"
        );
        let generation = runtime.terminal_session_generations[pane];
        assert_eq!(
            runtime.ingest_terminal_session_frame(
                pane,
                generation,
                TerminalSessionMode::Control,
                b"wrong grid",
                crate::model::TerminalFrame {
                    width: 2,
                    height: 9,
                    full: true
                }
            ),
            Some(false)
        );
        assert_eq!(runtime.snapshot.terminal.sequence, before);
        assert_eq!(
            runtime.ingest_terminal_session_frame(
                pane,
                generation,
                TerminalSessionMode::Control,
                b"replacement",
                crate::model::TerminalFrame {
                    width: 80,
                    height: 24,
                    full: true
                }
            ),
            Some(true)
        );
        assert_eq!(runtime.snapshot.terminal.sequence, before + 1);
        assert_eq!(
            live::decode_base64(
                &runtime
                    .snapshot
                    .terminal
                    .chunks
                    .last()
                    .unwrap()
                    .bytes_base64
            )
            .unwrap(),
            b"\x1bcreplacement"
        );
        assert!(!runtime.terminal_recovery.contains_key(pane));
        assert!(runtime.terminal_session_lifecycles[pane].message.is_none());
    }

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
        runtime.snapshot.pane_layouts = vec![layout.clone()];
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
            assert_eq!(runtime.snapshot.pane_layouts, vec![layout.clone()]);
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

        // The canvas that draws this pane reports its size, and an attach is
        // held back until one has arrived.
        runtime.terminal_sizes.insert(pane_id.to_owned(), (40, 120));
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

    /// AC6's failure half: an attach that fails names its reason on the pane
    /// it failed for and leaves every other pane alone.
    ///
    /// A launch pointed at a Herdr binary that is not there reaches the
    /// runtime as exactly this spawn error. That launch cannot be staged from
    /// outside the app - `HerdrRuntimeResolver.resolve` searches absolute
    /// paths that ignore both HOME and PATH, so any staging finds the
    /// operator's installed Herdr - which is why the failure is proven here,
    /// at the boundary the failure actually crosses, rather than by a window
    /// screenshot.
    #[test]
    fn attach_failure_names_its_reason_on_that_pane_and_leaves_the_others_idle() {
        let mut runtime = runtime();
        runtime.suppress_terminal_session_workers = true;
        // Both panes are projected the way the runtime projects them, so the
        // untouched one carries a real resting state rather than a zero value
        // a hand-built struct would have handed the assertion for free.
        runtime.ensure_terminal_pane("w1:p1");
        runtime.ensure_terminal_pane("w1:p2");
        runtime
            .terminal_session_generations
            .insert("w1:p1".to_owned(), 7);
        runtime.terminal_session_lifecycles.insert(
            "w1:p1".to_owned(),
            TerminalSessionLifecycle {
                state: "starting",
                generation: 7,
                attempt: 1,
                mode: Some(TerminalSessionMode::Control),
                ..TerminalSessionLifecycle::default()
            },
        );

        let reason = "herdr terminal control failed: no such file or directory";
        assert!(runtime.ingest_terminal_session_spawn(
            7,
            "w1:p1",
            TerminalSessionMode::Control,
            Err(reason.to_owned()),
            12,
            Weak::new(),
            crate::ffi::ChangeNotifier::noop(),
        ));

        let failed = runtime
            .snapshot
            .terminal
            .panes
            .iter()
            .find(|pane| pane.pane_id == "w1:p1")
            .expect("the pane whose attach failed is still projected");
        assert_eq!(failed.transport_state, "unavailable");
        assert!(failed.transport_message.as_ref().unwrap().contains(reason));
        assert!(
            failed
                .transport_message
                .as_ref()
                .unwrap()
                .contains("Retrying in 5 seconds")
        );
        assert_eq!(
            failed.transport_exit_category.as_deref(),
            Some("spawn_failed")
        );
        assert_eq!(failed.transport_retry_decision, "automatic_bounded");

        let untouched = runtime
            .snapshot
            .terminal
            .panes
            .iter()
            .find(|pane| pane.pane_id == "w1:p2")
            .expect("the other pane is still projected");
        assert_eq!(
            untouched.transport_state, "idle",
            "one pane's attach failure must not mark another pane unavailable"
        );
        assert_eq!(untouched.transport_message, None);
        assert_eq!(untouched.transport_exit_category, None);

        // The reason is also written into that pane's own byte stream, so the
        // operator reads it where the terminal would have been and nowhere
        // else on the canvas.
        let notices: Vec<&TerminalChunk> = runtime
            .snapshot
            .terminal
            .chunks
            .iter()
            .filter(|chunk| {
                String::from_utf8(live::decode_base64(&chunk.bytes_base64).expect("chunk bytes"))
                    .is_ok_and(|text| text.contains(reason))
            })
            .collect();
        assert_eq!(notices.len(), 1, "the reason is announced once");
        assert_eq!(notices[0].pane_id, "w1:p1");
    }

    #[test]
    fn runtime_owner_conflict_observes_ignores_stale_delivery_and_reconnects_once() {
        for reason in [
            "terminal attach failed: terminal 42 already has an attached client; retry with --takeover",
            "terminal attach taken over",
        ] {
            assert_owner_conflict_observes_and_reconnects(reason);
        }
    }

    fn assert_owner_conflict_observes_and_reconnects(owner_conflict: &str) {
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
        // The pane is one the operator is looking at, so its view has already
        // reported a size; an attach is held back until one has.
        runtime.terminal_sizes.insert("w1:p1".to_owned(), (40, 120));
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
        assert_eq!(
            runtime.ingest_terminal_session_frame(
                "w1:p1",
                40,
                TerminalSessionMode::Control,
                b"stale-generation",
                crate::model::TerminalFrame {
                    width: 80,
                    height: 24,
                    full: true
                },
            ),
            None
        );
        assert_eq!(
            runtime.ingest_terminal_session_frame(
                "w1:p1",
                41,
                TerminalSessionMode::Control,
                b"stale-mode",
                crate::model::TerminalFrame {
                    width: 80,
                    height: 24,
                    full: true
                },
            ),
            None
        );
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

    /// The Scratch projection's fixture: one Herdr workspace whose panes sit
    /// in the scratch folder, plus a project pane in another directory, so
    /// every assertion about separation has both sides present.
    fn scratch_session(
        scratch_root: &str,
        project_path: &str,
        title_token: Option<&str>,
    ) -> SessionSnapshotPayload {
        let agent = match title_token {
            Some(title) => serde_json::json!({
                "pane_id": "s1:p1",
                "cwd": scratch_root,
                "agent": "claude",
                "agent_status": "idle",
                "tokens": {"activity": "0000000000001", "hide_chat_title": title}
            }),
            None => serde_json::json!({
                "pane_id": "s1:p1",
                "cwd": scratch_root,
                "agent": "claude",
                "agent_status": "idle",
                "tokens": {"activity": "0000000000001"}
            }),
        };
        serde_json::from_value(serde_json::json!({
            "agents": [agent],
            "workspaces": [
                {"workspace_id": "s1", "label": "hide scratch"},
                {"workspace_id": "w1", "label": "project"}
            ],
            "panes": [
                {"pane_id": "s1:p1", "cwd": scratch_root},
                {"pane_id": "s1:p2", "cwd": scratch_root},
                {"pane_id": "w1:p1", "cwd": project_path}
            ],
            "tabs": [
                {"workspace_id": "s1", "tab_id": "s1:t1", "label": ""},
                {"workspace_id": "s1", "tab_id": "s1:t2", "label": ""},
                {"workspace_id": "w1", "tab_id": "w1:t1", "label": ""}
            ],
            "layouts": [
                {
                    "workspace_id": "s1", "tab_id": "s1:t1", "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "s1:p1",
                    "panes": [{"pane_id": "s1:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                },
                {
                    "workspace_id": "s1", "tab_id": "s1:t2", "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "s1:p2",
                    "panes": [{"pane_id": "s1:p2", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                },
                {
                    "workspace_id": "w1", "tab_id": "w1:t1", "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "w1:p1",
                    "panes": [{"pane_id": "w1:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                }
            ]
        }))
        .expect("scratch session payload")
    }

    fn runtime_with_scratch(scratch_root: &str) -> Runtime {
        let mut runtime = runtime();
        runtime.scratch_root = scratch_root.to_owned();
        runtime.snapshot.navigator.scratch.path = scratch_root.to_owned();
        runtime
    }

    fn ingest_scratch(runtime: &mut Runtime, payload: SessionSnapshotPayload) {
        let spaces = Runtime::session_spaces(&payload, &runtime.scratch_root);
        let catalog = session_sync::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: workspace::build_catalog(&[], &spaces, &no_worktrees()),
            roots: workspace::root_index(&spaces),
        };
        runtime.ingest_session_with_catalog(Ok(payload), Some(catalog));
    }

    /// AC4: a scratch pane reaches the Scratch node and nothing else. The
    /// regression this blocks is the one the feature exists to remove: the
    /// unregistered-folder fallback drew scratch panes as an orange temporary
    /// workspace in the middle of the project tree.
    #[test]
    fn a_scratch_pane_is_in_the_scratch_node_and_in_no_project() {
        let scratch_root = "/private/tmp/hide-scratch-fixture/scratch";
        let project_path = "/private/tmp/hide-scratch-fixture/project";
        let mut runtime = runtime_with_scratch(scratch_root);
        ingest_scratch(
            &mut runtime,
            scratch_session(scratch_root, project_path, None),
        );

        let navigator = &runtime.snapshot().navigator;
        assert_eq!(navigator.scratch.tabs.len(), 2);
        assert_eq!(navigator.scratch.session_workspace_ids, ["s1"]);
        assert_eq!(navigator.scratch.path, scratch_root);

        // One project row, for the project pane, and no temporary row for the
        // scratch folder.
        assert_eq!(navigator.workspaces.len(), 1);
        assert!(
            navigator
                .workspaces
                .iter()
                .all(|workspace| !workspace.temporary)
        );
        assert!(
            !navigator
                .workspaces
                .iter()
                .any(|workspace| crate::scratch::contains(scratch_root, &workspace.path)),
            "no project row may point inside the scratch folder"
        );
        let project_pane_ids = navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .map(|pane| pane.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(project_pane_ids, ["w1:p1"]);
    }

    /// R7: closing a Scratch tab leaves the operator inside Scratch.
    ///
    /// The selection reconciliation is written around checkouts, and Scratch
    /// is in none of them. Before this, closing a Scratch tab left the closed
    /// pane selected: `pane.projection_unavailable` was re-raised on every
    /// tick and ⌘T answered "create or register a workspace" with a Scratch
    /// tab still on screen.
    #[test]
    fn closing_the_selected_scratch_pane_selects_another_scratch_pane() {
        let scratch_root = "/private/tmp/hide-scratch-close/scratch";
        let project_path = "/private/tmp/hide-scratch-close/project";
        let mut runtime = runtime_with_scratch(scratch_root);
        ingest_scratch(
            &mut runtime,
            scratch_session(scratch_root, project_path, None),
        );
        runtime.snapshot.terminal.pane_id = Some("s1:p2".to_owned());
        runtime.snapshot.ui_state.selected_pane_id = Some("s1:p2".to_owned());

        let mut after_close = scratch_session(scratch_root, project_path, None);
        after_close.panes.retain(|pane| pane.pane_id != "s1:p2");
        after_close.tabs.retain(|tab| tab.tab_id != "s1:t2");
        after_close
            .layouts
            .retain(|layout| layout.tab_id != "s1:t2");
        ingest_scratch(&mut runtime, after_close);

        let snapshot = runtime.snapshot();
        assert_eq!(
            snapshot.ui_state.selected_pane_id.as_deref(),
            Some("s1:p1"),
            "the selection moves to the Scratch pane that is left"
        );
        assert_eq!(snapshot.focused.pane_id.as_deref(), Some("s1:p1"));
        assert!(
            snapshot.status.last_error.is_none(),
            "a closed Scratch pane is an expected transition, not a projection failure: {:?}",
            snapshot.status.last_error
        );
    }

    /// AC4: the same panes twice produce the same answer. A projection that
    /// appended rather than rebuilt would grow the Scratch node on every tick.
    #[test]
    fn projecting_the_same_scratch_panes_twice_gives_the_same_node() {
        let scratch_root = "/private/tmp/hide-scratch-idempotent/scratch";
        let project_path = "/private/tmp/hide-scratch-idempotent/project";
        let mut runtime = runtime_with_scratch(scratch_root);
        ingest_scratch(
            &mut runtime,
            scratch_session(scratch_root, project_path, None),
        );
        let first = runtime.snapshot().navigator.scratch.clone();
        ingest_scratch(
            &mut runtime,
            scratch_session(scratch_root, project_path, None),
        );
        assert_eq!(first, runtime.snapshot().navigator.scratch);
    }

    /// AC5: the row's title is the token when there is one, and the tab label
    /// when there is not. An empty title is the defect this blocks: a row that
    /// shows nothing is worse than one that shows `Tab 1`.
    #[test]
    fn a_scratch_row_titles_itself_from_the_token_and_falls_back_to_the_label() {
        let scratch_root = "/private/tmp/hide-scratch-title/scratch";
        let project_path = "/private/tmp/hide-scratch-title/project";
        let mut runtime = runtime_with_scratch(scratch_root);
        ingest_scratch(
            &mut runtime,
            scratch_session(scratch_root, project_path, Some("build me a parser")),
        );

        let tabs = &runtime.snapshot().navigator.scratch.tabs;
        let titled = tabs
            .iter()
            .find(|tab| tab.id == "s1:t1")
            .expect("the agent tab");
        assert_eq!(titled.title.as_deref(), Some("build me a parser"));
        // The second tab holds no agent, so it keeps Herdr's own label.
        let plain = tabs
            .iter()
            .find(|tab| tab.id == "s1:t2")
            .expect("the terminal tab");
        assert_eq!(plain.title, None);
        assert!(!plain.label.is_empty());
    }

    /// AC5: a pane whose agent never had a title written falls back rather
    /// than showing an empty row.
    #[test]
    fn an_agent_pane_without_a_title_token_shows_its_tab_label() {
        let scratch_root = "/private/tmp/hide-scratch-untitled/scratch";
        let project_path = "/private/tmp/hide-scratch-untitled/project";
        let mut runtime = runtime_with_scratch(scratch_root);
        ingest_scratch(
            &mut runtime,
            scratch_session(scratch_root, project_path, None),
        );
        let tab = runtime
            .snapshot()
            .navigator
            .scratch
            .tabs
            .iter()
            .find(|tab| tab.id == "s1:t1")
            .expect("the agent tab")
            .clone();
        assert_eq!(tab.title, None);
        assert!(!tab.label.is_empty());
    }

    /// AC6: the section is collapsed until the operator opens it, and the
    /// choice is what the store carries back.
    #[test]
    fn the_scratch_section_is_collapsed_until_the_operator_opens_it() {
        let mut runtime = runtime_with_scratch("/private/tmp/hide-scratch-expand/scratch");
        assert!(!runtime.snapshot().navigator.scratch.expanded);
        assert!(!runtime.snapshot().ui_state.scratch_expanded);

        let event = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": [],
                "selected_path": null,
                "selected_pane_id": null,
                "scratch_expanded": true
            }
        }))
        .expect("ui state event");
        assert!(runtime.dispatch_json(&event));
        assert!(runtime.snapshot().ui_state.scratch_expanded);
        assert!(runtime.snapshot().navigator.scratch.expanded);
    }

    /// AC9: the composer's remembered agent and bypass choice survive a save
    /// that did not mention them, so a navigator save cannot reset them.
    #[test]
    fn a_save_that_omits_the_composer_fields_keeps_them() {
        let mut runtime = runtime_with_scratch("/private/tmp/hide-scratch-remember/scratch");
        let remember = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": [],
                "selected_path": null,
                "selected_pane_id": null,
                "last_agent_kind": "codex",
                "last_agent_bypass": true
            }
        }))
        .expect("composer save");
        assert!(runtime.dispatch_json(&remember));
        assert_eq!(runtime.snapshot().ui_state.last_agent_kind, "codex");
        assert!(runtime.snapshot().ui_state.last_agent_bypass);

        let unrelated = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": ["/private/tmp"],
                "selected_path": null,
                "selected_pane_id": null
            }
        }))
        .expect("navigator save");
        assert!(runtime.dispatch_json(&unrelated));
        assert_eq!(runtime.snapshot().ui_state.last_agent_kind, "codex");
        assert!(runtime.snapshot().ui_state.last_agent_bypass);
    }

    /// D8: only the pane's detected kind permits an ordinary click report.
    /// A title, a different pane's agent, and missing detection permit no bytes.
    #[test]
    fn ordinary_click_uses_detected_pane_agent_and_never_sends_enter() {
        let mut runtime = runtime();
        let pane_id = "w-click:p1";
        runtime.terminal_sessions.insert(
            pane_id.to_owned(),
            live::TerminalSession::test_stub(pane_id, 1, TerminalSessionMode::Control),
        );
        let click = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "terminal_click",
            "payload": {"pane_id": pane_id, "column": 7, "row": 3, "modifiers": 3}
        }))
        .unwrap();
        for (detected_pane, kind, expected) in [
            ("w-click:p2", "claude", false),
            (pane_id, "codex", false),
            (pane_id, "unknown", false),
            (pane_id, "claude", true),
        ] {
            let payload = serde_json::from_value(serde_json::json!({
                "agents": [{"pane_id": detected_pane, "agent": kind, "state_change_seq": 1}]
            }))
            .unwrap();
            runtime.snapshot.navigator.agents = crate::sidebar::project_agents(payload).agents;
            runtime.dispatch_json(&click);
            let lines = runtime.terminal_sessions[pane_id].test_written_lines();
            assert_eq!(lines.len(), usize::from(expected), "{detected_pane} {kind}");
            if expected {
                let line: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
                assert_eq!(line["type"], "terminal.input");
                let bytes = live::decode_base64(line["bytes"].as_str().unwrap()).unwrap();
                assert_eq!(bytes, b"\x1b[<20;8;4M\x1b[<20;8;4m");
                assert!(!bytes.contains(&b'\r') && !bytes.contains(&b'\n'));
            }
        }
        runtime.snapshot.navigator.agents.clear();
        runtime.dispatch_json(&click);
        assert!(
            runtime.terminal_sessions[pane_id]
                .test_written_lines()
                .is_empty()
        );
    }

    #[test]
    fn removing_registration_in_use_preserves_it_and_explains_recovery() {
        let mut runtime = runtime();
        let path =
            std::env::temp_dir().join(format!("hide-registration-busy-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let registration =
            workspace::registration(path.to_str().unwrap(), "Busy project", "local").unwrap();
        runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
        runtime.last_session_spaces = vec![workspace::SessionSpace {
            id: "w1".into(),
            label: "Busy project".into(),
            cwds: vec![registration.path.clone()],
        }];
        runtime.rebuild_catalog();
        runtime.dispatch_json(&serde_json::to_vec(&serde_json::json!({
            "schema_version": 2, "kind": "remove_workspace", "payload": {"workspace_id": registration.id}
        })).unwrap());
        assert_eq!(
            runtime.snapshot.ui_state.workspace_registrations,
            vec![registration]
        );
        assert_eq!(
            runtime.snapshot.status.last_error.as_ref().unwrap().kind,
            "workspace.registration_in_use"
        );
        assert!(path.is_dir(), "Registration removal must not delete files");
        std::fs::remove_dir(path).unwrap();
    }

    #[test]
    fn removing_registration_converges_without_git_or_repeat_publication() {
        let mut runtime = runtime();
        let path =
            std::env::temp_dir().join(format!("hide-registration-empty-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let registration =
            workspace::registration(path.to_str().unwrap(), "Empty project", "local").unwrap();
        runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
        runtime.rebuild_catalog();
        let event = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2, "kind": "remove_workspace", "payload": {"workspace_id": registration.id}
        })).unwrap();
        let git_before = workspace::git_calls_on_this_thread();
        assert!(runtime.dispatch_json(&event));
        assert!(runtime.snapshot.ui_state.workspace_registrations.is_empty());
        assert!(runtime.snapshot.navigator.workspaces.is_empty());
        assert_eq!(workspace::git_calls_on_this_thread(), git_before);
        assert!(
            !runtime.dispatch_json(&event),
            "The same target state is already reached"
        );
        assert!(runtime.snapshot.status.last_error.is_none());
        assert!(path.is_dir());
        std::fs::remove_dir(path).unwrap();
    }

    fn context_payload() -> SessionSnapshotPayload {
        serde_json::from_value(serde_json::json!({
            "agents": [
                {"pane_id":"w1:p1", "agent":"codex", "agent_status":"working", "state_change_seq":10,
                 "cwd":"/tmp/hide-context-alpha", "tokens":{"activity":"1788871000000"}},
                {"pane_id":"w2:p1", "agent":"codex", "agent_status":"working", "state_change_seq":20,
                 "cwd":"/tmp/hide-context-zeta", "tokens":{"activity":"1788872000000"},
                 "agent_session":{"kind":"id", "value":"context-session"}, "spawned_from_pane_id":"w1:p1"}
            ],
            "workspaces":[{"workspace_id":"w1","label":"Alpha"},{"workspace_id":"w2","label":"Zeta"}],
            "tabs":[{"workspace_id":"w1","tab_id":"w1:t1","label":"1"},{"workspace_id":"w2","tab_id":"w2:t1","label":"1"}],
            "panes":[{"pane_id":"w1:p1","cwd":"/tmp/hide-context-alpha"},{"pane_id":"w2:p1","cwd":"/tmp/hide-context-zeta"}],
            "layouts":[
                {"workspace_id":"w1","tab_id":"w1:t1","zoomed":false,"area":{"x":0,"y":0,"width":80,"height":24},
                 "focused_pane_id":"w1:p1","panes":[{"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":80,"height":24}}],"splits":[]},
                {"workspace_id":"w2","tab_id":"w2:t1","zoomed":false,"area":{"x":0,"y":0,"width":80,"height":24},
                 "focused_pane_id":"w2:p1","panes":[{"pane_id":"w2:p1","rect":{"x":0,"y":0,"width":80,"height":24}}],"splits":[]}
            ]
        })).unwrap()
    }

    #[test]
    fn cleanup_review_without_live_state_is_visible_and_never_deletes() {
        let mut runtime = runtime();
        runtime.ingest_session(Ok(context_payload()));
        let project = runtime.snapshot.navigator.workspaces[0].clone();
        runtime.focus_checkout(&project.id, &project.checkouts[0].id);
        let root = runtime.focused_local_checkout().unwrap().0.path.clone();
        runtime.ingest_worktrees(crate::model::WorktreeCatalogSnapshot {
            projects: vec![crate::model::ProjectWorktreesSnapshot {
                root_path: root,
                ..Default::default()
            }],
        });
        runtime.dispatch_json(br#"{"schema_version":2,"kind":"cleanup_review","payload":{}}"#);
        let snapshot = serde_json::to_value(runtime.snapshot()).unwrap();
        assert_eq!(snapshot["git_worktrees"]["cleanup"]["phase"], "failed");
        assert!(
            snapshot["git_worktrees"]["cleanup"]["message"]
                .as_str()
                .unwrap()
                .contains("live Herdr connection")
        );
        runtime.dispatch_json(br#"{"schema_version":2,"kind":"cleanup_dismiss","payload":{}}"#);
        assert!(
            serde_json::to_value(runtime.snapshot()).unwrap()["git_worktrees"]["cleanup"].is_null()
        );
    }

    #[test]
    fn overview_inspection_does_not_focus_or_repeat_publish() {
        struct CountConnections(std::sync::mpsc::Sender<()>);
        impl crate::herdr_api::ApiConnector for CountConnections {
            fn connect(
                &self,
            ) -> Result<Box<dyn crate::herdr_api::ApiStream>, crate::herdr_api::ApiError>
            {
                let _ = self.0.send(());
                Err(crate::herdr_api::ApiError::Transport(
                    "fixture refused".into(),
                ))
            }
        }
        let mut runtime = live_runtime();
        runtime.ingest_session(Ok(context_payload()));
        let mut target = runtime.snapshot.navigator.workspaces[1].checkouts[0].clone();
        target.workspace_id = runtime.snapshot.navigator.workspaces[0].id.clone();
        runtime.snapshot.navigator.workspaces[0]
            .checkouts
            .push(target.clone());
        let project = runtime.snapshot.navigator.workspaces[0].clone();
        runtime.focus_checkout(&project.id, &project.checkouts[0].id);
        let focused = runtime.snapshot.focused.clone();
        let navigator = runtime.snapshot.navigator.clone();
        let ui = runtime.snapshot.ui_state.clone();
        let (send, receive) = std::sync::mpsc::channel();
        runtime.live.as_mut().unwrap().api_connector = Arc::new(CountConnections(send));
        let event = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2, "kind": "overview_select", "payload": {"checkout_path": target.path}
        })).unwrap();
        runtime.dispatch_json(&event);
        assert_eq!(
            runtime.snapshot.card.inspected_checkout_path,
            Some(target.path)
        );
        assert_eq!(runtime.snapshot.focused, focused);
        assert_eq!(runtime.snapshot.navigator, navigator);
        assert_eq!(runtime.snapshot.ui_state, ui);
        assert!(
            !runtime.dispatch_json(&event),
            "Repeating an inspection has no effects"
        );
        assert!(
            receive
                .recv_timeout(std::time::Duration::from_millis(30))
                .is_err(),
            "Inspection sent a Herdr request"
        );
        let pane = &target.tabs[0].panes[0].id;
        runtime.dispatch_json(&operator_focus_event(pane));
        assert!(
            receive
                .recv_timeout(std::time::Duration::from_secs(1))
                .is_ok(),
            "Explicit Open must notify Herdr"
        );
    }

    #[test]
    fn projects_follow_authoritative_activity_and_identical_snapshots_settle() {
        let mut runtime = runtime();
        let mut payload = context_payload();
        runtime.ingest_session(Ok(payload.clone()));
        assert!(
            runtime.snapshot.navigator.workspaces[0]
                .path
                .ends_with("/hide-context-zeta")
        );
        runtime.ingest_session(Ok(payload.clone()));
        let before = runtime.snapshot_delta_payload(0, 0);
        let revision = before.revision;
        for _ in 0..5 {
            runtime.ingest_session(Ok(payload.clone()));
            let delta = runtime.snapshot_delta_payload(revision, 0);
            assert!(
                delta.rest.is_none(),
                "Unchanged activity must not resend the navigator"
            );
        }
        payload.agents[0]
            .tokens
            .insert("activity".into(), serde_json::json!("1788873000000"));
        runtime.ingest_session(Ok(payload));
        assert!(
            runtime.snapshot.navigator.workspaces[0]
                .path
                .ends_with("/hide-context-alpha")
        );
    }

    #[test]
    fn overview_tracks_live_checkout_panes_and_drops_retired_lineage() {
        let mut runtime = runtime();
        let mut payload = context_payload();
        runtime.ingest_session(Ok(payload.clone()));
        let project = runtime
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|w| w.path.ends_with("zeta"))
            .unwrap()
            .clone();
        runtime.focus_checkout(&project.id, &project.checkouts[0].id);
        runtime.refresh_card();
        assert_eq!(runtime.snapshot.card.panes.len(), 1);
        assert_eq!(runtime.snapshot.card.panes[0].pane_id, "w2:p1");
        assert_eq!(
            runtime.snapshot.card.panes[0].session_id.as_deref(),
            Some("context-session")
        );
        assert_eq!(
            runtime.snapshot.card.panes[0].parent_pane_id.as_deref(),
            Some("w1:p1")
        );
        payload.agents.remove(0);
        payload.panes.remove(0);
        payload.layouts.remove(0);
        payload.tabs.remove(0);
        payload.workspaces.remove(0);
        runtime.ingest_session(Ok(payload.clone()));
        runtime.refresh_card();
        assert_eq!(runtime.snapshot.card.panes[0].parent_pane_id, None);
        payload.panes[0].cwd = Some("/tmp/hide-context-moved".into());
        payload.agents[0].cwd = Some("/tmp/hide-context-moved".into());
        runtime.ingest_session(Ok(payload));
        runtime.refresh_card();
        assert!(
            runtime.snapshot.card.panes.is_empty(),
            "A pane moved away is no longer checkout context"
        );
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

    #[test]
    fn branch_migration_receipt_preserves_core_focus() {
        let mut runtime = runtime();
        runtime.snapshot.terminal.pane_id = Some("existing:pane".into());
        runtime.snapshot.focused.surface = Surface::Terminal;
        runtime.snapshot.focused.pane_id = Some("existing:pane".into());
        runtime.snapshot.ui_state.selected_pane_id = Some("existing:pane".into());
        let id = runtime
            .begin_task_operation(
                "branch_migrate",
                Some("/fixture/repo".into()),
                Some("feature".into()),
                Some("main".into()),
                None,
            )
            .unwrap();

        assert!(runtime.ingest_task_operation_result(
            id,
            Ok(live::WorktreeTaskOutcome {
                path: "/fixture/worktree".into(),
                pane_id: "new:pane".into(),
            })
        ));
        assert_eq!(
            runtime.snapshot.terminal.pane_id.as_deref(),
            Some("existing:pane")
        );
        assert_eq!(
            runtime.snapshot.focused.pane_id.as_deref(),
            Some("existing:pane")
        );
        assert_eq!(
            runtime.snapshot.ui_state.selected_pane_id.as_deref(),
            Some("existing:pane")
        );
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
        assert!(
            !runtime
                .snapshot()
                .ui_state
                .pane_text_scales
                .contains_key("w1:p2")
        );

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
            runtime
                .snapshot()
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.clone()),
            Some("pane.text_scale_unknown_direction".to_owned())
        );
    }

    /// A runtime with a live context pointed at a socket that does not exist.
    ///
    /// Pane focus needs a live connection to be dispatched at all, and the
    /// worker it spawns fails on its own without touching this runtime, so a
    /// test can drive the real focus event rather than a shortcut into the
    /// read record.
    fn live_runtime() -> Runtime {
        let mut runtime = runtime();
        let socket_path = std::env::temp_dir()
            .join(format!(
                "herdr-core-read-record-{}-{}.sock",
                std::process::id(),
                NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
            ))
            .to_string_lossy()
            .into_owned();
        runtime.live = Some(live::LiveContext {
            socket_path: socket_path.clone().into(),
            herdr_bin: None,
            runtime: std::sync::Weak::new(),
            notifier: crate::ffi::ChangeNotifier::noop(),
            api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
        });
        runtime
    }

    /// The event the shell sends when the operator clicks an agent row, a
    /// pane, or picks one from the switcher.
    fn operator_focus_event(pane_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_pane",
            "payload": {"pane_id": pane_id, "origin": "operator"}
        }))
        .expect("focus pane event")
    }

    /// The event the shell sends once on launch to put the terminal back on
    /// the pane the last session ended on.
    fn restore_focus_event(pane_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_pane",
            "payload": {"pane_id": pane_id, "origin": "restore"}
        }))
        .expect("restore focus event")
    }

    /// The panes the sidebar still shows as unread, in snapshot order.
    fn unread_panes(runtime: &Runtime) -> Vec<String> {
        let mut panes = runtime
            .snapshot
            .navigator
            .agents
            .iter()
            .filter(|agent| agent.unread)
            .map(|agent| agent.pane_id.clone())
            .collect::<Vec<_>>();
        panes.sort();
        panes
    }

    /// Three finished panes side by side in one tab, the shape the operator
    /// reported. `focused` is the pane Herdr names as that tab's focus, which
    /// is a tab-scoped verdict Hide's read axis must not follow.
    fn finished_tab_payload(panes: &[(&str, u64)], focused: &str) -> SessionSnapshotPayload {
        let agents = panes
            .iter()
            .map(|(pane_id, seq)| {
                serde_json::json!({
                    "pane_id": pane_id,
                    "workspace_label": "Fixture",
                    "agent": "codex",
                    "agent_status": "done",
                    "state_change_seq": seq,
                    "tokens": {"status_done_new": "\u{25cf}", "activity": "0000000000001"}
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(panes.len(), 3, "the fixture is three panes side by side");
        let layout_panes = panes
            .iter()
            .enumerate()
            .map(|(index, (pane_id, _))| {
                serde_json::json!({
                    "pane_id": pane_id,
                    "rect": {"x": index * 30, "y": 0, "width": 30, "height": 24}
                })
            })
            .collect::<Vec<_>>();
        serde_json::from_value(serde_json::json!({
            "agents": agents,
            "tabs": [{"workspace_id": "w1", "tab_id": "t1", "label": ""}],
            "layouts": [{
                "workspace_id": "w1", "tab_id": "t1", "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 90, "height": 24},
                "focused_pane_id": focused,
                "panes": layout_panes,
                "splits": [
                    {"direction": "right", "ratio": 0.333_333_34,
                     "rect": {"x": 0, "y": 0, "width": 90, "height": 24}},
                    {"direction": "right", "ratio": 0.5,
                     "rect": {"x": 30, "y": 0, "width": 60, "height": 24}}
                ]
            }]
        }))
        .expect("session payload")
    }

    fn working_payload() -> SessionSnapshotPayload {
        serde_json::from_value(serde_json::json!({
            "agents": [{
                "pane_id": "w1:p1",
                "workspace_label": "Fixture",
                "agent": "codex",
                "agent_status": "working",
                "tokens": {"status_working": "\u{25cf}", "activity": "0000000000001"}
            }],
            "tabs": [{"workspace_id": "w1", "tab_id": "t1", "label": ""}],
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
            content: crate::pane_content::PaneContent::Terminal,
            herdr_label: None,
            terminal_title: None,
            workspace_label: None,
            cwd: cwd.to_owned(),
            status_label: "Attached".to_owned(),
            requires_close_confirmation: false,
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
            next_tab_label: crate::model::next_tab_label(std::iter::empty()),
            id: checkout_id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            label: checkout_id.to_owned(),
            path: path.to_owned(),
            branch: None,
            is_worktree: false,
            exists: true,
            temporary: false,
            has_panes: pane.is_some(),
            tabs: pane
                .clone()
                .map(|pane| vec![tab(workspace_id, checkout_id, Some(pane))])
                .unwrap_or_default(),
            active_tab_id: pane.map(|_| format!("{checkout_id}:tab")),
            strip: Vec::new(),
            ..CheckoutSnapshot::default()
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
            branches: Vec::new(),
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
                "collapsed_checkout_ids": ["checkout-a"],
                "selected_path": null,
                "selected_pane_id": null,
                "shortcut_bindings": {}
            }
        }))
        .expect("collapse workspace event");

        assert!(runtime.dispatch_json(&collapse));
        assert!(!runtime.snapshot().navigator.workspaces[0].expanded);
        assert_eq!(
            runtime.snapshot().ui_state.collapsed_checkout_ids,
            ["checkout-a"]
        );

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
        // An unrelated UI save must not reopen a collapsed checkout.
        assert_eq!(
            runtime.snapshot().ui_state.collapsed_checkout_ids,
            ["checkout-a"]
        );
        let reopen = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": [],
                "collapsed_checkout_ids": [],
                "selected_path": null,
                "selected_pane_id": null
            }
        }))
        .unwrap();
        assert!(runtime.dispatch_json(&reopen));
        assert!(
            runtime
                .snapshot()
                .ui_state
                .collapsed_checkout_ids
                .is_empty()
        );
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
        assert!(runtime.snapshot().active_pane_layout().is_none());
        assert!(runtime.snapshot().terminal.panes.is_empty());
    }

    /// One Herdr workspace whose tabs each hold one pane inside
    /// `checkout_path`. `tab_order` is the order Herdr reports its tabs in and
    /// `layout_order` is the order the layouts arrive in, so a test can hand
    /// the two orders apart and see which one the navigator follows.
    fn tab_order_payload(
        checkout_path: &str,
        tab_order: &[&str],
        layout_order: &[&str],
        active_tab_id: &str,
    ) -> SessionSnapshotPayload {
        let tabs = tab_order
            .iter()
            .map(|tab_id| {
                serde_json::json!({
                    "workspace_id": "w-order", "tab_id": tab_id, "label": ""
                })
            })
            .collect::<Vec<_>>();
        let panes = layout_order
            .iter()
            .map(|tab_id| {
                serde_json::json!({"pane_id": format!("{tab_id}:p"), "cwd": checkout_path})
            })
            .collect::<Vec<_>>();
        let layouts = layout_order
            .iter()
            .map(|tab_id| {
                serde_json::json!({
                    "workspace_id": "w-order",
                    "tab_id": tab_id,
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": format!("{tab_id}:p"),
                    "panes": [{
                        "pane_id": format!("{tab_id}:p"),
                        "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                    }],
                    "splits": []
                })
            })
            .collect::<Vec<_>>();
        // Herdr's keyboard is in the workspace, on the active tab's pane,
        // which is what a live `session.snapshot` reports.
        serde_json::from_value(serde_json::json!({
            "agents": [],
            "focused_workspace_id": "w-order",
            "focused_pane_id": format!("{active_tab_id}:p"),
            "workspaces": [{
                "workspace_id": "w-order",
                "label": "order",
                "active_tab_id": active_tab_id
            }],
            "tabs": tabs,
            "panes": panes,
            "layouts": layouts
        }))
        .expect("ordered session payload")
    }

    /// Two or more Herdr workspaces whose tabs all sit in `checkout_path`, so
    /// the path-keyed navigator folds them into one checkout. Each entry is
    /// `(workspace_id, tab_ids, active_tab_id)`; `focused_workspace_id` is
    /// the one holding Herdr's keyboard, on its active tab's pane. Panes are
    /// named `<tab>:p`.
    fn split_checkout_payload(
        checkout_path: &str,
        workspaces: &[(&str, &[&str], &str)],
        focused_workspace_id: &str,
    ) -> SessionSnapshotPayload {
        let mut sessions = Vec::new();
        let mut tabs = Vec::new();
        let mut panes = Vec::new();
        let mut layouts = Vec::new();
        for (workspace_id, tab_ids, active_tab_id) in workspaces {
            sessions.push(serde_json::json!({
                "workspace_id": workspace_id,
                "label": workspace_id,
                "active_tab_id": active_tab_id
            }));
            for tab_id in tab_ids.iter() {
                tabs.push(serde_json::json!({
                    "workspace_id": workspace_id, "tab_id": tab_id, "label": ""
                }));
                panes.push(serde_json::json!({
                    "pane_id": format!("{tab_id}:p"), "cwd": checkout_path
                }));
                layouts.push(serde_json::json!({
                    "workspace_id": workspace_id,
                    "tab_id": tab_id,
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": format!("{tab_id}:p"),
                    "panes": [{
                        "pane_id": format!("{tab_id}:p"),
                        "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                    }],
                    "splits": []
                }));
            }
        }
        let focused_pane_id = workspaces
            .iter()
            .find(|(workspace_id, _, _)| *workspace_id == focused_workspace_id)
            .map(|(_, _, active_tab_id)| format!("{active_tab_id}:p"))
            .expect("the focused workspace is one of the listed workspaces");
        serde_json::from_value(serde_json::json!({
            "agents": [],
            "focused_workspace_id": focused_workspace_id,
            "focused_pane_id": focused_pane_id,
            "workspaces": sessions,
            "tabs": tabs,
            "panes": panes,
            "layouts": layouts
        }))
        .expect("split checkout session payload")
    }

    /// A runtime with one registered checkout focused, ready to ingest
    /// [`tab_order_payload`]. Returns the checkout id the navigator gave it.
    fn tab_order_runtime(checkout_path: &str) -> (Runtime, String) {
        let mut runtime = runtime();
        runtime.snapshot.ui_state.workspace_registrations = vec![WorkspaceRegistration {
            id: "workspace:order".to_owned(),
            label: "order".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        }];
        runtime.rebuild_catalog();
        let checkout_id =
            workspace::checkout_id_for_path("workspace:order", Path::new(checkout_path));
        runtime.snapshot.navigator.focused_workspace_id = Some("workspace:order".to_owned());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        // A selection the operator made, so the first live session keeps it
        // instead of retiring it with the restore hint.
        runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
        runtime.reset_terminal_projection(None);
        (runtime, checkout_id)
    }

    fn ordered_tab_ids(runtime: &Runtime, checkout_id: &str) -> Vec<String> {
        runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .find(|checkout| checkout.id == checkout_id)
            .expect("the registered checkout")
            .tabs
            .iter()
            .map(|tab| tab.id.clone().expect("a Herdr tab always has an id"))
            .collect()
    }

    /// Every tab in the session ships its own layout, keyed by its tab id.
    /// Before this the snapshot carried one layout, so a tab the operator was
    /// not looking at had no geometry to draw and switching to it had to wait
    /// for Herdr to send one.
    #[test]
    fn tab_layouts_carry_every_tab_in_the_session() {
        let checkout_path = "/private/tmp/hide-tab-layouts-all";
        let (mut runtime, _checkout_id) = tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));

        assert_eq!(
            runtime
                .snapshot()
                .pane_layouts
                .iter()
                .map(|layout| layout.tab_id.clone())
                .collect::<Vec<_>>(),
            tabs.map(str::to_owned).to_vec()
        );
        // Each entry is that tab's own geometry rather than a copy of the
        // visible one.
        for tab_id in tabs {
            let layout = runtime
                .snapshot()
                .pane_layouts
                .iter()
                .find(|layout| layout.tab_id == tab_id)
                .expect("every tab has a layout")
                .clone();
            assert_eq!(layout.focused_pane_id, format!("{tab_id}:p"));
        }
    }

    /// Switching tabs empties nothing. The tab being selected already has its
    /// geometry, so the canvas draws it on the same dispatch instead of
    /// showing an empty canvas until Herdr confirms the focus.
    #[test]
    fn tab_layouts_survive_a_tab_switch_with_no_empty_canvas() {
        let checkout_path = "/private/tmp/hide-tab-layouts-switch";
        let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        assert_eq!(
            runtime
                .snapshot()
                .active_pane_layout()
                .map(|layout| layout.tab_id.clone()),
            Some("w-order:t1".to_owned())
        );

        let focus = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_tab",
            "payload": {
                "workspace_id": "workspace:order",
                "checkout_id": checkout_id,
                "tab_id": "w-order:t3"
            }
        }))
        .expect("focus tab event");
        assert!(runtime.dispatch_json(&focus));

        // Every tab still has its layout, and the canvas is already drawing
        // the one that was asked for.
        assert_eq!(
            runtime
                .snapshot()
                .pane_layouts
                .iter()
                .map(|layout| layout.tab_id.clone())
                .collect::<Vec<_>>(),
            tabs.map(str::to_owned).to_vec()
        );
        assert_eq!(
            runtime
                .snapshot()
                .active_pane_layout()
                .map(|layout| layout.tab_id.clone()),
            Some("w-order:t3".to_owned())
        );
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w-order:t3:p")
        );
    }

    /// A layout update for one tab changes that tab's entry and no other, so
    /// redrawing one tab cannot disturb what another tab draws.
    #[test]
    fn tab_layouts_update_only_the_tab_whose_layout_changed() {
        let checkout_path = "/private/tmp/hide-tab-layouts-one";
        let (mut runtime, _checkout_id) = tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        let before = runtime.snapshot().pane_layouts.clone();

        let mut next = tab_order_payload(checkout_path, &tabs, &tabs, "w-order:t1");
        next.layouts
            .iter_mut()
            .find(|layout| layout.tab_id == "w-order:t2")
            .expect("the second tab's layout")
            .zoomed = true;
        assert!(runtime.ingest_session(Ok(next)));

        let after = runtime.snapshot().pane_layouts.clone();
        assert_eq!(after.len(), before.len());
        for (was, now) in before.iter().zip(after.iter()) {
            if now.tab_id == "w-order:t2" {
                assert!(!was.zoomed);
                assert!(now.zoomed);
            } else {
                assert_eq!(was, now);
            }
        }
    }

    /// A layout Herdr has already sent still moves the canvas when it belongs
    /// to another tab. The return value is what fires the change notifier, so
    /// it has to report the projection that was rebuilt and not only the
    /// geometry that was compared. Before layouts survived a tab switch the
    /// two could not disagree, because a switch emptied the stored layout and
    /// every following layout counted as new.
    #[test]
    fn tab_layouts_report_a_projection_move_under_an_unchanged_layout() {
        let checkout_path = "/private/tmp/hide-tab-layouts-notify";
        let (mut runtime, _checkout_id) = tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));

        let second = runtime
            .snapshot()
            .pane_layouts
            .iter()
            .find(|layout| layout.tab_id == "w-order:t2")
            .expect("the second tab's layout")
            .clone();

        // The geometry is the one already stored, so only the projection
        // moves. That move still has to be reported.
        assert!(runtime.apply_pane_layout(second.clone()));
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w-order:t2:p")
        );

        // The same layout over the same projection changes nothing, and
        // reports nothing, so the canvas is not redrawn for a repeat.
        assert!(!runtime.apply_pane_layout(second));
    }

    /// The shell used to draw an even grid of the tab's panes whenever it had
    /// no layout, which is a geometry Herdr never applied and which decides
    /// the PTY size. With every tab's layout in the snapshot there is nothing
    /// left for it to stand in for, and nothing may bring it back.
    #[test]
    fn tab_layouts_have_no_uniform_grid_stand_in_left_in_the_shell() {
        let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS");
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&shell).expect("the shell source directory") {
            let path = entry.expect("a shell source entry").path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("swift") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a readable Swift source");
            if source.contains("uniformItems") {
                offenders.push(path.display().to_string());
            }
        }
        assert!(
            offenders.is_empty(),
            "a uniform pane grid stand-in is back in {offenders:?}"
        );
    }

    /// R8, AC14. The agent notes are what the next person reads before they
    /// touch this subsystem, and both of their claims about it were wrong: the
    /// shell was described as reading the snapshot on every change
    /// notification, and the only measured figure in the performance guide was
    /// a mutex wait from an incident measured while typing on a build three
    /// rounds of work ago. A document drifts silently, so the claims that
    /// matter are asserted here rather than trusted.
    #[test]
    fn agent_notes_state_the_view_authority_and_the_announcement_rule() {
        let notes =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../AGENTS.md"))
                .expect("the agent notes");
        let (architecture, rest) = notes
            .split_once("## Runtime Architecture")
            .expect("a Runtime Architecture section");
        assert!(
            architecture.len() < rest.len(),
            "the split must put the section body on the right"
        );
        let (architecture, _) = rest
            .split_once("## Herdr API Contract")
            .expect("Runtime Architecture ends at the Herdr API Contract");
        let (_, performance) = notes
            .split_once("## Performance Guide")
            .expect("a Performance Guide section");

        for (section, name, wanted) in [
            (
                architecture,
                "Runtime Architecture",
                vec![
                    // The boundary itself, both halves of it.
                    "visible tab",
                    "keyboard focus pane",
                    // The four paths a core-owned value can take.
                    "pending",
                    "followed",
                    "refusal",
                    // What replaced the per-change read and the second core.
                    "once per burst",
                    "before** it takes the lock",
                    "creates the core once",
                ],
            ),
            (
                performance,
                "Performance Guide",
                vec![
                    "serialize_snapshot_delta",
                    "ChangeNotifier",
                    "idle",
                    "driven",
                ],
            ),
        ] {
            for phrase in wanted {
                assert!(
                    section.contains(phrase),
                    "AGENTS.md {name} does not say {phrase:?}"
                );
            }
        }
        assert!(
            !performance.contains("main thread spent 47% of wall time"),
            "the stale 47% figure is replaced by measurements with their load, not kept beside them"
        );
    }

    /// R5, AC9. A launch used to create a core without the resolved Herdr
    /// binary, start its session sync, and then throw both away for a second
    /// core once the runtime was known. Destroying the first one joined a
    /// worker mid-bootstrap, which is where the measured 360 ms between the
    /// runtime resolving and the second core being ready went. One core per
    /// launch means one `session.snapshot`, one subscription and one catalog
    /// build, and the shell is the only place that can put the second one
    /// back.
    #[test]
    fn one_launch_creates_the_core_once_and_never_replaces_it() {
        let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS");
        let mut creations = Vec::new();
        let mut destructions = Vec::new();
        for entry in std::fs::read_dir(&shell).expect("the shell source directory") {
            let path = entry.expect("a shell source entry").path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("swift") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a readable Swift source");
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            for line in source.lines().map(str::trim) {
                if line.starts_with("//") {
                    continue;
                }
                if line.contains("herdr_core_create(") {
                    creations.push(format!("{name}: {line}"));
                }
                if line.contains("herdr_core_destroy(") {
                    destructions.push(format!("{name}: {line}"));
                }
            }
        }
        assert_eq!(
            creations.len(),
            1,
            "the shell must call herdr_core_create from one place: {creations:?}"
        );
        assert_eq!(
            destructions.len(),
            1,
            "a core is destroyed only when the bridge goes away: {destructions:?}"
        );
        let bridge = std::fs::read_to_string(shell.join("CoreBridge.swift"))
            .expect("the core bridge source");
        assert!(
            !bridge.contains("replaceCore"),
            "replacing a live core with a second one is the path this removed"
        );
        assert!(
            bridge.contains("runtimePreparation"),
            "the one core is created after the runtime resolves, so the resolution has to be awaited"
        );
    }

    /// R6, AC12. Herdr publishes a session snapshot on every event and on
    /// every agent refresh, and the wire has to be sized by what changed. A
    /// republish that moved nothing must leave the reader's cursor current, or
    /// the whole navigator, ui state, status and pet sections ride the next
    /// read for nothing.
    #[test]
    fn snapshot_delivery_leaves_the_rest_section_alone_when_a_republish_moves_nothing() {
        fn session() -> SessionSnapshotPayload {
            serde_json::from_value(serde_json::json!({
                "agents": [],
                "workspaces": [{"workspace_id": "w1", "label": "fixture"}],
                "panes": [{"pane_id": "w1:p1", "cwd": "/tmp/fixture"}],
                "tabs": [{"workspace_id": "w1", "tab_id": "w1:t1", "label": "1"}],
                "layouts": [{
                    "workspace_id": "w1",
                    "tab_id": "w1:t1",
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "w1:p1",
                    "panes": [{
                        "pane_id": "w1:p1",
                        "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                    }],
                    "splits": []
                }]
            }))
            .expect("a session payload")
        }

        let mut runtime = runtime();
        runtime.ingest_session(Ok(session()));
        let first = runtime.snapshot_delta_payload(0, 0);
        assert!(first.rest.is_some(), "a fresh cursor reads the whole state");
        let caught_up = first.revision;

        runtime.ingest_session(Ok(session()));
        let second = runtime.snapshot_delta_payload(caught_up, first.terminal_sequence);
        assert!(
            second.rest.is_none(),
            "an identical republish must not restamp the rest section"
        );
        assert_eq!(
            second.revision, caught_up,
            "a republish that moved nothing leaves the reader current"
        );
    }

    /// R6, AC12. The delta used to be serialized with the runtime mutex held,
    /// so every attach thread and the shell's next read waited behind the
    /// whole navigator, ui state and terminal output going through serde.
    ///
    /// The split is enforced twice. The signature is the first half: a
    /// payload owns everything the wire needs, so the function that turns it
    /// into bytes has no runtime in scope to lock. The call site is the
    /// second: the guard is taken for the payload and gone before serde runs.
    #[test]
    fn snapshot_delivery_serializes_the_delta_outside_the_runtime_lock() {
        let mut runtime = runtime();
        let payload = runtime.snapshot_delta_payload(0, 0);
        // A payload outlives the borrow it came from, which is what lets the
        // caller drop the guard between the two halves.
        drop(runtime);
        let serialize: fn(
            &crate::model::SnapshotDeltaPayload,
        ) -> Result<Vec<u8>, serde_json::Error> = serialize_snapshot_delta;
        let bytes = serialize(&payload).expect("a payload serializes on its own");
        assert!(!bytes.is_empty(), "the wire is written from the payload");

        let ffi = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ffi.rs"))
            .expect("the ffi source");
        let entry_point = ffi
            .split_once("pub extern \"C\" fn herdr_core_snapshot(")
            .expect("the snapshot entry point")
            .1;
        let body = entry_point
            .split_once("#[unsafe(no_mangle)]")
            .expect("the entry point after it")
            .0;
        assert!(
            body.contains("serialize_snapshot_delta(&payload)"),
            "the shell's read must serialize through the free function: {body}"
        );
        let between = body
            .split_once("snapshot_delta_payload(")
            .expect("the locked half")
            .1
            .split_once("serialize_snapshot_delta(")
            .expect("the unlocked half after it")
            .0;
        assert!(
            between.lines().any(|line| line.trim() == "};"),
            "the block scoping the runtime guard must close before serialization: {body}"
        );

        let source =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime.rs"))
                .expect("the runtime source");
        let locked_half = source
            .split_once("pub fn snapshot_delta_payload(")
            .expect("the payload function")
            .1
            .split_once("\n    pub fn ")
            .expect("the function after it")
            .0;
        assert!(
            !locked_half.contains("serde_json"),
            "the half that runs under the lock must not serialize: {locked_half}"
        );
    }

    /// R5, AC9. Herdr sizes a pane's PTY from the attach, so an attach before
    /// any view has reported a size draws a full frame at a guess and a
    /// second one after the resize corrects it. The wait is reported, because
    /// a pane that never attaches must not look like a pane with no output.
    #[test]
    fn one_launch_holds_an_attach_until_the_view_reports_a_size() {
        let mut runtime = runtime();
        runtime.suppress_terminal_session_workers = true;
        runtime.live = None;

        runtime.request_terminal_control("w-size:p1");
        assert!(
            !runtime.terminal_sessions.contains_key("w-size:p1"),
            "no session may be started for a pane with no reported size"
        );
        assert!(
            runtime
                .snapshot
                .status
                .diagnostics
                .iter()
                .any(|entry| entry.kind == "terminal.attach_deferred"),
            "the wait must be reported: {:?}",
            runtime.snapshot.status.diagnostics
        );
        assert!(runtime.panes_awaiting_size.contains("w-size:p1"));
        assert_eq!(
            runtime.terminal_session_lifecycles["w-size:p1"].state,
            "waiting_size"
        );
        assert!(
            runtime.terminal_session_lifecycles["w-size:p1"]
                .message
                .as_ref()
                .unwrap()
                .contains("Waiting")
        );
    }

    /// The panes the terminal projection is holding open, in snapshot order.
    fn projected_pane_ids(runtime: &Runtime) -> Vec<String> {
        runtime
            .snapshot()
            .terminal
            .panes
            .iter()
            .map(|pane| pane.pane_id.clone())
            .collect()
    }

    /// R3, AC5, SC1. A tab the operator leaves keeps its panes in the
    /// projection, so the attach that feeds its terminal view is never
    /// dropped and the scrollback is still arriving when they come back. The
    /// projection used to be rebuilt from the arriving layout alone, which
    /// took every other tab's panes out of it on each switch.
    #[test]
    fn retained_views_keep_a_visited_tab_in_the_projection_across_a_switch() {
        let checkout_path = "/private/tmp/hide-retained-views-switch";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        assert_eq!(projected_pane_ids(&runtime), vec!["w-order:t1:p"]);

        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t2",
        )));

        let projected = projected_pane_ids(&runtime);
        assert!(
            projected.contains(&"w-order:t1:p".to_owned()),
            "the tab left behind keeps its pane attached: {projected:?}"
        );
        assert!(projected.contains(&"w-order:t2:p".to_owned()));

        // And back again, with nothing having been rebuilt in between.
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t1")));
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        let returned = projected_pane_ids(&runtime);
        assert!(returned.contains(&"w-order:t1:p".to_owned()));
        assert!(returned.contains(&"w-order:t2:p".to_owned()));
    }

    /// R3, AC5, AC6. The half of retention the canvas cannot show: a tab the
    /// operator is not looking at keeps producing output, and that output has
    /// to reach the snapshot on its own sequence while it is hidden. If it
    /// only arrived once the tab was visible again, coming back would replay
    /// rather than resume.
    #[test]
    fn retained_views_keep_a_hidden_tabs_output_arriving() {
        let checkout_path = "/private/tmp/hide-retained-views-hidden-output";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        // Visit the second tab, which is what starts its attach, then leave it.
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t2",
        )));
        runtime
            .terminal_session_generations
            .insert("w-order:t2:p".to_owned(), 7);
        runtime.terminal_sessions.insert(
            "w-order:t2:p".to_owned(),
            TerminalSession::test_stub("w-order:t2:p", 7, TerminalSessionMode::Control),
        );
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t1")));
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        assert_eq!(runtime.snapshot().tab.id.as_deref(), Some("w-order:t1"));
        let before = runtime.snapshot().terminal.sequence;
        runtime
            .terminal_sizes
            .insert("w-order:t2:p".to_owned(), (40, 120));

        assert!(
            runtime.ingest_terminal_session_frame(
                "w-order:t2:p",
                7,
                TerminalSessionMode::Control,
                b"hidden tab still talking",
                crate::model::TerminalFrame {
                    width: 120,
                    height: 40,
                    full: true
                },
            ) == Some(true),
            "the session behind a hidden tab is still delivering"
        );

        let snapshot = runtime.snapshot();
        assert_eq!(
            snapshot.terminal.sequence,
            before + 1,
            "the chunk rides the cursor, so a hidden tab costs one sequence step and no resend"
        );
        let arrived = snapshot
            .terminal
            .chunks
            .last()
            .expect("the chunk that just arrived");
        assert_eq!(arrived.pane_id, "w-order:t2:p");
        assert_eq!(arrived.sequence, before + 1);
        assert_eq!(
            live::decode_base64(&arrived.bytes_base64).expect("chunk bytes"),
            b"hidden tab still talking".to_vec()
        );
    }

    /// AC5. A hidden pane's size is what the shell last reported for it, and a
    /// tab switch neither reports a new one nor lets the core invent one.
    /// Geometry decides the PTY size, so a size that moved while a pane was
    /// out of sight would reflow its contents behind the operator's back.
    #[test]
    fn retained_views_leave_a_hidden_panes_size_alone_across_a_switch() {
        let checkout_path = "/private/tmp/hide-retained-views-size";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        let resize = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "terminal_resize",
            "payload": {"pane_id": "w-order:t1:p", "rows": 40, "cols": 120}
        }))
        .expect("resize event");
        runtime.dispatch_json(&resize);
        assert_eq!(
            runtime.terminal_sizes.get("w-order:t1:p").copied(),
            Some((40, 120))
        );

        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t2",
        )));

        assert_eq!(
            runtime.terminal_sizes.get("w-order:t1:p").copied(),
            Some((40, 120)),
            "the hidden pane keeps the size it was last given"
        );
    }

    /// AC5, rule 1. Retention is bounded by the session. A pane whose tab has
    /// gone leaves the projection on the next update rather than accumulating
    /// there for the life of the process.
    #[test]
    fn retained_views_drop_a_pane_whose_tab_left_the_session() {
        let checkout_path = "/private/tmp/hide-retained-views-closed";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t2",
        )));
        assert!(projected_pane_ids(&runtime).contains(&"w-order:t1:p".to_owned()));

        let remaining = ["w-order:t2"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &remaining,
            &remaining,
            "w-order:t2",
        )));

        assert_eq!(projected_pane_ids(&runtime), vec!["w-order:t2:p"]);
    }

    /// AC5, R3. The shell drew one canvas keyed by the visible tab, so every
    /// switch destroyed its terminal views and the new ones started empty and
    /// reported a size. The surface now draws every visited tab and hides all
    /// but one, which is the mechanism zoom already uses for panes. Removing
    /// either half brings the blank frame and the switch-time resize back.
    #[test]
    fn retained_views_have_no_single_canvas_keyed_by_the_visible_tab() {
        let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS");
        let surface = std::fs::read_to_string(shell.join("HideUI.swift"))
            .expect("the terminal surface source");
        let presentation = std::fs::read_to_string(shell.join("ShellView.swift"))
            .expect("the pane grid presentation source");
        assert!(
            surface.contains("model.retainedTabCanvases"),
            "the terminal surface no longer draws every visited tab"
        );
        assert!(
            surface.contains(".opacity(canvas.isVisible ? 1 : 0)"),
            "a hidden tab is removed from the view tree instead of being hidden"
        );
        assert!(
            presentation.contains("func retainedCanvases("),
            "the rule deciding which tabs keep a canvas is gone"
        );
    }

    fn checkout_active_tab_id(runtime: &Runtime, checkout_id: &str) -> Option<String> {
        runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .find(|checkout| checkout.id == checkout_id)
            .expect("the registered checkout")
            .active_tab_id
            .clone()
    }

    /// A [`tab_order_runtime`] with a live Herdr context, so a view-state
    /// notification actually leaves and a wait is armed.
    fn live_tab_order_runtime(checkout_path: &str) -> (Runtime, String) {
        let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
        let socket_path = std::env::temp_dir()
            .join(format!(
                "herdr-core-view-authority-{}-{}.sock",
                std::process::id(),
                NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
            ))
            .to_string_lossy()
            .into_owned();
        runtime.live = Some(live::LiveContext {
            socket_path: socket_path.clone().into(),
            herdr_bin: None,
            runtime: std::sync::Weak::new(),
            notifier: crate::ffi::ChangeNotifier::noop(),
            api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
        });
        (runtime, checkout_id)
    }

    fn focus_tab_event(checkout_id: &str, tab_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_tab",
            "payload": {
                "workspace_id": "workspace:order",
                "checkout_id": checkout_id,
                "tab_id": tab_id
            }
        }))
        .expect("focus tab event")
    }

    fn diagnostic_count(runtime: &Runtime, kind: &str) -> usize {
        runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == kind)
            .count()
    }

    /// AC1, SC1. The strip's active mark and the canvas are the same field, so
    /// asking for a tab moves both on the dispatch that asked, without waiting
    /// for Herdr to answer.
    #[test]
    fn view_authority_a_tab_switch_moves_the_strip_and_the_canvas_in_one_snapshot() {
        let checkout_path = "/private/tmp/hide-view-authority-switch";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));

        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));

        let snapshot = runtime.snapshot();
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t3"),
            "the strip's active mark moves on the frame the operator asked for"
        );
        assert_eq!(snapshot.tab.id.as_deref(), Some("w-order:t3"));
        assert_eq!(
            snapshot
                .active_pane_layout()
                .map(|layout| layout.tab_id.as_str()),
            Some("w-order:t3"),
            "the canvas is drawing the requested tab in the same snapshot"
        );
    }

    /// AC1, SC1. Herdr's next session update still names the tab the operator
    /// left, because the notification has not landed. That is the answer to a
    /// question already asked, not a new focus, so it must not pull the canvas
    /// back.
    #[test]
    fn view_authority_a_stale_herdr_tab_does_not_undo_an_unconfirmed_switch() {
        let checkout_path = "/private/tmp/hide-view-authority-stale-tab";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));

        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));

        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t3")
        );
        assert_eq!(
            diagnostic_count(&runtime, "tab.focus.followed"),
            0,
            "an unconfirmed notification is not an external focus"
        );

        // The confirmation ends the wait, and the value is unchanged by it.
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t3",
        )));
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t3")
        );
        assert!(runtime.pending_tab_focus.is_none());
    }

    /// AC1, R1. With nothing in flight, a Herdr session naming another tab is
    /// somebody focusing that tab outside Hide. Hide follows it and says so.
    #[test]
    fn view_authority_an_external_tab_focus_is_followed_and_reported() {
        let checkout_path = "/private/tmp/hide-view-authority-external-tab";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        assert!(runtime.pending_tab_focus.is_none());

        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t2"
        ))));

        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t2")
        );
        assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 1);
        assert_eq!(
            runtime.snapshot().tab.id.as_deref(),
            Some("w-order:t2"),
            "the canvas follows the tab, not only the strip"
        );
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w-order:t2:p"),
            "and the keyboard lands in that tab rather than staying on a pane nobody can see"
        );
    }

    /// AC1, SC1. Registering a device rebuilds the catalog from scratch, and a
    /// freshly built checkout names no active tab. The visible tab is Hide's,
    /// so it survives a rebuild Herdr had no part in.
    #[test]
    fn view_authority_a_catalog_rebuild_keeps_the_visible_tab() {
        let checkout_path = "/private/tmp/hide-view-authority-rebuild";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));

        let register_device = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "register_device",
            "payload": {
                "id": "device-rebuild",
                "label": "Rebuild",
                "ssh_alias": "rebuild-host"
            }
        }))
        .expect("register device event");
        assert!(runtime.dispatch_json(&register_device));

        // The rebuilt catalog carries no tabs until the next session update,
        // which is what makes this the interesting moment: the tab the
        // operator chose has to survive the gap. It survives because a
        // rebuild is not a reconcile - the catalog says nothing about which
        // tab is visible, so nothing on this path may read it as Herdr
        // naming another one.
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));

        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t3"),
            "a rebuild is not Herdr moving the tab"
        );
        assert_eq!(
            runtime.snapshot().tab.id.as_deref(),
            Some("w-order:t3"),
            "and the canvas comes back on the tab it was showing"
        );
        assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);
    }

    /// AC2, R1. Herdr refusing the notification does not move the operator's
    /// screen. The tab stays where they put it and the refusal is reported.
    #[test]
    fn view_authority_a_refused_tab_focus_keeps_the_tab_and_reports_it() {
        let checkout_path = "/private/tmp/hide-view-authority-refused-tab";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));
        assert!(runtime.pending_tab_focus.is_some());

        runtime.ingest_local_control_result(
            RemoteControlAction::FocusTab {
                tab_id: "w-order:t3".to_owned(),
            },
            Err("tab.focus rejected".to_owned()),
            12,
        );

        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t3"),
            "a refusal is reported, not acted on by moving the screen"
        );
        assert_eq!(diagnostic_count(&runtime, "tab.focus.refused"), 1);
        assert!(runtime.pending_tab_focus.is_none());
    }

    /// AC2, R1. A notification Herdr never answers stops being pending, the
    /// value Hide chose is kept, and the silence is reported rather than
    /// waited on forever.
    #[test]
    fn view_authority_an_unanswered_notification_times_out_and_keeps_its_value() {
        let checkout_path = "/private/tmp/hide-view-authority-timeout";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));
        let requested_at = runtime
            .pending_tab_focus
            .as_ref()
            .expect("a notification is in flight")
            .requested_at_unix_ms;

        assert!(!runtime.expire_pending_view_focus(requested_at + 1));
        assert!(runtime.pending_tab_focus.is_some());
        assert!(
            runtime.expire_pending_view_focus(requested_at + VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS)
        );

        assert!(runtime.pending_tab_focus.is_none());
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t3"),
            "a silent Herdr is a reason to report, not to move the screen"
        );
        assert_eq!(diagnostic_count(&runtime, "view_focus.timed_out"), 1);
    }

    /// Two Herdr workspaces at one path share one checkout, and the second
    /// one in payload order names a different active tab. A tab focus on the
    /// first workspace is confirmed by that workspace showing the tab, even
    /// while Herdr's keyboard stays in the second one.
    #[test]
    fn split_checkout_a_tab_focus_is_confirmed_by_the_workspace_that_owns_the_tab() {
        let checkout_path = "/private/tmp/hide-split-checkout-confirm";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let before: [(&str, &[&str], &str); 2] = [
            ("wa", &["wa:t1", "wa:t2"], "wa:t1"),
            ("wb", &["wb:t1"], "wb:t1"),
        ];
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &before, "wb")));
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wb:t1"),
            "with no tab of its own Hide shows the tab Herdr has focused"
        );
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("wb:t1:p"),
            "and the keyboard is in that tab"
        );

        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "wa:t2")));
        assert!(runtime.pending_tab_focus.is_some());
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("wa:t2:p")
        );

        // Herdr shows the tab in its workspace; its keyboard stays in wb.
        let confirmed: [(&str, &[&str], &str); 2] = [
            ("wa", &["wa:t1", "wa:t2"], "wa:t2"),
            ("wb", &["wb:t1"], "wb:t1"),
        ];
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &confirmed, "wb")));

        assert!(
            runtime.pending_tab_focus.is_none(),
            "the notification is confirmed"
        );
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wa:t2")
        );
        assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);
        assert_eq!(diagnostic_count(&runtime, "view_focus.timed_out"), 0);
        assert_eq!(
            runtime
                .snapshot()
                .active_pane_layout()
                .map(|layout| layout.tab_id.as_str()),
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            "the tab drawn is the tab attached"
        );
    }

    /// The active tab of a workspace Herdr's keyboard is not in is that
    /// workspace's memory, not a focus. It never moves the canvas; a move of
    /// Herdr's keyboard into that workspace does.
    #[test]
    fn split_checkout_another_workspaces_active_tab_is_not_followed_until_herdr_focuses_it() {
        let checkout_path = "/private/tmp/hide-split-checkout-follow";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let start: [(&str, &[&str], &str); 2] = [
            ("wa", &["wa:t1"], "wa:t1"),
            ("wb", &["wb:t1", "wb:t2"], "wb:t1"),
        ];
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &start, "wa")));
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wa:t1")
        );

        // wb remembers another tab; Herdr's keyboard is still in wa.
        let wb_moved: [(&str, &[&str], &str); 2] = [
            ("wa", &["wa:t1"], "wa:t1"),
            ("wb", &["wb:t1", "wb:t2"], "wb:t2"),
        ];
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &wb_moved, "wa")));
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wa:t1"),
            "a non-focused workspace's active tab is not followed"
        );
        assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);

        // Herdr's keyboard moves into wb: that is a focus, and it is followed.
        assert!(runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &wb_moved, "wb"))));
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wb:t2")
        );
        assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 1);
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("wb:t2:p"),
            "the keyboard follows the tab"
        );
    }

    /// A restore keeps the persisted pane and draws the tab that holds it,
    /// not the tab Herdr's last workspace happens to name. The canvas drawn
    /// is the canvas attached.
    #[test]
    fn split_checkout_a_restore_draws_the_tab_holding_the_persisted_pane() {
        let checkout_path = "/private/tmp/hide-split-checkout-restore";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        runtime.snapshot.terminal.pane_id = Some("wa:t2:p".to_owned());
        runtime.snapshot.focused.pane_id = Some("wa:t2:p".to_owned());
        runtime.snapshot.ui_state.selected_pane_id = Some("wa:t2:p".to_owned());
        let session: [(&str, &[&str], &str); 2] = [
            ("wa", &["wa:t1", "wa:t2"], "wa:t1"),
            ("wb", &["wb:t1"], "wb:t1"),
        ];
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &session, "wb")));

        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("wa:t2:p"),
            "the persisted pane survives the first session"
        );
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wa:t2"),
            "the visible tab is the one holding that pane"
        );
        assert_eq!(
            runtime
                .snapshot()
                .active_pane_layout()
                .map(|layout| layout.tab_id.as_str()),
            Some("wa:t2"),
            "and it is the layout attached"
        );
    }

    /// A timed-out notification keeps Hide's tab through the reconcile that
    /// follows it: Herdr's focus has not moved, so there is nothing to
    /// follow.
    #[test]
    fn split_checkout_an_expired_notification_keeps_its_tab_through_the_next_reconcile() {
        let checkout_path = "/private/tmp/hide-split-checkout-expiry";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let session: [(&str, &[&str], &str); 2] = [
            ("wa", &["wa:t1", "wa:t2"], "wa:t1"),
            ("wb", &["wb:t1"], "wb:t1"),
        ];
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &session, "wa")));
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "wa:t2")));
        let requested_at = runtime
            .pending_tab_focus
            .as_ref()
            .expect("a notification is in flight")
            .requested_at_unix_ms;
        assert!(
            runtime.expire_pending_view_focus(requested_at + VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS)
        );
        assert!(runtime.pending_tab_focus.is_none());

        // Herdr never answered; its focus is where it was.
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &session, "wa")));
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wa:t2"),
            "a silent Herdr is a reason to report, not to move the screen"
        );
        assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);
        assert_eq!(diagnostic_count(&runtime, "view_focus.timed_out"), 1);
    }

    /// A created tab is the visible tab from Herdr's acknowledgment, and the
    /// `tab_focused` that follows confirms it rather than being followed.
    #[test]
    fn split_checkout_a_created_tab_is_visible_on_the_acknowledgment() {
        let checkout_path = "/private/tmp/hide-split-checkout-create";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let before: [(&str, &[&str], &str); 2] =
            [("wa", &["wa:t1"], "wa:t1"), ("wb", &["wb:t1"], "wb:t1")];
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &before, "wa")));

        runtime.ingest_local_control_result(
            RemoteControlAction::CreateTab {
                workspace_id: "wa".to_owned(),
                cwd: checkout_path.to_owned(),
                label: "2".to_owned(),
            },
            Ok(RemoteControlOutcome::Acknowledged {
                created_tab_id: Some("wa:t2".to_owned()),
                created_pane_id: Some("wa:t2:p".to_owned()),
            }),
            5,
        );
        assert_eq!(
            runtime
                .visible_tab_ids
                .get(&checkout_id)
                .map(String::as_str),
            Some("wa:t2"),
            "the created tab is Hide's visible tab before Herdr lists it"
        );
        assert!(runtime.pending_tab_focus.is_some());
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("wa:t2:p")
        );

        let after: [(&str, &[&str], &str); 2] = [
            ("wa", &["wa:t1", "wa:t2"], "wa:t2"),
            ("wb", &["wb:t1"], "wb:t1"),
        ];
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &after, "wa")));
        assert!(runtime.pending_tab_focus.is_none());
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wa:t2")
        );
        assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);
        assert_eq!(
            runtime
                .snapshot()
                .active_pane_layout()
                .map(|layout| layout.tab_id.as_str()),
            Some("wa:t2")
        );
    }

    /// Focusing a pane in a tab the checkout is not showing brings that tab
    /// forward in the same dispatch, so the keyboard never lands on a pane
    /// nobody can see.
    #[test]
    fn split_checkout_a_pane_focus_in_a_hidden_tab_brings_the_tab_forward() {
        let checkout_path = "/private/tmp/hide-split-checkout-align";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let session: [(&str, &[&str], &str); 2] = [
            ("wa", &["wa:t1", "wa:t2"], "wa:t1"),
            ("wb", &["wb:t1"], "wb:t1"),
        ];
        runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &session, "wa")));
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wa:t1")
        );

        assert!(runtime.dispatch_json(&operator_focus_event("wb:t1:p")));
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("wb:t1"),
            "the tab holding the focused pane is visible on the same dispatch"
        );
        assert_eq!(diagnostic_count(&runtime, "tab.visible_aligned"), 1);
        assert_eq!(
            runtime.snapshot().ui_state.selected_pane_id.as_deref(),
            Some("wb:t1:p"),
            "the persisted selection follows the ring"
        );
    }

    /// Two tabs with one agent each. The operator looks at the pane in tab 1
    /// and switches to tab 2. The read record follows the keyboard: tab 2's
    /// pane is read, and a state change in tab 1 afterwards is unread.
    #[test]
    fn read_record_follows_a_tab_switch_the_operator_made() {
        let checkout_path = "/private/tmp/hide-read-record-tab-switch";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let payload = |seq_t1: u64, seq_t2: u64| -> SessionSnapshotPayload {
            let tab = |tab_id: &str| {
                serde_json::json!({
                    "workspace_id": "w-order",
                    "tab_id": tab_id,
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": format!("{tab_id}:p"),
                    "panes": [{
                        "pane_id": format!("{tab_id}:p"),
                        "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                    }],
                    "splits": []
                })
            };
            let agent = |tab_id: &str, seq: u64| {
                serde_json::json!({
                    "pane_id": format!("{tab_id}:p"),
                    "workspace_label": "order",
                    "agent": "codex",
                    "agent_status": "done",
                    "state_change_seq": seq,
                    "tokens": {"status_done_new": "\u{25cf}", "activity": "0000000000001"}
                })
            };
            serde_json::from_value(serde_json::json!({
                "agents": [agent("w-order:t1", seq_t1), agent("w-order:t2", seq_t2)],
                "focused_workspace_id": "w-order",
                "focused_pane_id": "w-order:t1:p",
                "workspaces": [{
                    "workspace_id": "w-order", "label": "order", "active_tab_id": "w-order:t1"
                }],
                "tabs": [
                    {"workspace_id": "w-order", "tab_id": "w-order:t1", "label": ""},
                    {"workspace_id": "w-order", "tab_id": "w-order:t2", "label": ""}
                ],
                "panes": [
                    {"pane_id": "w-order:t1:p", "cwd": checkout_path},
                    {"pane_id": "w-order:t2:p", "cwd": checkout_path}
                ],
                "layouts": [tab("w-order:t1"), tab("w-order:t2")]
            }))
            .expect("two-tab agent payload")
        };
        runtime.ingest_session(Ok(payload(1, 1)));
        assert!(runtime.dispatch_json(&operator_focus_event("w-order:t1:p")));
        assert_eq!(unread_panes(&runtime), vec!["w-order:t2:p"]);

        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
        assert!(
            unread_panes(&runtime).is_empty(),
            "the pane of the tab the operator switched to holds the keyboard and is read"
        );

        runtime.ingest_session(Ok(payload(2, 1)));
        assert_eq!(
            unread_panes(&runtime),
            vec!["w-order:t1:p"],
            "a change in the tab the operator left is unread; the record did not stay there"
        );
    }

    /// Closing the visible tab lands on the tab Herdr moved to. Three tabs;
    /// the operator is on the third and closes it. The snapshot that follows
    /// names the second tab active and no focused pane yet, and the keyboard
    /// goes to that tab's pane, not to the checkout's first.
    #[test]
    fn closing_the_visible_tab_lands_on_the_tab_herdr_moved_to() {
        let checkout_path = "/private/tmp/hide-close-lands-on-herdr-tab";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t3",
        )));
        assert!(runtime.dispatch_json(&operator_focus_event("w-order:t3:p")));
        assert_eq!(
            runtime
                .visible_tab_ids
                .get(&checkout_id)
                .map(String::as_str),
            Some("w-order:t3")
        );

        let remaining = ["w-order:t1", "w-order:t2"];
        let mut after_close =
            tab_order_payload(checkout_path, &remaining, &remaining, "w-order:t2");
        after_close.focused_pane_id = None;
        runtime.ingest_session(Ok(after_close));

        assert_eq!(
            runtime.snapshot.ui_state.selected_pane_id.as_deref(),
            Some("w-order:t2:p"),
            "the keyboard follows Herdr to the tab it focused after the close"
        );
        assert_eq!(
            runtime
                .visible_tab_ids
                .get(&checkout_id)
                .map(String::as_str),
            Some("w-order:t2"),
            "the strip draws the tab Herdr moved to"
        );
    }

    /// A checkout switch releases the read record without raising one. Two
    /// checkouts at different paths; the operator reads pane A in the first,
    /// then reaches pane B, in the second checkout's other tab, through the
    /// sidebar, which sends a checkout focus and then a pane focus. Pane C,
    /// on the tab the second checkout comes forward with, is not marked read
    /// by the checkout switch, and a change on A afterwards is unread.
    #[test]
    fn read_record_is_released_and_not_raised_by_a_checkout_switch() {
        let mut runtime = live_runtime();
        let root_a = std::env::temp_dir()
            .join(format!(
                "hide-checkout-switch-a-{}-{}",
                std::process::id(),
                NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
            ))
            .to_string_lossy()
            .into_owned();
        let root_b = format!("{root_a}-b");
        std::fs::create_dir_all(&root_a).expect("checkout a");
        std::fs::create_dir_all(&root_b).expect("checkout b");
        // Each root is its own repository. The catalog keys a checkout by the
        // repository that contains it, so two plain directories under one
        // repository - which is what a temporary directory inside the project
        // is - collapse into a single checkout and this test loses the second
        // one before it starts.
        for root in [&root_a, &root_b] {
            assert!(
                std::process::Command::new("git")
                    .args(["init", "-q", "-b", "main"])
                    .current_dir(root)
                    .status()
                    .expect("git init runs")
                    .success()
            );
        }
        runtime.snapshot.ui_state.workspace_registrations = vec![
            WorkspaceRegistration {
                id: "workspace:a".to_owned(),
                label: "a".to_owned(),
                path: root_a.clone(),
                device_id: "local".to_owned(),
            },
            WorkspaceRegistration {
                id: "workspace:b".to_owned(),
                label: "b".to_owned(),
                path: root_b.clone(),
                device_id: "local".to_owned(),
            },
        ];
        runtime.rebuild_catalog();
        let checkout_of = |runtime: &Runtime, workspace_id: &str| -> String {
            runtime
                .snapshot
                .navigator
                .workspaces
                .iter()
                .find(|workspace| workspace.id == workspace_id)
                .and_then(|workspace| workspace.checkouts.first())
                .map(|checkout| checkout.id.clone())
                .expect("registered checkout")
        };
        let checkout_a = checkout_of(&runtime, "workspace:a");
        let checkout_b = checkout_of(&runtime, "workspace:b");
        runtime.snapshot.navigator.focused_workspace_id = Some("workspace:a".to_owned());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_a.clone());
        runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_a);
        runtime.reset_terminal_projection(None);

        let payload = |seq_a: u64| -> SessionSnapshotPayload {
            let layout = |workspace_id: &str, tab_id: &str, pane_ids: &[&str], focused: &str| {
                serde_json::json!({
                    "workspace_id": workspace_id,
                    "tab_id": tab_id,
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": focused,
                    "panes": pane_ids.iter().enumerate().map(|(index, pane_id)| serde_json::json!({
                        "pane_id": pane_id,
                        "rect": {"x": index * 40, "y": 0, "width": 40, "height": 24}
                    })).collect::<Vec<_>>(),
                    "splits": []
                })
            };
            let agent = |pane_id: &str, seq: u64| {
                serde_json::json!({
                    "pane_id": pane_id,
                    "workspace_label": "fixture",
                    "agent": "codex",
                    "agent_status": "done",
                    "state_change_seq": seq,
                    "tokens": {"status_done_new": "\u{25cf}", "activity": format!("{seq:013}")}
                })
            };
            serde_json::from_value(serde_json::json!({
                "agents": [agent("wa:pA", seq_a), agent("wb:pB", 1), agent("wb:pC", 1)],
                "focused_workspace_id": "wa",
                "focused_pane_id": "wa:pA",
                "workspaces": [
                    {"workspace_id": "wa", "label": "a", "active_tab_id": "wa:t1"},
                    {"workspace_id": "wb", "label": "b", "active_tab_id": "wb:t1"}
                ],
                "tabs": [
                    {"workspace_id": "wa", "tab_id": "wa:t1", "label": ""},
                    {"workspace_id": "wb", "tab_id": "wb:t1", "label": ""},
                    {"workspace_id": "wb", "tab_id": "wb:t2", "label": ""}
                ],
                "panes": [
                    {"pane_id": "wa:pA", "cwd": root_a},
                    {"pane_id": "wb:pB", "cwd": root_b},
                    {"pane_id": "wb:pC", "cwd": root_b}
                ],
                "layouts": [
                    layout("wa", "wa:t1", &["wa:pA"], "wa:pA"),
                    layout("wb", "wb:t1", &["wb:pC"], "wb:pC"),
                    layout("wb", "wb:t2", &["wb:pB"], "wb:pB")
                ]
            }))
            .expect("two-checkout payload")
        };
        runtime.ingest_session(Ok(payload(1)));
        assert!(runtime.dispatch_json(&operator_focus_event("wa:pA")));
        assert_eq!(unread_panes(&runtime), vec!["wb:pB", "wb:pC"]);

        let focus_b = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_checkout",
            "payload": {"workspace_id": "workspace:b", "checkout_id": checkout_b}
        }))
        .expect("focus checkout event");
        assert!(runtime.dispatch_json(&focus_b));
        assert_eq!(
            runtime.snapshot.navigator.focused_checkout_id.as_deref(),
            Some(checkout_b.as_str()),
            "error: {:?}",
            runtime.snapshot.status.last_error
        );
        assert_eq!(
            runtime.snapshot.ui_state.selected_pane_id.as_deref(),
            Some("wb:pC"),
            "the checkout comes forward on its remembered pane"
        );
        assert_eq!(
            unread_panes(&runtime),
            vec!["wb:pB", "wb:pC"],
            "the checkout coming forward raises no record for its remembered pane"
        );

        assert!(runtime.dispatch_json(&operator_focus_event("wb:pB")));
        assert_eq!(
            unread_panes(&runtime),
            vec!["wb:pC"],
            "only the pane the operator reached is read"
        );

        runtime.ingest_session(Ok(payload(2)));
        assert!(
            unread_panes(&runtime).contains(&"wa:pA".to_owned()),
            "a change on the pane the operator left is unread; the record did not stay there"
        );
        std::fs::remove_dir_all(&root_a).ok();
        std::fs::remove_dir_all(&root_b).ok();
    }

    /// The diagnostics list rides the revisioned rest section, so it keeps a
    /// bounded number of the newest entries.
    #[test]
    fn diagnostics_keep_the_newest_entries_up_to_the_retention() {
        let mut runtime = runtime();
        for index in 0..(DIAGNOSTIC_RETENTION + 10) {
            runtime.push_diagnostic("test.entry", format!("entry {index}"));
        }
        let diagnostics = &runtime.snapshot().status.diagnostics;
        assert_eq!(diagnostics.len(), DIAGNOSTIC_RETENTION);
        assert_eq!(
            diagnostics[0].message, "entry 10",
            "the oldest entries went first"
        );
        assert_eq!(
            diagnostics[DIAGNOSTIC_RETENTION - 1].message,
            format!("entry {}", DIAGNOSTIC_RETENTION + 9)
        );
    }

    /// AC1, AC7, SC3. The focus ring moves on the click, and the layout that
    /// was already on its way carrying the old focus does not take it back.
    /// The read record stays on the clicked pane through that arrival.
    #[test]
    fn view_authority_a_pane_click_moves_focus_and_a_stale_layout_does_not_undo_it() {
        let mut runtime = live_runtime();
        let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));

        assert!(runtime.dispatch_json(&operator_focus_event("w1:p3")));
        assert_eq!(
            runtime.snapshot().focused.pane_id.as_deref(),
            Some("w1:p3"),
            "the ring moves on the click, not on Herdr's confirming event"
        );
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w1:p3")
        );

        // Herdr's in-flight frame still names the pane the tab came forward
        // with. Its geometry is taken; its focus is not.
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));

        assert_eq!(runtime.snapshot().focused.pane_id.as_deref(), Some("w1:p3"));
        assert_eq!(
            unread_panes(&runtime),
            vec!["w1:p1", "w1:p2"],
            "the clicked row stays read through the stale arrival"
        );
        assert_eq!(diagnostic_count(&runtime, "pane.focus.followed"), 0);
    }

    /// AC1, AC8, R1. With nothing in flight, a Herdr layout naming another
    /// pane is a focus made outside Hide. Hide follows it and reports the
    /// panes and where the change came from.
    #[test]
    fn view_authority_an_external_pane_focus_is_followed_and_reported() {
        let mut runtime = live_runtime();
        let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));
        assert!(runtime.pending_pane_focus.is_none());

        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p2")));

        assert_eq!(runtime.snapshot().focused.pane_id.as_deref(), Some("w1:p2"));
        assert_eq!(diagnostic_count(&runtime, "pane.focus.followed"), 1);
    }

    /// AC2, R1. A refused pane focus leaves the keyboard where the operator
    /// put it and reports the refusal.
    #[test]
    fn view_authority_a_refused_pane_focus_keeps_the_pane_and_reports_it() {
        let mut runtime = live_runtime();
        let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));
        assert!(runtime.dispatch_json(&operator_focus_event("w1:p3")));

        runtime.ingest_pane_control_result(
            PaneControlAction::Focus {
                pane_id: "w1:p3".to_owned(),
            },
            Err("pane.focus rejected".to_owned()),
            9,
        );

        assert_eq!(runtime.snapshot().focused.pane_id.as_deref(), Some("w1:p3"));
        assert_eq!(diagnostic_count(&runtime, "pane.focus.refused"), 1);
        assert!(runtime.pending_pane_focus.is_none());
    }

    /// Rule 11. Reaching for the pane that already has the keyboard converges
    /// on the state it is already in and sends Herdr nothing a second time.
    /// The look itself still counts, because clicking the pane you are on is
    /// still looking at it.
    #[test]
    fn view_authority_repeating_a_focus_sends_no_second_notification() {
        let mut runtime = live_runtime();
        let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));

        assert!(runtime.dispatch_json(&operator_focus_event("w1:p3")));
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p3")));
        assert!(runtime.pending_pane_focus.is_none());
        let notifications = diagnostic_count(&runtime, "pane.focus.requested");

        runtime.dispatch_json(&operator_focus_event("w1:p3"));

        assert_eq!(
            diagnostic_count(&runtime, "pane.focus.requested"),
            notifications,
            "no second request leaves for a pane Herdr has already focused"
        );
        assert_eq!(runtime.snapshot().focused.pane_id.as_deref(), Some("w1:p3"));
        assert_eq!(unread_panes(&runtime), vec!["w1:p1", "w1:p2"]);
    }

    #[test]
    fn tab_order_follows_herdr_and_not_layout_arrival() {
        let checkout_path = "/private/tmp/hide-tab-order-arrival";
        let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
        // Herdr reports t1, t2, t3; the layouts arrive in the order the tabs
        // were first drawn, which is the order the navigator used to take.
        let payload = tab_order_payload(
            checkout_path,
            &["w-order:t1", "w-order:t2", "w-order:t3"],
            &["w-order:t3", "w-order:t1", "w-order:t2"],
            "w-order:t1",
        );
        assert!(runtime.ingest_session(Ok(payload)));
        assert_eq!(
            ordered_tab_ids(&runtime, &checkout_id),
            vec![
                "w-order:t1".to_owned(),
                "w-order:t2".to_owned(),
                "w-order:t3".to_owned()
            ]
        );
    }

    #[test]
    fn tab_order_follows_a_move_and_ignores_a_layout_redraw() {
        let checkout_path = "/private/tmp/hide-tab-order-moved";
        let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));

        // Herdr moved the second tab in front of the first.
        let moved = ["w-order:t2", "w-order:t1", "w-order:t3"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &moved,
            &tabs,
            "w-order:t1"
        ))));
        assert_eq!(
            ordered_tab_ids(&runtime, &checkout_id),
            moved.map(str::to_owned).to_vec()
        );

        // A layout redraw for an existing tab is detail, not order.
        assert!(!runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &moved,
            &["w-order:t3", "w-order:t2", "w-order:t1"],
            "w-order:t1"
        ))));
        assert_eq!(
            ordered_tab_ids(&runtime, &checkout_id),
            moved.map(str::to_owned).to_vec()
        );
    }

    #[test]
    fn tab_order_is_unchanged_by_a_tab_switch() {
        let checkout_path = "/private/tmp/hide-tab-order-focus";
        let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));

        let focus = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "focus_tab",
            "payload": {
                "workspace_id": "workspace:order",
                "checkout_id": checkout_id,
                "tab_id": "w-order:t3"
            }
        }))
        .expect("focus event");
        assert!(runtime.dispatch_json(&focus));

        assert_eq!(
            ordered_tab_ids(&runtime, &checkout_id),
            tabs.map(str::to_owned).to_vec(),
            "a tab switch must not move the tab it switched to"
        );
        // The active mark moves on the dispatch that asked for it, because
        // Hide owns the visible tab. It is still never inferred from
        // position: the strip order above did not change.
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t3")
        );
        assert_eq!(runtime.snapshot().tab.id.as_deref(), Some("w-order:t3"));
    }

    #[test]
    fn herdr_active_tab_names_the_projected_tab() {
        let checkout_path = "/private/tmp/hide-tab-order-active";
        let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t2"
        ))));
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t2")
        );
        assert_eq!(
            runtime.snapshot().tab.id.as_deref(),
            Some("w-order:t2"),
            "the active tab projection follows the id Herdr named"
        );
    }

    #[test]
    fn herdr_active_tab_unresolved_is_reported_not_replaced() {
        let checkout_path = "/private/tmp/hide-tab-order-unplaceable";
        let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t9"
        ))));

        // Hide owns the visible tab, so a checkout that has tabs shows one of
        // them. What Herdr named is still reported, because a name that
        // matches no tab in the session is worth knowing about.
        assert_eq!(
            checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
            Some("w-order:t1")
        );
        assert_eq!(
            runtime.snapshot().tab.id.as_deref(),
            Some("w-order:t1"),
            "a checkout with tabs never draws the empty-checkout state"
        );
        let unresolved = runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == "tab.active_unresolved")
            .count();
        assert_eq!(unresolved, 1);

        // Session sync reconciles once a second; the same unresolved state
        // must not append a diagnostic on every tick.
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t9",
        )));
        let unresolved = runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == "tab.active_unresolved")
            .count();
        assert_eq!(unresolved, 1);
    }

    fn strip_ids(runtime: &Runtime, checkout_id: &str) -> Vec<String> {
        runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .find(|checkout| checkout.id == checkout_id)
            .expect("the registered checkout")
            .strip
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    }

    fn strip_labels(runtime: &Runtime, checkout_id: &str) -> Vec<String> {
        runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .find(|checkout| checkout.id == checkout_id)
            .expect("the registered checkout")
            .strip
            .iter()
            .map(|entry| entry.label.clone())
            .collect()
    }

    /// A registered checkout the strip tests can drive.
    ///
    /// The temp root is a symlink on macOS and the catalog keys checkouts by
    /// the real path, so the fixture uses that. It is also made its own
    /// repository, because a directory inside another repository is
    /// catalogued under that repository's root instead.
    fn strip_checkout(name: &str) -> (Runtime, String, PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "hide-strip-{name}-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).expect("checkout directory");
        let directory = directory.canonicalize().expect("a real checkout path");
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&directory)
                .status()
                .expect("git init runs")
                .success()
        );
        std::fs::write(directory.join("notes.md"), "notes\n").expect("fixture file");
        let (runtime, checkout_id) = tab_order_runtime(&directory.to_string_lossy());
        (runtime, checkout_id, directory)
    }

    fn reorder_tab(
        runtime: &mut Runtime,
        checkout_id: &str,
        entry_id: &str,
        to_index: usize,
    ) -> bool {
        let event = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "reorder_tab",
            "payload": {
                "workspace_id": "workspace:order",
                "checkout_id": checkout_id,
                "tab_id": entry_id,
                "to_index": to_index
            }
        }))
        .expect("reorder tab event");
        runtime.dispatch_json(&event)
    }

    fn open_file(runtime: &mut Runtime, checkout_id: &str, path: &Path) {
        let event = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "file_open",
            "payload": {
                "path": path.to_string_lossy(),
                "workspace_id": "workspace:order",
                "checkout_id": checkout_id
            }
        }))
        .expect("file open event");
        assert!(runtime.dispatch_json(&event));
    }

    #[test]
    fn file_view_mode_is_per_open_tab_and_reopen_starts_preview() {
        let (mut runtime, checkout_id, directory) = strip_checkout("file-view-mode");
        let first = directory.join("notes.md");
        let second = directory.join("second.md");
        std::fs::write(&first, "# Original").unwrap();
        std::fs::write(&second, "# Second").unwrap();
        open_file(&mut runtime, &checkout_id, &first);
        let first_id = runtime.snapshot.editor.active_tab_id.clone().unwrap();
        let event = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "file_view",
            "payload": {"tab_id": first_id, "markdown_preview": false, "wrap": true}
        }))
        .unwrap();
        assert!(runtime.dispatch_json(&event));
        assert!(
            !runtime.dispatch_json(&event),
            "repeated selection publishes no change"
        );
        open_file(&mut runtime, &checkout_id, &second);
        assert!(
            runtime
                .snapshot
                .editor
                .tabs
                .last()
                .unwrap()
                .markdown_preview
        );
        open_file(&mut runtime, &checkout_id, &first);
        let tab = runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.id == first_id)
            .unwrap();
        assert!(!tab.markdown_preview);
        assert!(tab.wrap);
        let close = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "file_close", "payload": {"tab_id": first_id}
        }))
        .unwrap();
        runtime.dispatch_json(&close);
        open_file(&mut runtime, &checkout_id, &first);
        assert!(
            runtime
                .snapshot
                .editor
                .tabs
                .last()
                .unwrap()
                .markdown_preview
        );
    }

    #[test]
    fn recent_navigation_restores_file_and_diff_across_projects_atomically() {
        for diff in [false, true] {
            let (mut runtime, checkout_id, directory) = strip_checkout("recent-surface");
            let file = directory.join("notes.md");
            if diff {
                runtime.show_diff_tab(
                    "workspace:order",
                    &checkout_id,
                    &file.to_string_lossy(),
                    false,
                );
            } else {
                open_file(&mut runtime, &checkout_id, &file);
            }
            let tab_id = runtime.snapshot.editor.active_tab_id.clone().unwrap();
            runtime.snapshot.navigator.focused_workspace_id = Some("other-project".into());
            runtime.snapshot.navigator.focused_checkout_id = Some("other-checkout".into());
            runtime.deactivate_editor_tab();
            let event = serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION, "kind": "file_focus", "payload": {"tab_id": tab_id}
            })).unwrap();
            assert!(runtime.dispatch_json(&event));
            assert_eq!(
                runtime.snapshot.navigator.focused_workspace_id.as_deref(),
                Some("workspace:order")
            );
            assert_eq!(
                runtime.snapshot.navigator.focused_checkout_id.as_deref(),
                Some(checkout_id.as_str())
            );
            assert_eq!(
                runtime.snapshot.editor.active_tab_id.as_deref(),
                Some(tab_id.as_str())
            );
            assert_eq!(runtime.snapshot.editor.document.is_some(), !diff);
            if diff {
                assert_eq!(
                    runtime.snapshot.changes.selected_path.as_deref(),
                    Some(file.to_str().unwrap())
                );
            }
            let previous = runtime.snapshot.editor.active_tab_id.clone();
            let invalid = serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION, "kind": "file_focus", "payload": {"tab_id": "deleted-tab"}
            })).unwrap();
            assert!(runtime.dispatch_json(&invalid));
            assert_eq!(runtime.snapshot.editor.active_tab_id, previous);
            assert_eq!(
                runtime.snapshot.status.last_error.as_ref().unwrap().kind,
                "file.focus_failed"
            );
            runtime.snapshot.navigator.workspaces.clear();
            assert!(runtime.dispatch_json(&event));
            assert_eq!(runtime.snapshot.editor.active_tab_id, previous);
            assert_eq!(
                runtime.snapshot.status.last_error.as_ref().unwrap().kind,
                "editor.invalid_context"
            );
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn tab_strip_lists_herdr_tabs_and_then_the_file_that_was_opened() {
        let directory = std::env::temp_dir().join(format!(
            "hide-strip-open-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).expect("checkout directory");
        // The temp root is a symlink on macOS; the catalog keys checkouts by
        // the real path, so the fixture has to use it too. The fixture is also
        // made its own repository, because a directory inside another
        // repository is catalogued under that repository's root instead.
        let directory = directory.canonicalize().expect("a real checkout path");
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&directory)
                .status()
                .expect("git init runs")
                .success()
        );
        let file = directory.join("notes.md");
        std::fs::write(&file, "notes\n").expect("fixture file");
        let checkout_path = directory.to_string_lossy().into_owned();
        let (mut runtime, checkout_id) = tab_order_runtime(&checkout_path);

        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec!["herdr:w-order:t1".to_owned(), "herdr:w-order:t2".to_owned()]
        );

        open_file(&mut runtime, &checkout_id, &file);
        let with_file = strip_ids(&runtime, &checkout_id);
        assert_eq!(with_file.len(), 3);
        assert_eq!(
            &with_file[..2],
            &["herdr:w-order:t1".to_owned(), "herdr:w-order:t2".to_owned()]
        );
        assert!(with_file[2].starts_with("file:"));
        assert_eq!(strip_labels(&runtime, &checkout_id)[2], "notes.md");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn tab_strip_appends_a_reopened_file_nowhere_and_a_new_herdr_tab_at_the_end() {
        let directory = std::env::temp_dir().join(format!(
            "hide-strip-append-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).expect("checkout directory");
        // The temp root is a symlink on macOS; the catalog keys checkouts by
        // the real path, so the fixture has to use it too. The fixture is also
        // made its own repository, because a directory inside another
        // repository is catalogued under that repository's root instead.
        let directory = directory.canonicalize().expect("a real checkout path");
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&directory)
                .status()
                .expect("git init runs")
                .success()
        );
        let file = directory.join("notes.md");
        std::fs::write(&file, "notes\n").expect("fixture file");
        let checkout_path = directory.to_string_lossy().into_owned();
        let (mut runtime, checkout_id) = tab_order_runtime(&checkout_path);

        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        open_file(&mut runtime, &checkout_id, &file);
        let opened = strip_ids(&runtime, &checkout_id);

        // Opening a file that is already open activates its tab; it does not
        // add a second one.
        open_file(&mut runtime, &checkout_id, &file);
        assert_eq!(strip_ids(&runtime, &checkout_id), opened);

        // A Herdr tab created next to an open file goes to the end of the
        // strip, not in front of the file that was there first.
        let grown = ["w-order:t1", "w-order:t2", "w-order:t3"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &grown,
            &grown,
            "w-order:t1"
        ))));
        let after = strip_ids(&runtime, &checkout_id);
        assert_eq!(after.len(), 4);
        assert_eq!(&after[..3], &opened[..]);
        assert_eq!(after[3], "herdr:w-order:t3");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn tab_strip_keeps_a_file_in_its_slot_while_herdr_reorders_around_it() {
        let directory = std::env::temp_dir().join(format!(
            "hide-strip-slot-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).expect("checkout directory");
        // The temp root is a symlink on macOS; the catalog keys checkouts by
        // the real path, so the fixture has to use it too. The fixture is also
        // made its own repository, because a directory inside another
        // repository is catalogued under that repository's root instead.
        let directory = directory.canonicalize().expect("a real checkout path");
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&directory)
                .status()
                .expect("git init runs")
                .success()
        );
        let file = directory.join("notes.md");
        std::fs::write(&file, "notes\n").expect("fixture file");
        let checkout_path = directory.to_string_lossy().into_owned();
        let (mut runtime, checkout_id) = tab_order_runtime(&checkout_path);

        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        open_file(&mut runtime, &checkout_id, &file);
        let file_entry = strip_ids(&runtime, &checkout_id)[2].clone();

        // The operator dropped the file tab between the two Herdr tabs. The
        // move itself is the reorder event's job; what matters here is that
        // the slot the file took is the slot it keeps.
        runtime.checkout_tab_order.insert(
            checkout_id.clone(),
            vec![
                "herdr:w-order:t1".to_owned(),
                file_entry.clone(),
                "herdr:w-order:t2".to_owned(),
            ],
        );
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec![
                "herdr:w-order:t1".to_owned(),
                file_entry.clone(),
                "herdr:w-order:t2".to_owned()
            ]
        );

        // Herdr swapped its two tabs. They swap slots; the file does not move.
        let moved = ["w-order:t2", "w-order:t1"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &moved,
            &tabs,
            "w-order:t1"
        ))));
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec![
                "herdr:w-order:t2".to_owned(),
                file_entry,
                "herdr:w-order:t1".to_owned()
            ]
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn tab_strip_reorder_index_counts_positions_before_the_tab_leaves_the_list() {
        // Herdr inserts into the list it still holds and then takes the moved
        // tab out of its old place, so the index is where the tab that ends up
        // behind the moved one sits now. These three cases are the ones a live
        // 0.8.2 server was observed answering with exactly these orders.
        let current = ["t1", "t2", "t3"].map(str::to_owned);
        assert_eq!(
            herdr_insert_index(&current, &["t2", "t1", "t3"].map(str::to_owned), "t1"),
            Some(2)
        );
        assert_eq!(
            herdr_insert_index(&current, &["t2", "t3", "t1"].map(str::to_owned), "t1"),
            Some(3)
        );
        assert_eq!(
            herdr_insert_index(&current, &["t3", "t1", "t2"].map(str::to_owned), "t3"),
            Some(0)
        );
        // A file tab is not one of Herdr's, so there is no Herdr move to make.
        assert_eq!(
            herdr_insert_index(
                &current,
                &["t1", "t2", "t3"].map(str::to_owned),
                "file:notes"
            ),
            None
        );

        // A workspace split across two checkouts. The strip the operator drags
        // holds t1, t2 and t3; t5 is a sibling checkout's tab that Herdr still
        // counts. Dropping t1 at the end of that strip means "after t3", which
        // is one past the workspace's last position when t5 leads and t5's own
        // position when t5 trails. Reading the index off the strip alone gives
        // 3 in both cases, which puts the tab between t2 and t3.
        let leading = ["t5", "t1", "t2", "t3"].map(str::to_owned);
        assert_eq!(
            herdr_insert_index(&leading, &["t2", "t3", "t1"].map(str::to_owned), "t1"),
            Some(4)
        );
        let trailing = ["t1", "t2", "t3", "t5"].map(str::to_owned);
        assert_eq!(
            herdr_insert_index(&trailing, &["t2", "t3", "t1"].map(str::to_owned), "t1"),
            Some(3)
        );
        // A tab that keeps a successor in its own strip is placed in front of
        // it, wherever the workspace holds that successor.
        assert_eq!(
            herdr_insert_index(&leading, &["t2", "t1", "t3"].map(str::to_owned), "t1"),
            Some(3)
        );
    }

    #[test]
    fn tab_strip_reorder_moves_a_file_tab_without_asking_herdr() {
        let (mut runtime, checkout_id, directory) = strip_checkout("file-move");
        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &directory.to_string_lossy(),
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        open_file(&mut runtime, &checkout_id, &directory.join("notes.md"));
        let file_entry = strip_ids(&runtime, &checkout_id)[2].clone();

        // There is no live connection in this fixture, so a move that reached
        // Herdr would fail loudly. Landing silently is the assertion.
        assert!(reorder_tab(&mut runtime, &checkout_id, &file_entry, 1));
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec![
                "herdr:w-order:t1".to_owned(),
                file_entry.clone(),
                "herdr:w-order:t2".to_owned()
            ]
        );
        assert!(runtime.snapshot().status.last_error.is_none());
        assert!(runtime.pending_tab_move.is_empty());

        // The slot survives the next catalog rebuild, which is what makes the
        // move a move rather than a repaint.
        runtime.ingest_session(Ok(tab_order_payload(
            &directory.to_string_lossy(),
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec![
                "herdr:w-order:t1".to_owned(),
                file_entry,
                "herdr:w-order:t2".to_owned()
            ]
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn tab_strip_reorder_asks_herdr_and_lands_only_once_herdr_reports_the_order() {
        let (mut runtime, checkout_id, directory) = strip_checkout("herdr-move");
        let checkout_path = directory.to_string_lossy().into_owned();
        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        open_file(&mut runtime, &checkout_id, &directory.join("notes.md"));
        let file_entry = strip_ids(&runtime, &checkout_id)[2].clone();
        let before = strip_ids(&runtime, &checkout_id);

        // A Unix socket path has a hard length limit and the checkout fixture
        // can sit deep, so the fixture server lives at a short one of its own.
        let socket_root = PathBuf::from("/tmp").join(format!(
            "herdr-core-tab-move-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&socket_root).expect("socket directory");
        let socket_path = socket_root.join("herdr.sock");
        let listener =
            std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let (mut stream, _) = listener.accept().expect("accept tab.move");
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut line)
                .expect("read request");
            let request: serde_json::Value =
                serde_json::from_str(&line).expect("tab.move request JSON");
            writeln!(
                stream,
                "{}",
                serde_json::json!({
                    "id": request["id"],
                    "result": {
                        "type": "tab_list",
                        "tabs": [
                            {"tab_id": "w-order:t2"},
                            {"tab_id": "w-order:t1"}
                        ]
                    }
                })
            )
            .expect("write tab_list response");
            request
        });
        runtime.live = Some(live::LiveContext {
            socket_path: socket_path.clone(),
            herdr_bin: None,
            runtime: std::sync::Weak::new(),
            notifier: crate::ffi::ChangeNotifier::noop(),
            api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
        });

        // Move the first Herdr tab behind the second. The file tab does not
        // move, so the arrangement differs from Herdr's only in Herdr's own
        // order, which is Herdr's to grant.
        assert!(reorder_tab(
            &mut runtime,
            &checkout_id,
            "herdr:w-order:t1",
            1
        ));
        let request = server.join().expect("fixture server joins");
        assert_eq!(request["method"], "tab.move");
        assert_eq!(
            request["params"],
            serde_json::json!({"tab_id": "w-order:t1", "insert_index": 2})
        );

        // The strip does not move on the operator's word alone.
        assert_eq!(strip_ids(&runtime, &checkout_id), before);
        assert_eq!(
            runtime.pending_tab_move[&checkout_id].herdr_order,
            vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()]
        );

        // Herdr reports the new order; the arrangement lands with it.
        let moved = ["w-order:t2", "w-order:t1"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &moved,
            &tabs,
            "w-order:t1"
        ))));
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec![
                "herdr:w-order:t2".to_owned(),
                "herdr:w-order:t1".to_owned(),
                file_entry
            ]
        );
        assert!(runtime.pending_tab_move.is_empty());

        std::fs::remove_dir_all(&socket_root).ok();
        std::fs::remove_dir_all(&directory).ok();
    }

    /// A repository with one linked worktree, both holding panes of the same
    /// Herdr workspace, which is how a workspace comes to span two checkouts.
    /// Returns the runtime, the repository's checkout id, and both directories.
    fn split_workspace_checkouts(name: &str) -> (Runtime, String, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "hide-split-{name}-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("fixture root");
        let root = root.canonicalize().expect("a real fixture path");
        let repository = root.join("repo");
        std::fs::create_dir_all(&repository).expect("repository directory");
        let git = |arguments: &[&str], directory: &Path| {
            assert!(
                std::process::Command::new("git")
                    // The fixture owns its identity, and it must not reach for
                    // the operator's signing key: a commit the fixture makes
                    // failed whenever the signing agent was not answering,
                    // which made this suite fail for a reason that had nothing
                    // to do with the code under test.
                    .args(["-c", "commit.gpgsign=false"])
                    .args(arguments)
                    .current_dir(directory)
                    .env("GIT_AUTHOR_NAME", "fixture")
                    .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                    .env("GIT_COMMITTER_NAME", "fixture")
                    .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .status()
                    .expect("git runs")
                    .success(),
                "git {arguments:?}"
            );
        };
        git(&["init", "-q", "-b", "main"], &repository);
        std::fs::write(repository.join("notes.md"), "notes\n").expect("fixture file");
        git(&["add", "notes.md"], &repository);
        git(&["commit", "-qm", "notes"], &repository);
        // A linked worktree is a second checkout of the same project, so the
        // catalog gives it its own row under one project.
        let worktree = root.join("feature");
        git(
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                &worktree.to_string_lossy(),
            ],
            &repository,
        );
        let (runtime, checkout_id) = tab_order_runtime(&repository.to_string_lossy());
        (runtime, checkout_id, repository, worktree)
    }

    /// A session payload for one Herdr workspace whose tabs are split across
    /// two directories, in Herdr's own order.
    fn split_workspace_payload(
        tab_order: &[(&str, &str)],
        active_tab_id: &str,
    ) -> SessionSnapshotPayload {
        let tabs = tab_order
            .iter()
            .map(|(tab_id, _)| {
                serde_json::json!({"workspace_id": "w-order", "tab_id": tab_id, "label": ""})
            })
            .collect::<Vec<_>>();
        let panes = tab_order
            .iter()
            .map(|(tab_id, cwd)| serde_json::json!({"pane_id": format!("{tab_id}:p"), "cwd": cwd}))
            .collect::<Vec<_>>();
        let layouts = tab_order
            .iter()
            .map(|(tab_id, _)| {
                serde_json::json!({
                    "workspace_id": "w-order",
                    "tab_id": tab_id,
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": format!("{tab_id}:p"),
                    "panes": [{
                        "pane_id": format!("{tab_id}:p"),
                        "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                    }],
                    "splits": []
                })
            })
            .collect::<Vec<_>>();
        serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [{
                "workspace_id": "w-order",
                "label": "order",
                "active_tab_id": active_tab_id
            }],
            "tabs": tabs,
            "panes": panes,
            "layouts": layouts
        }))
        .expect("split session payload")
    }

    #[test]
    fn tab_strip_reorder_indexes_a_move_in_the_whole_workspace_not_one_checkout() {
        let (mut runtime, checkout_id, repository, worktree) =
            split_workspace_checkouts("index-scope");
        let repository_path = repository.to_string_lossy().into_owned();
        let worktree_path = worktree.to_string_lossy().into_owned();
        // Herdr's list leads with the worktree's tab, so this checkout's tabs
        // do not start at the workspace's first position.
        let order = [
            ("w-order:t5", worktree_path.as_str()),
            ("w-order:t1", repository_path.as_str()),
            ("w-order:t2", repository_path.as_str()),
            ("w-order:t3", repository_path.as_str()),
        ];
        assert!(runtime.ingest_session(Ok(split_workspace_payload(&order, "w-order:t1"))));
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec![
                "herdr:w-order:t1".to_owned(),
                "herdr:w-order:t2".to_owned(),
                "herdr:w-order:t3".to_owned()
            ],
            "the repository's checkout holds only its own three tabs"
        );

        let socket_root = PathBuf::from("/tmp").join(format!(
            "herdr-core-split-move-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&socket_root).expect("socket directory");
        let socket_path = socket_root.join("herdr.sock");
        let listener =
            std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let (mut stream, _) = listener.accept().expect("accept tab.move");
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut line)
                .expect("read request");
            let request: serde_json::Value =
                serde_json::from_str(&line).expect("tab.move request JSON");
            writeln!(
                stream,
                "{}",
                serde_json::json!({
                    "id": request["id"],
                    "result": {
                        "type": "tab_list",
                        "tabs": [
                            {"tab_id": "w-order:t5"},
                            {"tab_id": "w-order:t2"},
                            {"tab_id": "w-order:t3"},
                            {"tab_id": "w-order:t1"}
                        ]
                    }
                })
            )
            .expect("write tab_list response");
            request
        });
        runtime.live = Some(live::LiveContext {
            socket_path: socket_path.clone(),
            herdr_bin: None,
            runtime: std::sync::Weak::new(),
            notifier: crate::ffi::ChangeNotifier::noop(),
            api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
        });

        // Drag the first tab to the end of this checkout's strip. In the
        // checkout's own coordinates that reads as index 3, which Herdr would
        // apply to its four-tab list and land the tab between t2 and t3.
        assert!(reorder_tab(
            &mut runtime,
            &checkout_id,
            "herdr:w-order:t1",
            2
        ));
        let request = server.join().expect("fixture server joins");
        assert_eq!(request["method"], "tab.move");
        assert_eq!(
            request["params"],
            serde_json::json!({"tab_id": "w-order:t1", "insert_index": 4})
        );
        assert!(
            runtime.snapshot().status.last_error.is_none(),
            "a move Herdr can make is not an error"
        );

        // Herdr reports the order the request asked for, so the arrangement
        // lands and the sibling checkout's tab is untouched.
        let moved = [
            ("w-order:t5", worktree_path.as_str()),
            ("w-order:t2", repository_path.as_str()),
            ("w-order:t3", repository_path.as_str()),
            ("w-order:t1", repository_path.as_str()),
        ];
        assert!(runtime.ingest_session(Ok(split_workspace_payload(&moved, "w-order:t1"))));
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec![
                "herdr:w-order:t2".to_owned(),
                "herdr:w-order:t3".to_owned(),
                "herdr:w-order:t1".to_owned()
            ]
        );
        assert!(runtime.pending_tab_move.is_empty());

        std::fs::remove_dir_all(&socket_root).ok();
        std::fs::remove_dir_all(repository.parent().expect("fixture root")).ok();
    }

    #[test]
    fn tab_strip_reorder_keeps_herdrs_order_and_reports_when_a_move_is_refused() {
        let (mut runtime, checkout_id, directory) = strip_checkout("herdr-refused");
        let checkout_path = directory.to_string_lossy().into_owned();
        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        let before = strip_ids(&runtime, &checkout_id);
        runtime.pending_tab_move.insert(
            checkout_id.clone(),
            PendingTabMove {
                desired: vec!["herdr:w-order:t2".to_owned(), "herdr:w-order:t1".to_owned()],
                workspace_id: "w-order".to_owned(),
                herdr_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
                generation: 7,
            },
        );

        assert!(runtime.ingest_local_control_result(
            RemoteControlAction::MoveTab {
                checkout_id: checkout_id.clone(),
                tab_id: "w-order:t1".to_owned(),
                insert_index: 2,
                expected_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
                generation: 7,
            },
            Err("tab.move failed: tab_not_found: tab w-order:t9 not found".to_owned()),
            4,
        ));
        assert_eq!(strip_ids(&runtime, &checkout_id), before);
        assert!(runtime.pending_tab_move.is_empty());
        let error = runtime
            .snapshot()
            .status
            .last_error
            .clone()
            .expect("a refused move is reported");
        assert_eq!(error.kind, "tab.move_refused");
        assert!(error.message.contains("w-order:t1"), "{}", error.message);
        assert!(error.retryable);

        std::fs::remove_dir_all(&directory).ok();
    }

    /// AC12, SC6 recovery. A refused move leaves the strip where Herdr last
    /// put it and says so, and the operator's answer to that is to drag again.
    /// The retry has to be an ordinary drag: the same request goes out, and
    /// the order lands when Herdr reports it, with nothing left over from the
    /// refusal to hold it back.
    #[test]
    fn tab_strip_reorder_a_refused_drag_can_simply_be_dragged_again() {
        let (mut runtime, checkout_id, directory) = strip_checkout("herdr-retry");
        let checkout_path = directory.to_string_lossy().into_owned();
        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        let before = strip_ids(&runtime, &checkout_id);

        let socket_root = PathBuf::from("/tmp").join(format!(
            "herdr-core-tab-retry-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&socket_root).expect("socket directory");
        let socket_path = socket_root.join("herdr.sock");
        let listener =
            std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
        // Both drags are answered; what the answer says does not matter here,
        // because the worker's callback is what carries it and this test
        // drives that by hand.
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let mut requests = Vec::new();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept tab.move");
                let mut line = String::new();
                BufReader::new(stream.try_clone().expect("clone stream"))
                    .read_line(&mut line)
                    .expect("read request");
                let request: serde_json::Value =
                    serde_json::from_str(&line).expect("tab.move request JSON");
                writeln!(
                    stream,
                    "{}",
                    serde_json::json!({
                        "id": request["id"],
                        "result": {
                            "type": "tab_list",
                            "tabs": [{"tab_id": "w-order:t2"}, {"tab_id": "w-order:t1"}]
                        }
                    })
                )
                .expect("write tab_list response");
                requests.push(request);
            }
            requests
        });
        runtime.live = Some(live::LiveContext {
            socket_path: socket_path.clone(),
            herdr_bin: None,
            runtime: std::sync::Weak::new(),
            notifier: crate::ffi::ChangeNotifier::noop(),
            api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
        });

        // The first drag is refused.
        assert!(reorder_tab(
            &mut runtime,
            &checkout_id,
            "herdr:w-order:t1",
            1
        ));
        let generation = runtime.pending_tab_move[&checkout_id].generation;
        assert!(runtime.ingest_local_control_result(
            RemoteControlAction::MoveTab {
                checkout_id: checkout_id.clone(),
                tab_id: "w-order:t1".to_owned(),
                insert_index: 2,
                expected_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
                generation,
            },
            Err("tab.move failed: tab_not_found: tab w-order:t1 not found".to_owned()),
            4,
        ));
        assert_eq!(strip_ids(&runtime, &checkout_id), before);
        assert!(runtime.pending_tab_move.is_empty());
        assert_eq!(
            runtime
                .snapshot()
                .status
                .last_error
                .as_ref()
                .expect("a refused move is reported")
                .kind,
            "tab.move_refused"
        );

        // The same drag again, with nothing else in between.
        assert!(reorder_tab(
            &mut runtime,
            &checkout_id,
            "herdr:w-order:t1",
            1
        ));
        let requests = server.join().expect("fixture server joins");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["method"], "tab.move");
        assert_eq!(
            requests[1]["params"], requests[0]["params"],
            "the retry asks Herdr for exactly what the refused drag asked for"
        );
        assert_eq!(
            runtime.pending_tab_move[&checkout_id].herdr_order,
            vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()]
        );

        // Herdr grants it this time, and the strip lands.
        let moved = ["w-order:t2", "w-order:t1"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &checkout_path,
            &moved,
            &tabs,
            "w-order:t1"
        ))));
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec!["herdr:w-order:t2".to_owned(), "herdr:w-order:t1".to_owned()]
        );
        assert!(runtime.pending_tab_move.is_empty());

        std::fs::remove_dir_all(&socket_root).ok();
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn tab_strip_reorder_ignores_a_result_a_later_drag_has_replaced() {
        let (mut runtime, checkout_id, directory) = strip_checkout("herdr-superseded");
        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &directory.to_string_lossy(),
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        let live = PendingTabMove {
            desired: vec!["herdr:w-order:t2".to_owned(), "herdr:w-order:t1".to_owned()],
            workspace_id: "w-order".to_owned(),
            herdr_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
            generation: 9,
        };
        runtime
            .pending_tab_move
            .insert(checkout_id.clone(), live.clone());

        // The first drag's refusal arrives after a second drag replaced it.
        assert!(runtime.ingest_local_control_result(
            RemoteControlAction::MoveTab {
                checkout_id: checkout_id.clone(),
                tab_id: "w-order:t1".to_owned(),
                insert_index: 2,
                expected_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
                generation: 8,
            },
            Err("tab.move failed: transport".to_owned()),
            4,
        ));
        assert_eq!(runtime.pending_tab_move[&checkout_id], live);
        assert!(runtime.snapshot().status.last_error.is_none());

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn tab_strip_reorder_refuses_a_remote_checkout() {
        let (mut runtime, checkout_id, directory) = strip_checkout("remote-refusal");
        let tabs = ["w-order:t1", "w-order:t2"];
        assert!(runtime.ingest_session(Ok(tab_order_payload(
            &directory.to_string_lossy(),
            &tabs,
            &tabs,
            "w-order:t1"
        ))));
        let before = strip_ids(&runtime, &checkout_id);
        for workspace in &mut runtime.snapshot.navigator.workspaces {
            workspace.remote_target_id = Some("mini".to_owned());
        }

        assert!(reorder_tab(
            &mut runtime,
            &checkout_id,
            "herdr:w-order:t1",
            1
        ));
        assert_eq!(strip_ids(&runtime, &checkout_id), before);
        assert!(runtime.pending_tab_move.is_empty());
        assert_eq!(
            runtime
                .snapshot()
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.as_str()),
            Some("tab.reorder_remote")
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn tab_label_turns_a_herdr_number_into_a_name_and_keeps_a_named_tab() {
        assert_eq!(crate::model::display_tab_label("2", "w1:t2"), "Tab 2");
        assert_eq!(crate::model::display_tab_label(" 2 ", "w1:t2"), "Tab 2");
        assert_eq!(crate::model::display_tab_label("notes", "w1:t2"), "notes");
        // A tab Herdr reports with no label at all falls back to its own id,
        // which is still its identity rather than its place in the strip.
        assert_eq!(crate::model::display_tab_label("", "w1:t2"), "w1:t2");
    }

    /// The shell used to name an unlabelled tab after its position, which
    /// renamed every tab whenever one moved. Nothing may reintroduce that.
    #[test]
    fn tab_label_has_no_position_derived_path_left_in_the_shell() {
        let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS");
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&shell).expect("the shell source directory") {
            let path = entry.expect("a shell source entry").path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("swift") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a readable Swift source");
            if source.contains("fallbackIndex") || source.contains("displayLabel") {
                offenders.push(path.display().to_string());
            }
            // The same class of defect, read the other way: the shell taking a
            // label the core formatted and parsing the number back out of it.
            // That writes the "Tab N" convention down a second time across the
            // FFI boundary, where a change to either half breaks the other in
            // silence. The core decides the next label; the shell draws it.
            if source.contains("hasPrefix(\"tab \")") || source.contains("hasPrefix(\"Tab \")") {
                offenders.push(path.display().to_string());
            }
        }
        assert!(
            offenders.is_empty(),
            "a position-derived or reverse-parsed tab label path is back in {offenders:?}"
        );
    }

    #[test]
    fn tab_label_names_the_next_tab_after_the_lowest_free_herdr_number() {
        let next = |labels: &[&str]| crate::model::next_tab_label(labels.iter().copied());
        assert_eq!(next(&[]), "Tab 1");
        assert_eq!(next(&["1", "2"]), "Tab 3");
        // The gap is taken before the end, so closing tab 1 and adding one
        // gives Tab 1 back rather than climbing forever.
        assert_eq!(next(&["2", "3"]), "Tab 1");
        // A named tab holds no number. A tab Herdr stores the display way
        // holds its number too: the shell hands `Tab N` to `tab.create`
        // verbatim, and counting only bare numbers made every new tab
        // `Tab 2` beside the last one.
        assert_eq!(next(&["1", "notes"]), "Tab 2");
        assert_eq!(next(&["Tab 1"]), "Tab 2");
        assert_eq!(next(&["1", "Tab 2", "Tab 2"]), "Tab 3");
        assert_eq!(next(&[" 2 ", "1"]), "Tab 3");
    }

    /// Reads one view struct's body out of the shell's SwiftUI source.
    ///
    /// The two surfaces that make up the window's first row live in one file,
    /// so a whole-file scan would answer for views this rule does not reach.
    fn shell_view_body(source: &str, declaration: &str) -> String {
        let start = source
            .find(declaration)
            .unwrap_or_else(|| panic!("the shell no longer declares {declaration}"));
        let rest = &source[start..];
        // Every view in this file closes at column zero, so the first such
        // brace after the declaration ends the struct.
        let end = rest
            .find("\n}\n")
            .unwrap_or_else(|| panic!("{declaration} has no closing brace"));
        rest[..end].to_owned()
    }

    /// R2: the window draws no system titlebar and keeps its title string.
    ///
    /// What the window got is an AppKit answer, so the proof lives in the
    /// Swift suite `MainWindowChromeTests`, which builds a window, applies the
    /// chrome, and asks AppKit. `swift test --filter` exits zero when its
    /// filter matches nothing, so a check bound to that suite would go green
    /// if the suite were deleted. This is the guard that closes: it fails if
    /// the chrome stops being applied, and it fails if the suite that proves
    /// it is gone.
    #[test]
    fn main_window_hides_the_system_titlebar_and_keeps_its_title() {
        let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos");
        let read = |relative: &str| {
            std::fs::read_to_string(shell.join(relative))
                .unwrap_or_else(|_| panic!("the shell no longer has {relative}"))
        };

        let chrome = read("Sources/HerdrMacOS/MainWindowChrome.swift");
        for setting in [
            "static let title = \"hide\"",
            "window.styleMask.insert(.fullSizeContentView)",
            "window.titlebarAppearsTransparent = true",
            "window.titleVisibility = .hidden",
            "hosting.safeAreaRegions = []",
        ] {
            assert!(
                chrome.contains(setting),
                "the window chrome no longer says `{setting}`"
            );
        }

        let app = read("Sources/HerdrMacOS/HerdrApp.swift");
        assert!(
            app.contains("MainWindowChrome.apply(to: window"),
            "the main window no longer takes its chrome from MainWindowChrome"
        );
        assert!(
            !app.contains("window.title ="),
            "the window title is set beside the chrome again, so the two can disagree"
        );

        let suite = read("Tests/HerdrMacOSTests/MainWindowChromeTests.swift");
        for probe in [
            "window.styleMask.contains(.fullSizeContentView)",
            "window.titleVisibility == .hidden",
            "window.title == \"hide\"",
            "firstRow.origin.y == 0",
        ] {
            assert!(
                suite.contains(probe),
                "the window chrome suite no longer asks AppKit for `{probe}`"
            );
        }
    }

    /// R7: the strip's height, the traffic-light inset, and the spacing
    /// between the first row's controls come from `HideTheme`.
    ///
    /// A number written at the call site is how two surfaces that should
    /// match drift apart, and the traffic lights are the case where drifting
    /// puts a control underneath a system button.
    #[test]
    fn first_row_metrics_come_from_theme_tokens_and_not_from_view_literals() {
        let source = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS/HideUI.swift"),
        )
        .expect("the shell's SwiftUI source");

        let tokens = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../macos/Sources/HerdrMacOS/HideTheme.swift"),
        )
        .expect("the shell's theme token source");

        for token in ["tabStripHeight", "trafficLightInset"] {
            assert!(
                tokens.contains(&format!("static let {token}: CGFloat")),
                "HideTheme.Layout no longer declares {token}"
            );
        }

        let mut offenders = Vec::new();
        for declaration in [
            "private struct HideTabStrip: View {",
            "private struct HideBrandHeader: View {",
        ] {
            for line in shell_view_body(&source, declaration).lines() {
                let trimmed = line.trim();
                // Spacing between controls, the padding that clears the
                // traffic lights, and the row's own height. A square control
                // written `width:height:` is a control's size rather than one
                // of those three, so it is not this rule's business.
                let measured = trimmed
                    .split_once(".padding(")
                    .or_else(|| trimmed.split_once(".frame(height:"))
                    .or_else(|| trimmed.split_once("HStack(spacing:"))
                    .or_else(|| trimmed.split_once("VStack(spacing:"));
                let Some((_, arguments)) = measured else {
                    continue;
                };
                let head = arguments.split(')').next().unwrap_or(arguments);
                let value = head.rsplit(',').next().unwrap_or(head).trim();
                // Zero is the absence of spacing rather than a design value.
                if value == "0" {
                    continue;
                }
                if value.starts_with(|c: char| c.is_ascii_digit()) {
                    offenders.push(format!("{declaration} -> {trimmed}"));
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "the window's first row measures itself with literals: {offenders:#?}"
        );
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
        // Herdr's bare tab number reads as a label only after the display
        // rule turns it into a name.
        assert_eq!(checkout.tabs[0].label.as_deref(), Some("Tab 2"));
        assert_eq!(checkout.tabs[0].panes[0].cwd, checkout_path);
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("plain:p1")
        );
        assert_eq!(
            runtime
                .snapshot()
                .active_pane_layout()
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
        runtime.snapshot.navigator.workspaces =
            workspace::build_catalog(&[], &spaces, &no_worktrees());
        runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
        runtime.reset_terminal_projection(None);

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [{"workspace_id": "w3M", "label": "herdr-ide"}],
            "panes": [{"pane_id": "w3M:p1", "cwd": checkout_path}],
            "tabs": [{"workspace_id": "w3M", "tab_id": "w3M:t1", "label": ""}],
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
            workspaces: workspace::build_catalog(
                &[],
                &spaces,
                &crate::model::WorktreeCatalogSnapshot::default(),
            ),
            roots: workspace::root_index(&spaces),
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
        assert_eq!(
            workspace_snapshot.session_workspace_ids,
            vec!["w3M".to_owned()]
        );
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
        runtime.snapshot.navigator.workspaces =
            workspace::build_catalog(&[], &spaces, &no_worktrees());
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
            "tabs": [{"workspace_id": "w3P", "tab_id": "w3P:t1", "label": ""}, {"workspace_id": "w3Z", "tab_id": "w3Z:t1", "label": ""}],
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
            workspaces: workspace::build_catalog(
                &[],
                &spaces,
                &crate::model::WorktreeCatalogSnapshot::default(),
            ),
            roots: workspace::RootIndex::new(),
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
                .active_pane_layout()
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
            "tabs": [{"workspace_id": "w2W", "tab_id": "w2W:t1", "label": ""}],
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

        let spaces = Runtime::session_spaces(&payload, "");

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

        let catalog = workspace::build_catalog(&registrations, &spaces, &no_worktrees());

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
            workspaces: workspace::build_catalog(
                &[],
                &spaces,
                &crate::model::WorktreeCatalogSnapshot::default(),
            ),
            roots: workspace::root_index(&spaces),
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
        assert_eq!(
            navigator.focused_checkout_id.as_deref(),
            Some(checkout_id.as_str())
        );
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
        runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
            workspace_id: "w2X".to_owned(),
            tab_id: "w2X:t1".to_owned(),
            focused_pane_id: "w2X:pB".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w2X:pB".to_owned(),
            },
        }];
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
        assert!(runtime.snapshot().active_pane_layout().is_none());
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
            "tabs": [{"workspace_id": "w2X", "tab_id": "w2X:t1", "label": ""}, {"workspace_id": "w3V", "tab_id": "w3V:t1", "label": ""}],
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
            roots: workspace::RootIndex::new(),
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
                .active_pane_layout()
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
        runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
            workspace_id: "w-close".to_owned(),
            tab_id: "w-close:t1".to_owned(),
            focused_pane_id: "w-close:p1".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w-close:p1".to_owned(),
            },
        }];
        runtime.restore_hint_pending = false;

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [{"pane_id": "w-close:p2", "cwd": checkout_path}],
            "focused_pane_id": "w-close:p2",
            "tabs": [{"workspace_id": "w-close", "tab_id": "w-close:t1", "label": ""}],
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
                roots: workspace::RootIndex::new(),
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
                .active_pane_layout()
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
        runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
            workspace_id: "w-last".to_owned(),
            tab_id: "w-last:t1".to_owned(),
            focused_pane_id: "w-last:p1".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w-last:p1".to_owned(),
            },
        }];
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
                roots: workspace::RootIndex::new(),
            }),
        ));
        assert_eq!(runtime.snapshot().terminal.pane_id, None);
        assert_eq!(runtime.snapshot().focused.pane_id, None);
        assert_eq!(runtime.snapshot().ui_state.selected_pane_id, None);
        assert!(runtime.snapshot().active_pane_layout().is_none());
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
        runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
            workspace_id: "w-foreign".to_owned(),
            tab_id: "w-foreign:t1".to_owned(),
            focused_pane_id: "w-foreign:p1".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w-foreign:p1".to_owned(),
            },
        }];
        runtime.restore_hint_pending = false;

        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [],
            "panes": [{"pane_id": "w-focused:p2", "cwd": checkout_path}],
            "focused_pane_id": "w-focused:p2",
            "tabs": [{"workspace_id": "w-focused", "tab_id": "w-focused:t1", "label": ""}],
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
                roots: workspace::RootIndex::new(),
            }),
        ));
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w-foreign:p1")
        );
        assert!(runtime.snapshot().active_pane_layout().is_none());
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
            "tabs": [{"workspace_id": "old-workspace", "tab_id": "old-workspace:t1", "label": ""}],
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
            roots: workspace::RootIndex::new(),
        };

        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
        assert!(runtime.snapshot().active_pane_layout().is_none());
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
            "tabs": [{"workspace_id": "wL", "tab_id": "wL:t1", "label": ""}],
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
            roots: workspace::RootIndex::new(),
        };

        assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
        assert_eq!(runtime.snapshot().status.last_error, None);
        assert_eq!(
            runtime
                .snapshot()
                .active_pane_layout()
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
            "tabs": [{"workspace_id": "w19", "tab_id": "w19:t1", "label": ""}],
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
                roots: workspace::RootIndex::new(),
            }),
        ));
        assert_eq!(runtime.snapshot().status.last_error, None);
        assert_eq!(
            runtime.snapshot().terminal.pane_id.as_deref(),
            Some("w19:p1")
        );
        assert!(runtime.snapshot().active_pane_layout().is_some());
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
        runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
            workspace_id: "w3P".to_owned(),
            tab_id: "w3P:t1".to_owned(),
            focused_pane_id: "w3P:p1".to_owned(),
            zoomed: false,
            root: PaneLayoutNodeSnapshot::Pane {
                pane_id: "w3P:p1".to_owned(),
            },
        }];
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
            "tabs": [{"workspace_id": "w3P", "tab_id": "w3P:t1", "label": ""}],
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
                roots: workspace::RootIndex::new(),
            }),
        ));
        // The layout stays in the snapshot because it is Herdr's and it
        // describes a tab that exists. What must not happen is drawing it,
        // and that is settled by the tab projection going empty and the
        // projection-unavailable error being raised, both asserted here. The
        // shell has no checkout to look a tab up in, so it has no layout to
        // draw either.
        assert_eq!(runtime.snapshot().pane_layouts.len(), 1);
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

    /// AC2, AC4, SC3. An operator focus writes the pane's read record to the
    /// store, and a pane Herdr stops reporting is gone from the file on the
    /// next save, so the record cannot grow without bound.
    #[test]
    fn read_record_is_written_for_the_operator_focused_pane_and_evicted_when_it_disappears() {
        let mut runtime = live_runtime();
        let state_path = runtime.state_path.clone();
        runtime.ingest_session(Ok(working_payload()));
        assert!(
            runtime.snapshot().ui_state.pane_read_records.is_empty(),
            "the focus that arrived with the session is not a look the operator took"
        );

        assert!(runtime.dispatch_json(&operator_focus_event("w1:p1")));
        assert!(
            runtime
                .snapshot()
                .ui_state
                .pane_read_records
                .contains_key("w1:p1"),
            "the pane the operator chose is read"
        );
        let stored = std::fs::read_to_string(&state_path).expect("state file");
        assert!(stored.contains("w1:p1"), "the record reached the file");

        let empty: SessionSnapshotPayload =
            serde_json::from_value(serde_json::json!({"agents": []})).expect("empty payload");
        runtime.ingest_session(Ok(empty));
        assert!(
            runtime.snapshot().ui_state.pane_read_records.is_empty(),
            "a pane Herdr stopped reporting leaves no record behind"
        );
        // The persisted selection may still name the pane, so the read
        // records are checked on their own.
        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&state_path).expect("state file"))
                .expect("state file is JSON");
        assert!(
            stored["pane_read_records"]
                .as_object()
                .is_none_or(|records| !records.contains_key("w1:p1")),
            "the record left the file too"
        );
        let _ = std::fs::remove_file(&state_path);
    }

    /// AC2, AC7, SC1. The defect the PRD was written against, from the other
    /// side: one click on a Done row clears that row and nothing else, even
    /// though Herdr reports the whole tab seen and names its own focused pane.
    #[test]
    fn read_record_follows_the_row_the_operator_clicked_and_no_other() {
        let mut runtime = live_runtime();
        let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));
        assert_eq!(
            unread_panes(&runtime),
            vec!["w1:p1", "w1:p2", "w1:p3"],
            "nothing is read before the operator looks at anything"
        );

        assert!(runtime.dispatch_json(&operator_focus_event("w1:p3")));
        assert_eq!(
            unread_panes(&runtime),
            vec!["w1:p1", "w1:p2"],
            "only the clicked row leaves Done"
        );
    }

    /// AC2, SC1. Bringing a tab forward makes Herdr report the pane that tab
    /// last had focused. Nobody chose that pane in this session, so no record
    /// moves and the rows stay where they are.
    #[test]
    fn read_record_ignores_the_focus_a_tab_carries_when_it_comes_forward() {
        let mut runtime = live_runtime();
        let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p2")));

        assert!(
            runtime.snapshot().ui_state.pane_read_records.is_empty(),
            "a focus Hide only inherited is not a look the operator took"
        );
        assert_eq!(unread_panes(&runtime), vec!["w1:p1", "w1:p2", "w1:p3"]);
    }

    /// AC4, SC3. A launch inherits Herdr's focus and puts the terminal back on
    /// the pane the last session ended on. Neither is the operator looking at
    /// anything, so an item that was unread before the quit is still unread.
    #[test]
    fn read_record_survives_a_launch_that_inherits_a_focus() {
        let mut runtime = live_runtime();
        let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));
        assert!(runtime.dispatch_json(&restore_focus_event("w1:p1")));

        assert!(
            runtime.snapshot().ui_state.pane_read_records.is_empty(),
            "restoring the last session's selection reads nothing"
        );
        assert_eq!(unread_panes(&runtime), vec!["w1:p1", "w1:p2", "w1:p3"]);
    }

    /// AC3, R2. While the operator stays on the pane they chose, what the
    /// agent does there is read as it happens, so the row does not come back.
    #[test]
    fn read_record_keeps_up_with_the_pane_the_operator_is_watching() {
        let mut runtime = live_runtime();
        let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));
        assert!(runtime.dispatch_json(&operator_focus_event("w1:p3")));

        let moved = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6031)];
        runtime.ingest_session(Ok(finished_tab_payload(&moved, "w1:p3")));
        assert_eq!(
            unread_panes(&runtime),
            vec!["w1:p1", "w1:p2"],
            "the watched pane stays read as its agent moves"
        );
    }

    /// AC2, R2. Once Herdr moves focus off the pane the operator chose, that
    /// pane stops counting as watched, so its next change comes back unread.
    #[test]
    fn read_record_stops_following_a_pane_herdr_moved_focus_away_from() {
        let mut runtime = live_runtime();
        let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));
        assert!(runtime.dispatch_json(&operator_focus_event("w1:p3")));
        // The requested focus lands, then a spawned pane takes it away.
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p3")));
        runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));

        let moved = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6031)];
        runtime.ingest_session(Ok(finished_tab_payload(&moved, "w1:p1")));
        assert_eq!(
            unread_panes(&runtime),
            vec!["w1:p1", "w1:p2", "w1:p3"],
            "a pane nobody is watching comes back unread when it changes"
        );
    }

    /// AC2, AC3, AC4, R2. A pane is a pane: a remote pane earns a read record
    /// from the focus its own server reports, exactly as a local pane earns one
    /// from Hide's focus. The ledger is pruned by pane id namespace, so a local
    /// sync cannot drop a remote record and one target cannot drop another's.
    /// Before this, no record survived for a remote pane and a stopped remote
    /// pane the operator had read still demanded a close confirmation.
    #[test]
    fn read_record_is_scoped_by_pane_id_namespace_across_servers() {
        let mut runtime = live_runtime();
        let state_path = runtime.state_path.clone();
        for target_id in ["mini", "build"] {
            runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
                target_id: target_id.to_owned(),
                state: "not_connected".to_owned(),
                message: None,
                session: None,
                files: RemoteFileListSnapshot::idle(),
            });
        }
        let remote_session = |target_id: &str, pane_ids: &[&str], focused: Option<&str>| {
            let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
                "agents": pane_ids
                    .iter()
                    .map(|pane_id| serde_json::json!({
                        "pane_id": pane_id,
                        "workspace_label": "Remote",
                        "agent": "codex",
                        "agent_status": "idle",
                        "state_change_seq": 4,
                        "tokens": {"status_done_new": "\u{25cf}", "activity": "0000000000001"}
                    }))
                    .collect::<Vec<_>>()
            }))
            .expect("remote payload");
            let mut agents = project_agents(payload).agents;
            for agent in &mut agents {
                agent.pane_id = remote_pane_id_prefix(target_id) + &agent.pane_id;
                agent.id = agent.pane_id.clone();
            }
            // The pane tree arrives freshly projected with no read axis
            // applied, which is what a remote sync actually delivers.
            let panes = agents
                .iter()
                .map(|agent| {
                    let mut projected = pane(&agent.pane_id, "/tmp/hide-remote-tree");
                    projected.status_label = agent.status_label.clone();
                    projected.requires_close_confirmation = agent.requires_close_confirmation;
                    projected
                })
                .collect::<Vec<_>>();
            let mut remote_workspace = workspace(
                "remote:ws",
                "Remote",
                "/tmp/hide-remote-tree",
                vec![checkout(
                    "remote:ws",
                    "remote:checkout",
                    "/tmp/hide-remote-tree",
                    None,
                )],
            );
            remote_workspace.checkouts[0].tabs = vec![TabSnapshot {
                id: Some("remote:tab".to_owned()),
                workspace_id: Some("remote:ws".to_owned()),
                checkout_id: Some("remote:checkout".to_owned()),
                label: Some("Session".to_owned()),
                empty: false,
                panes,
            }];
            RemoteSessionSnapshot {
                workspaces: vec![remote_workspace],
                agents,
                active_tab_ids: BTreeMap::new(),
                focused_workspace_id: None,
                focused_checkout_id: None,
                focused_tab_id: None,
                focused_pane_id: focused.map(|pane_id| remote_pane_id_prefix(target_id) + pane_id),
                pane_layouts: Vec::new(),
            }
        };
        let stored_agents = |runtime: &Runtime, target_id: &str| {
            runtime
                .snapshot
                .status
                .remote
                .iter()
                .find(|status| status.target_id == target_id)
                .and_then(|status| status.session.as_ref())
                .map(|session| {
                    session
                        .agents
                        .iter()
                        .map(|agent| {
                            (
                                agent.pane_id.clone(),
                                (
                                    agent.status_label.clone(),
                                    agent.requires_close_confirmation,
                                    agent.group.clone(),
                                ),
                            )
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .expect("remote session")
        };
        let tree_panes = |runtime: &Runtime, target_id: &str| {
            runtime
                .snapshot
                .status
                .remote
                .iter()
                .find(|status| status.target_id == target_id)
                .and_then(|status| status.session.as_ref())
                .map(|session| {
                    session
                        .workspaces
                        .iter()
                        .flat_map(|workspace| workspace.checkouts.iter())
                        .flat_map(|checkout| checkout.tabs.iter())
                        .flat_map(|tab| tab.panes.iter())
                        .map(|pane| {
                            (
                                pane.id.clone(),
                                (pane.status_label.clone(), pane.requires_close_confirmation),
                            )
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .expect("remote session")
        };

        runtime.ingest_session(Ok(working_payload()));
        assert!(runtime.dispatch_json(&operator_focus_event("w1:p1")));
        assert!(
            runtime
                .snapshot
                .ui_state
                .pane_read_records
                .contains_key("w1:p1")
        );

        runtime.ingest_remote_session(
            "mini",
            Ok(remote_session("mini", &["w9:p1", "w9:p2"], Some("w9:p1"))),
        );
        assert!(
            runtime
                .snapshot
                .ui_state
                .pane_read_records
                .contains_key("remote:mini:pane:w9:p1"),
            "the pane the remote server focuses gets a read record like any other"
        );
        let mini = stored_agents(&runtime, "mini");
        assert_eq!(
            mini["remote:mini:pane:w9:p1"],
            ("Idle".to_owned(), false, "seen".to_owned()),
            "a stopped remote pane the operator has read closes without a prompt"
        );
        assert_eq!(
            mini["remote:mini:pane:w9:p2"],
            ("Done".to_owned(), true, "done".to_owned()),
            "the pane beside it is still unread"
        );
        // The pane tree is what the remote pane surface reads, so the read
        // axis has to reach it and not only the agent rows.
        let tree = tree_panes(&runtime, "mini");
        assert_eq!(
            tree["remote:mini:pane:w9:p1"],
            ("Idle".to_owned(), false),
            "the remote pane tree carries the read pane's answer, not the projected one"
        );
        assert_eq!(
            tree["remote:mini:pane:w9:p2"],
            ("Done".to_owned(), true),
            "the unread pane beside it still says so"
        );

        runtime.ingest_session(Ok(working_payload()));
        assert!(
            runtime
                .snapshot
                .ui_state
                .pane_read_records
                .contains_key("remote:mini:pane:w9:p1"),
            "a local sync never prunes a remote record"
        );

        runtime.ingest_remote_session(
            "build",
            Ok(remote_session("build", &["w2:p1"], Some("w2:p1"))),
        );
        assert!(
            runtime
                .snapshot
                .ui_state
                .pane_read_records
                .contains_key("remote:mini:pane:w9:p1"),
            "one target's sync never prunes another target's record"
        );

        runtime.ingest_remote_session("mini", Ok(remote_session("mini", &["w9:p2"], None)));
        let records = runtime
            .snapshot
            .ui_state
            .pane_read_records
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            records,
            vec!["remote:build:pane:w2:p1".to_owned(), "w1:p1".to_owned(),],
            "a target prunes only its own namespace"
        );
        let _ = std::fs::remove_file(&state_path);
    }

    /// R2, AC2. The read record pass prunes text scales on the same tick, and
    /// it may only drop what it has the authority to drop. Unscoped it deleted
    /// every remote pane's zoom on the next local sync, and it deleted the file
    /// editor's zoom on every agent state change, because the editor's scale
    /// was keyed into the pane map under a name no pane is ever reported under.
    #[test]
    fn read_record_change_leaves_remote_and_editor_zoom_alone() {
        let mut runtime = runtime();
        let state_path = runtime.state_path.clone();
        let zoom_pane = |runtime: &mut Runtime, pane_id: &str| {
            let event = serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "kind": "pane_text_scale",
                "payload": {"pane_id": pane_id, "direction": "in"}
            }))
            .expect("pane text scale event");
            assert!(runtime.dispatch_json(&event));
        };
        let idle_payload = || -> SessionSnapshotPayload {
            serde_json::from_value(serde_json::json!({
                "agents": [{
                    "pane_id": "w1:p1",
                    "workspace_label": "Fixture",
                    "agent": "codex",
                    "agent_status": "idle",
                    "tokens": {"status_idle": "\u{25cb}", "activity": "0000000000002"}
                }],
                "tabs": [{"workspace_id": "w1", "tab_id": "t1", "label": ""}],
                "layouts": [{
                    "workspace_id": "w1", "tab_id": "t1", "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "w1:p1",
                    "panes": [{"pane_id": "w1:p1",
                               "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                }]
            }))
            .expect("idle payload")
        };

        runtime.ingest_session(Ok(working_payload()));
        zoom_pane(&mut runtime, "w1:p1");
        zoom_pane(&mut runtime, "remote:mini:pane:w9:p1");
        zoom_pane(&mut runtime, "w1:p9");
        let editor_zoom = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "editor_text_scale",
            "payload": {"direction": "in"}
        }))
        .expect("editor text scale event");
        assert!(runtime.dispatch_json(&editor_zoom));
        assert_eq!(runtime.snapshot().ui_state.editor_text_scale, 1.1);

        // The focused pane's state moves, so the read record moves and the
        // prune runs. This is the tick that used to lose both zooms.
        runtime.ingest_session(Ok(idle_payload()));
        let scales = runtime.snapshot().ui_state.pane_text_scales.clone();
        assert_eq!(
            scales.get("remote:mini:pane:w9:p1"),
            Some(&1.1),
            "a local sync holds no remote pane list and must not prune remote keys"
        );
        assert_eq!(
            scales.get("w1:p1"),
            Some(&1.1),
            "a live pane keeps its zoom"
        );
        assert!(
            !scales.contains_key("w1:p9"),
            "a local pane the server stopped reporting still loses its zoom"
        );
        assert_eq!(
            runtime.snapshot().ui_state.editor_text_scale,
            1.1,
            "the editor is not a pane, so a pane prune cannot reach its zoom"
        );

        // A navigator or keyboard save carries the editor zoom through for the
        // same reason it carries the pane map through.
        let ui_state_update = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {"left_sidebar_visible": false}
        }))
        .expect("ui state update event");
        runtime.dispatch_json(&ui_state_update);
        assert_eq!(runtime.snapshot().ui_state.editor_text_scale, 1.1);

        let stored = std::fs::read_to_string(&state_path).expect("state file");
        assert!(
            stored.contains("editor_text_scale"),
            "the editor zoom is persisted, so it survives a restart"
        );
        let _ = std::fs::remove_file(&state_path);
    }

    /// The pane tree is projected separately from the navigator's agent rows.
    /// It published `Done` and demanded a close confirmation for every pane the
    /// operator had already read, because that projection never saw the record
    /// ledger.
    #[test]
    fn read_record_reaches_the_pane_tree_and_not_only_the_agent_rows() {
        let mut runtime = live_runtime();
        let state_path = runtime.state_path.clone();
        let idle = |pane_id: &str| {
            serde_json::json!({
                "pane_id": pane_id,
                "workspace_label": "Fixture",
                "agent": "codex",
                "agent_status": "idle",
                "tokens": {"status_idle": "\u{25cb}", "activity": "0000000000001"}
            })
        };
        let checkout_path = "/private/tmp/hide-read-record-pane-tree";
        runtime.snapshot.ui_state.workspace_registrations = vec![WorkspaceRegistration {
            id: "workspace:read-record".to_owned(),
            label: "read-record".to_owned(),
            path: checkout_path.to_owned(),
            device_id: "local".to_owned(),
        }];
        runtime.rebuild_catalog();
        let checkout_id =
            workspace::checkout_id_for_path("workspace:read-record", Path::new(checkout_path));
        runtime.snapshot.navigator.focused_workspace_id = Some("workspace:read-record".to_owned());
        runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id);
        runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
        runtime.reset_terminal_projection(None);
        let tab = |index: u8| {
            serde_json::json!({
                "workspace_id": "herdr-workspace",
                "tab_id": format!("herdr-workspace:t{index}"),
                "label": index.to_string()
            })
        };
        let layout = |index: u8| {
            let pane_id = format!("plain:p{index}");
            serde_json::json!({
                "workspace_id": "herdr-workspace",
                "tab_id": format!("herdr-workspace:t{index}"),
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": pane_id,
                "panes": [{"pane_id": pane_id,
                           "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            })
        };
        let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
            "agents": [idle("plain:p1"), idle("plain:p2")],
            "focused_pane_id": "plain:p1",
            "panes": [
                {"pane_id": "plain:p1", "cwd": checkout_path},
                {"pane_id": "plain:p2", "cwd": checkout_path}
            ],
            "tabs": [tab(1), tab(2)],
            "layouts": [layout(1), layout(2)]
        }))
        .expect("session payload");
        runtime.ingest_session(Ok(payload));
        assert!(runtime.dispatch_json(&operator_focus_event("plain:p1")));

        let snapshot = runtime.snapshot();
        let panes = snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .map(|pane| {
                (
                    pane.id.as_str(),
                    pane.status_label.as_str(),
                    pane.requires_close_confirmation,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            panes,
            vec![("plain:p1", "Idle", false), ("plain:p2", "Done", true)],
            "the read pane is Idle and closes without a prompt; the unread one does not"
        );

        for agent in &snapshot.navigator.agents {
            let pane = panes
                .iter()
                .find(|(id, _, _)| *id == agent.pane_id)
                .expect("every agent pane is in the tree");
            assert_eq!(
                (pane.1, pane.2),
                (
                    agent.status_label.as_str(),
                    agent.requires_close_confirmation
                ),
                "pane {} disagrees with its agent row",
                agent.pane_id
            );
        }
        let _ = std::fs::remove_file(&state_path);
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

    /// Two registered checkouts with the first focused, so a reveal into the
    /// second has a checkout switch to make. The second holds
    /// `deep/nested/leaf/target.txt`, which is the shape SC1 and SC2 describe.
    fn reveal_runtime() -> (Runtime, PathBuf, String, PathBuf, String) {
        let mut roots = Vec::new();
        for name in ["one", "two"] {
            let directory = std::env::temp_dir().join(format!(
                "hide-reveal-{name}-{}-{}",
                std::process::id(),
                NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(directory.join("deep/nested/leaf")).expect("fixture tree");
            let directory = directory.canonicalize().expect("a real checkout path");
            assert!(
                std::process::Command::new("git")
                    .args(["init", "-q", "-b", "main"])
                    .current_dir(&directory)
                    .status()
                    .expect("git init runs")
                    .success()
            );
            std::fs::write(directory.join("deep/nested/leaf/target.txt"), "found\n")
                .expect("fixture file");
            roots.push(directory);
        }
        let mut runtime = runtime();
        runtime.snapshot.ui_state.workspace_registrations = roots
            .iter()
            .enumerate()
            .map(|(index, path)| WorkspaceRegistration {
                id: format!("workspace:{index}"),
                label: format!("workspace {index}"),
                path: path.to_string_lossy().into_owned(),
                device_id: "local".to_owned(),
            })
            .collect();
        runtime.rebuild_catalog();
        let ids = roots
            .iter()
            .enumerate()
            .map(|(index, path)| {
                workspace::checkout_id_for_path(&format!("workspace:{index}"), path)
            })
            .collect::<Vec<_>>();
        runtime.snapshot.navigator.focused_workspace_id = Some("workspace:0".to_owned());
        runtime.snapshot.navigator.focused_checkout_id = Some(ids[0].clone());
        runtime.snapshot.ui_state.focused_checkout_id = Some(ids[0].clone());
        runtime.snapshot.navigator.root_path = Some(roots[0].to_string_lossy().into_owned());
        // The panel starts hidden on a section that is not the tree, which is
        // the state SC1 names: the reveal has to open it and switch it.
        runtime.snapshot.ui_state.right_panel_visible = false;
        runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Changes;
        let (first, second) = (roots[0].clone(), roots[1].clone());
        (runtime, first, ids[0].clone(), second, ids[1].clone())
    }

    fn reveal_event(
        workspace_id: &str,
        checkout_id: &str,
        path: &Path,
        is_directory: bool,
    ) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "reveal_path",
            "payload": {
                "path": path.to_string_lossy(),
                "workspace_id": workspace_id,
                "checkout_id": checkout_id,
                "is_directory": is_directory
            }
        }))
        .expect("reveal event")
    }

    /// AC5, R2, SC1. One click on a file printed by a pane in another checkout
    /// brings that checkout forward, opens the tree on it, expands every
    /// ancestor, selects the file and opens its editor tab - in one event,
    /// because the shell's dispatch is fire-and-forget and could not order
    /// four of them.
    #[test]
    fn revealing_a_file_switches_the_checkout_and_settles_the_whole_screen_at_once() {
        let (mut runtime, _first, first_id, second, second_id) = reveal_runtime();
        let target = second.join("deep/nested/leaf/target.txt");
        assert_eq!(
            runtime.snapshot().navigator.focused_checkout_id.as_deref(),
            Some(first_id.as_str())
        );

        assert!(runtime.dispatch_json(&reveal_event("workspace:1", &second_id, &target, false)));

        let snapshot = runtime.snapshot();
        assert_eq!(
            snapshot.navigator.focused_checkout_id.as_deref(),
            Some(second_id.as_str())
        );
        assert!(snapshot.ui_state.right_panel_visible);
        assert_eq!(
            snapshot.ui_state.right_panel_section,
            RightPanelSection::Explorer
        );
        assert_eq!(
            snapshot.ui_state.selected_path.as_deref(),
            Some(target.to_string_lossy().as_ref())
        );
        for ancestor in ["deep", "deep/nested", "deep/nested/leaf"] {
            let expected = second.join(ancestor).to_string_lossy().into_owned();
            assert!(
                snapshot.ui_state.expanded_paths.contains(&expected),
                "the tree does not open {expected}: {:?}",
                snapshot.ui_state.expanded_paths
            );
        }
        assert!(
            !snapshot
                .ui_state
                .expanded_paths
                .contains(&target.to_string_lossy().into_owned()),
            "a file is not a folder to expand"
        );
        let tab = snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.path == target.to_string_lossy())
            .expect("the revealed file takes an editor tab");
        assert_eq!(
            snapshot.editor.active_tab_id.as_deref(),
            Some(tab.id.as_str())
        );
        assert!(snapshot.status.last_error.is_none());

        // Rule 11: the same click again keeps the tab it already opened and
        // the selection it already made.
        assert!(runtime.dispatch_json(&reveal_event("workspace:1", &second_id, &target, false)));
        assert_eq!(runtime.snapshot().editor.tabs.len(), 1);
        assert_eq!(
            runtime.snapshot().ui_state.selected_path.as_deref(),
            Some(target.to_string_lossy().as_ref())
        );
    }

    /// AC5, R3, SC2. A folder opens the tree on itself and takes no editor
    /// AC5, R4. A reveal settles the whole screen at once or not at all. A
    /// file that cannot be read - deleted between being printed and being
    /// clicked, or unreadable - leaves the focused checkout, the panel, the
    /// expanded set and the selection exactly as they were, and says why.
    /// Reading after the screen has moved would leave a reveal half applied.
    #[test]
    fn revealing_a_file_that_cannot_be_read_moves_nothing_and_says_why() {
        let (mut runtime, _first, _first_id, second, second_id) = reveal_runtime();
        let missing = second.join("deep/nested/leaf/gone.txt");
        let before = runtime.snapshot().clone();

        assert!(runtime.dispatch_json(&reveal_event("workspace:1", &second_id, &missing, false)));

        let after = runtime.snapshot().clone();
        assert_eq!(
            after.navigator.focused_checkout_id, before.navigator.focused_checkout_id,
            "the checkout must not move for a file that cannot be read"
        );
        assert_eq!(
            after.ui_state.right_panel_visible,
            before.ui_state.right_panel_visible
        );
        assert_eq!(
            after.ui_state.right_panel_section,
            before.ui_state.right_panel_section
        );
        assert_eq!(
            after.ui_state.expanded_paths,
            before.ui_state.expanded_paths
        );
        assert_eq!(after.ui_state.selected_path, before.ui_state.selected_path);
        assert!(
            after.editor.tabs.is_empty(),
            "no tab for a file with no contents"
        );
        let error = after
            .status
            .last_error
            .as_ref()
            .expect("an unreadable file is reported");
        assert_eq!(error.kind, "file.open_failed");
    }

    /// tab. A4 records that the folder itself expands, not only its ancestors.
    #[test]
    fn revealing_a_folder_expands_it_and_opens_no_editor_tab() {
        let (mut runtime, _first, _first_id, second, second_id) = reveal_runtime();
        let target = second.join("deep/nested");

        assert!(runtime.dispatch_json(&reveal_event("workspace:1", &second_id, &target, true)));

        let snapshot = runtime.snapshot();
        assert!(snapshot.ui_state.right_panel_visible);
        assert_eq!(
            snapshot.ui_state.right_panel_section,
            RightPanelSection::Explorer
        );
        assert!(
            snapshot
                .ui_state
                .expanded_paths
                .contains(&target.to_string_lossy().into_owned()),
            "the clicked folder itself stays closed: {:?}",
            snapshot.ui_state.expanded_paths
        );
        assert!(
            snapshot
                .ui_state
                .expanded_paths
                .contains(&second.join("deep").to_string_lossy().into_owned())
        );
        assert_eq!(
            snapshot.ui_state.selected_path.as_deref(),
            Some(target.to_string_lossy().as_ref())
        );
        assert!(
            snapshot.editor.tabs.is_empty(),
            "a folder is not a document"
        );
    }

    /// AC5, R5. A reveal aimed at a checkout Hide does not have leaves the
    /// screen exactly as it was and says why.
    #[test]
    fn revealing_into_an_unregistered_checkout_changes_nothing_and_reports_it() {
        let (mut runtime, first, first_id, second, _second_id) = reveal_runtime();
        let before = runtime.snapshot().ui_state.clone();

        assert!(runtime.dispatch_json(&reveal_event(
            "workspace:9",
            "checkout:missing",
            &second.join("deep"),
            true
        )));

        let snapshot = runtime.snapshot();
        assert_eq!(
            snapshot
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.as_str()),
            Some("reveal.unknown_checkout")
        );
        assert_eq!(
            snapshot.navigator.focused_checkout_id.as_deref(),
            Some(first_id.as_str())
        );
        assert_eq!(
            snapshot.ui_state.right_panel_visible,
            before.right_panel_visible
        );
        assert_eq!(
            snapshot.ui_state.right_panel_section,
            before.right_panel_section
        );
        assert_eq!(snapshot.ui_state.expanded_paths, before.expanded_paths);
        assert_eq!(snapshot.ui_state.selected_path, before.selected_path);
        assert!(snapshot.editor.tabs.is_empty());
        let _ = first;
    }

    #[test]
    fn a_wheel_on_a_pane_with_no_reported_size_writes_nothing_and_says_so_once() {
        let mut runtime = runtime();
        let scroll = |lines: u16| {
            serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "kind": "terminal_scroll",
                "payload": {"pane_id": "w1:p1", "direction": "up", "lines": lines}
            }))
            .expect("scroll event")
        };
        assert!(runtime.dispatch_json(&scroll(3)));
        for _ in 0..20 {
            runtime.dispatch_json(&scroll(3));
        }

        let deferred = runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == "terminal.scroll_deferred")
            .count();
        assert_eq!(deferred, 1, "a wheel burst filled the diagnostics list");
        assert!(!runtime.terminal_sizes.contains_key("w1:p1"));

        // Once the view reports, the wait is over and a later wheel is
        // ordinary again.
        let resize = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "terminal_resize",
            "payload": {"pane_id": "w1:p1", "rows": 30, "cols": 100}
        }))
        .expect("resize event");
        runtime.dispatch_json(&resize);
        assert!(!runtime.panes_scrolled_before_size.contains("w1:p1"));
    }

    #[test]
    fn retiring_a_pane_clears_every_pane_keyed_terminal_state() {
        let mut runtime = runtime();
        let pane = "w-retired:p1";
        runtime.terminal_session_generations.insert(pane.into(), 7);
        runtime
            .terminal_session_lifecycles
            .insert(pane.into(), TerminalSessionLifecycle::default());
        runtime.terminal_recovery.insert(
            pane.into(),
            crate::terminal_recovery::Recovery::new(Instant::now(), "retry".into()),
        );
        runtime.terminal_sizes.insert(pane.into(), (80, 24));
        runtime.terminal_view_sizes.insert(pane.into(), (120, 40));
        runtime.terminal_frames_need_full.insert(pane.into());
        runtime.panes_awaiting_size.insert(pane.into());
        runtime.panes_scrolled_before_size.insert(pane.into());
        runtime.panes_closing.insert(pane.into());

        assert!(runtime.retain_terminal_pane_state(|known| known != pane));
        assert!(!runtime.terminal_session_generations.contains_key(pane));
        assert!(!runtime.terminal_session_lifecycles.contains_key(pane));
        assert!(!runtime.terminal_recovery.contains_key(pane));
        assert!(!runtime.terminal_sizes.contains_key(pane));
        assert!(!runtime.terminal_view_sizes.contains_key(pane));
        assert!(!runtime.terminal_frames_need_full.contains(pane));
        assert!(!runtime.panes_awaiting_size.contains(pane));
        assert!(!runtime.panes_scrolled_before_size.contains(pane));
        assert!(!runtime.panes_closing.contains(pane));
    }

    /// R6, R7. A pane keeps its session while its canvas is rebuilt - a zoom,
    /// a tab visit, a return to a checkout - and the view that comes back has
    /// an empty grid. Herdr sent the attach frame to the view that came
    /// before it and sends nothing more until the pane produces output, so
    /// the operator sees a blank pane that a single wheel notch repairs.
    /// The repaint has to reach Herdr even though the size did not change.
    #[test]
    fn a_rebuilt_view_for_an_attached_pane_is_given_a_frame_to_draw() {
        let checkout_path = "/private/tmp/hide-pane-repaint";
        let (mut runtime, _checkout_id) = live_tab_order_runtime(checkout_path);
        runtime.suppress_terminal_session_workers = true;
        let tabs = ["w-repaint:t1"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-repaint:t1",
        )));
        let pane_id = "w-repaint:t1:p";
        let resize = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "terminal_resize",
            "payload": {"pane_id": pane_id, "rows": 30, "cols": 100}
        }))
        .expect("resize event");
        // The first view reporting its size is what starts the attach.
        runtime.dispatch_json(&resize);
        assert!(runtime.terminal_sessions.contains_key(pane_id));
        let _attach_writes = runtime.terminal_sessions[pane_id].test_written_lines();
        runtime.terminal_frames_need_full.remove(pane_id);

        // The same size reported again is still swallowed: that is the report
        // every settled view makes, and answering it would double the frames.
        runtime.dispatch_json(&resize);
        assert!(
            runtime.terminal_sessions[pane_id]
                .test_written_lines()
                .is_empty(),
            "a size that did not change was forwarded to Herdr"
        );

        // A new view reports its first geometry, then its settled geometry.
        runtime.dispatch_json(
            &serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION, "kind": "terminal_viewport",
                "payload": {"pane_id": pane_id, "rows": 30, "cols": 100, "new_view": true}
            }))
            .unwrap(),
        );
        runtime.dispatch_json(&resize);
        let written = runtime.terminal_sessions[pane_id].test_written_lines();
        assert_eq!(
            written.len(),
            1,
            "the repaint did not reach Herdr exactly once"
        );
        let line: serde_json::Value =
            serde_json::from_str(written[0].trim_end()).expect("repaint line is JSON");
        assert_eq!(line["type"], "terminal.resize");
        assert_eq!(line["rows"], 30);
        assert_eq!(line["cols"], 100);
        assert!(runtime.snapshot().status.last_error.is_none());
    }

    /// AC8, R7, SC5. Attaching every tab the operator ever visited left a
    /// child process and a server-side render alive for each one. Only the tab
    /// on screen and the four before it keep their panes attached; the rest
    /// are released, say so, and attach again on the next visit.
    #[test]
    fn only_the_last_five_shown_tabs_keep_their_panes_attached() {
        let checkout_path = "/private/tmp/hide-attach-window";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        runtime.suppress_terminal_session_workers = true;
        let tabs = [
            "w-order:t1",
            "w-order:t2",
            "w-order:t3",
            "w-order:t4",
            "w-order:t5",
            "w-order:t6",
            "w-order:t7",
        ];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        // The first tab is already on screen; its view reports, which is what
        // starts an attach.
        let report_size = |runtime: &mut Runtime, tab_id: &str| {
            let resize = serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "kind": "terminal_resize",
                "payload": {"pane_id": format!("{tab_id}:p"), "rows": 30, "cols": 100}
            }))
            .expect("resize event");
            runtime.dispatch_json(&resize);
        };
        report_size(&mut runtime, "w-order:t1");
        assert!(runtime.terminal_sessions.contains_key("w-order:t1:p"));
        for tab_id in &tabs[1..] {
            assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, tab_id)));
            report_size(&mut runtime, tab_id);
        }

        let attached = runtime
            .terminal_sessions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            attached,
            [
                "w-order:t3:p",
                "w-order:t4:p",
                "w-order:t5:p",
                "w-order:t6:p",
                "w-order:t7:p"
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
            "the attach window is not the last five tabs shown"
        );

        // A released pane says what happened to it rather than looking broken.
        let released = runtime
            .snapshot()
            .terminal
            .panes
            .iter()
            .find(|pane| pane.pane_id == "w-order:t1:p")
            .expect("a released pane keeps its projection entry")
            .clone();
        assert_eq!(released.transport_state, "released");
        assert!(released.transport_exit_category.is_none());
        assert_eq!(
            runtime
                .snapshot()
                .status
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == "terminal.session_released"
                    && diagnostic.message.contains("w-order:t1:p"))
                .count(),
            1
        );

        // A tick that changes nothing must not attach any of them again.
        let before = runtime
            .terminal_sessions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t7",
        )));
        assert_eq!(
            runtime
                .terminal_sessions
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            before,
            "an idle tick re-attached a released pane"
        );

        // Going back attaches that tab's pane and nothing else.
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t1")));
        assert!(runtime.terminal_sessions.contains_key("w-order:t1:p"));
        assert_eq!(runtime.terminal_sessions.len(), ATTACHED_TAB_LIMIT);
    }

    /// AC11, R9, SC6. A checkout whose tabs come from two Herdr workspaces
    /// refused every drag, because ownership was decided for the checkout
    /// rather than for the drag. A drag that only steps over the other
    /// workspace's tabs changes nothing Herdr can see, so it lands locally.
    #[test]
    fn tab_strip_reorder_a_drag_past_another_workspaces_tabs_needs_no_herdr_move() {
        let (mut runtime, checkout_id, directory) = strip_checkout("split-local");
        let checkout_path = directory.to_string_lossy().into_owned();
        assert!(runtime.ingest_session(Ok(split_checkout_payload(
            &checkout_path,
            &[
                ("w-left", &["w-left:t1"][..], "w-left:t1"),
                ("w-right", &["w-right:t1", "w-right:t2"][..], "w-right:t1"),
            ],
            "w-left"
        ))));
        let before = strip_ids(&runtime, &checkout_id);
        assert_eq!(
            before.len(),
            3,
            "the split checkout holds both workspaces: {before:?}"
        );

        // There is no live connection here, so any request to Herdr would fail
        // loudly. Landing silently is the assertion.
        let moved = before[0].clone();
        assert!(reorder_tab(&mut runtime, &checkout_id, &moved, 1));

        assert_eq!(
            runtime
                .snapshot()
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.as_str()),
            None
        );
        let after = strip_ids(&runtime, &checkout_id);
        assert_eq!(after[1], moved, "the drop did not land: {after:?}");
        assert!(runtime.pending_tab_move.is_empty());

        std::fs::remove_dir_all(&directory).ok();
    }

    /// AC11, R9. A drag that does change the moved tab's own workspace order
    /// still asks Herdr, and asks with an index counted in that workspace, not
    /// in the mixed strip the operator sees.
    #[test]
    fn tab_strip_reorder_a_drag_within_one_workspace_uses_that_workspaces_index() {
        let (mut runtime, checkout_id, directory) = strip_checkout("split-herdr");
        let checkout_path = directory.to_string_lossy().into_owned();
        assert!(runtime.ingest_session(Ok(split_checkout_payload(
            &checkout_path,
            &[
                ("w-left", &["w-left:t1"][..], "w-left:t1"),
                ("w-right", &["w-right:t1", "w-right:t2"][..], "w-right:t1"),
            ],
            "w-left"
        ))));
        let before = strip_ids(&runtime, &checkout_id);

        let socket_root = PathBuf::from("/tmp").join(format!(
            "herdr-core-split-move-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&socket_root).expect("socket directory");
        let socket_path = socket_root.join("herdr.sock");
        let listener =
            std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let (mut stream, _) = listener.accept().expect("accept tab.move");
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut line)
                .expect("read request");
            let request: serde_json::Value =
                serde_json::from_str(&line).expect("tab.move request JSON");
            writeln!(
                stream,
                "{}",
                serde_json::json!({
                    "id": request["id"],
                    "result": {
                        "type": "tab_list",
                        "tabs": [
                            {"tab_id": "w-right:t2"},
                            {"tab_id": "w-right:t1"}
                        ]
                    }
                })
            )
            .expect("write tab_list response");
            request
        });
        runtime.live = Some(live::LiveContext {
            socket_path: socket_path.clone(),
            herdr_bin: None,
            runtime: std::sync::Weak::new(),
            notifier: crate::ffi::ChangeNotifier::noop(),
            api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
        });

        // Move the right workspace's first tab behind its second.
        let moved = before
            .iter()
            .find(|id| id.ends_with("w-right:t1"))
            .expect("the right workspace's first tab")
            .clone();
        let target = before
            .iter()
            .position(|id| id.ends_with("w-right:t2"))
            .expect("the right workspace's second tab");
        assert!(reorder_tab(&mut runtime, &checkout_id, &moved, target));

        let request = server.join().expect("fixture server joins");
        assert_eq!(request["method"], "tab.move");
        assert_eq!(
            request["params"],
            serde_json::json!({"tab_id": "w-right:t1", "insert_index": 2}),
            "the index was not counted in the moved tab's own workspace"
        );
        // Nothing lands on the operator's word alone.
        assert_eq!(strip_ids(&runtime, &checkout_id), before);

        // Herdr reports the new order for the workspace it moved a tab in.
        // The held arrangement has to land here: the checkout holds two
        // workspaces' tabs, so what Herdr reports for one of them is never the
        // whole checkout's list, and a hold that waited for the whole list
        // would never be released.
        assert!(runtime.ingest_session(Ok(split_checkout_payload(
            &checkout_path,
            &[
                ("w-left", &["w-left:t1"][..], "w-left:t1"),
                ("w-right", &["w-right:t2", "w-right:t1"][..], "w-right:t1"),
            ],
            "w-left"
        ))));
        let after = strip_ids(&runtime, &checkout_id);
        let right = after
            .iter()
            .filter(|id| id.contains("w-right:"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            right,
            vec!["herdr:w-right:t2".to_owned(), "herdr:w-right:t1".to_owned()],
            "the move Herdr granted did not land: {after:?}"
        );
        assert!(
            runtime.pending_tab_move.is_empty(),
            "a granted move stayed pending in a checkout two workspaces share"
        );

        std::fs::remove_dir_all(&socket_root).ok();
        std::fs::remove_dir_all(&directory).ok();
    }

    /// AC11, R9, SC6. A drag can both reorder the moved tab inside its own
    /// workspace and carry it past another workspace's tabs. The operator's
    /// arrangement is then an interleaving Herdr never reports, because Herdr
    /// only ever states one workspace's order. Holding the checkout's mixed
    /// order and waiting for it to come back left the drag pending forever and
    /// the strip snapped back to where the tab started.
    #[test]
    fn tab_strip_reorder_a_drag_that_interleaves_two_workspaces_still_lands() {
        let (mut runtime, checkout_id, directory) = strip_checkout("split-interleaved");
        let checkout_path = directory.to_string_lossy().into_owned();
        let workspaces: [(&str, &[&str], &str); 2] = [
            ("w-left", &["w-left:t1"][..], "w-left:t1"),
            ("w-right", &["w-right:t1", "w-right:t2"][..], "w-right:t1"),
        ];
        assert!(runtime.ingest_session(Ok(split_checkout_payload(
            &checkout_path,
            &workspaces,
            "w-left"
        ))));
        let before = strip_ids(&runtime, &checkout_id);
        assert_eq!(
            before,
            vec![
                "herdr:w-left:t1".to_owned(),
                "herdr:w-right:t1".to_owned(),
                "herdr:w-right:t2".to_owned()
            ],
            "the fixture strip is not the order this test reasons about: {before:?}"
        );

        let socket_root = PathBuf::from("/tmp").join(format!(
            "herdr-core-split-interleave-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&socket_root).expect("socket directory");
        let socket_path = socket_root.join("herdr.sock");
        let listener =
            std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let (mut stream, _) = listener.accept().expect("accept tab.move");
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut line)
                .expect("read request");
            let request: serde_json::Value =
                serde_json::from_str(&line).expect("tab.move request JSON");
            writeln!(
                stream,
                "{}",
                serde_json::json!({
                    "id": request["id"],
                    "result": {
                        "type": "tab_list",
                        "tabs": [
                            {"tab_id": "w-right:t2"},
                            {"tab_id": "w-right:t1"}
                        ]
                    }
                })
            )
            .expect("write tab_list response");
            request
        });
        runtime.live = Some(live::LiveContext {
            socket_path: socket_path.clone(),
            herdr_bin: None,
            runtime: std::sync::Weak::new(),
            notifier: crate::ffi::ChangeNotifier::noop(),
            api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
        });

        // The right workspace's second tab is dragged to the very front, over
        // the left workspace's tab as well as its own sibling.
        assert!(reorder_tab(
            &mut runtime,
            &checkout_id,
            "herdr:w-right:t2",
            0
        ));
        let request = server.join().expect("fixture server joins");
        assert_eq!(request["method"], "tab.move");
        assert_eq!(
            request["params"],
            serde_json::json!({"tab_id": "w-right:t2", "insert_index": 0}),
            "the index was not counted in the moved tab's own workspace"
        );

        // Herdr states the right workspace's new order. The checkout still
        // lists its tabs workspace by workspace, so what comes back is
        // `w-left:t1, w-right:t2, w-right:t1` - never the operator's
        // interleaving.
        let moved_workspaces: [(&str, &[&str], &str); 2] = [
            ("w-left", &["w-left:t1"][..], "w-left:t1"),
            ("w-right", &["w-right:t2", "w-right:t1"][..], "w-right:t1"),
        ];
        assert!(runtime.ingest_session(Ok(split_checkout_payload(
            &checkout_path,
            &moved_workspaces,
            "w-left"
        ))));

        assert!(
            runtime.pending_tab_move.is_empty(),
            "a granted move stayed pending because the operator interleaved two workspaces"
        );
        assert_eq!(
            strip_ids(&runtime, &checkout_id),
            vec![
                "herdr:w-right:t2".to_owned(),
                "herdr:w-left:t1".to_owned(),
                "herdr:w-right:t1".to_owned()
            ],
            "the drag did not stay where the operator dropped it"
        );

        std::fs::remove_dir_all(&socket_root).ok();
        std::fs::remove_dir_all(&directory).ok();
    }

    /// AC15, R11, SC7. Herdr closes the PTY before it reports the pane gone,
    /// so the attach child ends while the pane is still drawn. Reading that as
    /// a transport failure is what flashed "terminal attach ended" over a pane
    /// the operator had just closed.
    #[test]
    fn close_projection_a_close_hide_asked_for_is_not_a_transport_failure() {
        let checkout_path = "/private/tmp/hide-close-projection";
        let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
        runtime.suppress_terminal_session_workers = true;
        let tabs = ["w-order:t1"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        let pane_id = "w-order:t1:p";
        let resize = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "terminal_resize",
            "payload": {"pane_id": pane_id, "rows": 30, "cols": 100}
        }))
        .expect("resize event");
        runtime.dispatch_json(&resize);
        assert!(runtime.terminal_sessions.contains_key(pane_id));
        let generation = runtime.terminal_session_generations[pane_id];
        let chunks_before = runtime.snapshot().terminal.chunks.len();

        let close = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "close_pane",
            "payload": {"pane_id": pane_id, "confirmed": true}
        }))
        .expect("close pane event");
        runtime.dispatch_json(&close);
        assert!(runtime.ingest_terminal_session_closed(
            pane_id,
            generation,
            TerminalSessionMode::Control,
            None,
        ));

        let pane = runtime
            .snapshot()
            .terminal
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .expect("the pane is still drawn until Herdr removes it")
            .clone();
        assert_eq!(pane.transport_state, "closing");
        assert!(pane.transport_message.is_none());
        assert_eq!(
            runtime.snapshot().terminal.chunks.len(),
            chunks_before,
            "a notice was written over the pane's last frame"
        );
        let _ = checkout_id;
    }

    /// The other half of the same rule: a close that is not the pane going
    /// away still reports itself exactly as it did.
    #[test]
    fn close_projection_a_failure_on_a_living_pane_still_reports_ended() {
        let checkout_path = "/private/tmp/hide-close-failure";
        let (mut runtime, _checkout_id) = live_tab_order_runtime(checkout_path);
        runtime.suppress_terminal_session_workers = true;
        let tabs = ["w-order:t1"];
        runtime.ingest_session(Ok(tab_order_payload(
            checkout_path,
            &tabs,
            &tabs,
            "w-order:t1",
        )));
        let pane_id = "w-order:t1:p";
        let resize = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "terminal_resize",
            "payload": {"pane_id": pane_id, "rows": 30, "cols": 100}
        }))
        .expect("resize event");
        runtime.dispatch_json(&resize);
        let generation = runtime.terminal_session_generations[pane_id];

        assert!(runtime.ingest_terminal_session_closed(
            pane_id,
            generation,
            TerminalSessionMode::Control,
            Some("herdr terminal session control exited with status 1".to_owned()),
        ));

        let pane = runtime
            .snapshot()
            .terminal
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .expect("the pane is still projected")
            .clone();
        assert_eq!(pane.transport_state, "ended");
        assert!(pane.transport_message.is_some());
    }

    #[test]
    fn selecting_a_change_opens_one_diff_tab_and_keeps_its_reader_alive() {
        let (mut runtime, root, checkout_id, _second, _second_id) = reveal_runtime();
        let path = root.join("tracked.json");
        runtime.snapshot.changes.root_path = Some(root.to_string_lossy().into_owned());
        runtime.snapshot.changes.entries = vec![crate::model::ChangedFileSnapshot {
            path: path.to_string_lossy().into_owned(),
            relative_path: "tracked.json".to_owned(),
            status: crate::model::ChangedFileStatus::Modified,
            added_lines: Some(2),
            removed_lines: Some(1),
        }];
        let event = || {
            serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "kind": "changes_select",
                "payload": {"path": path, "committed": false}
            }))
            .expect("changes event")
        };

        assert!(runtime.dispatch_json(&event()));
        assert!(
            !runtime.dispatch_json(&event()),
            "opening the active diff publishes no duplicate state"
        );

        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.editor.tabs.len(), 1);
        let tab = &snapshot.editor.tabs[0];
        assert_eq!(tab.kind, EditorTabKind::Diff);
        assert_eq!(tab.checkout_id, checkout_id);
        assert_eq!(tab.diff_committed, Some(false));
        assert_eq!(
            snapshot.editor.active_tab_id.as_deref(),
            Some(tab.id.as_str())
        );
        assert!(snapshot.editor.document.is_none());
        assert!(!snapshot.ui_state.right_panel_visible);
        let request = runtime
            .changes_request()
            .expect("an active diff keeps its Changes reader alive");
        assert_eq!(
            request.selected_path.as_deref(),
            Some(path.to_string_lossy().as_ref())
        );

        std::fs::write(&path, "{}\n").expect("file fixture");
        runtime.open_file_tab("workspace:0", &checkout_id, path.to_string_lossy().as_ref());
        assert_eq!(runtime.snapshot().editor.tabs.len(), 2);
        assert_eq!(
            runtime
                .snapshot()
                .editor
                .tabs
                .iter()
                .map(|tab| tab.kind)
                .collect::<Vec<_>>(),
            vec![EditorTabKind::Diff, EditorTabKind::File],
            "opening the source does not reuse its diff tab"
        );
    }
    include!("runtime_lineage_tests.rs");
}
