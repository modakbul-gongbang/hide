use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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
    /// Every tab's layout in the local Herdr session, keyed by the tab id
    /// each one carries. The shell draws the entry whose tab is active, so a
    /// tab switch is a lookup rather than a wait: nothing here is emptied to
    /// mark a switch in progress, and the geometry drawn is always one Herdr
    /// has already applied.
    pub pane_layouts: Vec<PaneLayoutSnapshot>,
    pub terminal: TerminalSnapshot,
    pub editor: EditorSnapshot,
    pub changes: ChangesSnapshot,
    pub card: CheckoutCardSnapshot,
    pub git_worktrees: Option<ProjectWorktreesSnapshot>,
    pub git_worktrees_loading: bool,
    pub git_worktrees_remote: bool,
    pub worktree_removal: Option<WorktreeRemovalSnapshot>,
    pub task_operation: Option<TaskOperationSnapshot>,
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
    /// The four groups the sidebar draws, counted once. `needs_you` is the
    /// pet's "act now" number and `done` is what finished unseen, so the badge
    /// row and the sidebar cannot disagree.
    pub needs_you: usize,
    pub done: usize,
    pub working: usize,
    pub seen: usize,
    /// Retained agents on a server that stopped answering. They are counted
    /// separately because a stale count of what is waiting would be a lie.
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
    /// The one space that is not a project. Its own section, never a row in
    /// `workspaces` and never counted with them.
    pub scratch: ScratchSnapshot,
}

/// The Scratch node: one fixed folder, and the Herdr tabs living in it.
///
/// It is deliberately not a `WorkspaceSnapshot`. A project is a repository
/// with checkouts, worktrees, a branch and a card; Scratch is a folder with
/// tabs, and giving it the project shape would have meant answering all of
/// that with placeholders and then keeping it out of every project view by
/// hand.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScratchSnapshot {
    /// Always `scratch::NODE_ID`. Carried so the shell addresses the node by
    /// the value the core sent rather than by a string it repeats.
    pub id: String,
    pub label: String,
    /// The folder every Scratch pane runs in. The core creates it as the
    /// first step of the Scratch tab pipeline.
    pub path: String,
    /// Collapsed by default, so this is false until the operator opens it.
    pub expanded: bool,
    /// The Herdr workspaces holding Scratch panes, in Herdr order. Empty when
    /// Herdr has none yet, which is what tells a new tab to create one.
    pub session_workspace_ids: Vec<String>,
    pub tabs: Vec<ScratchTabSnapshot>,
}

/// One row in the Scratch section.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScratchTabSnapshot {
    /// Herdr's tab id.
    pub id: String,
    /// Herdr's own tab label, which is what a tab with no agent shows.
    pub label: String,
    /// The chat's title, read from the pane metadata token an agent tab was
    /// started with. Absent for a terminal tab, and for an agent tab whose
    /// title was never written, which then falls back to the label.
    pub title: Option<String>,
    pub panes: Vec<PaneSnapshot>,
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

