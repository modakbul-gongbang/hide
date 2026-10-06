use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::Deserialize;
use serde_json::Value;

use crate::model::{AgentStatusCode, PaneLayoutDirection, PaneReadRecord, SidebarAgentSnapshot};

/// Event provenance belongs to the local replica, not the external wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTabFocus {
    pub generation: u64,
    pub workspace_id: String,
    pub tab_id: String,
    pub revision: u64,
    pub creation: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionSnapshotPayload {
    #[serde(skip)]
    pub tab_focus: Option<SessionTabFocus>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    /// The Herdr workspace that holds Herdr's keyboard. Its active tab is the
    /// only tab Herdr can be said to have focused; every other workspace's
    /// `active_tab_id` is that workspace's memory of where it was last.
    #[serde(default)]
    pub focused_workspace_id: Option<String>,
    #[serde(default)]
    pub tabs: Vec<SessionTabPayload>,
    #[serde(default)]
    pub layouts: Vec<SessionLayoutPayload>,
    pub agents: Vec<SessionAgentPayload>,
    #[serde(default)]
    pub panes: Vec<SessionPanePayload>,
    #[serde(default)]
    pub workspaces: Vec<SessionWorkspacePayload>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionWorkspacePayload {
    #[serde(default)]
    pub worktree: Option<crate::domain::WorktreeProjection>,
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
    /// The tab Herdr reports as active in this workspace. It is the only
    /// authority for which tab is active; the navigator never falls back to a
    /// position.
    #[serde(default)]
    pub active_tab_id: Option<String>,
    /// Workspace metadata is the live purpose authority. Herdr caps token
    /// values at 80 characters before they reach this boundary.
    #[serde(default)]
    pub tokens: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionTabPayload {
    #[serde(default)]
    pub number: u32,
    pub tab_id: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SessionLayoutPayload {
    pub workspace_id: String,
    pub tab_id: String,
    pub zoomed: bool,
    pub area: SessionLayoutRect,
    pub focused_pane_id: String,
    pub panes: Vec<SessionLayoutPanePayload>,
    pub splits: Vec<SessionLayoutSplitPayload>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct SessionLayoutRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SessionLayoutPanePayload {
    pub pane_id: String,
    pub rect: SessionLayoutRect,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SessionLayoutSplitPayload {
    pub direction: PaneLayoutDirection,
    pub ratio: f32,
    pub rect: SessionLayoutRect,
}

/// The most a label sentence may run to on a row; anything past this is the
/// tooltip's.
const MAX_LABEL_TEXT_CHARS: usize = 80;

/// The longest `expected_reply` a label carries (PRD D-05). The projection
/// cuts at the same length so a reply that outran the rule cannot push the
/// row past one line.
pub const MAX_EXPECTED_REPLY_CHARS: usize = 40;

/// What the core's label worker says about an agent (PRD labels-in-hided
/// D-04): set by `labels::worker::LabelWorker::apply` only while the pane's
/// current session reference proves the session the label was made for.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct AgentLabel {
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub progress: Option<String>,
    #[serde(default)]
    pub expected_reply: Option<String>,
    /// The agent's last message asks the operator something specific.
    #[serde(default)]
    pub question: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionAgentPayload {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub workspace_label: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    /// Herdr's record of the conversation this agent is running, when it has
    /// one. `kind` says whether `value` is an id or a path; only an id can be
    /// handed to an agent's own fork command.
    #[serde(default)]
    pub agent_session: Option<SessionAgentSessionPayload>,
    /// The pane this agent was spawned from, as hcoord's `parent_pane` token
    /// declares it (Herdr records no lineage). `wire.rs` leaves it empty when
    /// the child's own session no longer matches the one the token was
    /// written for.
    #[serde(default)]
    pub spawned_from_pane_id: Option<String>,
    /// Stable identity of a parent on another machine, recorded by hcoord.
    #[serde(default)]
    pub spawned_from_machine_id: Option<String>,
    /// The digest of the parent's session the relationship was written for
    /// (`wire::session_digest`). `apply_lineage` keeps the parent only while
    /// its pane reports that session; a declared parent without one has
    /// nothing to be compared with and is a root.
    #[serde(default)]
    pub declared_parent_session: Option<String>,
    /// The digest of this pane's own session (`wire::session_digest`), which
    /// the rows that declare it as their parent are compared with.
    #[serde(default)]
    pub lineage_session: Option<String>,
    #[serde(default)]
    pub state_change_seq: Option<u64>,
    #[serde(default)]
    pub tokens: BTreeMap<String, Value>,
    /// The core's label for this agent's current session, if proven.
    #[serde(default)]
    pub label: Option<AgentLabel>,
    /// When the core last saw this agent change state, in epoch ms.
    #[serde(default)]
    pub changed_at_unix_ms: Option<u64>,
    /// What the label worker read of the current session, if proven; laid
    /// on by the label overlay, never read from Herdr.
    #[serde(skip)]
    pub(crate) facts: Option<crate::request_view::RowFacts>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SessionAgentSessionPayload {
    pub kind: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionPanePayload {
    #[serde(default)]
    pub foreground_process: Option<String>,
    pub pane_id: String,
    #[serde(default)]
    pub tokens: BTreeMap<String, Value>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// The name the user gave this pane in Herdr, when they gave it one.
    #[serde(default)]
    pub label: Option<String>,
    /// What the program running in the pane set the terminal title to.
    #[serde(default)]
    pub terminal_title: Option<String>,
}

/// What an agent needs from the operator.
///
/// A question is the core label's verdict on the agent's last message;
/// Herdr's `blocked` lifecycle is an approval. Whether the operator has read
/// either is Hide's own judgment (`apply_read_state`). `Error` has no source
/// since the hand-installed hook path was retired (PRD labels-in-hided D-06)
/// and stays only so the wire value keeps its meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentDemand {
    Error,
    Question,
    Approval,
    None,
}

impl AgentDemand {
    fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Question => "question",
            Self::Approval => "approval",
            Self::None => "none",
        }
    }
}

/// Whether an agent is running.
///
/// Herdr's `done` and `idle` are the same underlying stopped state. Completion
/// is recorded on its own axis; Herdr's tab-scoped seen judgment never becomes
/// Hide's pane-level read state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentActivity {
    Working,
    Stopped,
    Unknown,
}

impl AgentActivity {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }
}

/// The four groups the sidebar reads top to bottom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentGroup {
    NeedsYou,
    Done,
    Working,
    Seen,
}

impl AgentGroup {
    fn name(self) -> &'static str {
        match self {
            Self::NeedsYou => "needs_you",
            Self::Done => "done",
            Self::Working => "working",
            Self::Seen => "seen",
        }
    }

    /// Where the group sits in the sidebar, read top to bottom: what is
    /// waiting on the operator, what finished while they were away, what is
    /// still running, then everything already dealt with.
    fn rank(self) -> u8 {
        match self {
            Self::NeedsYou => 0,
            Self::Done => 1,
            Self::Working => 2,
            Self::Seen => 3,
        }
    }
}

/// Who the row belongs to, as the grouping rule needs to know it.
///
/// The distinction exists because delegation is only real if the operator
/// stops being called for the delegated work. A child's question, approval,
/// error or completion is the parent's problem: it shows on the parent's
/// badge, turns the parent's row unread, and never raises the operator's own
/// attention groups (PRD B15, B16, D-35, D-38). There is no clock that hands
/// a child back: the parent is asked for its status, not the child.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ownership {
    /// The operator's own work: a lineage root, or an orphan whose parent is
    /// gone.
    Operator,
    /// Delegated work. Its demands stay with its parent.
    Delegated,
}

/// The group a row belongs to, given who owns it.
///
/// `waiting_on_descendants` is the lineage pass's answer for a root that is
/// quiet itself while a descendant is still busy: such a row has not
/// finished, so it sits in Working, and it reaches Done only once it and
/// every descendant are quiet (sidebar-agent-status D-01). Its own unread
/// demand or a blocked prompt still wins, because the flag is only ever set
/// on a row with no demand of its own.
pub fn agent_group_for(
    demand: AgentDemand,
    activity: AgentActivity,
    completed: bool,
    unread: bool,
    blocked: bool,
    ownership: Ownership,
    waiting_on_descendants: bool,
) -> AgentGroup {
    if ownership == Ownership::Delegated {
        // The row keeps its own mark and status word; only its claim on the
        // operator's attention is withheld.
        return if activity == AgentActivity::Working {
            AgentGroup::Working
        } else {
            AgentGroup::Seen
        };
    }
    if blocked || (demand != AgentDemand::None && unread) {
        AgentGroup::NeedsYou
    } else if waiting_on_descendants {
        AgentGroup::Working
    } else if demand == AgentDemand::None
        && activity == AgentActivity::Stopped
        && completed
        && unread
    {
        AgentGroup::Done
    } else if activity == AgentActivity::Working {
        AgentGroup::Working
    } else {
        AgentGroup::Seen
    }
}

/// The mark a row draws, matching the retired label plugin's symbol table. It is
/// one decision for the row's own symbol and for every count of marks, so a
/// badge that stands for folded rows says what opening them would show.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RowMark {
    Error,
    Approval,
    Question,
    Working,
    Done,
    Idle,
    Unknown,
}

impl RowMark {
    fn of(agent: &SidebarAgentSnapshot) -> Self {
        // A root waiting on its children keeps the hollow ring and says so:
        // its own completion is not the news while a child is still busy, and
        // the badge beside it says what the children are doing (D-01, D-02).
        if agent.waiting_on_descendants {
            return Self::Idle;
        }
        let (demand, activity, unread) = axes_of(agent);
        match demand {
            AgentDemand::Error => Self::Error,
            AgentDemand::Question => Self::Question,
            AgentDemand::Approval => Self::Approval,
            AgentDemand::None => match activity {
                AgentActivity::Working => Self::Working,
                AgentActivity::Stopped if agent.completed && unread => Self::Done,
                AgentActivity::Stopped => Self::Idle,
                AgentActivity::Unknown => Self::Unknown,
            },
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Self::Error => "\u{d7}",
            Self::Approval => "!",
            Self::Question => "?",
            Self::Working => "\u{25cf}",
            Self::Done => "✓",
            Self::Idle => "\u{25cb}",
            Self::Unknown => "~",
        }
    }

    fn count_into(self, counts: &mut crate::model::MarkCountsSnapshot) {
        match self {
            Self::Error => counts.error += 1,
            Self::Approval => counts.approval += 1,
            Self::Question => counts.question += 1,
            Self::Working => counts.working += 1,
            Self::Done => counts.done += 1,
            Self::Idle => counts.idle += 1,
            Self::Unknown => {}
        }
    }
}

/// Where a row stands when one row has to speak for several.
///
/// Group order first, then the worst demand inside it, with an unknown
/// activity ahead of an ordinary idle so missing information is not hidden by
/// a quiet sibling. The Workspace summary chip and the pane header's parent
/// badge both read it, so the two cannot disagree about which child a badge
/// is describing (docs/status-model.md, Workspace aggregation).
pub fn representative_rank(agent: &SidebarAgentSnapshot) -> (u8, u8) {
    rank_within(agent, ownership_of(agent))
}

/// Where a child stands among its siblings, as the pane that spawned them
/// sees it.
///
/// Delegation moves a child's demand off the *operator's* attention groups,
/// not out of existence. Its parent is exactly who is supposed to answer it,
/// so the parent's badge ranks its children as if it owned them; ranking them
/// the way the sidebar does would flatten every child to Working or Seen and
/// leave the badge naming a quiet sibling over the one that failed (PRD B15,
/// B16, D-35, D-38).
fn child_representative_rank(agent: &SidebarAgentSnapshot) -> (u8, u8) {
    rank_within(agent, Ownership::Operator)
}

fn rank_within(agent: &SidebarAgentSnapshot, ownership: Ownership) -> (u8, u8) {
    let (demand, activity, unread) = axes_of(agent);
    let demand_rank = match demand {
        AgentDemand::Error => 0,
        AgentDemand::Approval => 1,
        AgentDemand::Question => 2,
        AgentDemand::None if agent.activity == "unknown" => 3,
        AgentDemand::None => 4,
    };
    let group = agent_group_for(
        demand,
        activity,
        agent.completed,
        unread,
        agent.blocked,
        ownership,
        agent.waiting_on_descendants,
    );
    (group.rank(), demand_rank)
}

