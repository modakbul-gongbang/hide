use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

mod editor;
mod events;
mod projects;

use events::*;

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
    EditorTabKind, EditorTabSnapshot, ExplorerOperationSnapshot, LastErrorSnapshot,
    PANE_TEXT_SCALE_STEP, PaneFindSnapshot, PaneFocusRequestSnapshot, PaneForkSnapshot,
    PaneLayoutSnapshot, PaneSnapshot, PetBadgesSnapshot, PetOriginSnapshot, PetSnapshot,
    RemoteFileEntrySnapshot, RemoteFileListSnapshot, RemoteSessionSnapshot, RightPanelSection,
    SCHEMA_VERSION, Snapshot, StripTabKind, StripTabSnapshot, Surface, TabSnapshot, TerminalChunk,
    TerminalPaneSnapshot, UiStateSnapshot, WorkspaceSnapshot, clamp_pane_text_scale,
};
use crate::recent_closed::{ClosedAgent, ClosedContext, ClosedItem, ClosedPane, push_bounded};
use crate::remote::RusshSftpTransport;
use crate::remote_files::{FileEntry, FileKind, FileService, RemoteFileService};
use crate::sidebar::{ReadRecordScope, SessionSnapshotPayload, project_agents};
use crate::{chromux, environment, files, live, persistence, pet, session_sync, workspace};

fn conversation_agent_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "claude" | "claude-code" | "claude_code" | "codex"
    )
}

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
/// `path` with the `source` prefix replaced by `destination`, or `None`
/// when `path` is neither `source` nor inside it. Component-wise, so
/// `/repo/src2` is not inside `/repo/src`.
fn retarget_path(path: &str, source: &str, destination: &str) -> Option<String> {
    if path == source {
        return Some(destination.to_owned());
    }
    let rest = path.strip_prefix(source)?.strip_prefix('/')?;
    Some(format!("{destination}/{rest}"))
}

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
    /// Present only for an explicit relationship Open/Return request. The
    /// core owns the outcome; the shell supplies this opaque correlation id
    /// only so it can consume the answer to the action it initiated.
    request_id: Option<String>,
    requested_at_unix_ms: u64,
}

impl PendingViewFocus {
    fn new(scope_id: impl Into<String>, target_id: impl Into<String>) -> Self {
        Self {
            scope_id: scope_id.into(),
            target_id: target_id.into(),
            request_id: None,
            requested_at_unix_ms: unix_milliseconds(),
        }
    }