/// An agent's state on three independent axes, plus the values the shell draws
/// from them.
///
/// The axes answer three different questions that a single flat state string
/// used to mix: what the agent needs from the operator (`demand`), whether it
/// is running (`activity`), and whether the operator has looked at it since it
/// last changed (`unread`). Everything below `unread` is derived here so the
/// shell only draws (design rule 4).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SidebarAgentSnapshot {
    pub id: String,
    pub pane_id: String,
    pub workspace_label: String,
    /// The checkout the agent's pane is in, once the navigator has placed it.
    #[serde(default)]
    pub checkout_label: Option<String>,
    pub agent_kind: String,
    /// What the agent needs from the operator: `question`, `approval`, `error`,
    /// or `none`. Herdr's `blocked` lifecycle is an approval.
    pub demand: String,
    /// Whether the agent is running: `working`, `stopped`, or `unknown`.
    /// Herdr's `done` and `idle` are the same activity; the difference between
    /// them is a read judgment Herdr makes per tab, and Hide does not use it.
    pub activity: String,
    /// Whether this pane has changed since the operator last had it focused.
    /// Owned by Hide per pane, never by Herdr's tab-scoped seen.
    pub unread: bool,
    /// Herdr reports an approval prompt on this pane right now. It holds the
    /// row in Needs You whether or not the operator has read it.
    pub blocked: bool,
    /// Derived: `needs_you`, `done`, `working`, or `seen`.
    pub group: String,
    pub symbol: String,
    /// Derived: rows in Needs You and Done are drawn bright, the rest subdued.
    pub emphasized: bool,
    /// Derived: the short human word for this row. No view shows an axis value.
    pub status_label: String,
    /// Derived: closing this pane would interrupt work or discard a result the
    /// operator has not read.
    pub requires_close_confirmation: bool,
    pub summary: String,
    pub elapsed: String,
    /// The ordering key: the label plugin's activity timestamp when it has one,
    /// otherwise Herdr's state change sequence zero-padded to the same width.
    pub last_activity: String,
    /// Herdr's own state change sequence, one of the three inputs to a pane's
    /// read record.
    #[serde(skip_serializing)]
    pub state_change_seq: Option<u64>,
    pub ambient: Option<AmbientSignal>,
    /// The conversation id this agent is running, kept only when Herdr recorded
    /// the session as an id. A session recorded as a path is dropped here,
    /// because neither agent's fork command takes one.
    #[serde(skip_serializing)]
    pub session_id: Option<String>,
    /// The pane this agent was spawned from, as Herdr's own lineage records it.
    #[serde(skip_serializing)]
    pub spawned_from_pane_id: Option<String>,
    /// The chat title the composer wrote onto this agent's pane, read back
    /// from Herdr's pane metadata token. Absent for an agent Hide did not
    /// start through the composer, which falls back to its tab label.
    pub chat_title: Option<String>,
    /// Ownership, derived from the lineage alone: a root is the operator's own
    /// work, a descendant is work the root delegated. It is the fourth derived
    /// axis beside demand, activity and read, and it is advice rather than a
    /// boundary - a delegated pane is still selectable and still takes input,
    /// because the operator has to be able to reach one when it escalates
    /// (PRD D-36, D-56).
    ///
    /// An orphan is a root again: when the parent is gone, ownership comes
    /// back to the person (PRD D-52).
    pub delegated: bool,
    /// The pane that spawned this one, when that pane's agent is still in the
    /// list. `spawned_from_pane_id` records what Herdr said; this records
    /// which row it actually resolved to, so a breadcrumb never points at a
    /// pane that is not there.
    pub lineage_parent_pane_id: Option<String>,
    /// The ancestors between the lineage root and this agent, root first and
    /// excluding this agent. The breadcrumb is this list; nothing persists a
    /// visited path, because a stored one rots across a restart, a tab switch
    /// or a child exiting (PRD D-18).
    pub lineage_path_pane_ids: Vec<String>,
    /// Every agent sharing this agent's parent, in the same order the parent
    /// lists its children, including this agent. It is what a breadcrumb
    /// step's dropdown offers (PRD B10).
    ///
    /// A root has none: the layer above a root is the sidebar, not the
    /// breadcrumb, and independent roots are not one another's siblings.
    pub lineage_sibling_pane_ids: Vec<String>,
    /// Tree-only presentation. The canonical agent list and its read axes stay flat.
    pub lineage_depth: usize,
    pub lineage_child_pane_ids: Vec<String>,
    pub lineage_root_checkout_id: Option<String>,
    pub lineage_worktree_badge: Option<String>,
    pub lineage_orphan: bool,
    pub lineage_hint: Option<String>,
    pub raised_hint: Option<String>,
    pub lineage_collapsed: bool,
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
    /// Local branch refs available as a base for a new worktree. This is not
    /// derived from checkout rows because a valid base need not be checked
    /// out anywhere.
    pub branches: Vec<String>,
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

/// Agent counts and representative after Hide applies its pane-level read records.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CheckoutAgentSummary {
    pub representative_pane_id: Option<String>,
    pub needs_you: usize,
    pub done: usize,
    pub working: usize,
    pub seen: usize,
    /// A subset of Seen, not an additional group.
    pub unknown: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CheckoutSnapshot {
    pub github: GithubStatusSnapshot,
    pub agent_summary: CheckoutAgentSummary,
    pub id: String,
    pub workspace_id: String,
    pub label: String,
    pub path: String,
    pub branch: Option<String>,
    pub is_worktree: bool,
    pub exists: bool,
    pub temporary: bool,
    /// Whether Herdr has a pane here. A worktree earns a row from git rather
    /// than from a pane, so the row needs this to draw the ones with no
    /// terminal dimmed and offer to start one.
    pub has_panes: bool,
    pub dirty: bool,
    pub changed_file_count: u32,
    pub base_branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub added_lines: u32,
    pub removed_lines: u32,
    pub unpushed: Option<UnpushedSnapshot>,
    /// This branch's pull request, absent when it has none or when `gh` could
    /// not say. `GithubStatusSnapshot` on the card is what tells those apart.
    pub pull_request: Option<PullRequestSnapshot>,
    /// The complete worktree row backing the card and both removal menus.
    /// All three surfaces therefore consume one core-owned policy result.
    pub worktree: Option<WorktreeSnapshot>,
    pub tabs: Vec<TabSnapshot>,
    /// The tab Herdr reports as active in this checkout, or `None` when the
    /// workspace's active tab lives in a sibling checkout. A checkout never
    /// substitutes its first tab for a missing one; an active id Herdr names
    /// but the navigator cannot place is reported as a diagnostic instead.
    pub active_tab_id: Option<String>,
    /// The one ordered tab strip for this checkout: Herdr tabs and file tabs
    /// in the order the operator sees them, which the shell draws as it is
    /// given rather than joining two lists of its own.
    pub strip: Vec<StripTabSnapshot>,
    /// The label the next Herdr tab created here should carry. Decided by the
    /// core from Herdr's raw labels, next to the function that formats them,
    /// so the shell never has to read a number back out of a label it was
    /// given to draw.
    pub next_tab_label: String,
}

/// One entry in a checkout's tab strip.
///
/// It carries identity, kind, and the label to draw. The panes, the dirty
/// mark, and the active mark stay on the snapshots the entry points at: those
/// change on a keystroke, and the strip rides the revisioned navigator
/// section, which must not be resent for every edited character.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StripTabSnapshot {
    /// Strip-wide identity, unique across kinds.
    pub id: String,
    pub kind: StripTabKind,
    /// The Herdr tab id or editor tab id this entry stands for.
    pub source_id: String,
    pub label: String,
}

/// The kinds of tab a strip holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StripTabKind {
    Herdr,
    File,
    Diff,
}