/// Aggregate physical pane ownership, never the visual lineage tree or raised rows.
/// Canonical order breaks ties after group and demand priority.
pub fn sync_checkout_agent_summaries(
    workspaces: &mut [crate::model::WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
) -> bool {
    let owners = workspaces
        .iter()
        .flat_map(|workspace| &workspace.checkouts)
        .enumerate()
        .flat_map(|(index, checkout)| {
            checkout
                .tabs
                .iter()
                .flat_map(|tab| &tab.panes)
                .map(move |pane| (pane.id.as_str(), index))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let count = workspaces
        .iter()
        .map(|workspace| workspace.checkouts.len())
        .sum();
    let mut summaries = vec![crate::model::CheckoutAgentSummary::default(); count];
    let mut ranks = vec![None; count];
    let mut counted = std::collections::HashSet::new();
    for agent in agents {
        let Some(&index) = owners.get(agent.pane_id.as_str()) else {
            continue;
        };
        if !counted.insert(agent.pane_id.as_str()) {
            continue;
        }
        let summary = &mut summaries[index];
        RowMark::of(agent).count_into(&mut summary.marks);
        let group = group_of(agent);
        match group {
            AgentGroup::NeedsYou => summary.needs_you += 1,
            AgentGroup::Done => summary.done += 1,
            AgentGroup::Working => summary.working += 1,
            AgentGroup::Seen => {
                summary.seen += 1;
                if agent.activity == "unknown" && agent.demand == "none" {
                    summary.unknown += 1;
                }
            }
        }
        let rank = representative_rank(agent);
        if ranks[index].is_none_or(|best| rank < best) {
            ranks[index] = Some(rank);
            summary.representative_pane_id = Some(agent.pane_id.clone());
        }
    }
    let mut changed = false;
    for (checkout, summary) in workspaces
        .iter_mut()
        .flat_map(|workspace| &mut workspace.checkouts)
        .zip(summaries)
    {
        if checkout.agent_summary != summary {
            checkout.agent_summary = summary;
            changed = true;
        }
    }
    // The removal confirmation's counts ride the same pass, so the dialog
    // names the panes the close will send and the agents still working in
    // them as of the same projection (D-10). "Running" is the deletion
    // gate's definition: the agent's activity axis says working. Every local
    // row offers `Remove project…`, a row Herdr shows without a registration
    // too (PRD sidebar-context-menus D-14), so every local row carries the
    // gate; a device's rows get theirs where its session is derived
    // (`device_catalog::apply_registrations`), because a remote tree arrives
    // freshly projected on every sync and a count written into it here
    // would read as a change on every tick.
    let running = agents
        .iter()
        .filter(|agent| agent.activity == AgentActivity::Working.name())
        .map(|agent| agent.pane_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for workspace in workspaces
        .iter_mut()
        .filter(|workspace| workspace.remote_target_id.is_none())
    {
        let panes = workspace
            .checkouts
            .iter()
            .flat_map(|checkout| &checkout.tabs)
            .flat_map(|tab| &tab.panes);
        let mut removal = crate::model::WorkspaceRemovalGateSnapshot::default();
        for pane in panes {
            removal.pane_count += 1;
            if running.contains(pane.id.as_str()) {
                removal.running_agent_count += 1;
            }
        }
        if workspace.removal != removal {
            workspace.removal = removal;
            changed = true;
        }
    }
    changed |= sync_checkout_purposes(workspaces, agents);
    changed
}

/// Resolves the non-persistent half of the checkout purpose ladder.
///
/// Token and branch-description values were selected while the catalog was
/// built and always win. Agent and pull-request titles are rebuilt whenever
/// either projection changes, so a vanished representative cannot leave a
/// stale sentence on the row.
pub fn sync_checkout_purposes(
    workspaces: &mut [crate::model::WorkspaceSnapshot],
    agents: &[crate::model::SidebarAgentSnapshot],
) -> bool {
    use crate::model::{CheckoutPurposeOrigin, CheckoutPurposeSnapshot};

    let agent_titles = agents
        .iter()
        .map(|agent| (agent.pane_id.as_str(), agent.identity_label.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut changed = false;
    for checkout in workspaces
        .iter_mut()
        .flat_map(|workspace| workspace.checkouts.iter_mut())
    {
        if checkout.purpose.as_ref().is_some_and(|purpose| {
            matches!(
                purpose.origin,
                CheckoutPurposeOrigin::Token | CheckoutPurposeOrigin::BranchDescription
            )
        }) {
            continue;
        }
        let next = checkout
            .agent_summary
            .representative_pane_id
            .as_deref()
            .and_then(|pane_id| agent_titles.get(pane_id).copied())
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(|title| CheckoutPurposeSnapshot {
                text: title.to_owned(),
                origin: CheckoutPurposeOrigin::AgentTitle,
            })
            .or_else(|| {
                checkout
                    .pull_request
                    .as_ref()
                    .map(|pull_request| pull_request.title.trim())
                    .filter(|title| !title.is_empty())
                    .map(|title| CheckoutPurposeSnapshot {
                        text: title.to_owned(),
                        origin: CheckoutPurposeOrigin::PullRequestTitle,
                    })
            });
        if checkout.purpose != next {
            checkout.purpose = next;
            changed = true;
        }
    }
    changed
}

/// The one short word a row shows. A reported completion the operator has not
/// read is `Done`; an ordinary stopped pane and a read completion are `Idle`.
/// No view ever shows an axis value, so nothing underscored can reach the
/// screen.
/// The status word of a root waiting on its children.
fn agent_status_code(
    demand: AgentDemand,
    activity: AgentActivity,
    completed: bool,
    unread: bool,
) -> AgentStatusCode {
    match demand {
        AgentDemand::Error => AgentStatusCode::Error,
        AgentDemand::Question => AgentStatusCode::Question,
        AgentDemand::Approval => AgentStatusCode::Approval,
        AgentDemand::None => match activity {
            AgentActivity::Working => AgentStatusCode::Working,
            AgentActivity::Stopped if completed && unread => AgentStatusCode::Done,
            AgentActivity::Stopped => AgentStatusCode::Idle,
            AgentActivity::Unknown => AgentStatusCode::Unknown,
        },
    }
}

/// Closing this pane would interrupt running work or discard an unresolved
/// demand. Read state remains a presentation axis, so an unread completion or
/// an ordinary stopped pane cannot create a close prompt by itself.
fn agent_requires_close_confirmation(
    activity: AgentActivity,
    demand: AgentDemand,
    blocked: bool,
) -> bool {
    activity == AgentActivity::Working || demand != AgentDemand::None || blocked
}

/// An activity-less pane cannot safely be described as idle in a destructive
/// confirmation. The caller must obtain a fresh status before it can close.
fn agent_requires_close_status_check(
    activity: AgentActivity,
    demand: AgentDemand,
    blocked: bool,
) -> bool {
    activity == AgentActivity::Unknown && demand == AgentDemand::None && !blocked
}

/// One agent that could not be read out of an otherwise valid snapshot.
///
/// A single broken record excludes only itself; the surrounding agents are
/// still projected, and the exclusion is reported rather than swallowed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentExclusion {
    pub source_index: usize,
    pub pane_id: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentProjection {
    pub agents: Vec<SidebarAgentSnapshot>,
    pub excluded: Vec<AgentExclusion>,
}

/// Projects the agent list with every row unread.
///
/// The read axis is a Hide judgment that needs the pane read record, which
/// lives in the runtime, so callers that hold one follow this with
/// [`apply_read_state`]. A caller with no record still gets a coherent
/// projection: unread is the honest answer when nothing says otherwise.
pub fn project_agents(payload: SessionSnapshotPayload) -> AgentProjection {
    let mut agents = Vec::with_capacity(payload.agents.len());
    let mut excluded = Vec::new();
    for (source_index, agent) in payload.agents.into_iter().enumerate() {
        let pane_id =
            non_empty(agent.pane_id.as_deref().or(agent.id.as_deref())).map(str::to_owned);
        match project_agent(agent) {
            Ok(projected) => agents.push(projected),
            Err(reason) => excluded.push(AgentExclusion {
                source_index,
                pane_id,
                reason,
            }),
        }
    }

    // Rows stay in the order Herdr sent them. Ordering is a function of the
    // group, and the group is a function of the read axis, which is decided
    // in `apply_read_state` where the pane read record lives.
    for agent in &mut agents {
        derive_from_axes(agent);
    }
    AgentProjection { agents, excluded }
}

/// Adds a tree view without reordering, duplicating, or changing the axes of
/// the canonical agent list. Child ordering belongs to this view alone.
/// Missing and cyclic parent references are visible orphan roots, so malformed
/// external lineage cannot hide an agent or recurse forever.
///
/// The same walk that finds each row's ancestors also hands the row's own
/// state up to every one of them, so the descendant badge and the descendant
/// signals the read record compares are derived on this pass and nowhere
/// else (PRD B3, B4, B5, D-05).
pub fn apply_lineage(
    agents: &mut [SidebarAgentSnapshot],
    workspaces: &[crate::model::WorkspaceSnapshot],
    expanded: &[String],
) {
    let by_pane = agents
        .iter()
        .enumerate()
        .map(|(index, agent)| (agent.pane_id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let checkouts = workspaces
        .iter()
        .flat_map(|workspace| &workspace.checkouts)
        .flat_map(|checkout| {
            checkout
                .tabs
                .iter()
                .flat_map(|tab| &tab.panes)
                .map(move |pane| {
                    (
                        pane.id.as_str(),
                        (checkout.id.clone(), checkout.label.clone()),
                    )
                })
        })
        .collect::<BTreeMap<_, _>>();
    let mut parents = agents
        .iter()
        .map(|agent| {
            agent
                .spawned_from_pane_id
                .as_ref()
                .and_then(|pane| by_pane.get(pane))
                .copied()
        })
        .collect::<Vec<_>>();
    // A declaration names a pane, and a pane outlives the agent it hosted: it
    // holds only while that pane still reports the session the declaration
    // was written for. One that moved on is no declaration at all - not a
    // line, not an orphan's hint - so its child is a root and the pane's new
    // agent adopts nobody (the child's own session was checked in `wire.rs`).
    for index in 0..agents.len() {
        let Some(parent) = parents[index] else {
            continue;
        };
        if !crate::wire::session_holds(
            agents[index].declared_parent_session.as_deref(),
            agents[parent].lineage_session.as_deref(),
        ) {
            parents[index] = None;
            agents[index].spawned_from_pane_id = None;
        }
    }
    // Walk parent chains iteratively: depth is data, never a recursion limit.
    let original_parents = parents.clone();
    for index in 0..agents.len() {
        let mut visited = std::collections::BTreeSet::new();
        let mut cursor = Some(index);
        while let Some(node) = cursor {
            if !visited.insert(node) {
                // Break only cycle members; descendants retain valid nesting.
                let mut cycle = node;
                loop {
                    parents[cycle] = None;
                    cycle = original_parents[cycle].expect("cycle has a parent");
                    if cycle == node {
                        break;
                    }
                }
                break;
            }
            cursor = original_parents[node];
        }
    }
    let mut children = vec![Vec::new(); agents.len()];
    for (index, parent) in parents.iter().enumerate() {
        if let Some(parent) = parent {
            children[*parent].push(index);
        }
    }
    for list in &mut children {
        list.sort_by(|left, right| {
            agents[*right]
                .last_activity
                .cmp(&agents[*left].last_activity)
        });
    }
    let close_descendants = close_order(&children, &parents);
    // Walk each row to its root, remembering the way, so the breadcrumb is
    // the same walk the depth already costs rather than a second traversal.
    // The same walk tells every ancestor what this row is doing, and it runs
    // before the rows are written so an ancestor that comes earlier in the
    // list has heard from every descendant by then. A closed pane is no
    // longer a row, so it drops out of every count on the next projection
    // (PRD D-12).
    let mut ancestry = Vec::with_capacity(agents.len());
    let mut descendant_counts =
        vec![crate::model::DescendantCountsSnapshot::default(); agents.len()];
    let mut descendant_signals = vec![BTreeSet::new(); agents.len()];
    for (index, agent) in agents.iter().enumerate() {
        let mut ancestors = Vec::new();
        let mut root = index;
        while let Some(parent) = parents[root] {
            ancestors.push(parent);
            root = parent;
        }
        let state = descendant_state(agent);
        let signals = descendant_signals_of(agent);
        for ancestor in &ancestors {
            state.count_into(&mut descendant_counts[*ancestor]);
            descendant_signals[*ancestor].extend(signals.iter().cloned());
        }
        ancestry.push((ancestors, root));
    }
    for index in 0..agents.len() {
        let (mut ancestors, root) = std::mem::take(&mut ancestry[index]);
        let depth = ancestors.len();
        ancestors.reverse();
        let path = ancestors
            .iter()
            .map(|ancestor| agents[*ancestor].pane_id.clone())
            .collect::<Vec<_>>();
        let siblings = parents[index]
            .map(|parent| {
                children[parent]
                    .iter()
                    .map(|sibling| agents[*sibling].pane_id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let parent_pane_id = parents[index].map(|parent| agents[parent].pane_id.clone());
        // Where this row came from, for a row whose parent is not drawn above
        // it. The name is the parent agent's when Hide can still see that
        // agent; a pane id is not a name and is never shown as one, because
        // the operator cannot act on an internal id (design principle 10).
        //
        // When it cannot, the line says only that. Hide knows the pane and
        // knows no agent is listed there - not that the agent ended, which
        // is equally consistent with a pane outside this list. Saying
        // "ended" would be a claim the projection cannot make.
        let spawned_from = agents[index].spawned_from_pane_id.clone();
        let spawn_parent = spawned_from
            .as_ref()
            .and_then(|pane| by_pane.get(pane))
            .map(|parent| agents[*parent].identity_label.clone());
        let hint = spawned_from.as_ref().map(|_| match &spawn_parent {
            Some(name) => format!("↳ from {name}"),
            None => "↳ from an agent Hide can't see".to_owned(),
        });
        let orphan = agents[index].spawned_from_pane_id.is_some() && parents[index].is_none();
        let own_checkout = checkouts.get(agents[index].pane_id.as_str());
        let parent_checkout =
            parents[index].and_then(|parent| checkouts.get(agents[parent].pane_id.as_str()));
        let badge = own_checkout
            .filter(|own| parent_checkout.is_some_and(|parent| own.0 != parent.0))
            .map(|(_, label)| label.clone());
        let root_checkout = checkouts
            .get(agents[root].pane_id.as_str())
            .map(|(id, _)| id.clone());
        let child_ids = children[index]
            .iter()
            .map(|child| agents[*child].pane_id.clone())
            .collect();
        let close_ids = close_descendants[index]
            .iter()
            .map(|descendant| agents[*descendant].pane_id.clone())
            .collect();
        let agent = &mut agents[index];
        // Ownership is the depth and nothing else. An orphan resolved to no
        // parent, so it is a root here and the dimming lifts with it.
        agent.delegated = depth > 0;
        agent.lineage_parent_pane_id = parent_pane_id;
        agent.lineage_path_pane_ids = path;
        agent.lineage_sibling_pane_ids = siblings;
        agent.lineage_depth = depth;
        agent.lineage_child_pane_ids = child_ids;
        agent.close_descendant_pane_ids = close_ids;
        agent.lineage_root_checkout_id = root_checkout;
        agent.lineage_worktree_badge = badge;
        agent.lineage_orphan = orphan;
        agent.lineage_hint = orphan.then(|| hint.clone()).flatten();
        agent.raised_hint = hint;
        // The pane the line points at, so the shell can offer the jump rather
        // than printing a dead end. Only set while that pane still holds an
        // agent Hide can name; an ended one is text and nothing more.
        agent.spawn_origin_pane_id = spawn_parent.and(spawned_from);
        // Folded is the default: a row with descendants opens only while
        // the operator has it in the expanded set (PRD D-06).
        agent.lineage_collapsed = !expanded.contains(&agent.pane_id);
        agent.descendant_counts = descendant_counts[index];
        agent.waiting_on_descendants =
            depth == 0 && quiet_itself(agent) && descendants_busy(&descendant_counts[index]);
        agent.descendant_signals = std::mem::take(&mut descendant_signals[index]);
    }
    // Ownership was unknown when the rows were first derived, because it is
    // the lineage that decides it. Rederiving here is what keeps every caller
    // of this function on one answer instead of each remembering to ask
    // (engineering rule 13). The order is left alone: this function is read
    // by index while it walks the tree, and the read pass that follows it
    // on every ingest is what sorts.
    for agent in agents.iter_mut() {
        derive_from_axes(agent);
    }
}

/// Each row's live descendants in the order closing the row takes them:
/// every descendant before its parent, siblings in the tree's own order, so
/// no child is left behind as an orphan root while its parent is still
/// open (PRD close-agent-subtree D-21). In a post-order walk a row's subtree
/// is the run of rows just before it, so one walk per root answers every row.
fn close_order(children: &[Vec<usize>], parents: &[Option<usize>]) -> Vec<Vec<usize>> {
    let mut order = vec![Vec::new(); children.len()];
    for root in (0..children.len()).filter(|index| parents[*index].is_none()) {
        let mut post = Vec::new();
        // (row, next child to visit, position of the row's first descendant)
        let mut stack = vec![(root, 0usize, 0usize)];
        while let Some((node, next, start)) = stack.pop() {
            if let Some(child) = children[node].get(next) {
                stack.push((node, next + 1, start));
                stack.push((*child, 0, post.len()));
            } else {
                order[node] = post[start..].to_vec();
                post.push(node);
            }
        }
    }
    order
}

/// A row with no demand of its own that is stopped, idle or done: the half
/// of the waiting-on-children judgement the row answers for itself.
fn quiet_itself(agent: &SidebarAgentSnapshot) -> bool {
    let (demand, activity, _) = axes_of(agent);
    demand == AgentDemand::None && activity == AgentActivity::Stopped && !agent.blocked
}

/// Whether any live descendant is still busy: working, or holding a
/// question, approval or error. A ready or finished child is quiet, and one
/// whose activity Herdr cannot classify is not counted as busy, because a
/// waiting state the projection cannot vouch for is not drawn (D-01).
fn descendants_busy(counts: &crate::model::DescendantCountsSnapshot) -> bool {
    counts.error + counts.approval + counts.question + counts.working > 0
}

/// What one descendant contributes to its ancestors' badges.
#[derive(Clone, Copy)]
enum DescendantState {
    Error,
    Approval,
    Question,
    Working,
    Done,
    /// Stopped with nothing reported: a ready pane, which is not news.
    Ready,
    Unknown,
}

impl DescendantState {
    fn count_into(self, counts: &mut crate::model::DescendantCountsSnapshot) {
        match self {
            Self::Error => counts.error += 1,
            Self::Approval => counts.approval += 1,
            Self::Question => counts.question += 1,
            Self::Working => counts.working += 1,
            Self::Done => counts.done += 1,
            Self::Ready => {}
            Self::Unknown => counts.unknown += 1,
        }
    }
}

/// The one state a descendant counts under: its demand first, then whether
/// it is running, then whether it reported a completion. The order is the
/// badge's own, worst first (PRD D-08).
fn descendant_state(agent: &SidebarAgentSnapshot) -> DescendantState {
    let (demand, activity, _) = axes_of(agent);
    match demand {
        AgentDemand::Error => DescendantState::Error,
        AgentDemand::Approval => DescendantState::Approval,
        AgentDemand::Question => DescendantState::Question,
        AgentDemand::None => match activity {
            AgentActivity::Working => DescendantState::Working,
            AgentActivity::Stopped if agent.completed => DescendantState::Done,
            AgentActivity::Stopped => DescendantState::Ready,
            AgentActivity::Unknown => DescendantState::Unknown,
        },
    }
}

/// The signals one descendant sends up its lineage: its outstanding demand,
/// and its completion, each on its own so that a demand being cleared from a
/// finished child does not read as a fresh completion (PRD B5, B6).
fn descendant_signals_of(agent: &SidebarAgentSnapshot) -> Vec<crate::model::DescendantSignal> {
    let (demand, activity, _) = axes_of(agent);
    let mut signals = Vec::with_capacity(2);
    if demand != AgentDemand::None {
        signals.push(crate::model::DescendantSignal {
            pane_id: agent.pane_id.clone(),
            kind: demand.name().to_owned(),
        });
    }
    if activity == AgentActivity::Stopped && agent.completed {
        signals.push(crate::model::DescendantSignal {
            pane_id: agent.pane_id.clone(),
            kind: "completed".to_owned(),
        });
    }
    signals
}

/// One agent, as every surface that names an agent in a line draws it: the
/// pane header's chip row, a breadcrumb step's sibling list, and the
/// Overview worktree row's agent line all read the same fields, so they
/// cannot describe the same agent differently (PRD B5, B10, B34).
pub fn agent_chip(agent: &SidebarAgentSnapshot) -> crate::model::AgentChipSnapshot {
    crate::model::AgentChipSnapshot {
        pane_id: agent.pane_id.clone(),
        label: agent.identity_label.clone(),
        checkout_label: agent.checkout_label.clone(),
        detail: agent.detail.clone(),
        status_word_visible: agent.status_word_visible,
        agent_kind: agent.agent_kind.clone(),
        demand: agent.demand.clone(),
        activity: agent.activity.clone(),
        emphasized: agent.emphasized,
        symbol: agent.symbol.clone(),
        status_code: agent.status_code,
        delegated: agent.delegated,
    }
}

/// What a row's second line says, decided by the row's group (PRD D-06).
///
/// Rows that still concern the operator - the Needs You group and an unread
/// Done - keep their status word and add what the operator is being asked
/// for (`expected_reply`), or what happened (`progress`) when nothing is
/// asked. A working row shows only its progress: the mark already says it is
/// working. A row the operator has read, and a row whose activity is unknown,
/// say nothing more than their name, unless it still holds a request. With no sentence at all, a row that
/// would have shown one keeps the status word alone, so a pane with no label
/// plugin behind it still reads as it did before (PRD B6, B13).
///
/// Returns whether the status word is drawn and the sentence beside it.
fn agent_second_line(
    group: AgentGroup,
    demand: AgentDemand,
    expected_reply: Option<&str>,
    progress: Option<&str>,
) -> (bool, Option<String>) {
    // An unresolved demand keeps its request whatever group the row sits in:
    // a read question in Seen and a delegated child's approval still say what
    // they are asking, so a view can keep that line until the request is
    // answered rather than until it is looked at (sidebar-agent-status B7).
    // Only a group that wanted a sentence falls back to the status word.
    let request = (demand != AgentDemand::None)
        .then(|| expected_reply.or(progress))
        .flatten();
    let sentence = match group {
        AgentGroup::NeedsYou | AgentGroup::Done => expected_reply.or(progress),
        AgentGroup::Working => request.or(progress),
        AgentGroup::Seen => return (false, request.map(str::to_owned)),
    };
    // The word is the sentence's stand-in, never its prefix: the mark and the
    // group heading already say Question or Done, and the word beside a
    // sentence took the width the sentence needed (2026-09-18).
    match sentence {
        Some(sentence) => (false, Some(sentence.to_owned())),
        None => (true, None),
    }
}

/// What one pane's header says about the work its agent delegated.
///
/// Returns `None` for a pane with no agent: a shell, an editor or a log has
/// no children to report and gets no mark saying so (PRD B22, D-30).
pub fn project_pane_children(
    agents: &[SidebarAgentSnapshot],
    pane_id: &str,
    tokens: crate::agent_hooks::PaneHookTokens,
    status_of: &dyn Fn(hide_agent_hooks::AgentRuntime) -> Option<hide_agent_hooks::HookStatus>,
) -> Option<crate::model::PaneChildrenSnapshot> {
    let agent = agents.iter().find(|agent| agent.pane_id == pane_id)?;
    let runtime = crate::agent_hooks::runtime_of(&agent.agent_kind);
    let status = runtime.and_then(status_of);
    let instrumentation = hide_agent_hooks::diagnosis::instrumentation(
        hide_agent_hooks::diagnosis::PaneObservation {
            runtime,
            token_version: tokens.version,
            working: tokens.working,
            done: tokens.done,
            blocked: tokens.blocked,
        },
        status.as_ref(),
    );
    // Pane children come from the lineage, which Herdr reports directly, so
    // they are known whether or not the hook is installed. Only the
    // in-process count depends on instrumentation (PRD D-24).
    let chips = agent
        .lineage_child_pane_ids
        .iter()
        .filter_map(|child| agents.iter().find(|agent| &agent.pane_id == child))
        .map(agent_chip)
        .collect::<Vec<_>>();
    let representative = agent
        .lineage_child_pane_ids
        .iter()
        .filter_map(|child| agents.iter().find(|agent| &agent.pane_id == child))
        .min_by_key(|child| child_representative_rank(child))
        .map(agent_chip);
    Some(crate::model::PaneChildrenSnapshot {
        instrumented: instrumentation.instrumented,
        uninstrumented_reason: instrumentation
            .reason
            .map(|reason| reason.message().to_owned()),
        uninstrumented_label: instrumentation
            .reason
            .map(|reason| reason.accessibility_label().to_owned()),
        uninstrumented_code: instrumentation
            .reason
            .map(|reason| reason.code().to_owned()),
        chips,
        representative,
        subagents: crate::model::SubagentCountsSnapshot {
            working: instrumentation.working,
            done: instrumentation.done,
            blocked: instrumentation.blocked,
        },
    })
}

/// The breadcrumb for one pane: its ancestors root first, each carrying that
/// layer's siblings for the step's dropdown.
///
/// Derived from the current list every time, so a departed ancestor shortens
/// it on the next projection with nothing to repair (PRD B9, D-18).
pub fn project_lineage_path(
    agents: &[SidebarAgentSnapshot],
    pane_id: &str,
) -> Vec<crate::model::LineageStepSnapshot> {
    let Some(agent) = agents.iter().find(|agent| agent.pane_id == pane_id) else {
        return Vec::new();
    };
    if agent.lineage_path_pane_ids.is_empty() {
        // A root has nowhere to go back to, so its header stays plain.
        return Vec::new();
    }
    agent
        .lineage_path_pane_ids
        .iter()
        .chain(std::iter::once(&agent.pane_id))
        .filter_map(|step| agents.iter().find(|agent| &agent.pane_id == step))
        .map(|step| crate::model::LineageStepSnapshot {
            pane_id: step.pane_id.clone(),
            label: agent_chip(step).label,
            siblings: step
                .lineage_sibling_pane_ids
                .iter()
                .filter_map(|sibling| agents.iter().find(|agent| &agent.pane_id == sibling))
                .map(agent_chip)
                .collect(),
        })
        .collect()
}

/// Expansion state belongs to pane existence, not whether it currently has
/// children. A fresh scoped agent list may evict it; a stale list may not.
pub fn prune_lineage_expansion(
    expanded: &mut Vec<String>,
    agents: &[SidebarAgentSnapshot],
    scope: ReadRecordScope<'_>,
) -> bool {
    let before = expanded.len();
    expanded.retain(|pane| !scope.owns(pane) || agents.iter().any(|agent| &agent.pane_id == pane));
    before != expanded.len()
}

/// The one ordering every agent surface reads: the two sidebar views, the pet
/// dashboard, and the agent switcher's candidate list.
///
/// Group order first, then most recent activity descending, then the order
/// Herdr sent the rows in. The sort is stable, so that third key costs
/// nothing, and re-running it on an already ordered list is the identity
/// (engineering rule 11).
///
/// It runs after the read axis, never inside `project_agents`: a row's group
/// depends on whether the operator has read it, and a projection that has not
/// met the read record ledger treats every completion as unread. Ordinary idle
/// rows still stay Seen. The order Hide shows is Hide's own.
fn sort_agents(agents: &mut [SidebarAgentSnapshot]) {
    agents.sort_by(|left, right| {
        group_of(left)
            .rank()
            .cmp(&group_of(right).rank())
            .then_with(|| right.last_activity.cmp(&left.last_activity))
    });
}

/// Reads the demand, activity and read axes back off a row.
///
/// `derive_from_axes` is the only writer of those fields and writes them from
/// these same enums, so the round trip is total.
fn axes_of(agent: &SidebarAgentSnapshot) -> (AgentDemand, AgentActivity, bool) {
    let demand = match agent.demand.as_str() {
        "error" => AgentDemand::Error,
        "question" => AgentDemand::Question,
        "approval" => AgentDemand::Approval,
        _ => AgentDemand::None,
    };
    let activity = match agent.activity.as_str() {
        "working" => AgentActivity::Working,
        "stopped" => AgentActivity::Stopped,
        _ => AgentActivity::Unknown,
    };
    (demand, activity, agent.unread)
}

/// Reads a row's ownership back off its published fields.
pub fn ownership_of(agent: &SidebarAgentSnapshot) -> Ownership {
    if agent.delegated {
        Ownership::Delegated
    } else {
        Ownership::Operator
    }
}

pub fn group_of(agent: &SidebarAgentSnapshot) -> AgentGroup {
    let (demand, activity, unread) = axes_of(agent);
    agent_group_for(
        demand,
        activity,
        agent.completed,
        unread,
        agent.blocked,
        ownership_of(agent),
        agent.waiting_on_descendants,
    )
}

/// The demand axis of a projected row, for callers outside this module.
///
/// It exists so nothing has to compare the published axis name against a
/// string literal of its own: the vocabulary lives in [`AgentDemand`] and is
/// read back through it.
pub fn demand_of(agent: &SidebarAgentSnapshot) -> AgentDemand {
    axes_of(agent).0
}

/// Fills in every value the shell draws from the lifecycle axes.
///
/// Called again whenever the read axis moves, so the derived values can never
/// describe a different read state than the row they sit on.
fn derive_from_axes(agent: &mut SidebarAgentSnapshot) {
    let (demand, activity, unread) = axes_of(agent);
    let waiting = agent.waiting_on_descendants;
    let group = agent_group_for(
        demand,
        activity,
        agent.completed,
        unread,
        agent.blocked,
        ownership_of(agent),
        waiting,
    );
    agent.group = group.name().to_owned();
    agent.symbol = RowMark::of(agent).symbol().to_owned();
    // A row the operator still has to deal with is drawn bright; everything
    // already read or merely running is subdued.
    agent.emphasized = matches!(group, AgentGroup::NeedsYou | AgentGroup::Done);
    agent.status_code = if waiting {
        AgentStatusCode::Waiting
    } else {
        agent_status_code(demand, activity, agent.completed, unread)
    };
    let (status_word_visible, detail) = agent_second_line(
        group,
        demand,
        agent.expected_reply.as_deref(),
        agent.progress.as_deref(),
    );
    agent.status_word_visible = status_word_visible;
    agent.detail = detail;
    agent.requires_close_confirmation =
        agent_requires_close_confirmation(activity, demand, agent.blocked);
    agent.requires_close_status_check =
        agent_requires_close_status_check(activity, demand, agent.blocked);
    // A sleeping agent keeps its group, order and read axis; only its mark
    // and word say that its process is gone until it is opened (PRD B10).
    if let Some(sleep) = &agent.sleep {
        agent.symbol = crate::agent_sleep::SLEEPING_SYMBOL.to_owned();
        agent.status_code = crate::agent_sleep::status_code(sleep);
    }
}

/// The name of an agent kind as a person says it: the two providers Hide
/// starts by their product names, any other kind exactly as Herdr reports it.
pub(crate) fn provider_name(kind: Option<&str>) -> String {
    match non_empty(kind) {
        Some("claude") => "Claude".to_owned(),
        Some("codex") => "Codex".to_owned(),
        Some("opencode") => "OpenCode".to_owned(),
        Some(kind) => kind.to_owned(),
        None => "Agent".to_owned(),
    }
}

/// Re-derives every drawn value of a row whose sleep mark was just set.
pub fn rederive(agent: &mut SidebarAgentSnapshot) {
    derive_from_axes(agent);
}

fn project_agent(agent: SessionAgentPayload) -> Result<SidebarAgentSnapshot, String> {
    let pane_id = non_empty(agent.pane_id.as_deref().or(agent.id.as_deref()))
        .map(str::to_owned)
        .ok_or_else(|| "session agent is missing a pane id".to_owned())?;
    let last_activity = projected_last_activity(&agent, &pane_id)?;
    let demand = agent_demand(&agent);
    let activity = agent_activity(&agent);
    let completed = agent_completed(&agent);
    let blocked = agent.agent_status.as_deref() == Some("blocked");
    let workspace_label = non_empty(agent.workspace_label.as_deref())
        .or_else(|| {
            agent
                .cwd
                .as_deref()
                .and_then(|cwd| cwd.rsplit('/').find(|segment| !segment.trim().is_empty()))
        })
        .unwrap_or("workspace")
        .to_owned();
    // The label's title and sentences, each one line. The worker already
    // bounds them; the cut here is the same bound applied once more so a
    // value that outran it cannot reach a row.
    let label = agent.label.as_ref();
    let task = label.and_then(|label| line_text(label.task.as_deref(), MAX_LABEL_TEXT_CHARS));
    let progress =
        label.and_then(|label| line_text(label.progress.as_deref(), MAX_LABEL_TEXT_CHARS));
    let expected_reply = label
        .and_then(|label| line_text(label.expected_reply.as_deref(), MAX_EXPECTED_REPLY_CHARS));
    let message = label.and_then(agent_message);

    let agent_kind = non_empty(agent.agent.as_deref()).unwrap_or("unknown");
    // The name every surface uses (PRD overview-request-view D-13): the
    // label's task, else the agent's own title for the session, else what it
    // is, never the Herdr workspace it happens to run in (PRD
    // checkout-workspace-binding D-09), which names another checkout as often
    // as this one.
    let native_title = agent
        .facts
        .as_ref()
        .and_then(|facts| line_text(facts.native_title.as_deref(), MAX_LABEL_TEXT_CHARS));
    let identity_label = task
        .or(native_title)
        .unwrap_or_else(|| provider_name(agent.agent.as_deref()));
    let projected = SidebarAgentSnapshot {
        id: agent.id.unwrap_or_else(|| pane_id.clone()),
        pane_id,
        workspace_label,
        checkout_label: None,
        agent_kind: agent_kind.to_owned(),
        demand: demand.name().to_owned(),
        activity: activity.name().to_owned(),
        completed,
        // Every agent is projected unread; the read axis and everything
        // derived from it are set once, by `apply_read_state`, where the
        // pane read record lives.
        unread: true,
        blocked,
        group: String::new(),
        symbol: String::new(),
        emphasized: false,
        status_code: AgentStatusCode::Unknown,
        requires_close_confirmation: false,
        requires_close_status_check: false,
        identity_label,
        progress,
        expected_reply,
        detail: None,
        message,
        status_word_visible: true,
        changed_at_unix_ms: agent.changed_at_unix_ms,
        last_activity,
        state_change_seq: agent.state_change_seq,
        session_id: agent
            .agent_session
            .as_ref()
            .filter(|session| session.kind == "id")
            .map(|session| session.value.clone())
            .filter(|value| !value.trim().is_empty()),
        own_find: crate::agent_find::agent_find(agent_kind).is_some(),
        spawned_from_pane_id: non_empty(agent.spawned_from_pane_id.as_deref()).map(str::to_owned),
        declared_parent_pane_id: non_empty(agent.spawned_from_pane_id.as_deref())
            .map(str::to_owned),
        spawned_from_machine_id: non_empty(agent.spawned_from_machine_id.as_deref())
            .map(str::to_owned),
        declared_parent_session: non_empty(agent.declared_parent_session.as_deref())
            .map(str::to_owned),
        lineage_session: non_empty(agent.lineage_session.as_deref()).map(str::to_owned),
        delegated: false,
        descendant_counts: crate::model::DescendantCountsSnapshot::default(),
        waiting_on_descendants: false,
        descendant_signals: BTreeSet::new(),
        lineage_parent_pane_id: None,
        lineage_path_pane_ids: Vec::new(),
        lineage_sibling_pane_ids: Vec::new(),
        lineage_depth: 0,
        lineage_child_pane_ids: Vec::new(),
        close_descendant_pane_ids: Vec::new(),
        lineage_root_checkout_id: None,
        lineage_worktree_badge: None,
        lineage_orphan: false,
        lineage_hint: None,
        raised_hint: None,
        spawn_origin_pane_id: None,
        lineage_collapsed: false,
        sleep: None,
        row_facts: agent.facts,
        request: None,
    };
    Ok(projected)
}

/// The prefix every pane id on a remote target carries. A pane id without it
/// belongs to the local Herdr server.
const REMOTE_PANE_ID_PREFIX: &str = "remote:";

/// Which slice of the read record ledger a pass is allowed to prune.
///
/// The ledger holds records for several Herdr servers. A pass that pruned
/// every key its own authoritative topology did not claim would drop a remote
/// pane's record on the next local sync and the reverse, so a stopped remote
/// pane the operator had already read came back as `Done` and demanded a close
/// confirmation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadRecordScope<'a> {
    /// Prune nothing. The caller has no authoritative pane topology.
    Retain,
    /// The caller holds the local server's full pane topology. Remote records
    /// are left alone.
    Local,
    /// The caller holds one remote target's full pane topology. The argument
    /// is that target's pane id prefix, and only records under it are dropped.
    Remote(&'a str),
}

impl ReadRecordScope<'_> {
    /// Whether this pass owns the record keyed by `pane_id` and may drop it.
    pub(crate) fn owns(self, pane_id: &str) -> bool {
        match self {
            Self::Retain => false,
            Self::Local => !pane_id.starts_with(REMOTE_PANE_ID_PREFIX),
            Self::Remote(prefix) => pane_id.starts_with(prefix),
        }
    }
}

/// One pane's read record moving, for the diagnostic that records it.
///
/// An eviction carries an empty record, so it needs its own flag: without one
/// a dropped record and a record raised on a pane with no sequence and no
/// demand read identically in the log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadRecordChange {
    pub pane_id: String,
    pub record: PaneReadRecord,
    pub evicted: bool,
}

/// What the operator is looking at on this pane right now, including what
/// its descendants are asking for or have finished.
fn read_fingerprint(agent: &SidebarAgentSnapshot) -> PaneReadRecord {
    PaneReadRecord {
        state_change_seq: agent.state_change_seq,
        session_id: agent.session_id.clone(),
        demand: agent.demand.clone(),
        activity: agent.activity.clone(),
        completed: agent.completed,
        descendant_signals: agent.descendant_signals.clone(),
    }
}

/// Whether a record still covers what the row shows now.
///
/// The row's own fields have to match exactly. A descendant signal is news
/// only when it is missing from the record: a signal the record holds and
/// the row no longer shows is a question answered or a finished child back
/// at work, neither of which calls the operator (PRD B5, B6).
fn record_covers(record: &PaneReadRecord, current: &PaneReadRecord) -> bool {
    record.state_change_seq == current.state_change_seq
        && record.session_id == current.session_id
        && record.demand == current.demand
        && record.activity == current.activity
        && record.completed == current.completed
        && current
            .descendant_signals
            .is_subset(&record.descendant_signals)
}

/// Rebases process-local sequence values after a Herdr connection bootstrap.
/// A restored agent keeps its read mark only when its process-local sequence
/// moved backwards, its stable session identity (when one was recorded), and
/// its operator-visible state still match. A sequence that moved forward is
/// new work completed while Hide was disconnected and must remain unread. The
/// pending set lets an agent that appears after the first restored topology be
/// reconciled when detection catches up.
pub fn reconcile_read_records(
    agents: &[SidebarAgentSnapshot],
    records: &mut BTreeMap<String, PaneReadRecord>,
    pending: &mut HashSet<String>,
) -> Vec<ReadRecordChange> {
    let mut changes = Vec::new();
    for agent in agents {
        if !pending.remove(&agent.pane_id) {
            continue;
        }
        let Some(record) = records.get(&agent.pane_id) else {
            continue;
        };
        let session_matches =
            record.session_id.is_none() || record.session_id.as_ref() == agent.session_id.as_ref();
        let sequence_reset = matches!(
            (record.state_change_seq, agent.state_change_seq),
            (Some(previous), Some(current)) if current < previous
        );
        if !sequence_reset
            || !session_matches
            || record.demand != agent.demand
            || record.activity != agent.activity
            || record.completed != agent.completed
        {
            continue;
        }
        // Only the row's own sequence is rebased. A descendant signal that
        // arrived while Hide was disconnected stays outside the record, so
        // the read pass still finds it as news (PRD B5).
        let mut current = read_fingerprint(agent);
        current.descendant_signals = record.descendant_signals.clone();
        if record != &current {
            records.insert(agent.pane_id.clone(), current.clone());
            changes.push(ReadRecordChange {
                pane_id: agent.pane_id.clone(),
                record: current,
                evicted: false,
            });
        }
    }
    changes
}

/// Drops records only when an authoritative pane topology says the pane is
/// gone. Agent detection may legitimately be empty or incomplete while a
/// restored server rebuilds its agent list, so it is never an eviction source.
pub fn prune_read_records(
    records: &mut BTreeMap<String, PaneReadRecord>,
    live_pane_ids: &HashSet<String>,
    scope: ReadRecordScope<'_>,
) -> Vec<ReadRecordChange> {
    let evicted = records
        .keys()
        .filter(|pane_id| scope.owns(pane_id) && !live_pane_ids.contains(*pane_id))
        .cloned()
        .collect::<Vec<_>>();
    let mut changes = Vec::with_capacity(evicted.len());
    for pane_id in evicted {
        records.remove(&pane_id);
        changes.push(ReadRecordChange {
            pane_id,
            record: PaneReadRecord::default(),
            evicted: true,
        });
    }
    changes
}

/// Raises the operator-focused pane's read record and sets every row's read
/// axis and derived values. Authoritative topology pruning is separate because
/// an agent list is not a pane list during server restoration.
///
/// This is the whole read authority. `operator_pane_id` is the pane the
/// operator chose to look at, never the pane Herdr happens to report focused:
/// Herdr marks every pane in a tab seen the moment the tab is focused, so
/// nothing here reads Herdr's `done` or `idle` or a token's `_new` suffix to
/// decide it. Running it twice over the same agents and the same pane changes
/// nothing the second time (engineering rule 11).
pub fn apply_read_state(
    agents: &mut [SidebarAgentSnapshot],
    records: &mut BTreeMap<String, PaneReadRecord>,
    operator_pane_id: Option<&str>,
) -> Vec<ReadRecordChange> {
    let mut changes = Vec::new();
    for agent in agents.iter_mut() {
        if operator_pane_id == Some(agent.pane_id.as_str()) {
            let current = read_fingerprint(agent);
            if records.get(&agent.pane_id) != Some(&current) {
                records.insert(agent.pane_id.clone(), current.clone());
                changes.push(ReadRecordChange {
                    pane_id: agent.pane_id.clone(),
                    record: current,
                    evicted: false,
                });
            }
            continue;
        }
        // A descendant signal that has gone away is dropped from the record
        // so the same descendant can be news again later; dropping never
        // changes the read axis, because a subset stays a subset.
        let Some(record) = records.get_mut(&agent.pane_id) else {
            continue;
        };
        let before = record.descendant_signals.len();
        record
            .descendant_signals
            .retain(|signal| agent.descendant_signals.contains(signal));
        if record.descendant_signals.len() != before {
            changes.push(ReadRecordChange {
                pane_id: agent.pane_id.clone(),
                record: record.clone(),
                evicted: false,
            });
        }
    }

    derive_read_state(agents, records, operator_pane_id);
    changes
}

/// Sets the read axis and every value derived from it.
///
/// The pane tree is projected from its own `project_agents` call rather than
/// from the navigator's agent rows, and a projection that skipped this
/// published `Done` and demanded a close confirmation for every pane the
/// operator had already read.
fn derive_read_state(
    agents: &mut [SidebarAgentSnapshot],
    records: &BTreeMap<String, PaneReadRecord>,
    operator_pane_id: Option<&str>,
) {
    for agent in agents.iter_mut() {
        // The pane the operator is looking at is read as of now whether or not
        // the ledger has caught up in this dispatch, so the two projections
        // agree regardless of which one the runtime builds first.
        let read = operator_pane_id == Some(agent.pane_id.as_str())
            || records
                .get(&agent.pane_id)
                .is_some_and(|record| record_covers(record, &read_fingerprint(agent)));
        agent.unread = !read;
        derive_from_axes(agent);
    }
    sort_agents(agents);
}

/// The demand axis. A question outranks an approval, so the worse thing
/// waiting is the one the row names.
fn agent_demand(agent: &SessionAgentPayload) -> AgentDemand {
    if agent.label.as_ref().is_some_and(|label| label.question) {
        AgentDemand::Question
    } else if agent.agent_status.as_deref() == Some("blocked") {
        AgentDemand::Approval
    } else {
        AgentDemand::None
    }
}

/// The activity axis. A state Herdr does not name is reported as unknown
/// rather than folded into idle (engineering rule 4).
fn agent_activity(agent: &SessionAgentPayload) -> AgentActivity {
    match agent.agent_status.as_deref() {
        Some("working") => AgentActivity::Working,
        Some("idle") | Some("done") => AgentActivity::Stopped,
        _ => AgentActivity::Unknown,
    }
}

/// Whether this stopped pane has actually completed work: Herdr's `done`.
/// It is deliberately separate from activity and read state; Herdr's
/// tab-scoped seen value never decides Hide's pane-level read axis.
fn agent_completed(agent: &SessionAgentPayload) -> bool {
    agent.agent_status.as_deref() == Some("done")
}

/// The ordering key: when the core saw the agent change state, as thirteen
/// digits, or Herdr's own state change sequence padded to twenty when the
/// core has not observed it.
fn projected_last_activity(agent: &SessionAgentPayload, pane_id: &str) -> Result<String, String> {
    match (agent.changed_at_unix_ms, agent.state_change_seq) {
        (Some(changed), _) => Ok(format!("{changed:013}")),
        (None, Some(sequence)) => Ok(format!("{sequence:020}")),
        (None, None) => Err(format!(
            "agent {pane_id} has neither a state change time nor state_change_seq"
        )),
    }
}

/// One line of label text: whitespace collapsed, empty dropped, cut at
/// `max_chars`.
fn line_text(value: Option<&str>, max_chars: usize) -> Option<String> {
    value.and_then(|value| crate::display_text::one_line(value, max_chars))
}

/// What the agent's label last said (PRD overview-lenses-tiles-agents
/// D-50): the expected reply and the progress whole, request first, each
/// trimmed but not collapsed or cut to a row, joined by a line break, and
/// each held to `MAX_LABEL_TEXT_CHARS` so a sentence cannot grow the
/// snapshot.
fn agent_message(label: &AgentLabel) -> Option<String> {
    let parts: Vec<String> = [label.expected_reply.as_deref(), label.progress.as_deref()]
        .into_iter()
        .flatten()
        .map(|value| {
            value
                .trim()
                .chars()
                .take(MAX_LABEL_TEXT_CHARS)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// The session every fixture row runs and every fixture declaration was
/// written for (`wire::session_digest`).
#[cfg(test)]
pub(crate) fn fixture_lineage_session() -> String {
    crate::wire::session_digest("fixture-lineage-session").unwrap()
}

/// A declared parent is proven by sessions (`wire::session_holds`), and the
/// fixtures that name parents do not care which session runs where, so each
/// row runs one shared session and each declaration was written for it:
/// whichever row a test makes another's parent still holds. A test about the
/// proof itself sets the sessions it means.
#[cfg(test)]
pub(crate) fn lineage_fixture_value(mut value: Value) -> Value {
    if let Some(agents) = value.get_mut("agents").and_then(Value::as_array_mut) {
        for agent in agents {
            if agent.get("lineage_session").is_none() {
                agent["lineage_session"] = Value::String(fixture_lineage_session());
            }
            let declares_parent = agent
                .get("spawned_from_pane_id")
                .is_some_and(|parent| !parent.is_null());
            if declares_parent && agent.get("declared_parent_session").is_none() {
                agent["declared_parent_session"] = Value::String(fixture_lineage_session());
            }
        }
    }
    value
}

/// Status and lineage fixtures were written as the retired plugin's tokens.
/// This turns them into what the core's label worker lays on the payload:
/// `task`, `progress`, `expected_reply` and a `status_question*` token become
/// the agent's `label`, and an `activity` token its state change time. Other
/// tokens stay, as the payload carries them.
#[cfg(test)]
pub(crate) fn owned_label_fixture<T: serde::de::DeserializeOwned>(
    value: Value,
) -> Result<T, serde_json::Error> {
    let mut value = lineage_fixture_value(value);
    if let Some(agents) = value.get_mut("agents").and_then(Value::as_array_mut) {
        for agent in agents {
            let Some(tokens) = agent.get_mut("tokens").and_then(Value::as_object_mut) else {
                continue;
            };
            let text = |tokens: &mut serde_json::Map<String, Value>, key: &str| {
                tokens
                    .remove(key)
                    .and_then(|value| value.as_str().map(str::to_owned))
            };
            let task = text(tokens, "task");
            let progress = text(tokens, "progress");
            let expected_reply = text(tokens, "expected_reply");
            let question = ["status_question", "status_question_new"]
                .iter()
                .any(|key| tokens.remove(*key).is_some_and(|value| !value.is_null()));
            let activity = text(tokens, "activity").and_then(|value| value.parse::<u64>().ok());
            // The plugin mirrored Herdr's lifecycle into a status token; a
            // fixture that gave only the token meant that lifecycle.
            let present = |name: &str| {
                [name.to_owned(), format!("{name}_new")]
                    .iter()
                    .any(|key| tokens.get(key).is_some_and(|value| !value.is_null()))
            };
            let status = if present("status_working") {
                Some("working")
            } else if present("status_approval") {
                Some("blocked")
            } else if present("status_done") {
                Some("done")
            } else if present("status_idle") || question {
                Some("idle")
            } else {
                None
            };
            tokens.retain(|key, _| !key.starts_with("status_"));
            if let Some(status) = status
                && agent.get("agent_status").is_none_or(Value::is_null)
            {
                agent["agent_status"] = Value::from(status);
            }
            if task.is_some() || progress.is_some() || expected_reply.is_some() || question {
                agent["label"] = serde_json::json!({
                    "task": task,
                    "progress": progress,
                    "expected_reply": expected_reply,
                    "question": question,
                });
            }
            if let Some(activity) = activity {
                agent["changed_at_unix_ms"] = Value::from(activity);
            }
        }
    }
    serde_json::from_value(value)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_agent_carries_its_state_change_time_only_when_the_worker_measured_one() {
        let agents = project_agents(payload(json!([
            {"pane_id": "measured", "workspace_label": "W", "agent": "codex", "agent_status": "running", "state_change_seq": 1, "tokens": {"activity": "1700000000000"}},
            {"pane_id": "unmeasured", "workspace_label": "W", "agent": "codex", "agent_status": "running", "state_change_seq": 1, "tokens": {}}
        ])))
        .agents;
        let changed_at = |pane: &str| {
            agents
                .iter()
                .find(|agent| agent.pane_id == pane)
                .map(|agent| agent.changed_at_unix_ms)
        };
        assert_eq!(changed_at("measured"), Some(Some(1_700_000_000_000)));
        assert_eq!(changed_at("unmeasured"), Some(None));
    }

    #[test]
    fn checkout_purpose_uses_agent_then_pull_request_after_persistent_sources() {
        use crate::model::{
            CheckoutPurposeOrigin, CheckoutPurposeSnapshot, CheckoutSnapshot, PullRequestBadge,
            PullRequestChecks, PullRequestSnapshot, WorkspaceSnapshot,
        };
        let mut checkout = CheckoutSnapshot {
            id: "checkout".into(),
            agent_summary: crate::model::CheckoutAgentSummary {
                representative_pane_id: Some("pane".into()),
                ..Default::default()
            },
            pull_request: Some(PullRequestSnapshot {
                closing_issues: Default::default(),
                number: 18,
                title: "Pull request fallback".into(),
                head_branch: "feature".into(),
                base_branch: "main".into(),
                url: "https://example.invalid/pull/18".into(),
                badge: PullRequestBadge::Open,
                review: None,
                checks: PullRequestChecks::default(),
                is_draft: false,
                merged_at_unix_ms: None,
                updated_at_unix_ms: None,
                created_at_unix_ms: None,
                closed_at_unix_ms: None,
                head_oid: None,
                cross_repository: false,
            }),
            ..Default::default()
        };
        let workspace = |checkout| WorkspaceSnapshot {
            home_issues: Default::default(),
            pull_requests: Vec::new(),
            tasks: Default::default(),
            id: "project".into(),
            label: "Project".into(),
            path: "/fixture/project".into(),
            remote_target_id: None,
            expanded: true,
            device_id: "local".into(),
            repo_name: "Project".into(),
            is_git: true,
            default_branch: Some("main".into()),
            branches: vec![],
            registered: true,
            temporary: false,
            session_workspace_ids: vec![],
            last_activity_unix_ms: None,
            checkouts: vec![checkout],
            pinned: false,
            is_home: false,
            inactive_checkouts: Default::default(),
            removal: Default::default(),
            disk: Default::default(),
            cleanup: None,
        };
        let agent = project_agents(payload(json!([{
            "pane_id": "pane",
            "workspace_label": "Workspace",
            "agent": "codex",
            "agent_status": "running",
            "state_change_seq": 1,
            "tokens": {"task": "Agent session title"}
        }])))
        .agents
        .remove(0);
        let mut workspaces = vec![workspace(checkout.clone())];

        assert!(sync_checkout_purposes(&mut workspaces, &[agent]));
        assert_eq!(
            workspaces[0].checkouts[0].purpose,
            Some(CheckoutPurposeSnapshot {
                text: "Agent session title".into(),
                origin: CheckoutPurposeOrigin::AgentTitle,
            })
        );

        checkout.agent_summary.representative_pane_id = None;
        let mut workspaces = vec![workspace(checkout.clone())];
        assert!(sync_checkout_purposes(&mut workspaces, &[]));
        assert_eq!(
            workspaces[0].checkouts[0]
                .purpose
                .as_ref()
                .map(|purpose| purpose.origin),
            Some(CheckoutPurposeOrigin::PullRequestTitle)
        );

        checkout.purpose = Some(CheckoutPurposeSnapshot {
            text: "Live token".into(),
            origin: CheckoutPurposeOrigin::Token,
        });
        let mut workspaces = vec![workspace(checkout)];
        assert!(!sync_checkout_purposes(&mut workspaces, &[]));
        assert_eq!(
            workspaces[0].checkouts[0].purpose.as_ref().unwrap().text,
            "Live token"
        );
    }

    #[test]
    fn workspace_summary_uses_physical_ownership_priority_and_unique_panes() {
        use crate::model::{
            AgentStatusCode, CheckoutSnapshot, PaneSnapshot, TabSnapshot, WorkspaceSnapshot,
        };
        let checkout = |id: &str, panes: &[&str]| CheckoutSnapshot {
            id: id.to_owned(),
            tabs: vec![TabSnapshot {
                agent: None,
                naming: Default::default(),
                panes: panes
                    .iter()
                    .map(|id| PaneSnapshot {
                        id: (*id).to_owned(),
                        herdr_label: None,
                        terminal_title: None,
                        cwd: "/fixture".to_owned(),
                        status_code: AgentStatusCode::Unknown,
                        requires_close_confirmation: false,
                        requires_close_status_check: false,
                        identity_label: None,
                        activity_at_unix_ms: None,
                        fork: Default::default(),
                        ports: vec![],
                        servers: vec![],
                        children: None,
                        lineage_path: Vec::new(),
                        sleep: None,
                        sleep_action: None,
                    })
                    .collect(),
                id: None,
                workspace_id: None,
                checkout_id: None,
                label: None,
                empty: false,
                delegated: false,
            }],
            ..Default::default()
        };
        let mut workspaces = vec![WorkspaceSnapshot {
            home_issues: Default::default(),
            pull_requests: Vec::new(),
            tasks: Default::default(),
            checkouts: vec![
                checkout(
                    "main",
                    &[
                        "error", "question", "done", "working", "unknown", "shell", "done",
                    ],
                ),
                checkout("child", &["child"]),
                checkout("empty", &["plain"]),
            ],
            id: "project".to_owned(),
            label: "Project".to_owned(),
            path: "/fixture".to_owned(),
            remote_target_id: None,
            expanded: true,
            device_id: "local".to_owned(),
            repo_name: "Project".to_owned(),
            is_git: true,
            default_branch: None,
            branches: vec![],
            registered: true,
            temporary: false,
            session_workspace_ids: vec![],
            last_activity_unix_ms: None,
            pinned: false,
            is_home: false,
            inactive_checkouts: Default::default(),
            removal: Default::default(),
            disk: Default::default(),
            cleanup: None,
        }];
        let mut agents = project_agents(payload(json!([
            {"pane_id":"error", "state_change_seq":1, "agent_status":"idle"},
            {"pane_id":"question", "state_change_seq":1, "agent_status":"idle", "tokens":{"status_question":"?"}},
            {"pane_id":"done", "state_change_seq":1, "agent_status":"done"},
            {"pane_id":"working", "state_change_seq":1, "agent_status":"working"},
            {"pane_id":"unknown", "state_change_seq":1, "agent_status":"unknown"},
            {"pane_id":"child", "state_change_seq":1, "agent_status":"blocked", "spawned_from_pane_id":"working"}
        ])))
        .agents;
        // No input reports an error today (labels-in-hided D-06); the row
        // is set on the axis so the summary's rule for it stays covered.
        let error = agents.iter_mut().find(|a| a.pane_id == "error").unwrap();
        error.demand = "error".to_owned();
        error.unread = false;
        derive_from_axes(error);
        let duplicate = agents.iter().find(|a| a.pane_id == "done").unwrap().clone();
        agents.push(duplicate);
        assert!(sync_checkout_agent_summaries(&mut workspaces, &agents));
        let summaries = &workspaces[0].checkouts;
        assert_eq!(
            summaries[0].agent_summary.representative_pane_id.as_deref(),
            Some("question")
        );
        assert_eq!(
            (
                summaries[0].agent_summary.needs_you,
                summaries[0].agent_summary.done,
                summaries[0].agent_summary.working,
                summaries[0].agent_summary.seen,
                summaries[0].agent_summary.unknown
            ),
            (1, 1, 1, 2, 1)
        );
        // Each row counts under the mark it draws: the read error keeps its
        // ×, and the unknown row draws `~`, which no badge claims.
        assert_eq!(
            summaries[0].agent_summary.marks,
            crate::model::MarkCountsSnapshot {
                error: 1,
                question: 1,
                working: 1,
                done: 1,
                ..Default::default()
            }
        );
        assert_eq!(
            summaries[1].agent_summary.representative_pane_id.as_deref(),
            Some("child")
        );
        assert_eq!(summaries[2].agent_summary, Default::default());
        assert!(!sync_checkout_agent_summaries(&mut workspaces, &agents));
        let question = agents.iter_mut().find(|a| a.pane_id == "question").unwrap();
        question.unread = false;
        derive_from_axes(question);
        sync_checkout_agent_summaries(&mut workspaces, &agents);
        assert_eq!(
            workspaces[0].checkouts[0]
                .agent_summary
                .representative_pane_id
                .as_deref(),
            Some("done")
        );
        agents.retain(|a| a.pane_id != "done");
        sync_checkout_agent_summaries(&mut workspaces, &agents);
        assert_eq!(
            workspaces[0].checkouts[0]
                .agent_summary
                .representative_pane_id
                .as_deref(),
            Some("working")
        );
    }

    fn payload(agents: Value) -> SessionSnapshotPayload {
        owned_label_fixture(json!({"agents": agents})).expect("valid fixture")
    }

    #[test]
    fn current_label_changes_preserve_read_records_lineage_and_control_names() {
        let snapshot = |task: &str| {
            payload(json!([
                {"pane_id":"root","name":"control-root","agent":"claude","agent_status":"working","state_change_seq":5,
                 "agent_session":{"kind":"id","value":"same-root"},"tokens":{"task":task,"progress":"현재 진행"}},
                {"pane_id":"child","name":"control-child","agent":"claude","agent_status":"idle","state_change_seq":6,
                 "agent_session":{"kind":"id","value":"same-child"},"spawned_from_pane_id":"root","tokens":{"task":task,"status_question_new":"?"}}
            ]))
        };
        let mut before = project_agents(snapshot("기존 현재 작업")).agents;
        let mut records = BTreeMap::new();
        apply_read_state(&mut before, &mut records, Some("root"));
        let saved = records.clone();
        let mut after = project_agents(snapshot("갱신된 현재 작업")).agents;
        assert!(apply_read_state(&mut after, &mut records, None).is_empty());
        assert_eq!(records, saved);
        for (before, after) in before.iter().zip(&after) {
            assert_eq!(before.id, after.id);
            assert_eq!(before.unread, after.unread);
            assert_eq!(ownership_of(before), ownership_of(after));
            assert_eq!(before.descendant_counts, after.descendant_counts);
            assert_eq!(before.group, after.group);
        }
        assert_eq!(after[0].identity_label, "갱신된 현재 작업");
    }

    /// AC1. A question comes only from the core's label and outranks the
    /// approval a blocked pane shows; a label without one leaves the demand
    /// to Herdr's lifecycle.
    #[test]
    fn a_labels_question_is_the_only_question_demand() {
        let label = |question: bool| json!({"task": "작업", "question": question});
        let projected = project_agents(payload(json!([
            {"pane_id":"asks","agent_status":"idle","state_change_seq":1,"label":label(true)},
            {"pane_id":"asks-done","agent_status":"done","state_change_seq":2,"label":label(true)},
            {"pane_id":"asks-blocked","agent_status":"blocked","state_change_seq":3,"label":label(true)},
            {"pane_id":"quiet","agent_status":"idle","state_change_seq":4,"label":label(false)},
            {"pane_id":"blocked","agent_status":"blocked","state_change_seq":5,"label":label(false)}
        ])))
        .agents;
        let axes = projected
            .iter()
            .map(|agent| {
                (
                    agent.pane_id.as_str(),
                    (
                        agent.demand.as_str(),
                        agent.activity.as_str(),
                        agent.symbol.as_str(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(axes["asks"], ("question", "stopped", "?"));
        assert_eq!(axes["asks-done"], ("question", "stopped", "?"));
        assert_eq!(axes["asks-blocked"], ("question", "unknown", "?"));
        assert_eq!(axes["quiet"], ("none", "stopped", "\u{25cb}"));
        assert_eq!(axes["blocked"], ("approval", "unknown", "!"));
    }

    /// AC1. Herdr's own lifecycle words, with no plugin token at all. Blocked
    /// is an approval, and `done` and `idle` produce the same activity. They
    /// differ on completion evidence: an idle agent is merely ready, while a
    /// done agent has a result to review. Neither lifecycle decides Hide's
    /// pane-level read state.
    #[test]
    fn axes_map_herdr_lifecycles_with_blocked_as_approval() {
        let projection = project_agents(payload(json!([
            {"pane_id":"working","agent_status":"working","state_change_seq":1},
            {"pane_id":"blocked","agent_status":"blocked","state_change_seq":2},
            {"pane_id":"done","agent_status":"done","state_change_seq":3},
            {"pane_id":"idle","agent_status":"idle","state_change_seq":4},
            {"pane_id":"unknown","agent_status":"unknown","state_change_seq":5}
        ])));

        assert!(projection.excluded.is_empty());
        let axes = projection
            .agents
            .iter()
            .map(|agent| {
                (
                    agent.pane_id.as_str(),
                    (
                        agent.demand.as_str(),
                        agent.activity.as_str(),
                        agent.completed,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(axes["working"], ("none", "working", false));
        assert_eq!(axes["blocked"], ("approval", "unknown", false));
        assert_eq!(axes["done"], ("none", "stopped", true));
        assert_eq!(axes["idle"], ("none", "stopped", false));
        assert_eq!(axes["unknown"], ("none", "unknown", false));
        assert_eq!(
            axes["done"].1, axes["idle"].1,
            "done and idle are both stopped"
        );
        assert_ne!(
            axes["done"].2, axes["idle"].2,
            "only a reported completion can become Done"
        );
        let idle = projection
            .agents
            .iter()
            .find(|agent| agent.pane_id == "idle")
            .expect("idle agent");
        assert_eq!(idle.group, "seen");
        assert_eq!(idle.status_code, AgentStatusCode::Idle);
        assert_eq!(idle.symbol, "○");
        let done = projection
            .agents
            .iter()
            .find(|agent| agent.pane_id == "done")
            .expect("completed agent");
        assert_eq!(done.group, "done");
        assert_eq!(done.status_code, AgentStatusCode::Done);
        assert_eq!(done.symbol, "✓");
        assert_eq!(
            projection.agents.len(),
            5,
            "an agent with no plugin token still projects"
        );
    }

    /// AC1, AC10. A blocked pane sits in Needs You whether or not it has been
    /// read, because the approval prompt is still on screen.
    #[test]
    fn axes_hold_a_blocked_pane_in_needs_you_after_it_is_read() {
        assert_eq!(
            agent_group_for(
                AgentDemand::Approval,
                AgentActivity::Unknown,
                false,
                false,
                true,
                Ownership::Operator,
                false
            ),
            AgentGroup::NeedsYou
        );
        assert_eq!(
            agent_group_for(
                AgentDemand::Approval,
                AgentActivity::Unknown,
                false,
                false,
                false,
                Ownership::Operator,
                false
            ),
            AgentGroup::Seen
        );
    }

    /// AC10. Closing a pane needs confirmation for running work or unresolved
    /// demand. An unknown activity is a separate status-check outcome, not a
    /// confirmation that guesses whether work is running.
    #[test]
    fn axes_require_close_confirmation_or_status_check_from_work_and_demand() {
        let cases = [
            (
                AgentDemand::None,
                AgentActivity::Working,
                false,
                false,
                true,
                false,
            ),
            (
                AgentDemand::Question,
                AgentActivity::Stopped,
                true,
                false,
                true,
                false,
            ),
            (
                AgentDemand::Approval,
                AgentActivity::Unknown,
                false,
                true,
                true,
                false,
            ),
            (
                AgentDemand::None,
                AgentActivity::Stopped,
                true,
                false,
                false,
                false,
            ),
            (
                AgentDemand::None,
                AgentActivity::Stopped,
                false,
                false,
                false,
                false,
            ),
            (
                AgentDemand::Question,
                AgentActivity::Stopped,
                false,
                false,
                true,
                false,
            ),
            (
                AgentDemand::None,
                AgentActivity::Unknown,
                true,
                false,
                false,
                true,
            ),
        ];
        for (demand, activity, unread, blocked, expected, status_check) in cases {
            let _group = agent_group_for(
                demand,
                activity,
                false,
                unread,
                blocked,
                Ownership::Operator,
                false,
            );
            assert_eq!(
                agent_requires_close_confirmation(activity, demand, blocked),
                expected,
                "{demand:?} {activity:?} unread={unread} blocked={blocked}"
            );
            assert_eq!(
                agent_requires_close_status_check(activity, demand, blocked),
                status_check,
                "status check: {demand:?} {activity:?} unread={unread} blocked={blocked}"
            );
        }
    }

    /// AC10. The shell keeps no list of agent state names of its own.
    ///
    /// Five copies of that vocabulary had drifted apart, which is how the pet
    /// dashboard and the sidebar came to disagree about whether a finished
    /// agent needs attention. The shell now reads the values derived here, so
    /// two marks of the old vocabulary are refused: the retired
    /// `unseen_completion` anywhere at all, and any literal array that lists
    /// both `blocked` and `question`, which only the retired state arrays did.
    #[test]
    fn axes_no_state_name_array_remains_in_the_shell() {
        let shell = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/src")
            .canonicalize()
            .expect("the shell sits beside the core");
        let mut offenders = Vec::new();
        let mut scanned = 0_usize;
        let mut stack = vec![shell];
        while let Some(entry) = stack.pop() {
            if entry.is_dir() {
                for child in std::fs::read_dir(&entry).expect("readable directory") {
                    stack.push(child.expect("readable entry").path());
                }
                continue;
            }
            if !matches!(
                entry.extension().and_then(|value| value.to_str()),
                Some("ts" | "tsx")
            ) {
                continue;
            }
            scanned += 1;
            let text = std::fs::read_to_string(&entry).expect("readable source");
            if text.contains("unseen_completion") {
                offenders.push(format!(
                    "{}: the retired unseen_completion state",
                    entry.display()
                ));
            }
            let mut rest = text.as_str();
            while let Some(open) = rest.find('[') {
                let after = &rest[open + 1..];
                let Some(close) = after.find(']') else { break };
                let span = &after[..close];
                if span.contains("\"blocked\"") && span.contains("\"question\"") {
                    offenders.push(format!("{}: a literal agent state array", entry.display()));
                }
                rest = &after[close + 1..];
            }
        }
        assert!(scanned > 0, "no shell sources were scanned");
        assert!(
            offenders.is_empty(),
            "the shell still keeps its own agent state vocabulary: {offenders:#?}"
        );
    }

    /// AC10. No view builds a label out of an axis value, so nothing
    /// underscored can reach the screen.
    #[test]
    fn axes_map_to_one_status_code_each() {
        let labels = [
            (
                AgentDemand::Question,
                AgentActivity::Unknown,
                false,
                true,
                AgentStatusCode::Question,
            ),
            (
                AgentDemand::Approval,
                AgentActivity::Unknown,
                false,
                true,
                AgentStatusCode::Approval,
            ),
            (
                AgentDemand::Error,
                AgentActivity::Unknown,
                false,
                true,
                AgentStatusCode::Error,
            ),
            (
                AgentDemand::None,
                AgentActivity::Working,
                false,
                true,
                AgentStatusCode::Working,
            ),
            (
                AgentDemand::None,
                AgentActivity::Stopped,
                true,
                true,
                AgentStatusCode::Done,
            ),
            (
                AgentDemand::None,
                AgentActivity::Stopped,
                false,
                true,
                AgentStatusCode::Idle,
            ),
            (
                AgentDemand::None,
                AgentActivity::Stopped,
                true,
                false,
                AgentStatusCode::Idle,
            ),
            (
                AgentDemand::None,
                AgentActivity::Unknown,
                false,
                true,
                AgentStatusCode::Unknown,
            ),
        ];
        for (demand, activity, completed, unread, expected) in labels {
            assert_eq!(
                agent_status_code(demand, activity, completed, unread),
                expected
            );
        }
    }

    /// AC5, R3, R4. Group membership follows the definitions, the order is
    /// group first and most recent activity second, and the label plugin's
    /// `sort_rank` token has no say: every row here carries one that would
    /// invert the result if Hide still read it.
    #[test]
    fn group_order_follows_the_four_groups_then_recent_activity() {
        let mut agents = projected(json!([
            {"pane_id":"read-idle","agent_status":"idle","state_change_seq":1,
             "tokens":{"status_idle":"○","sort_rank":"00","activity":"0000000000009"}},
            {"pane_id":"done-older","agent_status":"done","state_change_seq":2,
             "tokens":{"status_done_new":"●","sort_rank":"00","activity":"0000000000001"}},
            {"pane_id":"working","agent_status":"working","state_change_seq":3,
             "tokens":{"status_working":"●","sort_rank":"99","activity":"0000000000008"}},
            {"pane_id":"done-newer","agent_status":"done","state_change_seq":4,
             "tokens":{"status_done_new":"●","sort_rank":"00","activity":"0000000000002"}},
            {"pane_id":"asking","agent_status":"idle","state_change_seq":5,
             "tokens":{"status_question_new":"?","sort_rank":"99","activity":"0000000000000"}},
            {"pane_id":"blocked","agent_status":"blocked","state_change_seq":6,
             "tokens":{"activity":"0000000000003"}}
        ]));
        let mut records = BTreeMap::new();
        // An ordinary idle pane stays Seen even before a read record exists.
        records.insert(
            "read-idle".to_owned(),
            PaneReadRecord {
                state_change_seq: Some(1),
                session_id: None,
                demand: "none".to_owned(),
                activity: "stopped".to_owned(),
                completed: false,
                descendant_signals: BTreeSet::new(),
            },
        );
        apply_read_state(&mut agents, &mut records, None);

        assert_eq!(
            agents
                .iter()
                .map(|agent| (agent.pane_id.as_str(), agent.group.as_str()))
                .collect::<Vec<_>>(),
            [
                ("blocked", "needs_you"),
                ("asking", "needs_you"),
                ("done-newer", "done"),
                ("done-older", "done"),
                ("working", "working"),
                ("read-idle", "seen"),
            ]
        );
    }

    /// AC5. Two rows in the same group with the same activity keep the order
    /// Herdr sent them in, because the sort is stable and reads no third key.
    #[test]
    fn group_order_keeps_snapshot_order_for_equal_activity() {
        let same = |pane_id: &str| {
            json!({"pane_id": pane_id, "agent_status": "done", "state_change_seq": 1,
                   "tokens": {"status_done_new": "●", "activity": "0000000000004"}})
        };
        let mut agents = projected(json!([same("first"), same("second"), same("third")]));
        let mut records = BTreeMap::new();
        apply_read_state(&mut agents, &mut records, None);
        assert_eq!(
            agents
                .iter()
                .map(|agent| agent.pane_id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "third"]
        );
    }

    /// PRD D-01, checkout-workspace-binding D-09: the rolling task is the
    /// title and the provider is the fallback. Herdr's agent name, the
    /// retired `name` token and the Herdr workspace label (`home-graph`,
    /// another checkout's name as often as this one's) are never titles.
    #[test]
    fn identity_ladder_uses_task_then_provider_and_ignores_names_and_workspace_labels() {
        let projected = projected(json!([
            {"pane_id":"p2","id":"impl-x","agent":"claude","workspace_label":"home-graph","tokens":{"status_working":"●","activity":"0000000000002","name":"Hook 버그 확인","task":"hook 보고 경로 수정"}},
            {"pane_id":"p3","id":"sasu-implementor","agent":"claude","workspace_label":"home-graph","tokens":{"status_working":"●","activity":"0000000000003","name":"첫 프롬프트"}},
            {"pane_id":"p4","id":"p4","agent":"codex","workspace_label":"web-shell-pivot-s3","tokens":{"status_working":"●","activity":"0000000000004"}},
            {"pane_id":"p5","id":"p5","agent":"gemini","workspace_label":"home-graph","tokens":{"status_working":"●","activity":"0000000000005"}},
            {"pane_id":"p6","id":"p6","workspace_label":"home-graph","agent_status":"working","tokens":{"activity":"0000000000006"}}
        ]));
        let labels = projected
            .iter()
            .map(|agent| (agent.pane_id.as_str(), agent.identity_label.as_str()))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            labels.into_values().collect::<Vec<_>>(),
            ["hook 보고 경로 수정", "Claude", "Codex", "gemini", "Agent"]
        );
    }

    /// The old `summary` token is not read: a plugin still publishing it
    /// names nothing and prompts nothing (PRD B13).
    #[test]
    fn a_legacy_summary_token_is_ignored() {
        let projected = projected(json!([
            {"pane_id":"p1","id":"p1","agent":"codex","workspace_label":"task-factory","tokens":{"status_idle":"○","activity":"0000000000001","summary":"Check agent-context-labels settings"}}
        ]));
        assert_eq!(projected[0].identity_label, "Codex");
        assert_eq!(projected[0].detail, None);
    }

    /// PRD overview-lenses-tiles-agents D-50: `message` carries both hook
    /// sentences whole, request first and line breaks kept, while `detail`
    /// stays the row's one cut sentence; an agent that reported neither has none.
    #[test]
    fn message_carries_both_hook_sentences_uncut() {
        let long_reply = "가".repeat(60);
        let projected = projected(json!([
            {"pane_id":"p1","id":"p1","workspace_label":"hide","tokens":{"status_working":"●","activity":"0000000000001","progress":"  hook 보고\n경로 교체 중  ","expected_reply":long_reply}},
            {"pane_id":"p2","id":"p2","workspace_label":"hide","tokens":{"status_idle":"○","activity":"0000000000002","progress":"  "}}
        ]));
        assert_eq!(
            projected[0].message.as_deref(),
            Some(format!("{long_reply}\nhook 보고\n경로 교체 중").as_str())
        );
        assert_eq!(projected[1].message, None);
        let wire = serde_json::to_value(&projected[1]).unwrap();
        assert!(
            wire.get("message").is_none(),
            "an agent that said nothing carries no key"
        );
    }

    /// PRD D-05: the sentences are one line each and `expected_reply` is cut
    /// at the plugin's own 40-character rule.
    #[test]
    fn sentences_are_collapsed_and_expected_reply_is_cut_at_forty() {
        let long_reply = "가".repeat(60);
        let projected = projected(json!([
            {"pane_id":"p1","id":"p1","workspace_label":"hide","tokens":{"status_working":"●","activity":"0000000000001","progress":"  hook   보고\n경로 교체 중  ","expected_reply":long_reply}}
        ]));
        assert_eq!(
            projected[0].progress.as_deref(),
            Some("hook 보고 경로 교체 중")
        );
        assert_eq!(
            projected[0]
                .expected_reply
                .as_deref()
                .map(|value| value.chars().count()),
            Some(MAX_EXPECTED_REPLY_CHARS)
        );
    }

    /// PRD D-06: the second line by group. `agent_second_line` is what
    /// `derive_from_axes` writes into `detail` and `status_word_visible`.
    /// An unresolved demand keeps its request in every group, so a read
    /// question and a delegated child's approval still say what they ask
    /// (sidebar-agent-status B7).
    #[test]
    fn second_line_is_chosen_by_group_and_a_request_outlives_reading() {
        use AgentDemand::{None as Quiet, Question};
        let reply = Some("A/B 선택 후 승인");
        let progress = Some("푸시 완료, 승인 대기 중");
        let asked = (false, Some("A/B 선택 후 승인".to_owned()));
        let said = (false, Some("푸시 완료, 승인 대기 중".to_owned()));
        let cases = [
            (
                AgentGroup::NeedsYou,
                Question,
                reply,
                progress,
                asked.clone(),
            ),
            (AgentGroup::NeedsYou, Question, None, progress, said.clone()),
            (AgentGroup::NeedsYou, Question, None, None, (true, None)),
            (AgentGroup::Done, Quiet, None, progress, said.clone()),
            (AgentGroup::Working, Quiet, reply, progress, said.clone()),
            (AgentGroup::Working, Quiet, None, None, (true, None)),
            // A delegated child still running while it asks.
            (
                AgentGroup::Working,
                Question,
                reply,
                progress,
                asked.clone(),
            ),
            (AgentGroup::Seen, Quiet, reply, progress, (false, None)),
            // A read question, and a delegated child's question, in Seen.
            (AgentGroup::Seen, Question, reply, progress, asked),
            (AgentGroup::Seen, Question, None, progress, said),
            (AgentGroup::Seen, Question, None, None, (false, None)),
        ];
        for (group, demand, reply, progress, expected) in cases {
            assert_eq!(
                agent_second_line(group, demand, reply, progress),
                expected,
                "{group:?} {demand:?} reply={reply:?} progress={progress:?}"
            );
        }
    }

    /// The projected row carries the second line: a working row shows its
    /// progress without the word, and the chip every lineage surface draws
    /// says the same thing (PRD B4, D-06).
    #[test]
    fn projected_rows_carry_the_second_line_and_the_chip_repeats_it() {
        let projected = projected(json!([
            {"pane_id":"p1","id":"p1","workspace_label":"hide","agent_status":"working","tokens":{"status_working":"●","activity":"0000000000001","task":"Hook 버그 확인","progress":"hook 보고 경로를 소켓 호출로 교체 중","expected_reply":"무시됨"}},
            {"pane_id":"p2","id":"p2","workspace_label":"hide","tokens":{"status_question_new":"?","activity":"0000000000002","progress":"푸시 완료","expected_reply":"A/B 선택"}},
            {"pane_id":"p3","id":"p3","workspace_label":"hide","tokens":{"status_idle":"○","activity":"0000000000003"}}
        ]));
        assert_eq!(
            projected[0].detail.as_deref(),
            Some("hook 보고 경로를 소켓 호출로 교체 중")
        );
        assert!(!projected[0].status_word_visible);
        assert_eq!(projected[1].detail.as_deref(), Some("A/B 선택"));
        assert!(!projected[1].status_word_visible);
        // A newly opened idle row stays Seen with no second-line sentence.
        assert_eq!(projected[2].detail, None);
        assert!(!projected[2].status_word_visible);
        let chip = agent_chip(&projected[0]);
        assert_eq!(chip.label, "Hook 버그 확인");
        assert_eq!(
            chip.detail.as_deref(),
            Some("hook 보고 경로를 소켓 호출로 교체 중")
        );
        assert!(!chip.status_word_visible);
    }

    #[test]
    fn one_broken_agent_excludes_only_itself_and_names_why() {
        let projection = project_agents(payload(json!([
            {"pane_id":"good","tokens":{"status_working":"●","activity":"0000000000002"}},
            {"pane_id":"unordered","tokens":{"status_idle":"○"}},
            {"tokens":{"status_idle":"○","activity":"0000000000000"}},
            {"pane_id":"also-good","tokens":{"status_idle":"○","activity":"0000000000003"}}
        ])));

        assert_eq!(
            projection
                .agents
                .iter()
                .map(|agent| agent.pane_id.as_str())
                .collect::<Vec<_>>(),
            ["good", "also-good"]
        );
        assert_eq!(projection.excluded.len(), 2);
        assert_eq!(projection.excluded[0].pane_id.as_deref(), Some("unordered"));
        assert!(projection.excluded[0].reason.contains("state_change_seq"));
        assert_eq!(projection.excluded[1].pane_id, None);
        assert!(projection.excluded[1].reason.contains("pane id"));
    }

    #[test]
    fn official_state_change_sequence_orders_an_agent_the_core_has_not_timed() {
        let projection = project_agents(payload(json!([
            {
                "pane_id":"remote",
                "state_change_seq":218,
                "agent_status":"done",
                "tokens":{"task":"finished up"}
            },
            {
                "pane_id":"timed",
                "state_change_seq":219,
                "agent_status":"idle",
                "changed_at_unix_ms": 1_700_000_000_000_u64
            }
        ])));

        assert_eq!(projection.agents[0].last_activity, "00000000000000000218");
        assert_eq!(projection.agents[1].last_activity, "1700000000000");
        assert!(projection.excluded.is_empty());
    }

    fn projected(agents: Value) -> Vec<SidebarAgentSnapshot> {
        project_agents(payload(agents)).agents
    }

    fn finished(pane_id: &str, seq: u64) -> Value {
        json!({
            "pane_id": pane_id,
            "workspace_label": "Fixture",
            "agent": "codex",
            "agent_status": "done",
            "state_change_seq": seq,
            "tokens": {"status_done_new": "\u{25cf}", "activity": "0000000000001"}
        })
    }

    /// AC2, SC1. Three panes finish in one tab. Focusing one clears that one
    /// and leaves the other two unread, which is the whole reason Hide keeps a
    /// pane-level record instead of reading Herdr's tab-scoped seen.
    #[test]
    fn read_record_clears_one_pane_of_a_finished_tab() {
        let mut agents = projected(json!([
            finished("a", 1),
            finished("b", 2),
            finished("c", 3)
        ]));
        let mut records = BTreeMap::new();

        apply_read_state(&mut agents, &mut records, None);
        assert!(
            agents
                .iter()
                .all(|agent| agent.unread && agent.group == "done"),
            "nothing is read before the operator focuses anything"
        );

        apply_read_state(&mut agents, &mut records, Some("b"));
        let groups = agents
            .iter()
            .map(|agent| (agent.pane_id.as_str(), (agent.unread, agent.group.as_str())))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(groups["a"], (true, "done"));
        assert_eq!(groups["b"], (false, "seen"));
        assert_eq!(groups["c"], (true, "done"));
    }

    /// AC2. Herdr reporting the whole tab as seen, by dropping every pane from
    /// `done` to `idle` and bumping its sequence, does not read anything. Only
    /// Hide's own record does.
    #[test]
    fn read_record_ignores_herdr_marking_the_tab_seen() {
        let mut agents = projected(json!([finished("a", 1), finished("b", 2)]));
        let mut records = BTreeMap::new();
        apply_read_state(&mut agents, &mut records, Some("b"));

        let seen_by_herdr = json!([
            {"pane_id":"a","agent_status":"idle","state_change_seq":9,
             "tokens":{"status_done":"\u{25cf}","activity":"0000000000001"}},
            {"pane_id":"b","agent_status":"idle","state_change_seq":9,
             "tokens":{"status_done":"\u{25cf}","activity":"0000000000001"}}
        ]);
        let mut agents = projected(seen_by_herdr);
        apply_read_state(&mut agents, &mut records, None);
        assert!(
            agents.iter().all(|agent| agent.unread),
            "Herdr's tab-scoped seen never decides Hide's read axis"
        );
    }

    /// AC3, SC2. A demand raised on the pane the operator is already looking at
    /// is read on arrival; the same demand raised after focus moved away is
    /// unread.
    #[test]
    fn read_record_follows_the_focused_pane() {
        let asking = json!([{
            "pane_id":"a","agent_status":"idle","state_change_seq":2,
            "tokens":{"status_question_new":"?","activity":"0000000000002"}
        }]);
        let mut records = BTreeMap::new();

        let mut watched = projected(asking.clone());
        apply_read_state(&mut watched, &mut records, Some("a"));
        assert!(!watched[0].unread, "a question on the focused pane is read");
        assert_eq!(watched[0].group, "seen");

        let moved_on = json!([{
            "pane_id":"a","agent_status":"idle","state_change_seq":3,
            "tokens":{"status_question_new":"?","activity":"0000000000003"}
        }]);
        let mut later = projected(moved_on);
        apply_read_state(&mut later, &mut records, Some("elsewhere"));
        assert!(later[0].unread, "a new question raised elsewhere is unread");
        assert_eq!(later[0].group, "needs_you");
    }

    /// AC3. The axes alone can move without Herdr's sequence moving, so a
    /// demand appearing at an unchanged sequence still counts as a change.
    #[test]
    fn read_record_notices_an_axis_change_at_the_same_sequence() {
        let mut records = BTreeMap::new();
        let mut working = projected(json!([{
            "pane_id":"a","agent_status":"working","state_change_seq":7,
            "tokens":{"status_working":"\u{25cf}","activity":"0000000000001"}
        }]));
        apply_read_state(&mut working, &mut records, Some("a"));
        assert!(!working[0].unread);

        let mut asking = projected(json!([{
            "pane_id":"a","agent_status":"working","state_change_seq":7,
            "tokens":{"status_question_new":"?","status_working":"\u{25cf}","activity":"0000000000001"}
        }]));
        apply_read_state(&mut asking, &mut records, None);
        assert!(
            asking[0].unread,
            "a new demand is unread even at the same sequence"
        );
    }

    /// AC4. Agent detection can be empty while the pane still exists. Only an
    /// authoritative pane topology may evict its read record, and applying the
    /// same topology twice converges (engineering rule 11).
    #[test]
    fn read_records_are_evicted_only_against_authoritative_pane_topology() {
        let mut agents = projected(json!([finished("a", 1)]));
        let mut records = BTreeMap::new();
        apply_read_state(&mut agents, &mut records, Some("a"));
        let after_first = records.clone();
        assert!(apply_read_state(&mut agents, &mut records, Some("a")).is_empty());
        assert_eq!(records, after_first, "a repeated apply changes nothing");

        let mut none: Vec<SidebarAgentSnapshot> = Vec::new();
        apply_read_state(&mut none, &mut records, None);
        assert!(
            records.contains_key("a"),
            "an empty agent list must not wipe a pane record"
        );
        let live = HashSet::from(["a".to_owned()]);
        assert!(prune_read_records(&mut records, &live, ReadRecordScope::Local).is_empty());
        let gone = HashSet::new();
        assert_eq!(
            prune_read_records(&mut records, &gone, ReadRecordScope::Local).len(),
            1
        );
        assert!(
            records.is_empty(),
            "a pane the authoritative topology dropped leaves no record"
        );
    }

    /// PRD sidebar-context-menus D-06: an agent row carries the session id
    /// Herdr recorded, which Copy session id reads, and none when it recorded
    /// none or recorded a path.
    #[test]
    fn an_agent_row_carries_the_session_id_herdr_recorded() {
        let agent = |pane: &str, session: Value| {
            let mut agent = finished(pane, 1);
            agent["agent_session"] = session;
            agent
        };
        let rows = projected(json!([
            agent("a", json!({"kind":"id","value":"session-a"})),
            agent("b", Value::Null),
            agent("c", json!({"kind":"path","value":"/tmp/session.jsonl"})),
        ]));
        assert_eq!(rows.len(), 3);
        let wire = |pane: &str| {
            serde_json::to_value(rows.iter().find(|row| row.pane_id == pane).unwrap()).unwrap()
        };
        assert_eq!(wire("a")["session_id"], "session-a");
        assert!(wire("b").get("session_id").is_none());
        assert!(wire("c").get("session_id").is_none());
    }

    #[test]
    fn reconnect_rebases_the_same_agent_but_not_a_replacement() {
        let same_session = |seq: u64| {
            json!([{
                "pane_id":"a",
                "agent_status":"done",
                "state_change_seq":seq,
                "agent_session":{"kind":"id","value":"session-a"},
                "tokens":{"status_done_new":"\u{25cf}","activity":"0000000000001"}
            }])
        };
        let mut records = BTreeMap::new();
        let mut original = projected(same_session(40));
        apply_read_state(&mut original, &mut records, Some("a"));

        let mut restored = projected(same_session(3));
        let mut pending = HashSet::from(["a".to_owned()]);
        assert_eq!(
            reconcile_read_records(&restored, &mut records, &mut pending).len(),
            1
        );
        apply_read_state(&mut restored, &mut records, None);
        assert!(!restored[0].unread, "the restored session stays read");

        let mut progressed = projected(same_session(4));
        let mut pending = HashSet::from(["a".to_owned()]);
        assert!(reconcile_read_records(&progressed, &mut records, &mut pending).is_empty());
        apply_read_state(&mut progressed, &mut records, None);
        assert!(
            progressed[0].unread,
            "a forward sequence is new work, not a server restart"
        );

        let mut legacy_records = records.clone();
        legacy_records.get_mut("a").unwrap().session_id = None;
        let legacy_restored = projected(same_session(1));
        let mut pending = HashSet::from(["a".to_owned()]);
        assert_eq!(
            reconcile_read_records(&legacy_restored, &mut legacy_records, &mut pending).len(),
            1
        );
        assert_eq!(
            legacy_records["a"].session_id.as_deref(),
            Some("session-a"),
            "the first reset migrates a record written before session identity"
        );

        let mut replacement = projected(json!([{
            "pane_id":"a",
            "agent_status":"done",
            "state_change_seq":1,
            "agent_session":{"kind":"id","value":"session-b"},
            "tokens":{"status_done_new":"\u{25cf}","activity":"0000000000001"}
        }]));
        let mut pending = HashSet::from(["a".to_owned()]);
        assert!(reconcile_read_records(&replacement, &mut records, &mut pending).is_empty());
        apply_read_state(&mut replacement, &mut records, None);
        assert!(replacement[0].unread, "a replacement agent is new work");
    }

    /// PRD B5 across a reconnect: a child's question that arrived while Hide
    /// was disconnected is still news for its parent after the parent's own
    /// sequence is rebased, because the rebase keeps only what the operator
    /// had actually seen.
    #[test]
    fn reconnect_keeps_a_descendants_new_question_unread_on_the_parent() {
        let rows = |seq: u64, child_status: &str, child_tokens: Value| {
            let mut agents = projected(json!([
                {
                    "pane_id":"root",
                    "agent_status":"working",
                    "state_change_seq":seq,
                    "agent_session":{"kind":"id","value":"session-root"},
                    "tokens":{"status_working":"\u{25cf}","activity":"0000000000001"}
                },
                {
                    "pane_id":"child",
                    "agent_status":child_status,
                    "state_change_seq":1,
                    "tokens":child_tokens
                }
            ]));
            agents[1].spawned_from_pane_id = Some("root".to_owned());
            apply_lineage(&mut agents, &[], &[]);
            agents
        };
        let quiet = json!({"status_working":"\u{25cf}","activity":"0000000000001"});
        let asking = json!({"status_question_new":"?","activity":"0000000000002"});

        let mut records = BTreeMap::new();
        let mut before = rows(40, "working", quiet);
        apply_read_state(&mut before, &mut records, Some("root"));
        assert!(!before[0].unread);

        let mut restored = rows(3, "idle", asking);
        let mut pending = HashSet::from(["root".to_owned()]);
        assert_eq!(
            reconcile_read_records(&restored, &mut records, &mut pending).len(),
            1,
            "the parent's own sequence is rebased"
        );
        apply_read_state(&mut restored, &mut records, None);
        assert!(
            restored[0].unread,
            "the question that arrived while disconnected is still news"
        );
    }

    /// The ordering key falls back to Herdr's own sequence, zero padded to the
    /// activity token's width, when no label worker timestamp came.
    #[test]
    fn last_activity_falls_back_to_the_herdr_sequence() {
        let projection = project_agents(payload(json!([
            {"pane_id":"working","agent_status":"working","state_change_seq":1}
        ])));
        assert_eq!(projection.agents[0].last_activity, "00000000000000000001");
        assert_eq!(projection.agents[0].state_change_seq, Some(1));
    }
}
