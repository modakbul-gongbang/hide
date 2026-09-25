use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

mod agents;
mod attachments;
mod device_catalog;
mod devices;
mod documents;
mod editor;
mod events;
mod hosts;
mod issues;
mod memory;
mod operations;
mod projects;
mod session;
mod snapshot_delta;
mod terminal;
mod workspace_view;

pub use snapshot_delta::serialize_snapshot_delta;

use events::*;
use operations::*;
use workspace_view::{AreaIntent, WorkspaceViewPayload, WorkspaceViewStore};

use crate::ffi::ChangeNotifier;
use crate::fork::{ForkRequest, ForkableAgent, fork_name, is_forkable};
use crate::live::{
    LiveContext, PaneControlAction, PaneControlOutcome, PaneResizeDirection, PaneSplitDirection,
    RemoteControlAction, RemoteControlContext, RemoteControlOutcome, RemoteTerminalContext,
    SessionFetchError, TerminalSession, TerminalSessionContext, TerminalSessionMode,
};
use crate::model::{
    ArchiveDetailSnapshot, CheckoutSnapshot, CoreOptions, DEFAULT_PANE_TEXT_SCALE,
    DiagnosticSnapshot, EditorDocumentSnapshot, EditorTabKind, EditorTabSnapshot,
    ExplorerOperationSnapshot, LastErrorSnapshot, PANE_TEXT_SCALE_STEP, PaneFindSnapshot,
    PaneFocusRequestSnapshot, PaneForkSnapshot, PaneLayoutNodeSnapshot, PaneLayoutSnapshot,
    PaneSnapshot, PetBadgesSnapshot, PetOriginSnapshot, PetSnapshot, RemoteFileEntrySnapshot,
    RemoteFileListSnapshot, RemoteSessionSnapshot, RightPanelSection, SCHEMA_VERSION,
    SessionRowSnapshot, SidebarAgentSnapshot, Snapshot, StripTabKind, StripTabSnapshot, Surface,
    TabSnapshot, TerminalChunk, TerminalPaneSnapshot, UiStateSnapshot, WorkspaceSnapshot,
    clamp_pane_text_scale,
};
use crate::recent_closed::{ClosedAgent, ClosedContext, ClosedItem, ClosedPane, push_bounded};
use crate::remote::RusshSftpTransport;
use crate::sidebar::{ReadRecordScope, SessionSnapshotPayload, project_agents};
use crate::{chromux, environment, files, live, persistence, pet, session_sync, workspace};

fn conversation_agent_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "claude" | "claude-code" | "claude_code" | "codex"
    )
}