impl StripTabSnapshot {
    pub fn herdr(source_id: impl Into<String>, label: impl Into<String>) -> Self {
        let source_id = source_id.into();
        Self {
            id: format!("herdr:{source_id}"),
            kind: StripTabKind::Herdr,
            source_id,
            label: label.into(),
        }
    }

    pub fn file(source_id: impl Into<String>, label: impl Into<String>) -> Self {
        let source_id = source_id.into();
        Self {
            id: format!("file:{source_id}"),
            kind: StripTabKind::File,
            source_id,
            label: label.into(),
        }
    }

    pub fn diff(source_id: impl Into<String>, label: impl Into<String>) -> Self {
        let source_id = source_id.into();
        Self {
            id: format!("diff:{source_id}"),
            kind: StripTabKind::Diff,
            source_id,
            label: label.into(),
        }
    }

    /// The Herdr half of a strip, in the order the tabs are given.
    ///
    /// A tab with no id has no identity to key a strip entry by, so it is
    /// dropped rather than given a placeholder. Both the local strip and the
    /// remote projection build their Herdr entries here, so the rule that
    /// decides which tabs earn a slot and what an unlabelled one reads as has
    /// one implementation to change.
    pub fn from_herdr_tabs(tabs: &[TabSnapshot]) -> Vec<Self> {
        tabs.iter()
            .filter_map(|tab| {
                Some(Self::herdr(
                    tab.id.clone()?,
                    tab.label.clone().unwrap_or_default(),
                ))
            })
            .collect()
    }
}

/// The label a tab is drawn by, derived from the tab's own identity.
///
/// Herdr numbers an unnamed tab, which reads as a bare "2" in a strip; that
/// number becomes "Tab 2". A tab the operator named keeps its name. A tab
/// Herdr reports without any label falls back to its id, never to its
/// position in the strip: a position-derived label renames every tab when one
/// of them moves.
pub fn display_tab_label(raw_label: &str, tab_id: &str) -> String {
    let trimmed = raw_label.trim();
    if trimmed.is_empty() {
        return tab_id.to_owned();
    }
    match trimmed.parse::<u32>() {
        Ok(number) => format!("Tab {number}"),
        Err(_) => trimmed.to_owned(),
    }
}

/// The label to give the next Herdr tab created in a checkout.
///
/// Reads Herdr's raw labels, never the ones `display_tab_label` has already
/// formatted. The two are one convention: that function decides how a tab's
/// number is shown, this one decides which number is free. Deriving the free
/// number from formatted text instead would write the convention down a second
/// time, on the far side of the FFI boundary, where a change to either half
/// breaks the other silently.
///
/// A tab Herdr labels with a bare number, or with the `Tab N` this function
/// hands out and Herdr stores verbatim, holds that number; a tab labelled
/// anything else holds none. The answer is the lowest number no tab holds.
/// Counting only bare numbers made every created tab `Tab 2`.
pub fn next_tab_label<'a>(raw_labels: impl IntoIterator<Item = &'a str>) -> String {
    let used = raw_labels
        .into_iter()
        .filter_map(|label| {
            let label = label.trim();
            let number = label.strip_prefix("Tab ").unwrap_or(label);
            number.trim().parse::<u32>().ok()
        })
        .collect::<BTreeSet<_>>();
    // Bounded rather than an open range: n labels cannot cover n + 1
    // candidates, so a gap always exists in this span and the search is total.
    let number = (1..=used.len() as u32 + 1)
        .find(|candidate| !used.contains(candidate))
        .expect("a set of n numbers leaves one of n + 1 candidates free");
    format!("Tab {number}")
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
    pub content: crate::pane_content::PaneContent,
    /// The three names a pane can be shown by, in the order the header prefers
    /// them. The core ships the ingredients rather than a chosen title so the
    /// local and remote projections cannot disagree about the ladder, and so
    /// the one that runs it stays testable in the shell.
    pub herdr_label: Option<String>,
    pub terminal_title: Option<String>,
    pub workspace_label: Option<String>,
    pub cwd: String,
    /// The one short human word for the agent in this pane, from the same
    /// derivation the sidebar row uses.
    pub status_label: String,
    /// Whether closing this pane needs the operator to confirm first. Derived
    /// with the agent row's own value so the header and the core cannot
    /// disagree about it.
    pub requires_close_confirmation: bool,
    pub summary: Option<String>,
    pub activity_at_unix_ms: Option<u64>,
    pub fork: PaneForkSnapshot,
    /// The ports listened on from at or below this pane's working directory.
    pub ports: Vec<u16>,
    /// What this pane's agent delegated, or why that is unknown. `None` on a
    /// pane Herdr detected no agent in: a shell, an editor or a log gets
    /// neither chips nor an uninstrumented mark, because there is no agent
    /// there to have children (PRD B22, D-30).
    pub children: Option<PaneChildrenSnapshot>,
    /// The breadcrumb: this pane's ancestors root first, then this pane. It
    /// is empty for a lineage root, which is what leaves its header plain.
    pub lineage_path: Vec<LineageStepSnapshot>,
}