    fn pane_request(target_id: impl Into<String>, request_id: String) -> Self {
        Self {
            scope_id: String::new(),
            target_id: target_id.into(),
            request_id: Some(request_id),
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

/// When a waiting descendant starts showing on its lineage root, and when it
/// becomes the operator's problem.
///
/// Both are fixed. Which numbers are right is not knowable before the feature
/// has been operated, so there is no setting to get wrong in the meantime
/// (PRD D-41).
const STALL_SOFT_MS: u64 = 5 * 60_000;
const STALL_HARD_MS: u64 = 15 * 60_000;

/// Hide's own record of when it first saw a pane in the state it is in.
///
/// Herdr sends no timestamp with `state_change_seq`, so the clock has to be
/// Hide's. It lives only in memory: a restart starts every clock again, which
/// is what stops a morning launch from raising a screenful of escalations for
/// work that was never stuck (PRD B20, D-54).
#[derive(Clone, Debug, Eq, PartialEq)]
struct StallClock {
    /// What the pane looked like when this clock started. Herdr's sequence
    /// alone is not enough: it does not always rise when only plugin tokens
    /// change, which is the same gap the read record covers.
    fingerprint: (Option<u64>, String, String),
    /// Time already counted, excluding any stretch the server was away for.
    stalled_ms: u64,
    last_sample_unix_ms: u64,
}

impl StallClock {
    /// How long this pane has been waiting, as of `now`.
    fn elapsed(&self, now: u64) -> u64 {
        self.stalled_ms
            .saturating_add(now.saturating_sub(self.last_sample_unix_ms))
    }
}

/// How long a relocation waits before Hide asks again.
///
/// A failed move is retried on the next ingest that finds the child still
/// split, and a delegated child changes state often enough that one arrives.
/// The window keeps a persistently refusing Herdr from being asked once per
/// event without adding a timer of its own (PRD B36).
const RELOCATION_RETRY_INTERVAL_MS: u64 = 5_000;

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
    /// keyboard is in does not decide it. Older snapshots omit the active
    /// tab for a workspace that has only one tab; that shape is unambiguous.
    /// A multi-tab workspace without an active-tab verdict stays unknown so
    /// an old focused layout cannot acknowledge a new pane-focus request.
    fn is_active_in_its_workspace(&self, tab_id: &str) -> bool {
        let Some(workspace_id) = self.workspace_by_tab.get(tab_id) else {
            return false;
        };
        match self.active_tab_by_workspace.get(workspace_id) {
            Some(active_tab_id) => active_tab_id == tab_id,
            None => {
                self.workspace_by_tab
                    .values()
                    .filter(|candidate| *candidate == workspace_id)
                    .count()
                    == 1
            }
        }
    }

    fn active_tab_ids(&self) -> BTreeSet<String> {
        self.workspace_by_tab
            .keys()
            .filter(|tab_id| self.is_active_in_its_workspace(tab_id))
            .cloned()
            .collect()
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

/// A file tab that has been read but not yet put on screen.

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
    /// The foreign grid a pane's held frames are arriving at, so a burst is
    /// diagnosed once per grid rather than once per frame: one contested pane
    /// wrote 8,500 mismatch lines in twelve minutes and rotated the log.
    terminal_foreign_frame_sizes: HashMap<String, (u16, u16)>,
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
    /// How long each delegated child has been in the state it is in. Keyed by
    /// pane id, in memory only.
    stall_clocks: BTreeMap<String, StallClock>,
    /// Delegated child panes Hide has asked Herdr to move out of their
    /// parent's tab, and when it asked. A move that fails is retried quietly
    /// on a later ingest; it never reaches the pane header, because the
    /// operator did not ask for the move and cannot act on its failure
    /// (PRD B2, D-45).
    pane_relocations_in_flight: BTreeMap<String, u64>,
    /// What Hide's hook last reported for each pane, read out of the pane
    /// tokens the session snapshot already carries. Kept between ingests so
    /// the projection does not have to hold the whole payload alive.
    pane_hook_tokens: BTreeMap<String, crate::agent_hooks::PaneHookTokens>,
    /// The hook-install state of each runtime, read off the coordinator
    /// thread. `None` until that first read lands, which reads as "not known
    /// yet" rather than as "not installed".
    hook_diagnosis: Option<hide_agent_hooks::Diagnosis>,
    /// Runtimes the operator has approved an install for, waiting for the
    /// coordinator to do the file write off the mutex.
    pending_hook_installs: BTreeSet<hide_agent_hooks::AgentRuntime>,
    /// The operator's background AI choice as the core holds it. `None` until
    /// the coordinator's first read lands, which reads as "not known yet"
    /// rather than as "the defaults".
    ai_settings: Option<hide_ai::AiSettings>,
    /// A choice waiting for the coordinator to write it to the settings file
    /// off the mutex, the same way an approved hook install waits.
    pending_ai_settings_save: Option<hide_ai::AiSettings>,
    /// True while the Background AI group is on screen, which is the only
    /// time the provider probe runs.
    ai_observing: bool,
    /// What the providers last answered. Held beside the snapshot so a
    /// changed choice can restamp the rows without asking again.
    background_ai_providers: Vec<crate::model::BackgroundAiProviderSnapshot>,
    usage_window_visible: bool,
    usage_popover_open: bool,
    usage_popover_open_generation: u64,
    recent_visible_tabs: Vec<String>,
    /// Panes Hide has asked Herdr to close. Herdr closes the PTY first, so the
    /// attach child ends before the `pane_closed` event arrives and the pane
    /// is still on screen when its transport reports the close. Projecting
    /// that as a failure is what put "terminal attach ended" on screen for one
    /// frame every time the operator closed a pane.
    panes_closing: HashSet<String>,
    /// User-initiated local closes, newest last. Memory only by contract.
    recent_closed: VecDeque<ClosedItem>,
    close_captures_in_flight: HashSet<String>,
    close_capture_order: VecDeque<String>,
    close_capture_results: HashMap<
        String,
        (
            live::CloseCaptureRequest,
            Result<live::CloseCaptureOutcome, String>,
        ),
    >,
    recent_closed_sequence: u64,
    reopen_in_flight: Option<String>,
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
    /// The tabs Herdr most recently reported active in their own workspaces.
    /// A pane layout remembers a focused pane even while its tab is hidden,
    /// so the layout alone cannot confirm a pane-focus request.
    herdr_active_tab_ids: BTreeSet<String>,
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
    next_explorer_operation_id: u64,
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
            terminal_foreign_frame_sizes: HashMap::new(),
            terminal_sizes: pane_terminal_sizes.into_iter().collect(),
            panes_awaiting_size: HashSet::new(),
            panes_scrolled_before_size: HashSet::new(),
            stall_clocks: BTreeMap::new(),
            pane_relocations_in_flight: BTreeMap::new(),
            pane_hook_tokens: BTreeMap::new(),
            hook_diagnosis: None,
            pending_hook_installs: BTreeSet::new(),
            ai_settings: None,
            pending_ai_settings_save: None,
            ai_observing: false,
            background_ai_providers: Vec::new(),
            usage_window_visible: false,
            usage_popover_open: false,
            usage_popover_open_generation: 0,
            recent_visible_tabs: Vec::new(),
            panes_closing: HashSet::new(),
            recent_closed: VecDeque::new(),
            close_captures_in_flight: HashSet::new(),
            close_capture_order: VecDeque::new(),
            close_capture_results: HashMap::new(),
            recent_closed_sequence: 0,
            reopen_in_flight: None,
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
            herdr_active_tab_ids: BTreeSet::new(),
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
            next_explorer_operation_id: 0,
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
        let pane_focus_target = match (&payload.request, payload.report_pane_focus_outcome) {
            (RemoteControlRequest::FocusPane { pane_id }, true) => Some(pane_id.clone()),
            _ => None,
        };
        macro_rules! fail_request {
            ($kind:expr, $message:expr, $retryable:expr $(,)?) => {{
                let message = $message;
                if pane_focus_target.is_some() {
                    self.finish_pane_focus_request_by_id(
                        &request_id,
                        "failed",
                        Some(message.clone()),
                        $retryable,
                    );
                }
                self.set_error($kind, message, $retryable);
                return true;
            }};
        }
        if target_id.trim().is_empty() || request_id.trim().is_empty() {
            fail_request!(
                "remote.control.invalid_request",
                "Remote control requires non-empty target_id and request_id".to_owned(),
                false,
            );
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
        if let Some(pane_id) = pane_focus_target.as_deref() {
            self.snapshot.status.pane_focus_request = Some(PaneFocusRequestSnapshot {
                request_id: request_id.clone(),
                target_pane_id: pane_id.to_owned(),
                phase: "pending".to_owned(),
                message: None,
                retryable: false,
            });
        }
        let Some(context) = self.remote_controls.get(&target_id).cloned() else {
            fail_request!(
                "remote.control.unavailable",
                format!("Remote control is unavailable for target {target_id}"),
                true,
            );
        };
        let Some(remote) = self
            .snapshot
            .status
            .remote
            .iter()
            .find(|remote| remote.target_id == target_id)
        else {
            fail_request!(
                "remote.control.unknown_target",
                format!("Remote target {target_id} is not configured"),
                false,
            );
        };
        if remote.state != "connected" {
            fail_request!(
                "remote.control.not_connected",
                format!(
                    "Remote target {target_id} is {}; no command was sent",
                    remote.state
                ),
                true,
            );
        }
        let Some(session) = remote.session.clone() else {
            fail_request!(
                "remote.control.session_missing",
                format!("Remote target {target_id} has no authoritative session projection"),
                true,
            );
        };
        let mut source_pane_id = None;
        if let Some(pane_id) = payload.request.pane_id().map(str::to_owned) {
            if pane_id.trim().is_empty() {
                fail_request!(
                    "remote.control.invalid_pane",
                    "Remote pane control requires a non-empty pane_id".to_owned(),
                    false,
                );
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
                fail_request!(
                    "remote.control.pane_not_found",
                    format!("Pane {pane_id} does not belong to remote target {target_id}"),
                    false,
                );
            }
            source_pane_id = remote_pane_source_id(&target_id, &pane_id).map(str::to_owned);
            if source_pane_id.is_none() {
                fail_request!(
                    "remote.control.invalid_pane_scope",
                    format!("Pane {pane_id} is not scoped to remote target {target_id}"),
                    false,
                );
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
        let dispatched_request_id = request_id.clone();
        if let Err(message) = live::spawn_remote_control(context, request_id, action) {
            if let Some(key) = creation_key {
                self.remote_tab_creations_in_flight.remove(&key);
            }
            if pane_focus_target.is_some() {
                self.finish_pane_focus_request_by_id(
                    &dispatched_request_id,
                    "failed",
                    Some(message.clone()),
                    true,
                );
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
                delegated: false,
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
                expired.push((slot, pending.clone()));
                *self.pending_view_focus_mut(slot) = None;
            }
        }
        let changed = !expired.is_empty();
        for (slot, pending) in expired {
            let what = slot.what();
            let target_id = pending.target_id;
            if slot == ViewFocusSlot::Pane {
                self.finish_pane_focus_request(
                    pending.request_id.as_deref(),
                    &target_id,
                    "failed",
                    Some(format!(
                        "Herdr did not confirm pane focus within {VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS} ms."
                    )),
                    true,
                );
            }
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
                delegated: false,
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
                delegated: false,
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
                delegated: false,
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
        self.terminal_foreign_frame_sizes
            .retain(|pane_id, _| keep(pane_id));
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
            + self.terminal_foreign_frame_sizes.len()
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
        let session_confirms_pending_pane = fetched.as_ref().ok().is_some_and(|payload| {
            let Some(pending) = self.pending_pane_focus.as_ref() else {
                return false;
            };
            let herdr_tabs = HerdrTabView::from_payload(payload);
            payload.layouts.iter().any(|layout| {
                layout.focused_pane_id == pending.target_id
                    && layout
                        .panes
                        .iter()
                        .any(|pane| pane.pane_id == pending.target_id)
                    && herdr_tabs.is_active_in_its_workspace(&layout.tab_id)
            })
        });
        if let Ok(payload) = &fetched {
            self.herdr_active_tab_ids = HerdrTabView::from_payload(payload).active_tab_ids();
        }
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
        let protocol_details = fetched.as_ref().err().and_then(|error| {
            error
                .protocol_details()
                .map(|(expected, received, version)| {
                    (expected, received, version.map(str::to_owned))
                })
        });
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
                    self.fail_pending_pane_focus_for_target(
                        retired_pane_id,
                        format!("Pane {retired_pane_id} retired before Herdr confirmed focus."),
                    );
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
                    self.fail_pending_pane_focus_for_target(
                        pane_id,
                        format!("Pane {pane_id} is no longer available in the selected checkout."),
                    );
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
                self.pane_hook_tokens = payload
                    .panes
                    .iter()
                    .map(|pane| {
                        (
                            pane.pane_id.clone(),
                            crate::agent_hooks::PaneHookTokens::read(&pane.tokens),
                        )
                    })
                    .collect();
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
        let (expected_protocol, received_protocol, received_version) = protocol_details
            .map(|(expected, received, version)| (Some(expected), Some(received), version))
            .unwrap_or((None, None, None));
        if self.snapshot.status.herdr.state != state
            || self.snapshot.status.herdr.message.as_deref() != message.as_deref()
            || self.snapshot.status.herdr.expected_protocol != expected_protocol
            || self.snapshot.status.herdr.received_protocol != received_protocol
            || self.snapshot.status.herdr.received_version != received_version
        {
            self.snapshot.status.herdr.state = state.to_owned();
            self.snapshot.status.herdr.message = message;
            self.snapshot.status.herdr.expected_protocol = expected_protocol;
            self.snapshot.status.herdr.received_protocol = received_protocol;
            self.snapshot.status.herdr.received_version = received_version;
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
            self.apply_stall_escalation(&mut agents, unix_milliseconds());
            changed |= self.sync_conversation_modes(&agents);
            if self.snapshot.navigator.agents != agents {
                self.snapshot.navigator.agents = agents;
                changed = true;
            }
            changed |= self.sync_pane_lineage();
            changed |= self.relocate_delegated_child_panes();
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
            changed |= self.apply_pane_layout(layout, session_confirms_pending_pane);
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

    /// Which repositories to list worktrees for, and what base branch each
    /// branch is measured against.
    ///
    /// The bases come from the pull-request answer, so the first worktree read
    /// compares against the repository default and a later one compares
    /// against each pull request's own base. That is a changed request, which
    /// is always due, so the correction arrives without a second trigger.

    /// Combines subprocess answers with live pane and agent state, then sends
    /// that one model to the sidebar, Git section, and summary card.
    /// No filesystem or socket work occurs here.

    /// The worktree catalog the coordinator should build the next projection
    /// from. Held by the runtime so a rebuild triggered from anywhere uses the
    /// same worktrees the last read produced.

    /// Which repositories to look pull requests up for. Remote projects are
    /// out of scope, and a plain folder has no repository to ask about.

    /// Records that one project's pull requests must be read again.
    ///
    /// Repeated calls before the read happens are one refresh, not several:
    /// the counter is the request, and an unchanged request is not re-read.

    /// Worktrees whose size should be measured. An empty request while Git is
    /// hidden is intentional: idle sidebar projection must never launch du.

    /// The selected checkout, when it is one this machine owns. A remote
    /// checkout has no card: remote worktree management is out of scope, so
    /// nothing here describes one.

    /// Puts each branch's pull request on the checkout row that shows it.
    ///
    /// The row badge and the card read the same value from the same place, so
    /// the two cannot disagree about what a branch's pull request is.

    /// Rebuilds the summary card from what the readers have answered.
    ///
    /// Everything per-checkout already lives on the checkout row; what is
    /// assembled here is the repository's `gh` health, the one disk
    /// measurement, and whether this worktree may be removed.

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

    pub(crate) fn usage_activity(&self) -> crate::usage::UsageActivity {
        crate::usage::UsageActivity {
            window_visible: self.usage_window_visible,
            popover_open_generation: self.usage_popover_open_generation,
        }
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

    /// Recomputes the two inactive folds from current core facts. No row is
    /// moved or copied: the full collections stay authoritative for search,
    /// focus, and non-sidebar consumers.
    fn refresh_inactive_groups(&mut self) -> bool {
        crate::project_context::refresh_inactive_groups(
            &mut self.snapshot.navigator,
            &self.snapshot.ui_state,
            unix_milliseconds(),
        )
    }

    fn sync_conversation_modes(&mut self, agents: &[SidebarAgentSnapshot]) -> bool {
        let live: BTreeSet<String> = agents
            .iter()
            .filter(|agent| conversation_agent_kind(&agent.agent_kind))
            .map(|agent| agent.pane_id.clone())
            .collect();
        let ui_state = &mut self.snapshot.ui_state;
        let before_conversation = ui_state.conversation_pane_ids.clone();
        let before_terminal = ui_state.terminal_pane_ids.clone();

        ui_state
            .terminal_pane_ids
            .retain(|pane_id| live.contains(pane_id));
        let terminal = ui_state.terminal_pane_ids.clone();
        ui_state
            .conversation_pane_ids
            .retain(|pane_id| live.contains(pane_id) && !terminal.contains(pane_id));
        for pane_id in live {
            if !ui_state.terminal_pane_ids.contains(&pane_id) {
                ui_state.conversation_pane_ids.insert(pane_id);
            }
        }

        before_conversation != ui_state.conversation_pane_ids
            || before_terminal != ui_state.terminal_pane_ids
    }

    /// Refills every pane's child summary and breadcrumb from the final agent
    /// list.
    ///
    /// It runs after the read axis and the lineage, not while the panes are
    /// built: a chip's mark and emphasis come from the group, the group comes
    /// from the read axis, and the children come from the lineage, so a pass
    /// that ran earlier would publish a chip row describing a state the
    /// sidebar had already moved past.
    fn sync_pane_lineage(&mut self) -> bool {
        let agents = std::mem::take(&mut self.snapshot.navigator.agents);
        let diagnosis = self.hook_diagnosis.clone();
        let status_of = |runtime: hide_agent_hooks::AgentRuntime| {
            diagnosis
                .as_ref()
                .and_then(|diagnosis| diagnosis.status_of(runtime))
                .cloned()
        };
        let mut changed = false;
        let mut delegated_tabs_changed = false;
        // Collected on the same walk as the pane children, so the Settings
        // diagnosis and the pane's own mark can never disagree about which
        // sessions predate the install (PRD B27, D-61).
        let mut predating: Vec<crate::model::AgentHookPaneSnapshot> = Vec::new();
        // A tab is the operator's whenever it holds an agent they own. One
        // holding only delegated children is the pile this change exists to
        // take off the strip (PRD B1).
        for tab in self
            .snapshot
            .navigator
            .workspaces
            .iter_mut()
            .flat_map(|workspace| workspace.checkouts.iter_mut())
            .flat_map(|checkout| checkout.tabs.iter_mut())
        {
            let mut holds_an_agent = false;
            let mut all_delegated = true;
            for pane in &tab.panes {
                let Some(agent) = agents.iter().find(|agent| agent.pane_id == pane.id) else {
                    continue;
                };
                holds_an_agent = true;
                all_delegated &= agent.delegated;
            }
            let delegated = holds_an_agent && all_delegated;
            if tab.delegated != delegated {
                tab.delegated = delegated;
                delegated_tabs_changed = true;
            }
        }
        for pane in self
            .snapshot
            .navigator
            .workspaces
            .iter_mut()
            .flat_map(|workspace| workspace.checkouts.iter_mut())
            .flat_map(|checkout| checkout.tabs.iter_mut())
            .flat_map(|tab| tab.panes.iter_mut())
            .chain(
                self.snapshot
                    .navigator
                    .scratch
                    .tabs
                    .iter_mut()
                    .flat_map(|tab| tab.panes.iter_mut()),
            )
        {
            // A remote pane's answer is fixed and was decided where it was
            // projected; the local hook state says nothing about it.
            if crate::agent_hooks::is_remote_pane(&pane.id) {
                continue;
            }
            let tokens = self
                .pane_hook_tokens
                .get(&pane.id)
                .copied()
                .unwrap_or_default();
            let children =
                crate::sidebar::project_pane_children(&agents, &pane.id, tokens, &status_of);
            if let Some(children) = children.as_ref()
                && children.uninstrumented_code.as_deref()
                    == Some(
                        hide_agent_hooks::diagnosis::UninstrumentedReason::SessionPredatesInstall
                            .code(),
                    )
            {
                predating.push(crate::model::AgentHookPaneSnapshot {
                    pane_id: pane.id.clone(),
                    label: agents
                        .iter()
                        .find(|agent| agent.pane_id == pane.id)
                        .map(|agent| agent.chat_title.clone().unwrap_or_else(|| agent.id.clone()))
                        .unwrap_or_else(|| pane.id.clone()),
                    message: children.uninstrumented_reason.clone().unwrap_or_default(),
                });
            }
            let lineage_path = crate::sidebar::project_lineage_path(&agents, &pane.id);
            if pane.children != children {
                pane.children = children;
                changed = true;
            }
            if pane.lineage_path != lineage_path {
                pane.lineage_path = lineage_path;
                changed = true;
            }
        }
        self.snapshot.navigator.agents = agents;
        let hooks = crate::model::AgentHooksSnapshot {
            runtimes: self
                .hook_diagnosis
                .iter()
                .flat_map(|diagnosis| diagnosis.runtimes.iter())
                .map(|row| crate::model::AgentHookRuntimeSnapshot {
                    id: row.runtime.id().to_owned(),
                    label: row.label.clone(),
                    path: row.path.clone(),
                    headline: row.headline(),
                    installed: matches!(row.status, hide_agent_hooks::HookStatus::Installed { .. }),
                    offers_install: row.offers_install(),
                })
                .collect(),
            sessions_predating_install: predating,
        };
        if self.snapshot.status.agent_hooks != hooks {
            self.snapshot.status.agent_hooks = hooks;
            changed = true;
        }
        if delegated_tabs_changed {
            self.rebuild_tab_strips();
        }
        changed | delegated_tabs_changed | self.refresh_inactive_groups()
    }

    /// Whether a delegated child's clock should be running at all.
    ///
    /// A finished child is not stuck, a released pane has no session to be
    /// stuck in, an unknown activity gives nothing to measure, and a remote
    /// pane is uninstrumented by decision (PRD B19, D-51).
    fn stall_eligible(&self, agent: &SidebarAgentSnapshot) -> bool {
        if !agent.delegated || crate::agent_hooks::is_remote_pane(&agent.pane_id) {
            return false;
        }
        if agent.activity == "unknown" {
            return false;
        }
        if self
            .terminal_session_lifecycles
            .get(&agent.pane_id)
            .is_some_and(|lifecycle| lifecycle.state == "released")
        {
            return false;
        }
        // Waiting on the operator, or running with nothing to show for it.
        // A stopped child with no demand has finished, which is not waiting.
        agent.demand != "none" || agent.blocked || agent.activity == "working"
    }

    /// Advances every eligible child's clock and drops the rest.
    ///
    /// While the server is away the clocks hold their reading rather than
    /// counting: a disconnection is Hide's blindness, not the agent being
    /// stuck (PRD B20, D-54).
    fn advance_stall_clocks(&mut self, agents: &[SidebarAgentSnapshot], now: u64) {
        let connected = self.snapshot.status.herdr.state == "connected";
        let mut live = BTreeSet::new();
        for agent in agents {
            if !self.stall_eligible(agent) {
                continue;
            }
            live.insert(agent.pane_id.clone());
            let fingerprint = (
                agent.state_change_seq,
                agent.demand.clone(),
                agent.activity.clone(),
            );
            match self.stall_clocks.get_mut(&agent.pane_id) {
                Some(clock) if clock.fingerprint == fingerprint => {
                    if connected {
                        clock.stalled_ms = clock.elapsed(now);
                    }
                    clock.last_sample_unix_ms = now;
                }
                _ => {
                    self.stall_clocks.insert(
                        agent.pane_id.clone(),
                        StallClock {
                            fingerprint,
                            stalled_ms: 0,
                            last_sample_unix_ms: now,
                        },
                    );
                }
            }
        }
        self.stall_clocks
            .retain(|pane_id, _| live.contains(pane_id));
    }

    /// What each lineage root should be told about its descendants, as of
    /// `now`. A pure read, so the coordinator can ask whether a threshold is
    /// about to be crossed without changing anything.
    ///
    /// The notice lands on the root rather than climbing one level at a time:
    /// at depth three, one level per threshold would keep the operator
    /// waiting forty-five minutes for news of something stuck for fifteen
    /// (PRD B18, D-62).
    fn stall_escalations(
        &self,
        agents: &[SidebarAgentSnapshot],
        now: u64,
    ) -> BTreeMap<String, (&'static str, String, String)> {
        let mut worst: BTreeMap<String, (u64, u8, &'static str, String, String)> = BTreeMap::new();
        for agent in agents {
            let Some(clock) = self.stall_clocks.get(&agent.pane_id) else {
                continue;
            };
            if !self.stall_eligible(agent) {
                continue;
            }
            let elapsed = clock.elapsed(now);
            let level = if elapsed >= STALL_HARD_MS {
                "hard"
            } else if elapsed >= STALL_SOFT_MS {
                "soft"
            } else {
                continue;
            };
            let root = agent
                .lineage_path_pane_ids
                .first()
                .cloned()
                .unwrap_or_else(|| agent.pane_id.clone());
            let name = agent.chat_title.clone().unwrap_or_else(|| agent.id.clone());
            let notice = format!(
                "{name} has been waiting {} minutes on {}",
                elapsed / 60_000,
                waiting_on(agent)
            );
            let priority = stall_priority(agent);
            let candidate = (elapsed, priority, level, notice, agent.pane_id.clone());
            // Longest wait first, and on a tie the one asking for the most.
            // Without the second key the notice names whichever sibling the
            // row order happened to reach first, which is not an answer.
            match worst.get(&root) {
                Some(best) if best.0 > elapsed => {}
                Some(best) if best.0 == elapsed && best.1 <= priority => {}
                _ => {
                    worst.insert(root, candidate);
                }
            }
        }
        worst
            .into_iter()
            .map(|(root, (_, _, level, notice, pane_id))| (root, (level, notice, pane_id)))
            .collect()
    }

    /// Runs the clocks and writes what they say onto the rows.
    fn apply_stall_escalation(&mut self, agents: &mut [SidebarAgentSnapshot], now: u64) {
        self.advance_stall_clocks(agents, now);
        let escalations = self.stall_escalations(agents, now);
        let hard_children = escalations
            .values()
            .filter(|(level, _, _)| *level == "hard")
            .map(|(_, _, pane_id)| pane_id.clone())
            .collect::<BTreeSet<_>>();
        for agent in agents.iter_mut() {
            let (level, notice) = match escalations.get(&agent.pane_id) {
                Some((level, notice, _)) => ((*level).to_owned(), Some(notice.clone())),
                None => (String::new(), None),
            };
            agent.stall_level = level;
            agent.stall_notice = notice;
            // The child that ran out of time stops being drawn as somebody
            // else's work, because from here on it is the operator's.
            if hard_children.contains(&agent.pane_id) {
                agent.delegated = false;
            }
        }
        crate::sidebar::rederive_ownership(agents);
    }

    /// Whether a stall threshold has been crossed since the last publish.
    ///
    /// The coordinator asks this on the agent tick it already runs, so a
    /// stalled session - which by definition reports nothing new - still
    /// reaches the operator without a timer of Hide's own (PRD B36).
    pub fn stall_publish_due(&self) -> bool {
        self.stall_publish_due_at(unix_milliseconds())
    }

    fn stall_publish_due_at(&self, now: u64) -> bool {
        if self.snapshot.status.herdr.state != "connected" {
            return false;
        }
        let agents = &self.snapshot.navigator.agents;
        let escalations = self.stall_escalations(agents, now);
        agents.iter().any(|agent| {
            let level = escalations
                .get(&agent.pane_id)
                .map(|(level, _, _)| *level)
                .unwrap_or("");
            agent.stall_level != level
        })
    }

    /// Marks the tabs that exist only to hold delegated children, and asks
    /// Herdr to move any child still sharing its parent's tab into one.
    ///
    /// Detection is the same on every pass, so a child that arrives while
    /// Hide is running and a child already split when Hide started are the
    /// same case and take the same path (PRD B1, B3, D-44). Herdr keeps
    /// owning split geometry and the PTY size, so the pane is really moved
    /// rather than merely left undrawn (PRD D-15).
    fn relocate_delegated_child_panes(&mut self) -> bool {
        // Where Herdr currently holds each pane. The layout is the only place
        // that carries Herdr's own workspace and tab ids for a pane.
        let placement = self
            .snapshot
            .pane_layouts
            .iter()
            .flat_map(|layout| {
                layout
                    .pane_ids()
                    .into_iter()
                    .map(|pane_id| {
                        (
                            pane_id.to_owned(),
                            (layout.workspace_id.clone(), layout.tab_id.clone()),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<BTreeMap<_, _>>();
        let now = unix_milliseconds();
        let mut requests = Vec::new();
        for agent in &self.snapshot.navigator.agents {
            if !agent.delegated || crate::agent_hooks::is_remote_pane(&agent.pane_id) {
                continue;
            }
            let Some(parent_pane_id) = agent.lineage_parent_pane_id.as_deref() else {
                continue;
            };
            let (Some((workspace_id, tab_id)), Some((_, parent_tab_id))) =
                (placement.get(&agent.pane_id), placement.get(parent_pane_id))
            else {
                continue;
            };
            if tab_id != parent_tab_id {
                continue;
            }
            if self
                .pane_relocations_in_flight
                .get(&agent.pane_id)
                .is_some_and(|asked| now.saturating_sub(*asked) < RELOCATION_RETRY_INTERVAL_MS)
            {
                continue;
            }
            requests.push((
                agent.pane_id.clone(),
                workspace_id.clone(),
                agent.chat_title.clone().unwrap_or_else(|| agent.id.clone()),
            ));
        }
        // A pane Herdr no longer reports can never answer, so its record is
        // dropped rather than held forever.
        self.pane_relocations_in_flight
            .retain(|pane_id, _| placement.contains_key(pane_id));
        if requests.is_empty() {
            return false;
        }
        let Some(context) = self.live.as_ref().cloned() else {
            return false;
        };
        for (pane_id, workspace_id, label) in requests {
            self.pane_relocations_in_flight.insert(pane_id.clone(), now);
            if let Err(message) = live::spawn_pane_control(
                context.clone(),
                PaneControlAction::MoveToNewTab {
                    pane_id: pane_id.clone(),
                    workspace_id,
                    label,
                },
            ) {
                self.pane_relocations_in_flight.remove(&pane_id);
                self.push_diagnostic(
                    "lineage.relocate_failed",
                    format!("Could not move delegated pane {pane_id}: {message}"),
                );
            }
        }
        true
    }

    /// Takes the hook-install judgement the coordinator read off the lock.
    /// Queues an install the operator approved, or says why it cannot.
    ///
    /// The write itself happens on the coordinator thread: it is file I/O,
    /// and nothing that touches the disk runs under this mutex.
    fn request_agent_hook_install(&mut self, runtime_id: &str) -> bool {
        let Some(runtime) = hide_agent_hooks::AgentRuntime::from_id(runtime_id) else {
            self.set_error(
                "agent_hooks.unknown_runtime",
                format!("Hide has no agent hook adapter for {runtime_id}"),
                false,
            );
            return true;
        };
        // Approving twice is one install: the request is a set, and the
        // install itself rewrites the same hook group either way.
        self.pending_hook_installs.insert(runtime);
        true
    }

    /// Applies one Background AI settings event.
    ///
    /// The choice takes effect on the snapshot at once, so the control moves
    /// under the operator's hand rather than after a file write; the write
    /// itself is queued for the coordinator, because nothing that touches the
    /// disk runs under this mutex.
    fn apply_ai_settings(&mut self, payload: AiSettingsPayload) -> bool {
        let mut changed = false;
        if let Some(observing) = payload.observing
            && self.ai_observing != observing
        {
            self.ai_observing = observing;
            changed = true;
        }

        // A model without the provider it belongs to is not applied to
        // whichever provider happens to be selected: the event is refused and
        // says so.
        if payload.provider.is_none() && payload.model.is_some() {
            self.set_error(
                "ai_settings.model_without_provider",
                "A background AI model must name the provider it belongs to",
                false,
            );
            return true;
        }

        if let Some(id) = payload.provider.as_deref() {
            let Some(provider) = hide_ai::ProviderId::from_id(id) else {
                self.set_error(
                    "ai_settings.unknown_provider",
                    format!("Hide has no background AI provider called {id}"),
                    false,
                );
                return true;
            };
            let mut settings = self.ai_settings.clone().unwrap_or_default();
            // Naming a model keeps the current selection; naming only a
            // provider selects it. Choosing a model for the provider that is
            // already selected does both, which is the same thing.
            match payload.model {
                Some(model) => settings.set_model(provider, model),
                None => settings.provider = provider,
            }
            if self.ai_settings.as_ref() != Some(&settings) {
                self.ai_settings = Some(settings.clone());
                self.pending_ai_settings_save = Some(settings);
                changed = true;
            }
        }

        if changed {
            self.refresh_background_ai();
        }
        true
    }

    /// What the provider probe should ask, and whether it should ask at all.
    ///
    /// An empty answer while the group is off screen is intentional, the same
    /// way an empty disk request is: an idle Hide must never start a provider
    /// process.
    pub fn ai_request(&self) -> crate::ai::AiRequest {
        let settings = self.ai_settings.clone().unwrap_or_default();
        crate::ai::AiRequest {
            observing: self.ai_observing,
            models: hide_ai::PROVIDERS
                .iter()
                .map(|provider| (*provider, settings.model(*provider).to_owned()))
                .collect(),
        }
    }

    /// Hands a queued settings write to the caller that can perform it.
    pub(crate) fn take_ai_settings_save(&mut self) -> Option<hide_ai::AiSettings> {
        self.pending_ai_settings_save.take()
    }

    /// Stores the choice the coordinator read from the settings file, and
    /// whether reading it failed.
    ///
    /// A failed read is not taken as the defaults in silence: the defaults
    /// are used and the reason travels to the screen with them.
    pub(crate) fn ingest_ai_settings(
        &mut self,
        settings: hide_ai::AiSettings,
        chosen: bool,
        unavailable_reason: Option<String>,
    ) -> bool {
        let same = self.ai_settings.as_ref() == Some(&settings)
            && self.snapshot.status.background_ai.chosen == chosen
            && self.snapshot.status.background_ai.unavailable_reason == unavailable_reason;
        if same {
            return false;
        }
        self.ai_settings = Some(settings);
        self.snapshot.status.background_ai.chosen = chosen;
        self.snapshot.status.background_ai.unavailable_reason = unavailable_reason;
        self.refresh_background_ai();
        true
    }

    /// Stores what the providers answered.
    pub(crate) fn ingest_background_ai(
        &mut self,
        read: crate::model::BackgroundAiSnapshot,
    ) -> bool {
        if self.background_ai_providers == read.providers {
            return false;
        }
        self.background_ai_providers = read.providers;
        self.refresh_background_ai();
        true
    }

    /// Reports a settings write that did not happen, so a choice the operator
    /// made and the file on disk cannot silently disagree.
    pub(crate) fn report_ai_settings_failure(&mut self, reason: String) -> bool {
        if self
            .snapshot
            .status
            .background_ai
            .unavailable_reason
            .as_deref()
            == Some(reason.as_str())
        {
            return false;
        }
        self.snapshot.status.background_ai.unavailable_reason = Some(reason);
        true
    }

    /// Rebuilds the Background AI section from the choice and the last
    /// provider answers. The model each row reports is the configured one,
    /// which is what the probe was run with.
    fn refresh_background_ai(&mut self) {
        let settings = self.ai_settings.clone().unwrap_or_default();
        let mut providers = if self.background_ai_providers.is_empty() {
            crate::model::BackgroundAiSnapshot::unread().providers
        } else {
            self.background_ai_providers.clone()
        };
        for row in &mut providers {
            if let Some(provider) = hide_ai::ProviderId::from_id(&row.id) {
                row.model = settings.model(provider).to_owned();
            }
        }
        self.snapshot.status.background_ai.provider = settings.provider.as_str().to_owned();
        self.snapshot.status.background_ai.providers = providers;
    }

    /// Hands the queued installs to the caller that can perform them.
    pub(crate) fn take_agent_hook_installs(&mut self) -> Vec<hide_agent_hooks::AgentRuntime> {
        std::mem::take(&mut self.pending_hook_installs)
            .into_iter()
            .collect()
    }

    pub(crate) fn ingest_hook_diagnosis(&mut self, diagnosis: hide_agent_hooks::Diagnosis) -> bool {
        if self.hook_diagnosis.as_ref() == Some(&diagnosis) {
            return false;
        }
        self.hook_diagnosis = Some(diagnosis);
        self.sync_pane_lineage();
        true
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
    fn focus_pane(&mut self, pane_id: String, origin: PaneFocusOrigin, request_id: Option<String>) {
        let request_id = request_id.filter(|value| !value.trim().is_empty());
        if let Some(request_id) = request_id.as_deref() {
            if self
                .snapshot
                .status
                .pane_focus_request
                .as_ref()
                .is_some_and(|request| request.request_id == request_id)
            {
                self.push_diagnostic(
                    "pane.focus.duplicate_ignored",
                    format!("Ignored duplicate pane focus request {request_id} for {pane_id}"),
                );
                return;
            }
            self.snapshot.status.pane_focus_request = Some(PaneFocusRequestSnapshot {
                request_id: request_id.to_owned(),
                target_pane_id: pane_id.clone(),
                phase: "pending".to_owned(),
                message: None,
                retryable: false,
            });
            if !self.pane_exists_for_focus(&pane_id) {
                let message = format!("Pane {pane_id} is no longer available.");
                self.finish_pane_focus_request(
                    Some(request_id),
                    &pane_id,
                    "failed",
                    Some(message.clone()),
                    true,
                );
                self.set_error("pane.focus_target_unavailable", message, true);
                return;
            }
        }
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
            let message = "Pane focus requires a live Herdr connection".to_owned();
            self.finish_pane_focus_request(
                request_id.as_deref(),
                &pane_id,
                "failed",
                Some(message.clone()),
                true,
            );
            self.set_error("pane.control_unavailable", message, true);
            return;
        };
        // Rule 11: focusing the pane that already has the keyboard, with
        // nothing in flight, converges without a second notification. The
        // look itself still counts, so only the notification is skipped.
        // Settled means Herdr's own layout agrees too. A checkout coming
        // forward selects a pane locally without telling Herdr, and skipping
        // the notification then let Herdr's next layout take the focus back.
        let herdr_agrees = self.layout_holding_pane(&pane_id).is_none_or(|layout| {
            layout.focused_pane_id == pane_id && self.herdr_active_tab_ids.contains(&layout.tab_id)
        });
        let notify = !already_focused
            || !herdr_agrees
            || !self.view_focus_settled_on(ViewFocusSlot::Pane, &pane_id);
        if notify {
            if let Some(pending) = self.pending_pane_focus.take() {
                self.finish_pane_focus_request(
                    pending.request_id.as_deref(),
                    &pending.target_id,
                    "failed",
                    Some("A newer pane focus replaced this request.".to_owned()),
                    true,
                );
            }
            self.push_diagnostic("pane.focus.requested", format!("Focusing pane {pane_id}"));
            if let Err(message) = live::spawn_pane_control(
                context,
                PaneControlAction::Focus {
                    pane_id: pane_id.clone(),
                },
            ) {
                self.finish_pane_focus_request(
                    request_id.as_deref(),
                    &pane_id,
                    "failed",
                    Some(message.clone()),
                    true,
                );
                self.set_error("pane.focus_worker_failed", message, true);
                return;
            }
            // Latest request wins, so a second click while the first is
            // unconfirmed cannot be pulled back by Herdr's answer to the
            // first.
            self.pending_pane_focus = Some(match request_id {
                Some(request_id) => PendingViewFocus::pane_request(pane_id.clone(), request_id),
                None => PendingViewFocus::new(String::new(), pane_id.clone()),
            });
        } else {
            self.finish_pane_focus_request(
                request_id.as_deref(),
                &pane_id,
                "succeeded",
                None,
                false,
            );
        }
        if origin == PaneFocusOrigin::Restore {
            return;
        }
        self.operator_focused_pane_id = Some(pane_id);
        self.refresh_pane_read_state();
    }

    fn pane_exists_for_focus(&self, pane_id: &str) -> bool {
        self.snapshot
            .pane_layouts
            .iter()
            .any(|layout| layout.pane_ids().contains(&pane_id))
            || self
                .snapshot
                .terminal
                .panes
                .iter()
                .any(|pane| pane.pane_id == pane_id)
    }

    fn finish_pane_focus_request(
        &mut self,
        request_id: Option<&str>,
        target_pane_id: &str,
        phase: &str,
        message: Option<String>,
        retryable: bool,
    ) {
        let Some(request_id) = request_id else { return };
        let Some(request) = self.snapshot.status.pane_focus_request.as_mut() else {
            return;
        };
        if request.request_id != request_id || request.target_pane_id != target_pane_id {
            return;
        }
        request.phase = phase.to_owned();
        request.message = message;
        request.retryable = retryable;
    }

    fn finish_pane_focus_request_by_id(
        &mut self,
        request_id: &str,
        phase: &str,
        message: Option<String>,
        retryable: bool,
    ) {
        let Some(request) = self.snapshot.status.pane_focus_request.as_mut() else {
            return;
        };
        if request.request_id != request_id {
            return;
        }
        request.phase = phase.to_owned();
        request.message = message;
        request.retryable = retryable;
    }

    fn fail_pending_pane_focus_for_target(&mut self, target_pane_id: &str, message: String) {
        let Some(pending) = self
            .pending_pane_focus
            .as_ref()
            .filter(|pending| pending.target_id == target_pane_id)
            .cloned()
        else {
            return;
        };
        self.pending_pane_focus = None;
        self.finish_pane_focus_request(
            pending.request_id.as_deref(),
            target_pane_id,
            "failed",
            Some(message),
            true,
        );
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

    fn apply_pane_layout(
        &mut self,
        layout: PaneLayoutSnapshot,
        session_confirms_pending_pane: bool,
    ) -> bool {
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
        let pending_pane = self.pending_pane_focus.clone();
        let arriving_confirms_pending_tab = self
            .pending_tab_focus
            .as_ref()
            .is_some_and(|pending| pending.target_id == arriving_tab_id);
        let adopt_focus = match pending_pane.as_ref() {
            Some(pending)
                if pending.target_id == layout.focused_pane_id && session_confirms_pending_pane =>
            {
                self.pending_pane_focus = None;
                self.finish_pane_focus_request(
                    pending.request_id.as_deref(),
                    &pending.target_id,
                    "succeeded",
                    None,
                    false,
                );
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
        let Some(pending) = self
            .pending_view_focus(slot)
            .as_ref()
            .filter(|pending| pending.target_id == target_id)
            .cloned()
        else {
            return;
        };
        *self.pending_view_focus_mut(slot) = None;
        if slot == ViewFocusSlot::Pane {
            self.finish_pane_focus_request(
                pending.request_id.as_deref(),
                target_id,
                "failed",
                Some(message.to_owned()),
                true,
            );
        }
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
        changed | self.refresh_inactive_groups()
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
                self.apply_pane_layout(layout, false);
                true
            }
            (PaneControlAction::MoveToNewTab { pane_id, .. }, outcome) => {
                match outcome {
                    Ok(_) => {
                        self.pane_relocations_in_flight.remove(&pane_id);
                        self.push_diagnostic(
                            "lineage.relocated",
                            format!(
                                "Delegated pane {pane_id} moved to its own tab in {elapsed_ms} ms"
                            ),
                        );
                    }
                    Err(error) => {
                        // The request's stamp stays, so the next attempt waits
                        // out RELOCATION_RETRY_INTERVAL_MS: dropping it here
                        // re-sent a refused move on every tick.
                        // Deliberately not a `pane.` diagnostic: the pane
                        // header reads those, and this failure must not
                        // appear over a child the operator never asked to
                        // move (PRD B2).
                        self.push_diagnostic(
                            "lineage.relocate_failed",
                            format!("Could not move delegated pane {pane_id}: {error}"),
                        );
                    }
                }
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
        let is_pane_focus = matches!(
            &action,
            RemoteControlAction::Pane(PaneControlAction::Focus { .. })
        );
        if let Some(key) = remote_tab_creation_key(target_id, &action) {
            self.remote_tab_creations_in_flight.remove(&key);
        }
        match result {
            Ok(RemoteControlOutcome::Acknowledged {
                created_tab_id,
                created_pane_id,
            }) => {
                if is_pane_focus {
                    self.finish_pane_focus_request_by_id(request_id, "succeeded", None, false);
                }
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
                if is_pane_focus {
                    self.finish_pane_focus_request_by_id(
                        request_id,
                        "failed",
                        Some(format!(
                            "{action_kind} for {target_id} returned an invalid outcome"
                        )),
                        true,
                    );
                }
                self.set_error(
                    "remote.control.failed",
                    format!("{action_kind} for {target_id} returned a tab order remotely"),
                    false,
                );
            }
            Err(message) => {
                if is_pane_focus {
                    self.finish_pane_focus_request_by_id(
                        request_id,
                        "failed",
                        Some(message.clone()),
                        true,
                    );
                }
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

    /// Decides an explorer change under the lock and runs it off the lock.
    ///
    /// The decision reads nothing from disk: `plan` refuses a path outside
    /// the focused checkout and a name that is not one component from the
    /// strings alone, and the refusal lands in the slot as a failed
    /// operation so the tree can say why under the row. The filesystem call
    /// then runs on a worker with the mutex released and reports back
    /// through `ingest_explorer_operation_result`; a runtime without a
    /// worker context has no shared mutex and runs it in place.
    fn start_explorer_operation(
        &mut self,
        plan: impl FnOnce(&Path) -> Result<files::ExplorerOperation, String>,
        root: &str,
        started_from: &str,
    ) -> bool {
        if self
            .snapshot
            .explorer_operation
            .as_ref()
            .is_some_and(|operation| operation.phase == "working")
        {
            self.set_error(
                "explorer.busy",
                "Another file operation is still running",
                true,
            );
            return true;
        }
        self.next_explorer_operation_id = self.next_explorer_operation_id.wrapping_add(1).max(1);
        let id = self.next_explorer_operation_id;
        let planned = match self.snapshot.navigator.root_path.as_deref() {
            Some(focused_root) if focused_root == root => plan(Path::new(root)),
            Some(focused_root) => Err(format!("{root} is not the focused checkout {focused_root}")),
            None => Err("No local checkout is focused".to_owned()),
        };
        let operation = match planned {
            Ok(operation) => operation,
            Err(message) => {
                self.snapshot.explorer_operation = Some(ExplorerOperationSnapshot {
                    id,
                    kind: "refused".to_owned(),
                    phase: "failed".to_owned(),
                    path: started_from.to_owned(),
                    destination: started_from.to_owned(),
                    message: Some(message.clone()),
                });
                self.push_diagnostic("explorer.refused", format!("{started_from}: {message}"));
                return true;
            }
        };
        self.snapshot.explorer_operation = Some(ExplorerOperationSnapshot {
            id,
            kind: operation.kind.as_str().to_owned(),
            phase: "working".to_owned(),
            path: operation.source.to_string_lossy().into_owned(),
            destination: operation.destination.to_string_lossy().into_owned(),
            message: None,
        });
        let Some(context) = self.worker_context.clone() else {
            let result = files::apply_explorer_operation(&operation);
            return self.ingest_explorer_operation_result(id, &operation, result);
        };
        let worker_operation = operation.clone();
        match thread::Builder::new()
            .name(format!("herdr-core-explorer-{}", operation.kind.as_str()))
            .spawn(move || {
                let operation = worker_operation;
                let result = files::apply_explorer_operation(&operation);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_explorer_operation_result(id, &operation, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            }) {
            Ok(_) => true,
            Err(error) => {
                let message = format!("The file operation worker could not start: {error}");
                self.ingest_explorer_operation_result(id, &operation, Err(message))
            }
        }
    }

    /// Settles the slot with what the filesystem said. Success moves the
    /// selection to the operation's `selection` and carries the paths the
    /// core owns - the tree's expanded folders and any open file tab - from
    /// the old path to the new one, so a renamed folder stays open and a
    /// renamed file's tab still saves to the file it shows. An item moved to
    /// the Trash drops its expanded folders and keeps its file tabs: the tab
    /// is the operator's draft, and saving it recreates the file (D-05).
    /// Failure changes no core state beyond the message.
    pub(crate) fn ingest_explorer_operation_result(
        &mut self,
        id: u64,
        operation: &files::ExplorerOperation,
        result: Result<(), String>,
    ) -> bool {
        let Some(slot) = self.snapshot.explorer_operation.as_mut() else {
            return false;
        };
        if slot.id != id || slot.phase != "working" {
            return false;
        }
        let source = operation.source.to_string_lossy().into_owned();
        let destination = operation.destination.to_string_lossy().into_owned();
        match result {
            Ok(()) => {
                slot.phase = "finished".to_owned();
                if source != destination {
                    for expanded in &mut self.snapshot.ui_state.expanded_paths {
                        if let Some(moved) = retarget_path(expanded, &source, &destination) {
                            *expanded = moved;
                        }
                    }
                    let mut retargeted = Vec::new();
                    for tab in &mut self.snapshot.editor.tabs {
                        if tab.kind != EditorTabKind::File {
                            continue;
                        }
                        if let Some(moved) = retarget_path(&tab.path, &source, &destination) {
                            tab.label = Path::new(&moved)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .filter(|name| !name.is_empty())
                                .map(str::to_owned)
                                .unwrap_or_else(|| moved.clone());
                            tab.path = moved.clone();
                            retargeted.push((tab.id.clone(), moved));
                        }
                    }
                    for (tab_id, moved) in retargeted {
                        if let Some(document) = self.editor_documents.get_mut(&tab_id) {
                            document.path = moved;
                        }
                    }
                    self.sync_active_editor_document();
                }
                if operation.kind == files::ExplorerOperationKind::PathTrash {
                    self.snapshot.ui_state.expanded_paths.retain(|expanded| {
                        expanded != &source && !expanded.starts_with(&format!("{source}/"))
                    });
                }
                self.snapshot.ui_state.selected_path =
                    Some(operation.selection.to_string_lossy().into_owned());
                self.push_diagnostic(
                    format!("explorer.{}", operation.kind.as_str()),
                    format!("{source} -> {destination}"),
                );
                // A created file opens as an editor tab in this same result,
                // so the tree selection and the tab land in one frame with no
                // second dispatch from the shell. New Folder, Rename and move
                // open nothing. If the file cannot be read into a tab it still
                // exists on disk, so the reason rides the finished slot's
                // message and the tree keeps the created file (B10 pattern).
                if operation.kind == files::ExplorerOperationKind::FileCreate
                    && let Some((workspace_id, checkout_id)) = self
                        .focused_local_checkout()
                        .map(|(workspace, checkout)| (workspace.id.clone(), checkout.id.clone()))
                {
                    match self.prepare_file_tab(&workspace_id, &checkout_id, &destination) {
                        Ok(prepared) => {
                            self.show_file_tab(prepared, &workspace_id, &checkout_id, &destination)
                        }
                        Err(message) => {
                            if let Some(slot) = self.snapshot.explorer_operation.as_mut() {
                                slot.message = Some(message.clone());
                            }
                            self.push_diagnostic(
                                "explorer.file_create.open_failed",
                                format!("{destination}: {message}"),
                            );
                        }
                    }
                }
                self.persist_current_ui_state();
            }
            Err(message) => {
                slot.phase = "failed".to_owned();
                slot.message = Some(message.clone());
                self.push_diagnostic(
                    format!("explorer.{}_failed", operation.kind.as_str()),
                    format!("{source}: {message}"),
                );
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

    /// The grid a frame has to arrive at to be drawn: the view's own grid
    /// while one is known, else the settled size the attach asked for.
    fn expected_terminal_size(&self, pane_id: &str) -> Option<(u16, u16)> {
        self.terminal_view_sizes
            .get(pane_id)
            .or_else(|| self.terminal_sizes.get(pane_id))
            .copied()
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
        let expected = self.expected_terminal_size(pane_id);
        let arrived = (frame.height, frame.width);
        if expected != Some(arrived) {
            self.terminal_frames_need_full.insert(pane_id.to_owned());
            // Logged to the file and stderr sink only, once per foreign grid.
            // A push into the snapshot's diagnostics would restamp the
            // revisioned rest section on every frame of a mismatch burst.
            if self
                .terminal_foreign_frame_sizes
                .insert(pane_id.to_owned(), arrived)
                != Some(arrived)
            {
                crate::diagnostic!(serde_json::json!({
                    "component": "terminal", "kind": "terminal.frame_geometry_mismatch",
                    "pane_id": pane_id, "frame": [frame.width, frame.height],
                    "expected": expected.map(|(height, width)| [width, height]),
                }));
            }
            return Some(false);
        }
        self.terminal_foreign_frame_sizes.remove(pane_id);
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

    /// Puts a prepared file tab on screen. Nothing here can fail on the file.

    /// Restores an editor tab and the project context that owns it as one
    /// caller-visible transition. Reopen uses the same path as a tab click so
    /// an already-open file cannot appear over the wrong checkout.

    /// Opens one Changes row in the central editor strip.
    ///
    /// The right panel remains the list and the tab owns the reading surface,
    /// so closing the panel cannot make an open diff disappear.

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
        self.refresh_inactive_groups();
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
        let event = match events::decode(bytes) {
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
        // The view's grid is the one the frame guard accepts, so it is the
        // grid the attach asks for. Starting at a settled size the view has
        // already left holds every frame until a resize, and a pane that is
        // not drawn sends none: five attempts at 50x25 against a 41x18 view,
        // then retries exhausted (2026-09-14).
        if let Some(view) = self.terminal_view_sizes.get(pane_id).copied() {
            self.terminal_sizes.insert(pane_id.to_owned(), view);
        }
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
                // Both are refilled from the final agent list once the read
                // axis and the lineage are applied; see `sync_pane_lineage`.
                children: None,
                lineage_path: Vec::new(),
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

/// What a waiting child is waiting for, in the words the tooltip uses.
///
/// It names the demand when there is one, because "waiting for an approval"
/// and "running with nothing to show for it" ask different things of the
/// operator.
/// The agent line for one worktree row.
///
/// Empty with no reason is nobody working here; empty with a reason is a
/// worktree Hide cannot see into. Keeping those two apart is the whole point
/// of the third uninstrumented position (PRD B35, D-60).
fn worktree_agent_line(
    panes: Option<&HashSet<String>>,
    chips: &HashMap<String, crate::model::AgentChipSnapshot>,
    instrumentation: &HashMap<String, crate::model::PaneChildrenSnapshot>,
    order: &[SidebarAgentSnapshot],
) -> crate::model::WorktreeAgentLineSnapshot {
    let Some(panes) = panes else {
        return crate::model::WorktreeAgentLineSnapshot::default();
    };
    // The sidebar's order, so the two screens read the same way.
    let agents = order
        .iter()
        .filter(|agent| panes.contains(&agent.pane_id))
        .filter_map(|agent| chips.get(&agent.pane_id).cloned())
        .collect::<Vec<_>>();
    // The first reason in the resolution order the crate declares, rather
    // than the first one the pane iteration happened to reach.
    let worst = order
        .iter()
        .filter(|agent| panes.contains(&agent.pane_id))
        .filter_map(|agent| instrumentation.get(&agent.pane_id))
        .filter_map(|children| children.uninstrumented_code.as_deref())
        .filter_map(hide_agent_hooks::diagnosis::UninstrumentedReason::from_code)
        .min();
    crate::model::WorktreeAgentLineSnapshot {
        agents,
        uninstrumented_reason: worst.map(|reason| reason.message().to_owned()),
        uninstrumented_label: worst.map(|reason| reason.accessibility_label().to_owned()),
        uninstrumented_code: worst.map(|reason| reason.code().to_owned()),
    }
}

/// How badly one waiting child needs an answer, worst first. It breaks ties
/// between descendants that have waited exactly as long.
fn stall_priority(agent: &SidebarAgentSnapshot) -> u8 {
    match agent.demand.as_str() {
        "error" => 0,
        "approval" => 1,
        "question" => 2,
        _ if agent.blocked => 1,
        _ => 3,
    }
}

fn waiting_on(agent: &SidebarAgentSnapshot) -> &'static str {
    match agent.demand.as_str() {
        "error" => "an error",
        "question" => "a question",
        "approval" => "an approval",
        _ if agent.blocked => "an approval",
        _ => "no visible progress",
    }
}

fn unix_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