/// Which Herdr workspace each tab belongs to, from each workspace's tab order.
pub(super) fn tab_owners(order: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, String> {
    order
        .iter()
        .flat_map(|(workspace_id, tab_ids)| {
            tab_ids
                .iter()
                .map(move |tab_id| (tab_id.clone(), workspace_id.clone()))
        })
        .collect()
}

/// A device's Herdr workspaces' tab orders, keyed by Herdr's own workspace id
/// and naming each tab by its device-scoped id, as the device's strips do.
/// The raw session carries one checkout per Herdr workspace, in Herdr's order.
pub(super) fn device_workspace_tab_order(
    raw: &RemoteSessionSnapshot,
) -> BTreeMap<String, Vec<String>> {
    raw.workspaces
        .iter()
        .filter_map(|workspace| {
            let herdr_id = workspace.session_workspace_ids.first()?;
            let tabs = workspace
                .checkouts
                .iter()
                .flat_map(|checkout| checkout.tabs.iter())
                .filter_map(|tab| tab.id.clone())
                .collect();
            Some((herdr_id.clone(), tabs))
        })
        .collect()
}

/// Places one checkout's strip from its Herdr and editor tabs, settling a
/// held reorder once Herdr reports the order it asked for.
///
/// A held reorder lands the moment Herdr reports the order it asked for,
/// whichever path carried it: the `tab_moved` event, a move Herdr had already
/// made, or a move made from the TUI. A held reorder whose tabs are no longer
/// the checkout's tabs can never be reported, so it is dropped rather than
/// kept waiting for an order that cannot arrive.
fn place_checkout_strip(
    checkout: &mut CheckoutSnapshot,
    editor: Vec<StripTabSnapshot>,
    owners: &BTreeMap<String, String>,
    order: &mut BTreeMap<String, Vec<String>>,
    pending: &mut BTreeMap<String, PendingTabMove>,
    dropped_moves: &mut Vec<String>,
) {
    let herdr = StripTabSnapshot::from_herdr_tabs(&checkout.tabs);
    let stored = order.entry(checkout.id.clone()).or_default();
    if let Some(held) = pending.get(&checkout.id) {
        // Only the tabs the move was asked about: the workspace it named, as
        // this checkout currently holds them.
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
    target_id: String,
    generation: u64,
    connection_generation: u64,
    phase: String,
    stage: String,
    started_at_unix_ms: u64,
    deadline_at_unix_ms: Option<u64>,
    message: Option<String>,
    retryable: bool,
}

/// The Herdr that carries a tab move: this machine's, or the device whose
/// checkout the moved tab is in.
enum TabMoveCarrier {
    Local(LiveContext),
    Device(RemoteControlContext),
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
/// The checkout an explorer change was decided for: its tabs, its device's
/// expanded folders and its host are the ones the result applies to,
/// whatever is in front when the result lands.
#[derive(Clone, Debug)]
pub(crate) struct ExplorerTarget {
    pub(crate) workspace_id: String,
    pub(crate) checkout_id: String,
    pub(crate) device_id: String,
    pub(crate) root: String,
}

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

/// The absolute lifetime of one stage of a Herdr-owned mutation: restore
/// capture, the request itself, and the topology wait each get this long
/// before the operation becomes a caller-visible unknown result (PRD D-08).
const CLOSE_STAGE_TIMEOUT_MS: u64 = 5_000;

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

/// Whether a pane id is one the core scoped to a remote target
/// (`operations::remote_pane_id`), whichever target it was; a Herdr pane id on
/// this machine never takes that form.
fn is_remote_scoped_pane_id(pane_id: &str) -> bool {
    pane_id
        .strip_prefix("remote:")
        .is_some_and(|rest| rest.contains(":pane:"))
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
                    agent.requires_close_status_check,
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
        let Some((status_label, requires_close_confirmation, requires_close_status_check)) =
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
        if pane.requires_close_status_check != requires_close_status_check {
            pane.requires_close_status_check = requires_close_status_check;
            changed = true;
        }
    }
    changed |= crate::sidebar::sync_checkout_agent_summaries(workspaces, agents);
    changed |= sync_strip_agent_identity(workspaces, agents);
    changed |= crate::project_context::sort_projects(workspaces, agents);
    changed
}

/// Titles each Herdr strip entry after the one agent its tab holds.
///
/// Runs on the same passes as the pane status, so the entry's mark and
/// emphasis follow the read axis, and again whenever a strip is rebuilt, so
/// a fresh entry never reaches the shell without its identity (PRD D-16).
fn sync_strip_agent_identity(
    workspaces: &mut [WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
) -> bool {
    let mut changed = false;
    for checkout in workspaces
        .iter_mut()
        .flat_map(|workspace| workspace.checkouts.iter_mut())
    {
        for entry in checkout
            .strip
            .iter_mut()
            .filter(|entry| entry.kind == crate::model::StripTabKind::Herdr)
        {
            let identity = checkout
                .tabs
                .iter()
                .find(|tab| tab.id.as_deref() == Some(entry.source_id.as_str()))
                .and_then(|tab| {
                    let mut held = agents
                        .iter()
                        .filter(|agent| tab.panes.iter().any(|pane| pane.id == agent.pane_id));
                    let first = held.next()?;
                    held.next()
                        .is_none()
                        .then(|| crate::sidebar::agent_chip(first))
                });
            if entry.agent_identity != identity {
                entry.agent_identity = identity;
                changed = true;
            }
        }
    }
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
            .focused_checkout_id
            .as_deref()
            .and_then(|checkout_id| session.active_tab_ids.get(checkout_id))
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
        // A workspace created for a registered project is keyed by its folder.
        RemoteControlAction::CreateWorkspace { cwd, label } => Some((
            target_id.to_owned(),
            cwd.clone(),
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct PurposeOperationTarget {
    id: u64,
    checkout_id: String,
    remote_target_id: Option<String>,
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
    file_roots: Option<crate::files::FileRoots>,
    state_path: PathBuf,
    state_save_pending: bool,
    state_save_active: bool,
    state_save_worker: Option<thread::JoinHandle<()>>,
    /// Where the SSH config that names each registered device's host lives;
    /// `None` when the process has no usable HOME, which every connect then
    /// reports rather than guessing a path.
    home_path: Option<PathBuf>,
    /// The SSH client and coordinator of each registered device that is
    /// connected or connecting, keyed by device id. Removing a device drops
    /// its entry; the coordinator handle moves to `retired_remote_syncs`.
    remote_connections: HashMap<String, devices::RemoteDeviceConnection>,
    /// Coordinators of removed devices, waiting for the FFI layer to join
    /// them off the runtime lock: a join under the lock would wait for a
    /// worker that is itself waiting for the lock.
    retired_remote_syncs: Vec<session_sync::SessionSyncHandle>,
    /// The last connection test of each device, kept apart from the device
    /// rows because those are rebuilt with every catalog.
    remote_device_tests: HashMap<String, crate::model::DeviceTestSnapshot>,
    /// Each device's helper connection and the consent it runs under.
    device_hosts: HashMap<String, hosts::DeviceHost>,
    /// The last helper attempt number, for every device. Never reset, so an
    /// answer from an attempt made before a device was removed and added
    /// again under the same id cannot match the new attempt.
    last_host_generation: u64,
    /// The device and folder `snapshot.changes` was read for.
    changes_published_key: Option<crate::changes::ChangesKey>,
    /// Each device's session as its Herdr reported it, before its projects
    /// are grouped from the helper's facts (`device_catalog`).
    device_raw_sessions: HashMap<String, RemoteSessionSnapshot>,
    device_facts: HashMap<String, crate::device_catalog::DeviceFacts>,
    /// Each device's repositories' worktrees, read through its helper
    /// (`hide_host::worktrees`); the device's rows carry them.
    device_worktrees: HashMap<String, crate::device_catalog::DeviceWorktrees>,
    /// This machine's file host: the helper's dispatch, run in place.
    local_host: Arc<dyn crate::host_access::HostChannel>,
    /// Where each open file tab's saves go.
    document_places: HashMap<String, crate::files::DocumentPlace>,
    /// Each file tab's save in flight, the newest draft waiting behind it,
    /// and a save whose answer was lost.
    document_saves: HashMap<String, documents::SaveSlot>,
    /// Remote reads not yet shown as tabs, by tab id, with the generation
    /// that fences a late answer.
    document_opens: HashMap<String, documents::OpenRequest>,
    next_document_generation: u64,
    host_packages: crate::remote::host::HelperPackages,
    host_helper_root: String,
    live: Option<LiveContext>,
    remote_controls: HashMap<String, RemoteControlContext>,
    remote_terminals: HashMap<String, RemoteTerminalContext>,
    remote_file_transports: HashMap<String, RusshSftpTransport>,
    remote_control_requests: VecDeque<(String, String)>,
    /// Remote mutations waiting for a transport answer or fresh topology,
    /// keyed by target and request id.
    remote_operations: HashMap<(String, String), PendingRemoteOperation>,
    /// Advances when a remote target's session connects or drops, so a late
    /// answer from an older connection cannot settle a newer request.
    remote_connection_generations: HashMap<String, u64>,
    remote_tab_creations_in_flight: HashSet<(String, String, String, String)>,
    terminal_sessions: HashMap<String, TerminalSession>,
    terminal_session_generations: HashMap<String, u64>,
    terminal_session_lifecycles: HashMap<String, TerminalSessionLifecycle>,
    terminal_recovery: HashMap<String, crate::terminal_recovery::Recovery>,
    next_terminal_session_generation: u64,
    next_remote_file_generation: u64,
    attachment: Option<attachments::PendingAttachment>,
    attachment_rejection: Option<crate::model::AsyncOperationSnapshot>,
    attachment_worker: Option<thread::JoinHandle<()>>,
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
    /// User closes in request order. A close is promoted onto
    /// `recent_closed` only from the front of this queue and only once its
    /// topology is confirmed, so the confirmed stack keeps the order the
    /// operator closed things in (PRD D-11).
    close_capture_order: VecDeque<String>,
    close_operations: HashMap<String, PendingClose>,
    close_status_checks_in_flight: HashSet<String>,
    pane_operations: HashMap<String, PendingPaneOperation>,
    /// The last complete raw layout Herdr confirmed for each tab. Mutation
    /// operations compare fresh geometry with this value, because a model
    /// layout has pane ids and split ratios but no PTY rectangle to confirm a
    /// resize against.
    confirmed_pane_layout_signatures: HashMap<String, PaneTopologySignature>,
    recent_closed_sequence: u64,
    reopen_in_flight: Option<String>,
    /// Advances on every `set_live`, so a worker started against an earlier
    /// local Herdr connection cannot settle an operation on the current one.
    live_generation: u64,
    /// A shell-requested read-only `agent.list` refresh, drained by the
    /// coordinator on its next pass.
    status_refresh_requested: bool,
    next_async_operation_id: u64,
    /// Panes that were scrolled before any view reported their size. One
    /// diagnostic answers for the whole wait; a wheel burst against a pane
    /// with no size would otherwise fill the bounded diagnostics list with the
    /// same sentence and push out everything else that happened.
    #[cfg(test)]
    suppress_terminal_session_workers: bool,
    workspace_creations_in_flight: HashSet<String>,
    editor_documents: HashMap<String, EditorDocumentSnapshot>,
    archive_documents: HashMap<String, ArchiveDetailSnapshot>,
    session_catalog_rows: Vec<SessionRowSnapshot>,
    memory_catalog_rows: Vec<crate::model::MemoryRowSnapshot>,
    memory_sessions_load_in_flight: bool,
    memory_sessions_load_pending: bool,
    memory_sessions_load_generation: u64,
    archive_detail_load_generation: u64,
    memory_operation_in_flight: bool,
    memory_operation_generation: u64,
    memory_operation_checkout_path: Option<String>,
    memory_pending_action: Option<(String, events::MemoryActionPayload)>,
    memory_cancel: Option<hide_ai::CancelToken>,
    /// True only after the operator approves hook updates while enabling
    /// Memory. A diagnosis can finish that intent, but cannot create it.
    memory_enable_after_hook_update: bool,
    memory_poll_in_flight: bool,
    memory_next_poll_unix_ms: u64,
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
    /// Persisted records waiting to compare against an agent after a fresh
    /// Herdr connection. Agent detection can trail restored pane topology.
    pending_read_record_reconciliation: HashSet<String>,
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
    issue_tokens: crate::wire::IssueTokens,
    issue_candidates: BTreeMap<String, crate::issues::IssueCandidate>,
    issue_write_pending: Option<(u64, String, String)>,
    unconfirmed_issue_tokens: BTreeMap<String, issues::UnconfirmedIssueToken>,
    /// The catalog and root index most recently accepted from the sync
    /// coordinator, reused when a later precomputation arrives stale so the
    /// reconcile never rebuilds under the runtime lock.
    last_accepted_catalog: Option<Vec<WorkspaceSnapshot>>,
    catalog_roots: workspace::RootIndex,
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
    cleanup: Option<live::cleanup::CleanupSnapshot>,
    next_cleanup_id: u64,
    /// Bumped when the worktree list itself is known to have changed through a
    /// manual refresh, an in-app removal, or an observed Herdr worktree event.
    worktree_generation: u64,
    /// Identifies one delete handshake across core, Herdr and the shell.
    /// A repeated callback for an older request cannot authorize a newer one.
    next_worktree_removal_id: u64,
    /// Registered projects whose panes a `Remove project…` is closing on a
    /// worker thread. The registration is only removed once the worker
    /// reports Herdr's confirmation, so a repeat for the same project while
    /// that runs is a quiet no-op rather than a second round of closes.
    workspace_removals_in_flight: HashSet<String>,
    next_task_operation_id: u64,
    /// The checkout a purpose receipt belongs to. A remote checkout lives in
    /// `status.remote[].session`, not the local navigator, so the operation
    /// carries this target separately from its shell-facing receipt.
    purpose_operation_target: Option<PurposeOperationTarget>,
    /// Creation purpose writes registered after Herdr confirms the checkout
    /// but before the token request starts. This closes the event-ordering
    /// window where the mirror could observe the token before the task worker
    /// reports whether Git accepted it.
    created_purpose_writes_in_flight: HashMap<String, String>,
    /// Creation values whose token write succeeded but whose Git mirror and
    /// compensating token clear both failed. Kept only for this process so the
    /// row shows its fallback until the user tries Set purpose again.
    unconfirmed_created_purposes: HashMap<String, String>,
    next_explorer_operation_id: u64,
    delta: snapshot_delta::DeltaState,
    /// Each Workspace's presentation; present only in a shell that draws
    /// separate Agent and View areas (`CoreOptions::workspace_views_path`).
    workspace_views: Option<WorkspaceViewStore>,
}

#[derive(Clone)]
struct RuntimeWorkerContext {
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

impl Runtime {
    pub fn new(options: CoreOptions, environment: environment::EnvironmentReport) -> Self {
        let state_path = PathBuf::from(&options.app_state_path);
        let (host_packages, host_helper_root) = Self::helper_packages_from(&options);
        let mut snapshot = Snapshot::initial(&options);
        snapshot.status.environment = environment.statuses;
        let (ui_state, pane_terminal_sizes, disposition) = persistence::load(&state_path);
        snapshot.ui_state = ui_state;
        snapshot.navigator.devices = workspace::devices(&snapshot.ui_state.device_registrations);
        // This machine's row names the root a device consent would name, so
        // the add form can show it before the first device exists.
        if let Some(local) = snapshot
            .navigator
            .devices
            .iter_mut()
            .find(|device| device.kind != "remote")
        {
            local.host.helper_root = Some(host_helper_root.clone());
        }
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
        let workspace_views = options.workspace_views_path.as_ref().map(|path| {
            let (store, diagnostic) = WorkspaceViewStore::open(
                PathBuf::from(path),
                (
                    snapshot.ui_state.right_panel_visible,
                    snapshot.ui_state.right_panel_section,
                ),
            );
            if let Some((kind, message)) = diagnostic {
                crate::diagnostic!(serde_json::json!({
                    "component": "workspace_views",
                    "kind": kind,
                    "message": message,
                    "fallback": "defaults"
                }));
                snapshot.status.diagnostics.push(DiagnosticSnapshot {
                    kind: kind.to_owned(),
                    message,
                    occurred_at: unix_milliseconds(),
                });
            }
            store
        });
        let mut runtime = Self {
            snapshot,
            file_roots: None,
            state_path,
            home_path: environment.home_path,
            remote_connections: HashMap::new(),
            retired_remote_syncs: Vec::new(),
            remote_device_tests: HashMap::new(),
            device_hosts: HashMap::new(),
            last_host_generation: 0,
            changes_published_key: None,
            device_raw_sessions: HashMap::new(),
            device_facts: HashMap::new(),
            device_worktrees: HashMap::new(),
            local_host: Arc::new(crate::host_access::InProcessHost),
            document_places: HashMap::new(),
            document_saves: HashMap::new(),
            document_opens: HashMap::new(),
            next_document_generation: 0,
            host_packages,
            host_helper_root,
            live: None,
            remote_controls: HashMap::new(),
            remote_terminals: HashMap::new(),
            remote_file_transports: HashMap::new(),
            remote_control_requests: VecDeque::new(),
            remote_operations: HashMap::new(),
            remote_connection_generations: HashMap::new(),
            remote_tab_creations_in_flight: HashSet::new(),
            terminal_sessions: HashMap::new(),
            terminal_session_generations: HashMap::new(),
            terminal_session_lifecycles: HashMap::new(),
            terminal_recovery: HashMap::new(),
            next_terminal_session_generation: 0,
            next_remote_file_generation: 0,
            attachment: None,
            attachment_rejection: None,
            attachment_worker: None,
            terminal_view_sizes: HashMap::new(),
            terminal_frames_need_full: HashSet::new(),
            terminal_foreign_frame_sizes: HashMap::new(),
            terminal_sizes: pane_terminal_sizes.into_iter().collect(),
            panes_awaiting_size: HashSet::new(),
            panes_scrolled_before_size: HashSet::new(),
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
            close_capture_order: VecDeque::new(),
            close_operations: HashMap::new(),
            close_status_checks_in_flight: HashSet::new(),
            pane_operations: HashMap::new(),
            confirmed_pane_layout_signatures: HashMap::new(),
            recent_closed_sequence: 0,
            reopen_in_flight: None,
            live_generation: 0,
            status_refresh_requested: false,
            next_async_operation_id: 0,
            #[cfg(test)]
            suppress_terminal_session_workers: false,
            workspace_creations_in_flight: HashSet::new(),
            editor_documents: HashMap::new(),
            archive_documents: HashMap::new(),
            session_catalog_rows: Vec::new(),
            memory_catalog_rows: Vec::new(),
            memory_sessions_load_in_flight: false,
            memory_sessions_load_pending: false,
            memory_sessions_load_generation: 0,
            archive_detail_load_generation: 0,
            memory_operation_in_flight: false,
            memory_operation_generation: 0,
            memory_operation_checkout_path: None,
            memory_pending_action: None,
            memory_cancel: None,
            memory_enable_after_hook_update: false,
            memory_poll_in_flight: false,
            memory_next_poll_unix_ms: 0,
            editor_tab_history: Vec::new(),
            worker_context: None,
            state_save_pending: false,
            state_save_active: false,
            state_save_worker: None,
            pet_active_at_unix_ms: unix_milliseconds(),
            pet_waking_until_unix_ms: 0,
            pet_dragging: false,
            operator_focused_pane_id: None,
            pending_read_record_reconciliation: HashSet::new(),
            visible_tab_ids: BTreeMap::new(),
            pending_tab_focus: None,
            herdr_focused_tab_seen: None,
            pending_pane_focus: None,
            herdr_active_tab_ids: BTreeSet::new(),
            pet_unseen_observed: std::collections::BTreeMap::new(),
            restore_hint_pending: true,
            last_session_spaces: Vec::new(),
            issue_tokens: Default::default(),
            issue_candidates: Default::default(),
            issue_write_pending: None,
            unconfirmed_issue_tokens: BTreeMap::new(),
            last_accepted_catalog: None,
            catalog_roots: workspace::RootIndex::new(),
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
            cleanup: None,
            next_cleanup_id: 0,
            worktree_generation: 0,
            next_worktree_removal_id: 0,
            workspace_removals_in_flight: HashSet::new(),
            next_task_operation_id: 0,
            purpose_operation_target: None,
            created_purpose_writes_in_flight: HashMap::new(),
            unconfirmed_created_purposes: HashMap::new(),
            next_explorer_operation_id: 0,
            delta: snapshot_delta::DeltaState::default(),
            workspace_views,
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
        if self.snapshot.ui_state.right_panel_visible
            && self.snapshot.ui_state.right_panel_section == RightPanelSection::Sessions
        {
            self.request_sessions_refresh();
        }
    }

    pub(crate) fn take_state_save_worker(&mut self) -> Option<thread::JoinHandle<()>> {
        self.state_save_worker.take()
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The daemon installs only handles opened under its pinned registrations.
    /// An empty set still enforces the boundary; Swift leaves this as `None`.
    /// Returns whether the front Workspace's View tabs, which waited for its
    /// root, began to restore, so the caller announces the change.
    pub fn set_file_roots(&mut self, roots: crate::files::FileRoots) -> bool {
        self.file_roots = Some(roots);
        self.restore_front_when_ready()
    }

    pub fn set_live(&mut self, context: LiveContext) {
        self.live_generation = self.live_generation.saturating_add(1);
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

    pub fn dispatch_json(&mut self, bytes: &[u8]) -> bool {
        let event = match events::decode(bytes) {
            Ok(event) => event,
            Err(error) => {
                self.set_error(error.kind, error.message, false);
                return true;
            }
        };

        let cleared_error = self.snapshot.status.last_error.take().is_some();
        let intent = self
            .separate_view_areas()
            .then(|| AreaIntent::of(&event))
            .flatten();
        let high_frequency = event.is_terminal_io();
        let chooses_workspace = matches!(
            event,
            Event::FocusCheckout(_) | Event::FocusPane(_) | Event::FocusTab(_)
        );
        let changed = self.apply(event) || cleared_error;
        // The areas follow the event that moved the screen, in the same
        // frame (D-08); terminal input and output never move them, so they
        // skip the pass.
        if self.separate_view_areas() && !high_frequency {
            if let Some(intent) = intent
                && self.snapshot.status.last_error.is_none()
            {
                self.apply_area_intent(intent);
            }
            if chooses_workspace && self.snapshot.status.last_error.is_none() {
                self.mark_front_chosen();
            }
            self.sync_workspace_view();
        }
        // Every event that can move the visible tab funnels through here, so
        // the attach window is maintained once rather than at each of the four
        // places a tab becomes visible.
        let released = self.track_visible_tab_attachments();
        changed || released
    }
}

/// One tab's panes, as the navigator draws them.
///
/// Lifted out of the placement loop so every section projects a pane row
/// through the same code: a row that differed between sections would be a
/// second definition of what a pane is.
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
                requires_close_status_check: agent
                    .is_some_and(|agent| agent.requires_close_status_check),
                identity_label: agent.map(|agent| agent.identity_label.clone()),
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

fn unix_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(cancel) = self.memory_cancel.take() {
            cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests;