/// What a pane header says about the work its agent delegated.
///
/// The three shapes it can take are deliberately different screens: chips
/// mean known children, an empty chip list on an instrumented pane means a
/// confirmed "this agent is working alone", and `instrumented: false` means
/// Hide cannot see and says why (PRD B21, B23, B32).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PaneChildrenSnapshot {
    pub instrumented: bool,
    /// The first matching reason from the fixed order, present exactly when
    /// `instrumented` is false. It is never empty and never a guess.
    pub uninstrumented_reason: Option<String>,
    /// The accessible name for the uninstrumented mark, so the symbol never
    /// carries the meaning by itself (PRD B37).
    pub uninstrumented_label: Option<String>,
    /// One chip per pane child, in the lineage's own child order. Only panes:
    /// an in-process subagent has no pane, so it cannot be a chip the
    /// operator clicks into (PRD D-63).
    pub chips: Vec<ChildChipSnapshot>,
    /// The parent badge, chosen from the pane children by the same priority
    /// the Workspace summary uses. In-process subagents take no part in it.
    pub representative: Option<ChildChipSnapshot>,
    /// In-process subagents, summarised and never added to the chip count.
    pub subagents: SubagentCountsSnapshot,
}

impl PaneChildrenSnapshot {
    /// The permanent answer for a pane on another machine.
    pub fn remote() -> Self {
        let reason = hide_agent_hooks::diagnosis::UninstrumentedReason::RemoteHost;
        Self {
            instrumented: false,
            uninstrumented_reason: Some(reason.message().to_owned()),
            uninstrumented_label: Some(reason.accessibility_label().to_owned()),
            ..Self::default()
        }
    }
}

/// One child in the pane header's chip row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChildChipSnapshot {
    pub pane_id: String,
    /// The short name the chip shows beside its mark.
    pub label: String,
    /// The longer description for the chip's tooltip.
    pub detail: String,
    pub agent_kind: String,
    pub demand: String,
    pub activity: String,
    pub emphasized: bool,
    pub symbol: String,
    pub status_label: String,
    /// Whether this child is still delegated work. It lifts when the child
    /// has been stalled long enough to become the operator's problem.
    pub delegated: bool,
}

/// The in-process subagents a pane's session reports.
///
/// Each count is optional because each is separately knowable. An adapter
/// that cannot observe one leaves it `None`, and `None` draws as unknown
/// rather than as zero (PRD B24, B32, D-53).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SubagentCountsSnapshot {
    pub working: Option<u32>,
    pub done: Option<u32>,
    pub blocked: Option<u32>,
}

impl SubagentCountsSnapshot {
    /// Whether there is anything at all to draw.
    pub fn is_silent(&self) -> bool {
        self.working.is_none_or(|count| count == 0)
            && self.done.is_none_or(|count| count == 0)
            && self.blocked.is_none_or(|count| count == 0)
    }
}

/// One step of a pane header's lineage breadcrumb.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LineageStepSnapshot {
    pub pane_id: String,
    pub label: String,
    /// That layer's other agents, in the parent's own child order, so the
    /// step's dropdown can offer them without a second traversal. It includes
    /// the step itself, so the current position is visible in the list.
    pub siblings: Vec<ChildChipSnapshot>,
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

