use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

mod agents;
mod editor;
mod events;
mod projects;
mod session;

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
