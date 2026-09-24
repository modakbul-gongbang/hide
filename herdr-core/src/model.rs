use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CoreOptions {
    pub schema_version: u32,
    pub herdr_socket_path: Option<String>,
    #[serde(default)]
    pub herdr_bin_path: Option<String>,
    pub app_state_path: String,
    /// The folder holding `hide-host-helper` builds for devices. Absent in a
    /// shell that serves no device files (the Swift shell until S10), which
    /// leaves every device's host `unsupported` with that reason.
    #[serde(default)]
    pub host_helper_dir: Option<String>,
    /// Where the helper is installed on devices; `~/` is the device account's
    /// home. Absent means `remote::host::DEFAULT_HELPER_ROOT`.
    #[serde(default)]
    pub host_helper_root: Option<String>,
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
    pub sessions: SessionsSnapshot,
    pub changes: ChangesSnapshot,
    pub card: CheckoutCardSnapshot,
    pub git_worktrees: Option<ProjectWorktreesSnapshot>,
    pub git_worktrees_loading: bool,
    pub git_worktrees_remote: bool,
    pub worktree_removal: Option<WorktreeRemovalSnapshot>,
    pub task_operation: Option<TaskOperationSnapshot>,
    pub explorer_operation: Option<ExplorerOperationSnapshot>,
    pub find: PaneFindSnapshot,
    pub ui_state: UiStateSnapshot,
    pub ime: ImeSnapshot,
    pub input_generation: u64,
    pub status: StatusSnapshot,
    pub pet: PetSnapshot,
    pub recent_closed: RecentClosedSnapshot,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RecentClosedSnapshot {
    pub count: usize,
    pub top_label: Option<String>,
    pub restoring: bool,
    pub notices: Vec<RecentClosedNoticeSnapshot>,
    /// Close reservations whose topology is not confirmed yet. These are
    /// deliberately separate from `count`: a pending close must not evict a
    /// confirmed undo entry or become reopenable by accident.
    pub pending: Vec<RecentClosedPendingSnapshot>,
    pub can_reopen: bool,
    pub reopen_blocked_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RecentClosedNoticeSnapshot {
    pub pane_id: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RecentClosedPendingSnapshot {
    pub key: String,
    pub target_id: String,
    pub label: String,
    pub phase: String,
    pub checking: bool,
    pub message: Option<String>,
    pub retryable: bool,
}

/// A core-owned asynchronous mutation. The shell renders this record in the
/// existing request-scoped affordance for the subject and never infers a
/// server confirmation from the transport response alone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AsyncOperationSnapshot {
    pub id: String,
    pub kind: String,
    pub target_id: String,
    pub scope_id: String,
    pub phase: String,
    pub stage: String,
    pub started_at_unix_ms: u64,
    pub deadline_at_unix_ms: Option<u64>,
    pub message: Option<String>,
    pub retryable: bool,
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
    /// In-process subagents Hide's hook reported as working, summed over the
    /// agents on an answering server. It is the one count the hook can vouch
    /// for; an uninstrumented session contributes nothing, not a zero.
    pub subagents_active: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PetOriginSnapshot {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NavigatorSnapshot {
    pub root_path: Option<String>,
    /// The focused local checkout's History scope. A registered subfolder may
    /// be narrower than the Git checkout used by Explorer and editor tabs.
    pub changes_root_path: Option<String>,
    pub focused_device_id: Option<String>,
    pub focused_workspace_id: Option<String>,
    pub focused_checkout_id: Option<String>,
    pub devices: Vec<DeviceSnapshot>,
    pub workspaces: Vec<WorkspaceSnapshot>,
    /// Project rows the core grouped at the bottom of each device's Projects
    /// list. `workspaces` remains the one authoritative row collection so
    /// search, focus, and project navigation never lose a folded project.
    pub inactive_projects: Vec<InactiveProjectGroupSnapshot>,
    pub agents: Vec<SidebarAgentSnapshot>,
    pub provider_usage: Vec<ProviderUsageSnapshot>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct InactiveProjectGroupSnapshot {
    pub device_id: String,
    pub expanded: bool,
    /// IDs in the same recent-activity order as `NavigatorSnapshot.workspaces`.
    pub project_ids: Vec<String>,
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
    pub last_success_at_unix_ms: Option<u64>,
    pub last_error_kind: Option<String>,
    pub buckets: Vec<ProviderUsageBucketSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProviderUsageBucketSnapshot {
    pub label: String,
    pub state: String,
    pub used_percent: Option<f64>,
    pub resets_at_unix_seconds: Option<u64>,
    pub message: Option<String>,
}

impl ProviderUsageSnapshot {
    pub fn initial_rows() -> Vec<Self> {
        vec![
            Self::loading("claude", "Claude Code"),
            Self::loading("codex", "Codex"),
        ]
    }

    pub fn loading(provider: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            label: label.into(),
            window_minutes: 10_080,
            state: "loading".to_owned(),
            used_percent: None,
            resets_at_unix_seconds: None,
            message: Some("Checking usage…".to_owned()),
            last_checked_at_unix_ms: None,
            last_success_at_unix_ms: None,
            last_error_kind: None,
            buckets: Vec::new(),
        }
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
            last_success_at_unix_ms: None,
            last_error_kind: None,
            buckets: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeviceSnapshot {
    pub id: String,
    pub label: String,
    pub kind: String,
    /// `local` for this Mac; for an SSH device `ready`, `unavailable` or
    /// `disabled`, read off its remote status.
    pub state: String,
    /// Why an SSH device is not `ready`, in the words its remote status
    /// carries; `None` while it is.
    pub message: Option<String>,
    pub ssh_alias: Option<String>,
    /// The Herdr socket the registration names on the device, if any.
    pub herdr_socket_path: Option<String>,
    pub agent_count: u32,
    /// The last connection test the operator asked for, or the one running.
    pub test: Option<DeviceTestSnapshot>,
    /// Where file and Git work for this device runs, and whether it may.
    pub host: DeviceHostSnapshot,
}

/// A device's file host as the operator reads it in Settings and wherever a
/// file or Git action needs it (PRD S5.5 B36, B50-B52).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DeviceHostSnapshot {
    /// `this_machine` for the daemon's own machine; for a device `none`,
    /// `granted`, or `outdated` when the consent covers an older scope.
    pub consent: String,
    /// The install root the consent names or would name. On this machine's
    /// row it is the root a new device consent would name, so the add form
    /// can say where the helper goes before the operator agrees.
    pub helper_root: Option<String>,
    pub contract: u32,
    /// `user@host:port (SHA256:...)` the consent is bound to, once bound.
    pub bound_identity: Option<String>,
    pub granted_at_unix_ms: Option<u64>,
    /// `ready`, `connecting`, `not_allowed`, `identity_changed`,
    /// `unsupported` (this build cannot serve the device) or `unavailable`.
    pub state: String,
    pub message: Option<String>,
    /// `macos aarch64` as the helper reported it.
    pub platform: Option<String>,
    pub helper_path: Option<String>,
}

/// One staged connection test of an SSH device: SSH, authentication, Herdr,
/// protocol, PTY, SFTP and Git, each with the host's answer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeviceTestSnapshot {
    /// `running`, `passed` or `failed`.
    pub state: String,
    pub checked_at_unix_ms: Option<u64>,
    pub stages: Vec<DeviceTestStageSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeviceTestStageSnapshot {
    pub stage: String,
    /// `pending`, `passed` or `failed`.
    pub state: String,
    pub detail: String,
}

/// An agent's state on four independent lifecycle axes, plus the values the shell draws
/// from them.
///
/// The axes answer four different questions that a single flat state string
/// used to mix: what the agent needs from the operator (`demand`), whether it
/// is running (`activity`), whether it has reported a completion (`completed`),
/// and whether the operator has looked at it since it last changed (`unread`).
/// Everything below `unread` is derived here so the shell only draws (design
/// rule 4).
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
    /// Herdr's `done` and `idle` are the same activity. Completion and Hide's
    /// pane-level read state remain separate axes.
    pub activity: String,
    /// Whether Herdr or the label plugin reported a completed turn. A newly
    /// opened agent can be stopped while it waits for its first instruction;
    /// that ready state is not a completion and must not appear as Done.
    #[serde(skip_serializing)]
    pub completed: bool,
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
    /// Derived: the activity evidence is incomplete, so a destructive close
    /// must wait for a fresh status rather than assuming the pane is idle.
    pub requires_close_status_check: bool,
    /// The title every surface calls this agent by; `sidebar.rs` owns the
    /// ladder that picks it (PRD D-01).
    pub identity_label: String,
    /// The label plugin's one-line progress sentence.
    #[serde(skip_serializing)]
    pub progress: Option<String>,
    /// The one action the operator is being asked for, at most 40 characters.
    #[serde(skip_serializing)]
    pub expected_reply: Option<String>,
    /// Derived: the row's second line, chosen by the group from the three
    /// sentences above (PRD D-06). `None` draws no sentence.
    pub detail: Option<String>,
    /// Derived: whether the row draws its status word. It leaves working and
    /// read rows, where the mark already says it, and stays on rows that
    /// still concern the operator.
    pub status_word_visible: bool,
    pub elapsed: String,
    /// The ordering key: the label plugin's activity timestamp when it has one,
    /// otherwise Herdr's state change sequence zero-padded to the same width.
    pub last_activity: String,
    /// Herdr's own state change sequence, one of the three inputs to a pane's
    /// read record.
    #[serde(skip_serializing)]
    pub state_change_seq: Option<u64>,
    /// The conversation id this agent is running, kept only when Herdr recorded
    /// the session as an id. A session recorded as a path is dropped here,
    /// because neither agent's fork command takes one.
    #[serde(skip_serializing)]
    pub session_id: Option<String>,
    /// The pane this agent was spawned from: Herdr's own lineage record, or
    /// the `parent_pane` token its spawner declared when Herdr recorded none.
    /// `wire.rs::lineage_parent` is the one place that resolves the two.
    #[serde(skip_serializing)]
    pub spawned_from_pane_id: Option<String>,
    /// Ownership, derived from the lineage alone: a root is the operator's own
    /// work, a descendant is work the root delegated. It is the fourth derived
    /// axis beside demand, activity and read, and it is advice rather than a
    /// boundary - a delegated pane is still selectable and still takes input,
    /// because the operator has to be able to read a child's blocked prompt
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
    /// What every live descendant of this row is doing, counted by state, for
    /// the badge the row wears while its descendants are folded away. It is
    /// derived on the lineage pass and counts all descendants, not only the
    /// direct children, because a grandchild's question is still this row's
    /// to answer (PRD B3, B4, D-05).
    pub descendant_counts: DescendantCountsSnapshot,
    /// The demands and completions of this row's live descendants, keyed by
    /// the descendant pane. It is what the read record compares against, so
    /// a descendant asking or finishing turns this row unread the same way
    /// its own state change would (PRD B5, B6, D-03).
    #[serde(skip_serializing)]
    pub descendant_signals: BTreeSet<DescendantSignal>,
    /// Tree-only presentation. The canonical agent list and its read axes stay flat.
    pub lineage_depth: usize,
    pub lineage_child_pane_ids: Vec<String>,
    pub lineage_root_checkout_id: Option<String>,
    pub lineage_worktree_badge: Option<String>,
    pub lineage_orphan: bool,
    pub lineage_hint: Option<String>,
    pub raised_hint: Option<String>,
    /// The pane the hint points at, when that pane still holds an agent Hide
    /// can name. It is what makes the line something to follow rather than
    /// something to read.
    pub spawn_origin_pane_id: Option<String>,
    pub lineage_collapsed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WorkspaceSnapshot {
    pub home_issues: crate::issues::ProjectIssuesSnapshot,
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
    /// and the persisted focus in place. Commands that need any workspace use
    /// the first entry; purpose projection and persistence use the last
    /// checkout occupant because that is the token whose value wins. An empty
    /// list means Herdr has none here yet. Commands may reuse an entry only
    /// while no other project also contains it.
    pub session_workspace_ids: Vec<String>,
    /// The newest activity anywhere in this project, in Unix milliseconds:
    /// the latest of its checkouts' last commit and its agents' last activity,
    /// the same signal `project_context::sort_projects` orders the list by.
    /// Absent when the project has neither, so the row can leave its time
    /// blank instead of claiming the epoch. An additive snapshot field: the
    /// shell reads it when present and keeps its previous behavior when not.
    #[serde(default)]
    pub last_activity_unix_ms: Option<u64>,
    /// Whether the operator pinned this project's registration. A pinned
    /// project sorts before its device's unpinned ones and is exempt from the
    /// device's inactive fold; the shell draws the pinned rows under their own
    /// `Pinned` section. It is the registration's flag carried onto the row
    /// (D-07), so an unregistered workspace is never pinned.
    #[serde(default)]
    pub pinned: bool,
    pub checkouts: Vec<CheckoutSnapshot>,
    /// Checkout rows grouped after the active rows in this project. The full
    /// rows stay in `checkouts`, which remains the authority for focus,
    /// search, tab state, and every non-sidebar consumer.
    pub inactive_checkouts: InactiveCheckoutGroupSnapshot,
    /// What `Remove project…` would close, counted by the core so the
    /// confirmation names the same panes the close will send to Herdr
    /// (D-10). Both are zero for a project Herdr has no pane in, which is
    /// the registration-only removal.
    #[serde(default)]
    pub removal: WorkspaceRemovalGateSnapshot,
}

/// The counts the project removal confirmation reads (D-10).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WorkspaceRemovalGateSnapshot {
    pub pane_count: usize,
    /// Panes whose agent is currently working, the same definition
    /// `WorktreeDeletionGateSnapshot` warns with.
    pub running_agent_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct InactiveCheckoutGroupSnapshot {
    pub expanded: bool,
    /// IDs in the same recent-activity order as `WorkspaceSnapshot.checkouts`.
    pub checkout_ids: Vec<String>,
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

/// The source of the one-line checkout purpose chosen by the core.
///
/// The shell uses this only for presentation tone. Resolution stays here so
/// every surface reads the same fallback order instead of reconstructing it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckoutPurposeOrigin {
    Token,
    BranchDescription,
    AgentTitle,
    PullRequestTitle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckoutPurposeSnapshot {
    pub text: String,
    pub origin: CheckoutPurposeOrigin,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CheckoutSnapshot {
    pub issue: Option<crate::issues::IssueLinkSnapshot>,
    #[serde(skip_serializing)]
    pub branch_issue: Option<String>,
    pub github: GithubStatusSnapshot,
    pub agent_summary: CheckoutAgentSummary,
    pub id: String,
    pub workspace_id: String,
    pub label: String,
    pub path: String,
    pub branch: Option<String>,
    /// One line chosen in this order: Herdr workspace token, branch
    /// description, representative agent title, pull-request title.
    pub purpose: Option<CheckoutPurposeSnapshot>,
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
    /// Whether this editor entry is the checkout's replaceable preview tab,
    /// which the strip titles in italic. A Herdr entry is never one. It rides
    /// the strip rather than only the editor tab because promotion and
    /// replacement change the slot itself, and both happen once per tab, not
    /// per keystroke.
    pub preview: bool,
    /// The one agent this tab holds, when it holds exactly one, drawn as the
    /// tab's identity where a tab is named: the Recent Panels switcher shows
    /// its name and mark instead of the Herdr label, which for an unnamed tab
    /// is only a number (PRD D-16). A shell-only tab and a tab with several
    /// agents carry `None` and keep their label.
    pub agent_identity: Option<AgentChipSnapshot>,
}

/// The kinds of tab a strip holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StripTabKind {
    Herdr,
    File,
    Diff,
    Session,
    Memory,
}

impl StripTabSnapshot {
    pub fn herdr(source_id: impl Into<String>, label: impl Into<String>) -> Self {
        let source_id = source_id.into();
        Self {
            id: format!("herdr:{source_id}"),
            kind: StripTabKind::Herdr,
            source_id,
            label: label.into(),
            preview: false,
            agent_identity: None,
        }
    }

    pub fn file(source_id: impl Into<String>, label: impl Into<String>, preview: bool) -> Self {
        let source_id = source_id.into();
        Self {
            id: format!("file:{source_id}"),
            kind: StripTabKind::File,
            source_id,
            label: label.into(),
            preview,
            agent_identity: None,
        }
    }

    pub fn diff(source_id: impl Into<String>, label: impl Into<String>, preview: bool) -> Self {
        let source_id = source_id.into();
        Self {
            id: format!("diff:{source_id}"),
            kind: StripTabKind::Diff,
            source_id,
            label: label.into(),
            preview,
            agent_identity: None,
        }
    }

    pub fn session(source_id: impl Into<String>, label: impl Into<String>, preview: bool) -> Self {
        let source_id = source_id.into();
        Self {
            id: format!("session:{source_id}"),
            kind: StripTabKind::Session,
            source_id,
            label: label.into(),
            preview,
            agent_identity: None,
        }
    }

    pub fn memory(source_id: impl Into<String>, label: impl Into<String>, preview: bool) -> Self {
        let source_id = source_id.into();
        Self {
            id: format!("memory:{source_id}"),
            kind: StripTabKind::Memory,
            source_id,
            label: label.into(),
            preview,
            agent_identity: None,
        }
    }

    /// The strip entry an editor tab stands behind, so a replaced preview tab
    /// can hand its slot to the tab that took its place.
    pub fn editor(tab: &EditorTabSnapshot) -> Self {
        match tab.kind {
            EditorTabKind::File => Self::file(tab.id.clone(), tab.label.clone(), tab.preview),
            EditorTabKind::Diff => Self::diff(tab.id.clone(), tab.label.clone(), tab.preview),
            EditorTabKind::Session => Self::session(tab.id.clone(), tab.label.clone(), tab.preview),
            EditorTabKind::Memory => Self::memory(tab.id.clone(), tab.label.clone(), tab.preview),
        }
    }

    /// The Herdr half of a strip, in the order the tabs are given.
    ///
    /// A tab with no id has no identity to key a strip entry by, so it is
    /// dropped rather than given a placeholder. Both the local strip and the
    /// remote projection build their Herdr entries here, so the rule that
    /// decides which tabs earn a slot and what an unlabelled one reads as has
    /// one implementation to change.
    /// The Herdr tabs a checkout draws in its strip.
    ///
    /// A tab holding only delegated children is left out: the canvas keeps
    /// one pane, and a strip slot for every child would put the pile back
    /// where the split used to be (PRD B1).
    pub fn from_herdr_tabs(tabs: &[TabSnapshot]) -> Vec<Self> {
        tabs.iter()
            .filter(|tab| !tab.delegated)
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
    /// Every agent in this tab is somebody else's delegated child, so the tab
    /// exists only to hold work the operator did not ask to look at. The tab
    /// strip leaves it out; the sidebar and the breadcrumb still reach it
    /// (PRD B1, B4, D-40).
    pub delegated: bool,
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
    /// Whether the core needs a fresh activity status before allowing a close.
    pub requires_close_status_check: bool,
    /// The agent's title, from the same ladder the sidebar row shows, so the
    /// header and the row cannot call one pane two things (PRD D-09).
    pub identity_label: Option<String>,
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
    /// The reason's stable name. The Settings diagnosis reads it to list the
    /// panes whose session predates the install, so that judgement is made
    /// once here rather than by matching a sentence on two screens (PRD B27,
    /// D-61).
    pub uninstrumented_code: Option<String>,
    /// One chip per pane child, in the lineage's own child order. Only panes:
    /// an in-process subagent has no pane, so it cannot be a chip the
    /// operator clicks into (PRD D-63).
    pub chips: Vec<AgentChipSnapshot>,
    /// The parent badge, chosen from the pane children by the same priority
    /// the Workspace summary uses. In-process subagents take no part in it.
    pub representative: Option<AgentChipSnapshot>,
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
            uninstrumented_code: Some(reason.code().to_owned()),
            ..Self::default()
        }
    }
}

/// One agent in a line of them: a pane header chip, a breadcrumb step's
/// sibling, or an Overview worktree row's agent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgentChipSnapshot {
    pub pane_id: String,
    /// The short name the chip shows beside its mark.
    pub label: String,
    /// The row's second line: the sentence the group chose, or nothing.
    pub detail: Option<String>,
    /// Whether the status word is drawn beside that sentence.
    pub status_word_visible: bool,
    pub agent_kind: String,
    pub demand: String,
    pub activity: String,
    pub emphasized: bool,
    pub symbol: String,
    pub status_label: String,
    /// Whether this child is still delegated work. It lifts only when the
    /// child's parent is gone and the child is a root again.
    pub delegated: bool,
}

/// How many live descendants of a row are in each state the badge draws.
///
/// Every count is a real count over rows the projection holds; a descendant
/// whose activity Herdr reports as unknown is in none of them, because a
/// badge that cannot say what a child is doing has nothing to claim, and the
/// exclusion goes to the diagnostic log instead (PRD B7).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DescendantCountsSnapshot {
    pub error: u32,
    pub approval: u32,
    pub question: u32,
    pub working: u32,
    pub done: u32,
    /// Descendants Herdr cannot classify. Not drawn; the log carries it.
    #[serde(skip_serializing)]
    pub unknown: u32,
}

impl DescendantCountsSnapshot {
    /// Whether any drawn count is above zero, so a row with descendants that
    /// are all merely ready wears no badge rather than an empty one.
    pub fn any_drawn(&self) -> bool {
        self.error + self.approval + self.question + self.working + self.done > 0
    }
}

/// One thing a descendant is doing that its ancestors are told about: an
/// outstanding demand, or a completion.
///
/// It is the unit the read record keeps per ancestor. A signal that appears
/// turns the ancestor unread; a signal that goes away does not, because a
/// question being answered or a finished child starting new work is not
/// news the operator has to act on (PRD B5, B6).
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct DescendantSignal {
    pub pane_id: String,
    /// `question`, `approval`, `error` or `completed`.
    pub kind: String,
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
    pub siblings: Vec<AgentChipSnapshot>,
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

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct TerminalInputSent {
    pub id: u64,
    pub milliseconds: f64,
    pub outcome: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
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
    pub archive_detail: Option<ArchiveDetailSnapshot>,
    /// Files being read on a device before their tabs can show; one per
    /// file, in the order they were asked for.
    pub opening: Vec<EditorOpeningSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EditorOpeningSnapshot {
    pub workspace_id: String,
    pub checkout_id: String,
    pub path: String,
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
    /// Whether a Markdown file tab draws its formatting in place (Live) or
    /// shows the source editor. Per tab; a reopened tab starts Live.
    pub markdown_live: bool,
    pub wrap: bool,
    pub dirty: bool,
    /// The checkout's one replaceable preview tab (VS Code's model): opened by
    /// a single click, replaced in place by the next single click, and
    /// promoted to an ordinary tab by a double-click, the first edit, Keep
    /// Open, or a drag. A dirty tab is never replaced. Editor tabs are
    /// ephemeral, so this is never persisted.
    pub preview: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorTabKind {
    File,
    Diff,
    Session,
    Memory,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArchiveDetailSnapshot {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub provider: Option<String>,
    pub unavailable_reason: Option<String>,
    pub events: Vec<ArchiveEventSnapshot>,
    pub memory: Option<MemoryDetailSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArchiveEventSnapshot {
    pub role: String,
    pub kind: String,
    pub at_unix_ms: u64,
    pub text: String,
    pub memory_attached_count: Option<usize>,
    pub memory_attached_item_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MemoryDetailSnapshot {
    pub id: String,
    pub body: String,
    pub lifecycle: String,
    pub revision: u64,
    pub source_count: usize,
    pub provided_session_count: usize,
    pub learned_at_unix_ms: u64,
    pub conflict_existing_id: Option<String>,
    pub conflict_candidate_id: Option<String>,
    pub sources: Vec<MemorySourceSnapshot>,
    pub revisions: Vec<MemoryRevisionSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MemorySourceSnapshot {
    pub provider: String,
    pub session_id: String,
    pub event_offset: u64,
    pub available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MemoryRevisionSnapshot {
    pub revision: u64,
    pub body: String,
    pub lifecycle: String,
    pub created_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionsMode {
    #[default]
    Sessions,
    Memory,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionsProviderFilter {
    #[default]
    All,
    Codex,
    Claude,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SessionsSnapshot {
    pub project_id: Option<String>,
    pub checkout_path: Option<String>,
    pub mode: SessionsMode,
    pub provider_filter: SessionsProviderFilter,
    pub query: String,
    pub loading: bool,
    pub unavailable_reason: Option<String>,
    pub rows: Vec<SessionRowSnapshot>,
    pub total_session_count: usize,
    pub memories: Vec<MemoryRowSnapshot>,
    pub memory_enabled: bool,
    pub memory_disclosure_accepted: bool,
    pub memory_active_count: usize,
    pub memory_conflict_count: usize,
    pub memory_capacity_reached: bool,
    pub analysis: MemoryAnalysisSnapshot,
    pub notice: Option<MemoryNoticeSnapshot>,
    pub this_turn_memory_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionRowSnapshot {
    pub id: String,
    pub provider: String,
    pub provider_label: String,
    pub locator: String,
    pub checkout_path: String,
    pub first_human_request: Option<String>,
    pub started_at_unix_ms: Option<u64>,
    pub updated_at_unix_ms: u64,
    pub title: Option<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MemoryRowSnapshot {
    pub id: String,
    pub body: String,
    pub lifecycle: String,
    pub revision: u64,
    pub source_count: usize,
    pub provided_session_count: usize,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct MemoryAnalysisSnapshot {
    pub state: String,
    pub discovered: usize,
    pub analyzed: usize,
    pub failed: usize,
    pub message: Option<String>,
    pub action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MemoryNoticeSnapshot {
    pub message: String,
    pub undo_batch_id: Option<String>,
}

/// What kind of document an open file is, decided once by the host that
/// read it (`hide_host::document`) and drawn by the shell as one view per
/// kind. Adding a kind is one variant there and one case in the shell's
/// switch; nothing else in the shell inspects extensions or bytes.
pub use hide_host::document::DocumentKind;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditorDocumentSnapshot {
    pub path: String,
    pub language: Option<String>,
    pub document_kind: DocumentKind,
    pub contents_utf8: Option<String>,
    pub opened_modified_at_unix_ms: Option<u64>,
    /// The content revision (`sha256:<hex>`) the draft is based on: read at
    /// open, moved by each save, and what the next save is checked against.
    /// Present exactly for an editable document.
    pub revision: Option<String>,
    pub dirty: bool,
    /// Why an otherwise editable document takes no edits: its size or its
    /// permissions. The kind, not this field, says a PDF or image is read-only.
    pub readonly_reason: Option<String>,
    pub conflict: Option<EditorConflictSnapshot>,
    /// A save that has not come to a known result yet.
    pub save: Option<EditorSaveSnapshot>,
}

/// A save in progress, or one whose answer was lost (PRD S5.5 B14).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EditorSaveSnapshot {
    /// `saving` while it runs (a newer draft may wait behind it), `waiting`
    /// while it waits for the device's helper to finish connecting, `unknown`
    /// when the answer was lost and the file has not been read back yet, and
    /// `checking` while it is read back. An unknown save blocks the next one.
    pub state: String,
    pub message: Option<String>,
}

/// The right panel's four persisted sections.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RightPanelSection {
    #[default]
    #[serde(alias = "git")]
    Overview,
    Explorer,
    Changes,
    Sessions,
}

impl RightPanelSection {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "overview" | "git" => Some(Self::Overview),
            "explorer" => Some(Self::Explorer),
            "changes" => Some(Self::Changes),
            "sessions" => Some(Self::Sessions),
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
    /// Which of the right panel's four sections is showing. Persisted rather
    /// than held in the view, because hiding the panel tears the view down
    /// and the section has to come back the way it was left.
    #[serde(default)]
    pub right_panel_section: RightPanelSection,
    #[serde(default)]
    pub sessions_mode_by_project: BTreeMap<String, SessionsMode>,
    pub expanded_paths: Vec<String>,
    #[serde(default)]
    pub collapsed_workspace_ids: Vec<String>,
    /// Sidebar workspaces (checkout paths) whose agent rows are hidden.
    #[serde(default)]
    pub collapsed_checkout_ids: Vec<String>,
    /// Projects whose Inactive checkout group the operator opened. Absence is
    /// the default collapsed state, so old stores need no migration.
    #[serde(default)]
    pub expanded_inactive_checkout_project_paths: Vec<String>,
    /// Device groups whose Inactive projects group the operator opened.
    /// Absence is the default collapsed state.
    #[serde(default)]
    pub expanded_inactive_project_device_ids: Vec<String>,
    #[serde(default)]
    pub project_base_branches: BTreeMap<String, String>,
    /// Agent panes whose descendants the operator opened in the sidebar tree.
    /// Absence is the default folded state, so a row with children starts
    /// folded and shows its descendant badge until the operator opens it.
    /// The set is keyed by pane id and lives as long as the pane does: a
    /// Herdr restart mints new ids, so it starts folded again (PRD D-06,
    /// D-11).
    #[serde(default)]
    pub expanded_agent_pane_ids: Vec<String>,
    pub selected_path: Option<String>,
    pub selected_pane_id: Option<String>,
    pub shortcut_bindings: BTreeMap<String, String>,
    /// The web shell's own pane chords, command id to chord. They live apart
    /// from `shortcut_bindings` because the two hosts reserve different keys
    /// (Chrome keeps ⌘W), so one host's rebind must never become the other's.
    #[serde(default)]
    pub browser_shortcut_bindings: BTreeMap<String, String>,
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
    /// Eligible agent panes whose terminal is replaced by the local
    /// conversation ledger. This is snapshot-only interaction state;
    /// persistence owns a separate stored representation and intentionally
    /// omits this set. A pane enters it only through `toggle_conversation`:
    /// a new agent pane opens on its terminal, and a pane that leaves the
    /// session leaves the set with it.
    #[serde(default)]
    pub conversation_pane_ids: BTreeSet<String>,
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
}

/// One pane's read mark: the state the operator was looking at the last time
/// the pane held keyboard focus.
///
/// A pane is unread when its current state does not match this record, so a
/// missing record means unread. The session id distinguishes a new agent in a
/// reused pane from the same agent restored by a new Herdr server.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneReadRecord {
    #[serde(default)]
    pub state_change_seq: Option<u64>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub demand: String,
    pub activity: String,
    /// Part of the pane-level fingerprint so an idle-to-completed transition
    /// becomes unread even when Herdr's process-local sequence does not move.
    #[serde(default)]
    pub completed: bool,
    /// The descendant demands and completions the operator had seen when this
    /// pane was last read. A descendant signal missing from here is news and
    /// keeps the pane unread; one that has since gone away is trimmed on the
    /// next projection, so the same descendant can be news again after it
    /// works and finishes a second time (PRD B5, B6, B8).
    #[serde(default)]
    pub descendant_signals: BTreeSet<DescendantSignal>,
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
            sessions_mode_by_project: BTreeMap::new(),
            expanded_paths: Vec::new(),
            collapsed_workspace_ids: Vec::new(),
            collapsed_checkout_ids: Vec::new(),
            expanded_inactive_checkout_project_paths: Vec::new(),
            expanded_inactive_project_device_ids: Vec::new(),
            project_base_branches: BTreeMap::new(),
            expanded_agent_pane_ids: Vec::new(),
            selected_path: None,
            selected_pane_id: None,
            shortcut_bindings: BTreeMap::new(),
            browser_shortcut_bindings: BTreeMap::new(),
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
            conversation_pane_ids: BTreeSet::new(),
            pane_read_records: BTreeMap::new(),
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
    /// Absent in a store written before projects could be pinned, which
    /// loads as unpinned without a warning (D-07). Removing the registration
    /// takes the pin with it; nothing else about the pin is stored.
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceRegistration {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub ssh_alias: Option<String>,
    /// The Herdr socket on the device, for a host whose server does not
    /// listen at its default path; absent reads the host's default server.
    #[serde(default)]
    pub herdr_socket_path: Option<String>,
    /// The operator's one consent for Hide's helper on this device (PRD S5.5
    /// D-20, D-23). Absent until given; a device registered before consent
    /// existed asks before its first file or Git use.
    #[serde(default)]
    pub host_consent: Option<HostConsent>,
}

/// What the operator allowed on a device: install and update Hide's helper
/// under `helper_root`, run it only for the life of an SSH connection, and
/// perform file and Git work inside registered checkouts, with trash moves and
/// worktree removals still confirmed one by one. `contract` names that scope;
/// a build whose scope differs asks again, and so does a device that answers
/// with another identity than the one the consent was first used on.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostConsent {
    pub contract: u32,
    pub helper_root: String,
    pub granted_at_unix_ms: u64,
    /// The account, address and host key the helper first ran on; bound on
    /// the first connection after consent and never rewritten by one.
    #[serde(default)]
    pub identity: Option<HostIdentity>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostIdentity {
    pub user: String,
    pub hostname: String,
    pub port: u16,
    pub host_key_sha256: String,
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
    /// The revision the draft was based on.
    pub opened_revision: String,
    /// What the file holds now; absent when it was removed or could not be
    /// read.
    pub disk_revision: Option<String>,
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
    Renamed,
    Conflict,
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
        if matches!(code, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU") {
            return Self::Conflict;
        }
        if index == '?' && worktree == '?' {
            return Self::Untracked;
        }
        if index == 'R' || worktree == 'R' {
            return Self::Renamed;
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
            Self::Renamed => "renamed",
            Self::Conflict => "conflict",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChangedFileSnapshot {
    /// Absolute, so activating a row needs no second join against the root.
    pub path: String,
    /// Relative to the checkout root, which is what the row shows.
    pub relative_path: String,
    /// The source side of a rename, relative to the checkout root. Absent for
    /// every other status. The destination remains `relative_path`, so
    /// opening a row always addresses the file that exists now.
    pub previous_relative_path: Option<String>,
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
    pub closing_issues: Vec<crate::issues::IssueReference>,
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
    /// Successful component payloads, including a successful empty answer.
    /// Internal reader provenance, not a snapshot wire field.
    #[serde(skip)]
    pub pull_requests_read: bool,
    #[serde(skip)]
    pub issues_read: bool,
    pub issues: crate::issues::ProjectIssuesSnapshot,
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
    /// Commits the upstream has that this branch does not, as of the last
    /// fetch. Absent when the branch has no upstream, the upstream is gone,
    /// or the count could not be read; `upstream_state` says which, so the
    /// Overview draws no cell, `?`, or a number rather than a silent zero.
    pub behind_upstream: Option<u32>,
    /// When a linked worktree was added: the creation time of its
    /// `.git/worktrees/<name>` entry. The main worktree has none, and it
    /// leads the Overview's list regardless.
    pub created_at_unix_ms: Option<u64>,
    pub unavailable_reason: Option<String>,
    pub last_fetch_at_unix_ms: Option<u64>,
    pub measured_at_unix_ms: Option<u64>,
    pub pane_count: usize,
    pub running_agent_count: usize,
    /// Who is working in this worktree and on what (PRD B34, B35, D-32,
    /// D-55). Overview's own value is width, so this is one line rather than
    /// a new area.
    pub agent_line: WorktreeAgentLineSnapshot,
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

/// The Overview worktree row's agent line.
///
/// An empty `agents` with no reason is a worktree nobody is working in. An
/// empty one carrying a reason is a worktree Hide cannot see into, which is a
/// different answer and is drawn as one (PRD B35, D-60, `design/principles.md`
/// rule 9).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WorktreeAgentLineSnapshot {
    /// The agents attached to this worktree's panes, in the sidebar's own
    /// order so the two screens name them the same way.
    pub agents: Vec<AgentChipSnapshot>,
    /// The same mark and sentence the pane header shows, when one of those
    /// agents is uninstrumented. The reason is the first in the resolution
    /// order among them, so the line agrees with the pane it came from.
    pub uninstrumented_reason: Option<String>,
    pub uninstrumented_label: Option<String>,
    pub uninstrumented_code: Option<String>,
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
    /// How starting `agent_kind` in the created pane went, kept apart from
    /// the creation itself: `starting`, `started`, `failed`, or `unknown`
    /// when Herdr did not answer and the pane has to be looked at. `None`
    /// when the task started no agent.
    pub agent_phase: Option<String>,
    pub agent_message: Option<String>,
}

/// The explorer's most recent filesystem change and how far it got.
///
/// One slot, like `TaskOperationSnapshot`: the shell reads the finished
/// phase to reload the parents of `path` and `destination`, and the failed
/// phase to draw `message` under the row the change started from. A new
/// request replaces a settled slot; a request while one is working is
/// refused, so a double-click cannot run the same rename twice.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExplorerOperationSnapshot {
    pub id: u64,
    pub kind: String,
    pub phase: String,
    /// The path the change started from: the item being renamed or moved,
    /// or the path a new item takes.
    pub path: String,
    /// The item's path once the change has landed; equal to `path` for a
    /// creation.
    pub destination: String,
    pub message: Option<String>,
}

/// One repository's worktrees.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ProjectWorktreesSnapshot {
    pub github: GithubStatusSnapshot,
    pub pull_requests: Vec<PullRequestSnapshot>,
    pub pull_request_window: String,
    pub cleanup: Option<crate::live::cleanup::CleanupSnapshot>,
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
    pub agent_hooks: AgentHooksSnapshot,
    pub background_ai: BackgroundAiSnapshot,
    pub diagnostics: Vec<DiagnosticSnapshot>,
    pub last_error: Option<LastErrorSnapshot>,
    /// Core-owned operations which are waiting for a transport result or an
    /// authoritative Herdr event. Keeping these beside status lets every
    /// surface show the same bounded, target-scoped state.
    pub async_operations: Vec<AsyncOperationSnapshot>,
    /// The core-owned outcome of the latest explicitly correlated pane-focus
    /// request. Ordinary focus events have no request id and do not replace
    /// this receipt, so a relationship control never mistakes another pane's
    /// error or an older focused layout for its own answer.
    pub pane_focus_request: Option<PaneFocusRequestSnapshot>,
}

/// One explicitly correlated pane-focus request and its core-owned outcome.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PaneFocusRequestSnapshot {
    pub request_id: String,
    pub target_pane_id: String,
    pub phase: String,
    pub message: Option<String>,
    pub retryable: bool,
}

/// Which agent and model the background AI features use, and what each
/// provider can do about it right now.
///
/// The choice is the operator's, stored by `hide-ai` in its own file; the
/// availability and the model lists come from asking the providers, on the
/// coordinator's reader, never under the runtime mutex.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BackgroundAiSnapshot {
    /// The provider a background request runs on first. It is the operator's
    /// choice when they have made one and the default otherwise.
    pub provider: String,
    /// Whether `provider` is a saved choice rather than the default.
    pub chosen: bool,
    /// One row per provider Hide can route to, in the offered order. A
    /// provider that is not on this Mac is still a row, because "not here"
    /// and "not signed in" are different answers.
    pub providers: Vec<BackgroundAiProviderSnapshot>,
    /// Why the saved choice could not be read or written. The defaults are in
    /// use while this is set; it is never left empty to stand for success.
    pub unavailable_reason: Option<String>,
}

impl BackgroundAiSnapshot {
    /// Every provider, none of them asked yet. This is what the screen shows
    /// before the first read lands, and what an unobserved read answers. The
    /// choice it names is the default, because nothing has been read that
    /// could have changed it.
    pub fn unread() -> Self {
        Self {
            provider: hide_ai::AiSettings::default().provider.as_str().to_owned(),
            providers: hide_ai::PROVIDERS
                .iter()
                .map(|provider| BackgroundAiProviderSnapshot {
                    id: provider.as_str().to_owned(),
                    label: provider.label().to_owned(),
                    state: "unread".to_owned(),
                    headline: "Not checked yet".to_owned(),
                    message: None,
                    model: hide_ai::settings::default_model(*provider).to_owned(),
                    models: Vec::new(),
                    models_unavailable_reason: None,
                })
                .collect(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BackgroundAiProviderSnapshot {
    /// The provider layer's own id: `codex` or `claude`.
    pub id: String,
    pub label: String,
    /// The availability class the provider layer reported: `ready`,
    /// `needs_login`, `not_installed`, `unavailable`, `unsupported`, or
    /// `unread` before it has been asked.
    pub state: String,
    /// The short words beside the provider's name. The core writes them; no
    /// view builds a sentence out of `state`.
    pub headline: String,
    /// The provider layer's own reason, when its state carries one.
    pub message: Option<String>,
    /// The model this provider is asked for.
    pub model: String,
    /// The models this provider offers. Empty when they are not known, which
    /// `models_unavailable_reason` then says.
    pub models: Vec<String>,
    pub models_unavailable_reason: Option<String>,
}

/// What the Settings diagnosis says about agent hooks (PRD B27, B28, D-31,
/// D-48).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AgentHooksSnapshot {
    /// One row per runtime Hide has an adapter for, in the crate's own order.
    /// A runtime that is not on this Mac is still a row, because "not here"
    /// and "not installed" are different answers.
    pub runtimes: Vec<AgentHookRuntimeSnapshot>,
    /// Panes running a session that started before the hook was installed.
    /// They are the ones a restart would fix, and they are the reason the
    /// screen exists: the hook can be installed and a pane still uninstrumented.
    pub sessions_predating_install: Vec<AgentHookPaneSnapshot>,
    /// The sentence describing the last hook report Herdr did not take, when
    /// the most recent report failed. It is what separates "installed but
    /// every report is refused" from the restart advice above: with it on
    /// screen, a restart is not the fix and the sentence says what is.
    pub last_report_failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentHookRuntimeSnapshot {
    pub id: String,
    pub label: String,
    /// The configuration file this row describes, so the operator can look.
    pub path: String,
    pub headline: String,
    pub installed: bool,
    /// Whether the operator can be offered an install for this runtime. Hide
    /// never reinstalls on its own after the first run (PRD B28, D-31).
    pub offers_install: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentHookPaneSnapshot {
    pub pane_id: String,
    pub label: String,
    pub message: String,
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
    pub expected_protocol: Option<u64>,
    pub received_protocol: Option<u64>,
    pub received_version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RemoteStatusSnapshot {
    pub target_id: String,
    pub state: String,
    pub message: Option<String>,
    /// The version reported by `herdr status server --json` on this device.
    /// A missing version keeps version-gated mutations disabled.
    pub herdr_version: Option<String>,
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
                changes_root_path: None,
                focused_device_id: None,
                focused_workspace_id: None,
                focused_checkout_id: None,
                devices: vec![crate::workspace::local_device()],
                workspaces: Vec::new(),
                inactive_projects: Vec::new(),
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
                delegated: false,
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
                archive_detail: None,
                opening: Vec::new(),
            },
            sessions: SessionsSnapshot::default(),
            changes: ChangesSnapshot::default(),
            card: CheckoutCardSnapshot::default(),
            git_worktrees: None,
            git_worktrees_loading: true,
            git_worktrees_remote: false,
            worktree_removal: None,
            task_operation: None,
            explorer_operation: None,
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
                    expected_protocol: None,
                    received_protocol: None,
                    received_version: None,
                },
                remote: Vec::new(),
                chromux: ChromuxStatusSnapshot {
                    state: "not_checked".to_owned(),
                    profile: "default".to_owned(),
                    current_url: None,
                    current_title: None,
                    message: Some("Browser availability has not been checked".to_owned()),
                    last_checked_at_unix_ms: None,
                },
                environment: Vec::new(),
                agent_hooks: AgentHooksSnapshot::default(),
                background_ai: BackgroundAiSnapshot::unread(),
                diagnostics: Vec::new(),
                last_error: None,
                async_operations: Vec::new(),
                pane_focus_request: None,
            },
            pet: PetSnapshot::initial(),
            recent_closed: RecentClosedSnapshot::default(),
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
    pub sessions: SessionsSnapshot,
    pub card: CheckoutCardSnapshot,
    pub git_worktrees: Option<ProjectWorktreesSnapshot>,
    pub git_worktrees_loading: bool,
    pub git_worktrees_remote: bool,
    pub worktree_removal: Option<WorktreeRemovalSnapshot>,
    pub task_operation: Option<TaskOperationSnapshot>,
    pub explorer_operation: Option<ExplorerOperationSnapshot>,
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
    pub recent_closed: RecentClosedSnapshot,
}

impl RestSections {
    pub fn capture(snapshot: &Snapshot) -> Self {
        Self {
            navigator: snapshot.navigator.clone(),
            sessions: snapshot.sessions.clone(),
            card: snapshot.card.clone(),
            git_worktrees: snapshot.git_worktrees.clone(),
            git_worktrees_loading: snapshot.git_worktrees_loading,
            git_worktrees_remote: snapshot.git_worktrees_remote,
            worktree_removal: snapshot.worktree_removal.clone(),
            task_operation: snapshot.task_operation.clone(),
            explorer_operation: snapshot.explorer_operation.clone(),
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
            recent_closed: snapshot.recent_closed.clone(),
        }
    }

    /// Field-by-field equality against the live snapshot, so the unchanged
    /// case costs a comparison instead of a clone.
    pub fn matches(&self, snapshot: &Snapshot) -> bool {
        self.navigator == snapshot.navigator
            && self.sessions == snapshot.sessions
            && self.card == snapshot.card
            && self.git_worktrees == snapshot.git_worktrees
            && self.git_worktrees_loading == snapshot.git_worktrees_loading
            && self.git_worktrees_remote == snapshot.git_worktrees_remote
            && self.worktree_removal == snapshot.worktree_removal
            && self.task_operation == snapshot.task_operation
            && self.explorer_operation == snapshot.explorer_operation
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
            && self.recent_closed == snapshot.recent_closed
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
    pub sessions: &'a SessionsSnapshot,
    pub card: &'a CheckoutCardSnapshot,
    pub git_worktrees: &'a Option<ProjectWorktreesSnapshot>,
    pub git_worktrees_loading: bool,
    pub git_worktrees_remote: bool,
    pub worktree_removal: &'a Option<WorktreeRemovalSnapshot>,
    pub task_operation: &'a Option<TaskOperationSnapshot>,
    pub explorer_operation: &'a Option<ExplorerOperationSnapshot>,
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
    pub recent_closed: &'a RecentClosedSnapshot,
}

impl<'a> RestWire<'a> {
    fn borrow(rest: &'a RestSections) -> Self {
        Self {
            navigator: &rest.navigator,
            sessions: &rest.sessions,
            card: &rest.card,
            git_worktrees: &rest.git_worktrees,
            git_worktrees_loading: rest.git_worktrees_loading,
            git_worktrees_remote: rest.git_worktrees_remote,
            worktree_removal: &rest.worktree_removal,
            task_operation: &rest.task_operation,
            explorer_operation: &rest.explorer_operation,
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
            recent_closed: &rest.recent_closed,
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

#[cfg(test)]
mod wire_enum_tests {
    //! The shell decodes these strings strictly, so the values the core emits
    //! are a contract, pinned in `contracts/snapshot-wire-enums.json` and
    //! decoded by the shell's `SnapshotWireEnumTests`. Each list below is
    //! matched exhaustively: a new variant fails to compile here until it is
    //! listed, and then fails this test until it is in the contract file.
    use super::*;

    fn contract() -> serde_json::Map<String, serde_json::Value> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../contracts/snapshot-wire-enums.json"
        );
        let text = std::fs::read_to_string(path).expect("contracts/snapshot-wire-enums.json");
        match serde_json::from_str(&text).expect("contract is JSON") {
            serde_json::Value::Object(map) => map,
            other => panic!("contract must be an object, got {other}"),
        }
    }

    fn assert_wire<T: Serialize>(
        contract: &serde_json::Map<String, serde_json::Value>,
        key: &str,
        variants: &[T],
    ) {
        let listed = contract[key]
            .as_array()
            .unwrap_or_else(|| panic!("{key} must be an array"))
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .unwrap_or_else(|| panic!("{key} holds strings"))
                    .to_owned()
            })
            .collect::<Vec<_>>();
        let emitted = variants
            .iter()
            .map(|variant| match serde_json::to_value(variant).unwrap() {
                serde_json::Value::String(value) => value,
                other => panic!("{key} serializes as a string, got {other}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            emitted, listed,
            "{key}: the core emits the left, the contract lists the right"
        );
    }

    #[test]
    fn every_snapshot_enum_value_is_in_the_contract() {
        let contract = contract();
        let mut checked = BTreeSet::new();

        let origins = [
            CheckoutPurposeOrigin::Token,
            CheckoutPurposeOrigin::BranchDescription,
            CheckoutPurposeOrigin::AgentTitle,
            CheckoutPurposeOrigin::PullRequestTitle,
        ];
        for variant in origins {
            match variant {
                CheckoutPurposeOrigin::Token
                | CheckoutPurposeOrigin::BranchDescription
                | CheckoutPurposeOrigin::AgentTitle
                | CheckoutPurposeOrigin::PullRequestTitle => {}
            }
        }
        assert_wire(&contract, "checkout_purpose_origin", &origins);
        checked.insert("checkout_purpose_origin");

        let badges = [
            PullRequestBadge::Merged,
            PullRequestBadge::Closed,
            PullRequestBadge::Review,
            PullRequestBadge::Open,
        ];
        for variant in badges {
            match variant {
                PullRequestBadge::Merged
                | PullRequestBadge::Closed
                | PullRequestBadge::Review
                | PullRequestBadge::Open => {}
            }
        }
        assert_wire(&contract, "pull_request_badge", &badges);
        checked.insert("pull_request_badge");

        let decisions = [
            ReviewDecision::ReviewRequired,
            ReviewDecision::ChangesRequested,
            ReviewDecision::Approved,
        ];
        for variant in decisions {
            match variant {
                ReviewDecision::ReviewRequired
                | ReviewDecision::ChangesRequested
                | ReviewDecision::Approved => {}
            }
        }
        assert_wire(&contract, "review_decision", &decisions);
        checked.insert("review_decision");

        let checks = [
            PullRequestChecks::Unknown,
            PullRequestChecks::None,
            PullRequestChecks::Pending,
            PullRequestChecks::Failed,
            PullRequestChecks::Passing,
        ];
        for variant in checks {
            match variant {
                PullRequestChecks::Unknown
                | PullRequestChecks::None
                | PullRequestChecks::Pending
                | PullRequestChecks::Failed
                | PullRequestChecks::Passing => {}
            }
        }
        assert_wire(&contract, "pull_request_checks", &checks);
        checked.insert("pull_request_checks");

        let statuses = [
            ChangedFileStatus::Modified,
            ChangedFileStatus::Added,
            ChangedFileStatus::Deleted,
            ChangedFileStatus::Untracked,
            ChangedFileStatus::Renamed,
            ChangedFileStatus::Conflict,
        ];
        for variant in statuses {
            match variant {
                ChangedFileStatus::Modified
                | ChangedFileStatus::Added
                | ChangedFileStatus::Deleted
                | ChangedFileStatus::Untracked
                | ChangedFileStatus::Renamed
                | ChangedFileStatus::Conflict => {}
            }
        }
        assert_wire(&contract, "changed_file_status", &statuses);
        checked.insert("changed_file_status");

        let tab_kinds = [
            EditorTabKind::File,
            EditorTabKind::Diff,
            EditorTabKind::Session,
            EditorTabKind::Memory,
        ];
        for variant in tab_kinds {
            match variant {
                EditorTabKind::File
                | EditorTabKind::Diff
                | EditorTabKind::Session
                | EditorTabKind::Memory => {}
            }
        }
        assert_wire(&contract, "editor_tab_kind", &tab_kinds);
        checked.insert("editor_tab_kind");

        let document_kinds = [
            DocumentKind::Text,
            DocumentKind::Markdown,
            DocumentKind::Image,
            DocumentKind::Pdf,
            DocumentKind::Binary,
        ];
        for variant in document_kinds {
            match variant {
                DocumentKind::Text
                | DocumentKind::Markdown
                | DocumentKind::Image
                | DocumentKind::Pdf
                | DocumentKind::Binary => {}
            }
        }
        assert_wire(&contract, "document_kind", &document_kinds);
        checked.insert("document_kind");

        let sections = [
            RightPanelSection::Overview,
            RightPanelSection::Explorer,
            RightPanelSection::Changes,
            RightPanelSection::Sessions,
        ];
        for variant in sections {
            match variant {
                RightPanelSection::Overview
                | RightPanelSection::Explorer
                | RightPanelSection::Changes
                | RightPanelSection::Sessions => {}
            }
        }
        assert_wire(&contract, "right_panel_section", &sections);
        checked.insert("right_panel_section");

        let strip_kinds = [
            StripTabKind::Herdr,
            StripTabKind::File,
            StripTabKind::Diff,
            StripTabKind::Session,
            StripTabKind::Memory,
        ];
        for variant in strip_kinds {
            match variant {
                StripTabKind::Herdr
                | StripTabKind::File
                | StripTabKind::Diff
                | StripTabKind::Session
                | StripTabKind::Memory => {}
            }
        }
        assert_wire(&contract, "strip_tab_kind", &strip_kinds);
        checked.insert("strip_tab_kind");

        let unchecked = contract
            .keys()
            .filter(|key| !checked.contains(key.as_str()))
            .collect::<Vec<_>>();
        assert!(
            unchecked.is_empty(),
            "contract lists enums this test does not pin: {unchecked:?}"
        );
    }
}