impl Snapshot {
    /// The layout being drawn: the one belonging to the tab that holds the
    /// selected pane. Every tab's layout is carried, so this is a lookup and
    /// never waits for Herdr to name the visible tab again.
    pub fn active_pane_layout(&self) -> Option<&PaneLayoutSnapshot> {
        let pane_id = self.terminal.pane_id.as_deref()?;
        self.pane_layouts
            .iter()
            .find(|layout| layout.pane_ids().contains(&pane_id))
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<TerminalFrame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_sent: Option<TerminalInputSent>,
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub struct TerminalInputTrace {
    pub id: u64,
    pub started_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TerminalInputSent {
    pub id: u64,
    pub milliseconds: f64,
    pub outcome: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TerminalFrame {
    pub width: u16,
    pub height: u16,
    pub full: bool,
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
    pub transport_last_attempt_at_unix_ms: Option<u64>,
    pub transport_exit_category: Option<String>,
    pub transport_retry_decision: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditorSnapshot {
    pub tabs: Vec<EditorTabSnapshot>,
    pub active_tab_id: Option<String>,
    pub document: Option<EditorDocumentSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditorTabSnapshot {
    pub id: String,
    pub workspace_id: String,
    pub checkout_id: String,
    pub path: String,
    pub label: String,
    pub kind: EditorTabKind,
    /// Which Changes group a diff tab represents. Present only for diff tabs.
    pub diff_committed: Option<bool>,
    pub markdown_preview: bool,
    pub wrap: bool,
    pub dirty: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorTabKind {
    File,
    Diff,
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

/// The right panel's four persisted sections.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RightPanelSection {
    #[default]
    Overview,
    Explorer,
    Changes,
    Git,
}

impl RightPanelSection {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "overview" => Some(Self::Overview),
            "explorer" => Some(Self::Explorer),
            "changes" => Some(Self::Changes),
            "git" => Some(Self::Git),
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
    /// Sidebar workspaces (checkout paths) whose agent rows are hidden.
    #[serde(default)]
    pub collapsed_checkout_ids: Vec<String>,
    #[serde(default)]
    pub project_base_branches: BTreeMap<String, String>,
    #[serde(default)]
    pub collapsed_agent_pane_ids: Vec<String>,
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
    /// nothing else. A pane's terminal bytes are sized by `pane_text_scales`
    /// and the editor's code by `editor_text_scale`, so no two of the three
    /// ever apply to the same text.
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Text scale for one pane's own content, keyed by pane id. A pane at the
    /// default scale is absent rather than present at 1.0, so the map stays
    /// the size of what the user actually changed.
    ///
    /// Every key here is a pane id Herdr reports, which is what lets a pane
    /// that goes away take its entry with it. Nothing else may be stored here.
    #[serde(default)]
    pub pane_text_scales: BTreeMap<String, f32>,
    /// Text scale for the file editor's code, which is one surface rather than
    /// one per document.
    ///
    /// It is its own field and not a row in `pane_text_scales` because the
    /// editor is not a pane: keyed into that map it had no pane id to be
    /// reported under, so the pass that drops a departed pane's scale dropped
    /// the editor's zoom on every agent state change.
    #[serde(default = "default_pane_text_scale")]
    pub editor_text_scale: f32,
    /// What the operator had already seen on each pane, keyed by pane id.
    ///
    /// This is Hide's own record and the only authority for the read axis.
    /// Herdr marks every pane in a tab seen the moment that tab is focused, so
    /// three finished agents side by side would clear together; a pane-level
    /// record is what keeps them separate. It rides the existing store rather
    /// than a second file (engineering rule 7), and a store written before it
    /// existed loads with an empty record, which reads as everything unread.
    // The store owns persistence; the shell only reads the derived unread axis.
    #[serde(default, skip_serializing)]
    pub pane_read_records: BTreeMap<String, PaneReadRecord>,
    /// The agent the composer offers next time, which is the one the operator
    /// last started. Written on submission only, so it costs the revisioned
    /// section nothing between submissions.
    #[serde(default = "default_agent_kind")]
    pub last_agent_kind: String,
    /// Whether the composer's bypass toggle is on. The operator's choice
    /// survives a restart because they asked for it to (D-15); the warning
    /// beside the chip is what keeps it visible rather than forgetting it.
    #[serde(default)]
    pub last_agent_bypass: bool,
    /// Whether the Scratch section is open.
    ///
    /// Its own field rather than a row in `collapsed_workspace_ids`, because
    /// that list records the exceptions to a default of expanded and Scratch
    /// defaults to collapsed. Encoding "collapsed by default" in a collapsed
    /// list needs a sentinel for "never recorded"; one boolean says it.
    #[serde(default)]
    pub scratch_expanded: bool,
}

/// One pane's read mark: the state the operator was looking at the last time
/// the pane held keyboard focus.
///
/// A pane is unread when its current state does not match this record, so a
/// missing record means unread. Equality rather than "newer than" is
/// deliberate: a Herdr server restart can reset the sequence, and showing a
/// pane as unread is the safe answer when the record can no longer be trusted.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneReadRecord {
    #[serde(default)]
    pub state_change_seq: Option<u64>,
    pub demand: String,
    pub activity: String,
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

pub(crate) fn default_pane_text_scale() -> f32 {
    DEFAULT_PANE_TEXT_SCALE
}

impl Default for UiStateSnapshot {
    fn default() -> Self {
        Self {
            left_sidebar_visible: true,
            right_panel_visible: true,
            right_panel_section: RightPanelSection::default(),
            expanded_paths: Vec::new(),
            collapsed_workspace_ids: Vec::new(),
            collapsed_checkout_ids: Vec::new(),
            project_base_branches: BTreeMap::new(),
            collapsed_agent_pane_ids: Vec::new(),
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
            editor_text_scale: DEFAULT_PANE_TEXT_SCALE,
            pane_read_records: BTreeMap::new(),
            last_agent_kind: default_agent_kind(),
            last_agent_bypass: false,
            scratch_expanded: false,
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

/// The composer's agent before the operator has started one.
pub(crate) fn default_agent_kind() -> String {
    "claude".to_owned()
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
    /// The working tree's own changes: what `git status` reports.
    pub entries: Vec<ChangedFileSnapshot>,
    /// What commits on this branch changed since [`Self::base_branch`].
    /// Separate from `entries` because they answer different questions - what
    /// is not saved yet, and what this branch is - and the view shows them as
    /// two groups for that reason.
    pub committed: Vec<ChangedFileSnapshot>,
    /// What the committed group is measured against. Absent for a plain
    /// folder and for a repository whose base could not be resolved, in which
    /// case the committed group is not shown at all.
    pub base_branch: Option<String>,
    pub selected_path: Option<String>,
    /// Which group the selection is in. A path can appear in both groups and
    /// its two diffs are different, so the group is part of the selection.
    pub selected_committed: bool,
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
    /// Lines added and removed in this file. Absent for a file git cannot
    /// count - an untracked file has no index side and a binary file has no
    /// lines - so the row shows no numbers rather than a misleading zero.
    pub added_lines: Option<u32>,
    pub removed_lines: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChangedFileDiffSnapshot {
    pub path: String,
    pub text: String,
    /// Set when the diff was cut short, naming the limit that cut it. A
    /// silently truncated diff would read as a complete one.
    pub notice: Option<String>,
}

/// What a branch's pull request is, reduced to the five values the row badge
/// and the card show. The mapping from `gh`'s `state`/`reviewDecision`/
/// `isDraft` triple lives in [`crate::github`]; nothing downstream re-derives
/// it, so the badge cannot drift between the row and the card.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestBadge {
    Merged,
    Closed,
    /// Open, not a draft, and a review decision has been recorded. The three
    /// decisions are one badge with three colours rather than three badges.
    Review,
    /// Open with no review decision, or open as a draft.
    Open,
}

impl PullRequestBadge {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::Review => "review",
            Self::Open => "open",
        }
    }

    /// The two states a worktree may be removed from. Everything else means
    /// work is still live on the branch, so the card offers no button at all.
    pub fn is_settled(self) -> bool {
        matches!(self, Self::Merged | Self::Closed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    ReviewRequired,
    ChangesRequested,
    Approved,
}

/// CI rollup. Unknown and absent checks must never look like a pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestChecks {
    #[default]
    Unknown,
    None,
    Pending,
    Failed,
    Passing,
}

/// One branch's pull request, already tie-broken against every other pull
/// request on that branch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PullRequestSnapshot {
    pub title: String,
    pub checks: PullRequestChecks,
    pub number: u32,
    pub head_branch: String,
    pub base_branch: String,
    pub url: String,
    pub badge: PullRequestBadge,
    /// Present only for a `review` badge, and only to pick its colour.
    pub review: Option<ReviewDecision>,
    pub is_draft: bool,
    pub merged_at_unix_ms: Option<u64>,
    pub updated_at_unix_ms: Option<u64>,
}

/// How a repository's `gh` lookup is doing, independent of what it found.
///
/// "No pull request on this branch" and "the lookup failed" are different
/// answers and the card must not show one as the other, so availability,
/// staleness, and the reason travel beside the results rather than being
/// inferred from an empty list.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GithubStatusSnapshot {
    pub failure_category: Option<String>,
    /// `gh` is installed and logged in.
    pub available: bool,
    /// No lookup has completed yet for this repository.
    pub loading: bool,
    /// The last lookup failed, so the values shown are the previous ones.
    pub stale: bool,
    pub last_success_at_unix_ms: Option<u64>,
    /// The card's one allowed sentence: gh missing, logged out, or the exact
    /// failure. Absent when the lookup is healthy.
    pub unavailable_reason: Option<String>,
}

/// One repository's pull requests as `gh` reported them.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GithubProjectSnapshot {
    /// The repository's main worktree, which is what identifies a project.
    pub root_path: String,
    pub status: GithubStatusSnapshot,
    /// One entry per branch that has a pull request.
    pub pull_requests: Vec<PullRequestSnapshot>,
}

/// Every repository's pull-request state, keyed by main worktree path.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GithubSnapshot {
    pub projects: Vec<GithubProjectSnapshot>,
}

impl GithubSnapshot {
    pub fn project(&self, root_path: &str) -> Option<&GithubProjectSnapshot> {
        self.projects
            .iter()
            .find(|project| project.root_path == root_path)
    }
}

/// How far a branch is from the remote it tracks. Absent entirely when the
/// branch has no upstream, because "nothing to push" and "nowhere to push to"
/// are different facts and the card shows only the first.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct UnpushedSnapshot {
    pub remote: String,
    pub count: u32,
}

/// One worktree of one repository, as `git worktree list` reports it plus the
/// counts the row badge and the card show.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WorktreeSnapshot {
    pub head_sha: Option<String>,
    pub last_commit_subject: Option<String>,
    pub last_commit_unix_seconds: Option<u64>,
    pub nested: bool,
    pub merged: Option<bool>,
    pub upstream_state: String,
    pub unavailable_reason: Option<String>,
    pub last_fetch_at_unix_ms: Option<u64>,
    pub measured_at_unix_ms: Option<u64>,
    pub pane_count: usize,
    pub running_agent_count: usize,
    pub disk: DiskUsageSnapshot,
    pub pull_request: Option<PullRequestSnapshot>,
    pub github: GithubStatusSnapshot,
    pub deletion_gate: WorktreeDeletionGateSnapshot,
    pub open_error: Option<String>,
    pub path: String,
    pub branch: Option<String>,
    /// Git lists the worktree but its path is not on disk.
    pub missing: bool,
    pub is_main: bool,
    pub dirty: bool,
    pub changed_file_count: u32,
    /// What the ahead/behind and line counts are measured against: the pull
    /// request's base when there is one, the repository default branch
    /// otherwise, and absent when neither could be resolved.
    pub base_branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Lines added and removed by commits on this branch since the base.
    /// Working-tree changes are deliberately excluded; the Changes view's
    /// uncommitted group is where those are counted.
    pub added_lines: u32,
    pub removed_lines: u32,
    pub unpushed: Option<UnpushedSnapshot>,
}

/// One policy shared by all worktree deletion surfaces.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WorktreeDeletionGateSnapshot {
    pub blocked_reason: Option<String>,
    pub warnings: Vec<String>,
    pub button_label: String,
    pub can_delete_branch: bool,
}

/// Shell authorization issued only after Herdr confirms every pane is gone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorktreeRemovalSnapshot {
    pub id: u64,
    pub repository_root: String,
    pub checkout_path: String,
    pub expected_head_sha: Option<String>,
    pub expected_branch: Option<String>,
    pub protected_base_branch: Option<String>,
    pub branch: Option<String>,
    pub delete_branch: bool,
    pub phase: String,
    pub message: Option<String>,
}

/// One user-initiated task whose blocking work runs outside the runtime mutex.
///
/// The shell observes this receipt to keep a sheet locked, focus a created
/// pane, or show the exact failed step. A single slot also makes a repeated
/// submission idempotent while an operation is in flight.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskOperationSnapshot {
    pub id: u64,
    pub kind: String,
    pub phase: String,
    pub repository_root: Option<String>,
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub path: Option<String>,
    pub pane_id: Option<String>,
    pub agent_kind: Option<String>,
    pub message: Option<String>,
}

/// One repository's worktrees.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ProjectWorktreesSnapshot {
    pub github: GithubStatusSnapshot,
    pub pull_requests: Vec<PullRequestSnapshot>,
    pub pull_request_window: String,
    pub cleanup: Option<crate::live::cleanup::CleanupSnapshot>,
    pub history: Option<crate::worktrees::history::GitHistorySnapshot>,
    pub shared_git_path: Option<String>,
    pub shared_git_disk: DiskUsageSnapshot,
    pub disk_total_bytes: Option<u64>,
    pub disk_confirmed_bytes: Option<u64>,
    pub linked_disk_bytes: Option<u64>,
    pub disk_unavailable_reason: Option<String>,
    pub base_branch: Option<String>,
    pub base_source: String,
    pub base_branch_fallback: Option<String>,
    pub root_path: String,
    pub default_branch: Option<String>,
    pub branches: Vec<String>,
    pub worktrees: Vec<WorktreeSnapshot>,
    /// Why this repository has no worktree list. An empty list with no reason
    /// means the repository genuinely has none.
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WorktreeCatalogSnapshot {
    pub projects: Vec<ProjectWorktreesSnapshot>,
}

impl WorktreeCatalogSnapshot {
    pub fn project(&self, root_path: &str) -> Option<&ProjectWorktreesSnapshot> {
        self.projects
            .iter()
            .find(|project| project.root_path == root_path)
    }
}

/// How much disk one checkout occupies, and which of its top-level folders is
/// the biggest share of it.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DiskUsageSnapshot {
    pub measured_at_unix_ms: Option<u64>,
    /// The checkout this measurement describes. A card whose selection has
    /// moved on compares this against its own path and shows `measuring`
    /// rather than the previous checkout's size.
    pub path: Option<String>,
    pub total_bytes: Option<u64>,
    pub largest_child_name: Option<String>,
    pub largest_child_bytes: Option<u64>,
    pub unavailable_reason: Option<String>,
}

/// The right panel's summary card for the selected checkout.
///
/// The per-checkout git facts are not repeated here: they live on
/// [`CheckoutSnapshot`], which the card reads through the focused checkout.
/// What this carries is everything the card needs that a checkout row does
/// not: the repository's `gh` health, the one measured checkout's disk usage,
/// and whether the worktree may be removed.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CheckoutCardSnapshot {
    pub inspected_checkout_path: Option<String>,
    /// Absent when nothing is selected, or when the selection is a remote
    /// checkout - remote worktree management is out of scope, so no card.
    pub checkout_id: Option<String>,
    pub github: GithubStatusSnapshot,
    pub disk: DiskUsageSnapshot,
    /// True while the selected checkout's size is still being measured.
    pub disk_measuring: bool,
    pub deletion_gate: Option<WorktreeDeletionGateSnapshot>,
    /// Current topology, not a claim about who authored HEAD or older commits.
    pub panes: Vec<CheckoutPaneContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckoutPaneContext {
    pub pane_id: String,
    pub title: String,
    pub status: String,
    pub session_id: Option<String>,
    pub parent_pane_id: Option<String>,
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

/// Carries no "last checked" stamp on purpose. Every check restamped it, the
/// stamp rides `rest`, and `rest` is compared field by field to decide whether
/// the reader is current - so a clock reading nothing renders made every
/// heartbeat re-send the whole navigator, ui state, status and pet.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProviderStatusSnapshot {
    pub state: String,
    pub socket_path: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RemoteStatusSnapshot {
    pub target_id: String,
    pub state: String,
    pub message: Option<String>,
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
                scratch: ScratchSnapshot {
                    id: crate::scratch::NODE_ID.to_owned(),
                    label: crate::scratch::LABEL.to_owned(),
                    path: crate::scratch::root().to_string_lossy().into_owned(),
                    expanded: false,
                    session_workspace_ids: Vec::new(),
                    tabs: Vec::new(),
                },
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
            pane_layouts: Vec::new(),
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
            card: CheckoutCardSnapshot::default(),
            git_worktrees: None,
            git_worktrees_loading: true,
            git_worktrees_remote: false,
            worktree_removal: None,
            task_operation: None,
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
                },
                remote: options
                    .remote_targets
                    .iter()
                    .map(|target| RemoteStatusSnapshot {
                        target_id: target.id.clone(),
                        state: "not_connected".to_owned(),
                        message: Some("Waiting for the first remote connection attempt".to_owned()),
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
    pub card: CheckoutCardSnapshot,
    pub git_worktrees: Option<ProjectWorktreesSnapshot>,
    pub git_worktrees_loading: bool,
    pub git_worktrees_remote: bool,
    pub worktree_removal: Option<WorktreeRemovalSnapshot>,
    pub task_operation: Option<TaskOperationSnapshot>,
    pub overlay: OverlaySnapshot,
    pub tab: TabSnapshot,
    pub connection: ConnectionSnapshot,
    pub zoomed: Option<String>,
    pub focused: FocusedSnapshot,
    pub pane_layouts: Vec<PaneLayoutSnapshot>,
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
            card: snapshot.card.clone(),
            git_worktrees: snapshot.git_worktrees.clone(),
            git_worktrees_loading: snapshot.git_worktrees_loading,
            git_worktrees_remote: snapshot.git_worktrees_remote,
            worktree_removal: snapshot.worktree_removal.clone(),
            task_operation: snapshot.task_operation.clone(),
            overlay: snapshot.overlay.clone(),
            tab: snapshot.tab.clone(),
            connection: snapshot.connection.clone(),
            zoomed: snapshot.zoomed.clone(),
            focused: snapshot.focused.clone(),
            pane_layouts: snapshot.pane_layouts.clone(),
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
            && self.card == snapshot.card
            && self.git_worktrees == snapshot.git_worktrees
            && self.git_worktrees_loading == snapshot.git_worktrees_loading
            && self.git_worktrees_remote == snapshot.git_worktrees_remote
            && self.worktree_removal == snapshot.worktree_removal
            && self.task_operation == snapshot.task_operation
            && self.overlay == snapshot.overlay
            && self.tab == snapshot.tab
            && self.connection == snapshot.connection
            && self.zoomed == snapshot.zoomed
            && self.focused == snapshot.focused
            && self.pane_layouts == snapshot.pane_layouts
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

/// One delta response taken from the runtime, holding everything the wire
/// needs and nothing that reaches back into the runtime.
///
/// Taking a payload is what runs under the runtime lock; serializing it is
/// what must not. The three large sections ride the reference-counted copies
/// the runtime already retains for revision stamping, so taking one copies no
/// section body - it bumps three refcounts and clones the chunks that arrived
/// since the caller's cursor.
pub struct SnapshotDeltaPayload {
    pub schema_version: u32,
    pub revision: u64,
    pub rest: Option<Arc<RestSections>>,
    pub editor: Option<Arc<EditorSnapshot>>,
    pub changes: Option<Arc<ChangesSnapshot>>,
    pub find: PaneFindSnapshot,
    pub input_generation: u64,
    pub terminal_sequence: u64,
    pub chunks: Vec<TerminalChunk>,
    pub chunks_dropped: bool,
}

/// One delta response on the snapshot wire. `rest`, `editor`, and `changes`
/// are present only when the caller's `have_revision` predates their last
/// change; `chunks` carries only sequences past the caller's cursor. The
/// changes view holds a whole file's diff text, so it is kept off `rest`,
/// which restamps whenever any agent's elapsed time ticks.
///
/// It borrows from a `SnapshotDeltaPayload`, never from the runtime, so
/// building and serializing it needs no lock.
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
    pub chunks: &'a [TerminalChunk],
    pub chunks_dropped: bool,
}

impl<'a> SnapshotDeltaWire<'a> {
    pub fn borrow(payload: &'a SnapshotDeltaPayload) -> Self {
        Self {
            schema_version: payload.schema_version,
            revision: payload.revision,
            rest: payload.rest.as_deref().map(RestWire::borrow),
            editor: payload.editor.as_deref(),
            changes: payload.changes.as_deref(),
            find: &payload.find,
            input_generation: payload.input_generation,
            terminal_sequence: payload.terminal_sequence,
            chunks: &payload.chunks,
            chunks_dropped: payload.chunks_dropped,
        }
    }
}

#[derive(Serialize)]
pub struct RestWire<'a> {
    pub navigator: &'a NavigatorSnapshot,
    pub card: &'a CheckoutCardSnapshot,
    pub git_worktrees: &'a Option<ProjectWorktreesSnapshot>,
    pub git_worktrees_loading: bool,
    pub git_worktrees_remote: bool,
    pub worktree_removal: &'a Option<WorktreeRemovalSnapshot>,
    pub task_operation: &'a Option<TaskOperationSnapshot>,
    pub overlay: &'a OverlaySnapshot,
    pub tab: &'a TabSnapshot,
    pub connection: &'a ConnectionSnapshot,
    pub zoomed: &'a Option<String>,
    pub focused: &'a FocusedSnapshot,
    pub pane_layouts: &'a [PaneLayoutSnapshot],
    pub terminal: TerminalMetaWire<'a>,
    pub ui_state: &'a UiStateSnapshot,
    pub ime: &'a ImeSnapshot,
    pub status: &'a StatusSnapshot,
    pub pet: &'a PetSnapshot,
}

impl<'a> RestWire<'a> {
    fn borrow(rest: &'a RestSections) -> Self {
        Self {
            navigator: &rest.navigator,
            card: &rest.card,
            git_worktrees: &rest.git_worktrees,
            git_worktrees_loading: rest.git_worktrees_loading,
            git_worktrees_remote: rest.git_worktrees_remote,
            worktree_removal: &rest.worktree_removal,
            task_operation: &rest.task_operation,
            overlay: &rest.overlay,
            tab: &rest.tab,
            connection: &rest.connection,
            zoomed: &rest.zoomed,
            focused: &rest.focused,
            pane_layouts: &rest.pane_layouts,
            terminal: TerminalMetaWire {
                pane_id: &rest.terminal_pane_id,
                closed: rest.terminal_closed,
                exit_code: rest.terminal_exit_code,
                panes: &rest.terminal_panes,
            },
            ui_state: &rest.ui_state,
            ime: &rest.ime,
            status: &rest.status,
            pet: &rest.pet,
        }
    }
}

#[derive(Serialize)]
pub struct TerminalMetaWire<'a> {
    pub pane_id: &'a Option<String>,
    pub closed: bool,
    pub exit_code: Option<i32>,
    pub panes: &'a [TerminalPaneSnapshot],
}
