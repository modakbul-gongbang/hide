use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::model::{AmbientSignal, PaneLayoutDirection, PaneReadRecord, SidebarAgentSnapshot};

#[derive(Clone, Debug, Deserialize)]
pub struct SessionSnapshotPayload {
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
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
    /// The tab Herdr reports as active in this workspace. It is the only
    /// authority for which tab is active; the navigator never falls back to a
    /// position.
    #[serde(default)]
    pub active_tab_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionTabPayload {
    pub tab_id: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
pub struct SessionLayoutPanePayload {
    pub pane_id: String,
    pub rect: SessionLayoutRect,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionLayoutSplitPayload {
    pub direction: PaneLayoutDirection,
    pub ratio: f32,
    pub rect: SessionLayoutRect,
}

/// What the sidebar shows for an agent whose context-label plugin reported
/// no summary. It is a prompt to the user, not a name, so surfaces that name
/// a pane must not adopt it.
pub const MISSING_SUMMARY: &str = "Check agent-context-labels settings";

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
    /// The pane this agent was spawned from, as Herdr's own lineage records it.
    /// Present only on an agent started through `agent.new` with a source pane.
    #[serde(default)]
    pub spawned_from_pane_id: Option<String>,
    #[serde(default)]
    pub state_change_seq: Option<u64>,
    #[serde(default)]
    pub tokens: BTreeMap<String, Value>,
    /// Passed through verbatim; the strict shape check lives in
    /// [`parse_ambient`] so a broken record can never partially survive.
    #[serde(default)]
    pub ambient: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SessionAgentSessionPayload {
    pub kind: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionPanePayload {
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
/// The label plugin reports each of the three in an unread form
/// (`status_question_new`) and a read form (`status_question`); both mean the
/// demand exists, because whether the operator has read it is Hide's own
/// judgment and not the token's. Herdr's `blocked` lifecycle is an approval:
/// the plugin defines `!` as "Herdr's blocked lifecycle, or the hook seeing a
/// permission request".
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
/// Herdr's `done` and `idle` are the same underlying stopped state; `done` only
/// adds Herdr's own tab-scoped judgment that nobody has looked yet, which Hide
/// does not use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentActivity {
    Working,
    Stopped,
    Unknown,
}

impl AgentActivity {
    fn name(self) -> &'static str {
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
/// badge and never raises the operator's own attention groups (PRD B15, B16,
/// D-35, D-38). A child stuck long enough to be nobody's problem comes back
/// as the operator's through `Escalated`, which is the whole point of the
/// safety net (PRD B18, D-42).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ownership {
    /// The operator's own work: a lineage root, or an orphan whose parent is
    /// gone.
    Operator,
    /// Delegated work. Its demands stay with its parent.
    Delegated,
    /// Delegated work that stalled past the hard threshold, so ownership has
    /// come back to the operator.
    Escalated,
}

/// The group a row belongs to, given who owns it.
pub fn agent_group_for(
    demand: AgentDemand,
    activity: AgentActivity,
    unread: bool,
    blocked: bool,
    ownership: Ownership,
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
    if blocked || ownership == Ownership::Escalated || (demand != AgentDemand::None && unread) {
        AgentGroup::NeedsYou
    } else if demand == AgentDemand::None && activity == AgentActivity::Stopped && unread {
        AgentGroup::Done
    } else if activity == AgentActivity::Working {
        AgentGroup::Working
    } else {
        AgentGroup::Seen
    }
}

/// The mark drawn for a row, matching the label plugin's own symbol table.
fn agent_symbol(demand: AgentDemand, activity: AgentActivity, unread: bool) -> &'static str {
    match demand {
        AgentDemand::Error => "\u{d7}",
        AgentDemand::Question => "?",
        AgentDemand::Approval => "!",
        AgentDemand::None => match activity {
            AgentActivity::Working => "\u{25cf}",
            AgentActivity::Stopped if unread => "✓",
            AgentActivity::Stopped => "\u{25cb}",
            AgentActivity::Unknown => "~",
        },
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
    let group = agent_group_for(demand, activity, unread, agent.blocked, ownership);
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
    changed
}

/// The one short word a row shows. A stopped agent the operator has not read
/// is `Done`; once read, the same agent is `Idle`. No view ever shows an axis
/// value, so nothing underscored can reach the screen.
fn agent_status_label(demand: AgentDemand, activity: AgentActivity, unread: bool) -> &'static str {
    match demand {
        AgentDemand::Error => "Error",
        AgentDemand::Question => "Question",
        AgentDemand::Approval => "Approval",
        AgentDemand::None => match activity {
            AgentActivity::Working => "Working",
            AgentActivity::Stopped if unread => "Done",
            AgentActivity::Stopped => "Idle",
            AgentActivity::Unknown => "Unknown",
        },
    }
}

/// Closing this pane would interrupt running work or throw away a result the
/// operator has not read yet.
fn agent_requires_close_confirmation(activity: AgentActivity, group: AgentGroup) -> bool {
    activity == AgentActivity::Working || matches!(group, AgentGroup::NeedsYou | AgentGroup::Done)
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
pub fn apply_lineage(
    agents: &mut [SidebarAgentSnapshot],
    workspaces: &[crate::model::WorkspaceSnapshot],
    collapsed: &[String],
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
    for index in 0..agents.len() {
        // Walk to the root, remembering the way, so the breadcrumb is the
        // same walk the depth already costs rather than a second traversal.
        let mut ancestors = Vec::new();
        let mut root = index;
        while let Some(parent) = parents[root] {
            ancestors.push(parent);
            root = parent;
        }
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
        let hint = agents[index].spawned_from_pane_id.as_ref().map(|pane| {
            let name = by_pane
                .get(pane)
                .map(|parent| agents[*parent].id.as_str())
                .unwrap_or(pane.as_str());
            format!("↳ from {name}")
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
        let agent = &mut agents[index];
        // Ownership is the depth and nothing else. An orphan resolved to no
        // parent, so it is a root here and the dimming lifts with it.
        agent.delegated = depth > 0;
        agent.lineage_parent_pane_id = parent_pane_id;
        agent.lineage_path_pane_ids = path;
        agent.lineage_sibling_pane_ids = siblings;
        agent.lineage_depth = depth;
        agent.lineage_child_pane_ids = child_ids;
        agent.lineage_root_checkout_id = root_checkout;
        agent.lineage_worktree_badge = badge;
        agent.lineage_orphan = orphan;
        agent.lineage_hint = orphan.then(|| hint.clone()).flatten();
        agent.raised_hint = hint;
        agent.lineage_collapsed = collapsed.contains(&agent.pane_id);
    }
    // Ownership was unknown when the rows were first derived, because it is
    // the lineage that decides it. Rederiving here is what keeps every caller
    // of this function on one answer instead of each remembering to ask
    // (engineering rule 13). The order is left alone: this function is read
    // by index while it walks the tree, and sorting is the ingest's last
    // step, after the stall clocks have had their say.
    for agent in agents.iter_mut() {
        derive_from_axes(agent);
    }
}

/// The chip one agent shows as somebody else's child.
fn child_chip(agent: &SidebarAgentSnapshot) -> crate::model::ChildChipSnapshot {
    crate::model::ChildChipSnapshot {
        pane_id: agent.pane_id.clone(),
        // The name the operator gave the chat, when there is one; otherwise
        // Herdr's own agent name. Never the missing-summary prompt, which is
        // an instruction to the operator rather than a name for anything.
        label: agent.chat_title.clone().unwrap_or_else(|| agent.id.clone()),
        detail: if agent.summary == MISSING_SUMMARY {
            agent.status_label.clone()
        } else {
            agent.summary.clone()
        },
        agent_kind: agent.agent_kind.clone(),
        demand: agent.demand.clone(),
        activity: agent.activity.clone(),
        emphasized: agent.emphasized,
        symbol: agent.symbol.clone(),
        status_label: agent.status_label.clone(),
        delegated: agent.delegated,
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
            remote: crate::agent_hooks::is_remote_pane(pane_id),
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
        .map(child_chip)
        .collect::<Vec<_>>();
    let representative = agent
        .lineage_child_pane_ids
        .iter()
        .filter_map(|child| agents.iter().find(|agent| &agent.pane_id == child))
        .min_by_key(|child| child_representative_rank(child))
        .map(child_chip);
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
            label: child_chip(step).label,
            siblings: step
                .lineage_sibling_pane_ids
                .iter()
                .filter_map(|sibling| agents.iter().find(|agent| &agent.pane_id == sibling))
                .map(child_chip)
                .collect(),
        })
        .collect()
}

/// Re-derives every value that depends on ownership, then reorders.
///
/// The read pass runs before the lineage is known, so ownership is settled
/// afterwards and the groups it decides have to be recomputed rather than
/// left describing a row nobody owned yet.
pub fn rederive_ownership(agents: &mut [SidebarAgentSnapshot]) {
    for agent in agents.iter_mut() {
        derive_from_axes(agent);
    }
    sort_agents(agents);
}

/// Collapse state belongs to pane existence, not whether it currently has
/// children. A fresh scoped agent list may evict it; a stale list may not.
pub fn prune_lineage_collapse(
    collapsed: &mut Vec<String>,
    agents: &[SidebarAgentSnapshot],
    scope: ReadRecordScope<'_>,
) -> bool {
    let before = collapsed.len();
    collapsed.retain(|pane| !scope.owns(pane) || agents.iter().any(|agent| &agent.pane_id == pane));
    before != collapsed.len()
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
/// met the read record ledger believes every stopped row is Done. The label
/// plugin's `sort_rank` token is not read at all, so the order Hide shows is
/// Hide's own.
fn sort_agents(agents: &mut [SidebarAgentSnapshot]) {
    agents.sort_by(|left, right| {
        group_of(left)
            .rank()
            .cmp(&group_of(right).rank())
            .then_with(|| right.last_activity.cmp(&left.last_activity))
    });
}

/// Reads the three axes back off a row.
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
    if agent.stall_level == "hard" {
        Ownership::Escalated
    } else if agent.delegated {
        Ownership::Delegated
    } else {
        Ownership::Operator
    }
}

pub fn group_of(agent: &SidebarAgentSnapshot) -> AgentGroup {
    let (demand, activity, unread) = axes_of(agent);
    agent_group_for(demand, activity, unread, agent.blocked, ownership_of(agent))
}

/// The demand axis of a projected row, for callers outside this module.
///
/// It exists so nothing has to compare the published axis name against a
/// string literal of its own: the vocabulary lives in [`AgentDemand`] and is
/// read back through it.
pub fn demand_of(agent: &SidebarAgentSnapshot) -> AgentDemand {
    axes_of(agent).0
}

/// Fills in every value the shell draws from the three axes.
///
/// Called again whenever the read axis moves, so the derived values can never
/// describe a different read state than the row they sit on.
fn derive_from_axes(agent: &mut SidebarAgentSnapshot) {
    let (demand, activity, unread) = axes_of(agent);
    let group = agent_group_for(demand, activity, unread, agent.blocked, ownership_of(agent));
    agent.group = group.name().to_owned();
    agent.symbol = agent_symbol(demand, activity, unread).to_owned();
    // A row the operator still has to deal with is drawn bright; everything
    // already read or merely running is subdued.
    agent.emphasized = matches!(group, AgentGroup::NeedsYou | AgentGroup::Done);
    agent.status_label = agent_status_label(demand, activity, unread).to_owned();
    agent.requires_close_confirmation = agent_requires_close_confirmation(activity, group);
}

fn project_agent(agent: SessionAgentPayload) -> Result<SidebarAgentSnapshot, String> {
    let pane_id = non_empty(agent.pane_id.as_deref().or(agent.id.as_deref()))
        .map(str::to_owned)
        .ok_or_else(|| "session agent is missing a pane id".to_owned())?;
    let last_activity = projected_last_activity(&agent, &pane_id)?;
    let demand = agent_demand(&agent);
    let activity = agent_activity(&agent);
    let blocked = agent.agent_status.as_deref() == Some("blocked");
    let ambient = match agent.ambient.as_ref() {
        Some(raw) => parse_ambient(raw)?,
        None => None,
    };
    let workspace_label = non_empty(agent.workspace_label.as_deref())
        .or_else(|| {
            agent
                .cwd
                .as_deref()
                .and_then(|cwd| cwd.rsplit('/').find(|segment| !segment.trim().is_empty()))
        })
        .unwrap_or("workspace")
        .to_owned();
    let summary = token_string(&agent.tokens, "summary")
        .map(collapse_whitespace)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(30).collect())
        .unwrap_or_else(|| MISSING_SUMMARY.to_owned());
    let elapsed = token_string(&agent.tokens, "elapsed")
        .filter(|value| valid_elapsed(value))
        .unwrap_or_else(|| "0s".to_owned());
    // The title the composer wrote onto this pane when it started the chat.
    // Herdr holds it, so it survives a Hide restart the way the pane does.
    let chat_title = token_string(&agent.tokens, crate::scratch::TITLE_TOKEN)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    Ok(SidebarAgentSnapshot {
        id: agent.id.unwrap_or_else(|| pane_id.clone()),
        pane_id,
        workspace_label,
        checkout_label: None,
        agent_kind: non_empty(agent.agent.as_deref())
            .unwrap_or("unknown")
            .to_owned(),
        demand: demand.name().to_owned(),
        activity: activity.name().to_owned(),
        // Every agent is projected unread; the read axis and everything
        // derived from it are set once, by `apply_read_state`, where the
        // pane read record lives.
        unread: true,
        blocked,
        group: String::new(),
        symbol: String::new(),
        emphasized: false,
        status_label: String::new(),
        requires_close_confirmation: false,
        summary,
        elapsed,
        last_activity,
        state_change_seq: agent.state_change_seq,
        ambient,
        session_id: agent
            .agent_session
            .as_ref()
            .filter(|session| session.kind == "id")
            .map(|session| session.value.clone())
            .filter(|value| !value.trim().is_empty()),
        spawned_from_pane_id: non_empty(agent.spawned_from_pane_id.as_deref()).map(str::to_owned),
        chat_title,
        delegated: false,
        stall_level: String::new(),
        stall_notice: None,
        lineage_parent_pane_id: None,
        lineage_path_pane_ids: Vec::new(),
        lineage_sibling_pane_ids: Vec::new(),
        lineage_depth: 0,
        lineage_child_pane_ids: Vec::new(),
        lineage_root_checkout_id: None,
        lineage_worktree_badge: None,
        lineage_orphan: false,
        lineage_hint: None,
        raised_hint: None,
        lineage_collapsed: false,
    })
}

/// The prefix every pane id on a remote target carries. A pane id without it
/// belongs to the local Herdr server.
const REMOTE_PANE_ID_PREFIX: &str = "remote:";

/// Which slice of the read record ledger a pass is allowed to prune.
///
/// A pass carries the agent list of exactly one Herdr server, and the ledger
/// holds records for all of them. A pass that pruned every key its own list
/// did not claim would drop a remote pane's record on the next local sync and
/// the reverse, so a stopped remote pane the operator had already read came
/// back as `Done` and demanded a close confirmation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadRecordScope<'a> {
    /// Prune nothing. The caller's agent list may be stale or from before the
    /// first sync, and an empty list would otherwise wipe the whole ledger.
    Retain,
    /// The caller holds the local server's full agent list, so local records
    /// no agent claims are dropped. Remote records are left alone.
    Local,
    /// The caller holds one remote target's full agent list. The argument is
    /// that target's pane id prefix, and only records under it are dropped.
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

/// What the operator is looking at on this pane right now.
fn read_fingerprint(agent: &SidebarAgentSnapshot) -> PaneReadRecord {
    PaneReadRecord {
        state_change_seq: agent.state_change_seq,
        demand: agent.demand.clone(),
        activity: agent.activity.clone(),
    }
}

/// Raises the operator-focused pane's read record, drops records for panes
/// that are gone, and sets every row's read axis and derived values.
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
    scope: ReadRecordScope<'_>,
) -> Vec<ReadRecordChange> {
    let mut changes = Vec::new();
    // A pane the server no longer reports can never be unread again, so its
    // record is dropped rather than growing the store forever. Only the pass
    // that owns the key's namespace may do this, and only when it holds a
    // fresh agent list: an empty list from before the first sync, or a local
    // list that has never heard of a remote pane, would otherwise wipe records
    // the operator restarted with.
    {
        let live = agents
            .iter()
            .map(|agent| agent.pane_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let evicted = records
            .keys()
            .filter(|pane_id| scope.owns(pane_id) && !live.contains(*pane_id))
            .cloned()
            .collect::<Vec<_>>();
        for pane_id in evicted {
            records.remove(&pane_id);
            changes.push(ReadRecordChange {
                pane_id,
                record: PaneReadRecord::default(),
                evicted: true,
            });
        }
    }

    for agent in agents.iter_mut() {
        if operator_pane_id != Some(agent.pane_id.as_str()) {
            continue;
        }
        let current = read_fingerprint(agent);
        if records.get(&agent.pane_id) != Some(&current) {
            records.insert(agent.pane_id.clone(), current.clone());
            changes.push(ReadRecordChange {
                pane_id: agent.pane_id.clone(),
                record: current,
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
            || records.get(&agent.pane_id) == Some(&read_fingerprint(agent));
        agent.unread = !read;
        derive_from_axes(agent);
    }
    sort_agents(agents);
}

/// Reads a pane's optional `ambient` object.
///
/// `null` means the server sent nothing for this pane. Any other unreadable
/// shape excludes the whole record rather than partially extracting it, and
/// unknown keys are dropped so nothing but the three counts can ever reach
/// app state (see docs/status-model.md, Ambient signals).
fn parse_ambient(raw: &Value) -> Result<Option<AmbientSignal>, String> {
    if raw.is_null() {
        return Ok(None);
    }
    let object = raw
        .as_object()
        .ok_or_else(|| "ambient signal is not an object".to_owned())?;
    let count = |key: &str| -> Result<u32, String> {
        match object.get(key) {
            None => Ok(0),
            Some(value) => value
                .as_u64()
                .and_then(|number| u32::try_from(number).ok())
                .ok_or_else(|| format!("ambient signal {key} is not a count")),
        }
    };
    Ok(Some(AmbientSignal {
        subagents_active: count("subagents_active")?,
        background_running: count("background_running")?,
        background_failed: count("background_failed")?,
    }))
}

/// The demand axis. Error outranks question, which outranks approval, so the
/// worst thing waiting is the one the row names.
fn agent_demand(agent: &SessionAgentPayload) -> AgentDemand {
    let tokens = &agent.tokens;
    if demand_token(tokens, "status_error") {
        AgentDemand::Error
    } else if demand_token(tokens, "status_question") {
        AgentDemand::Question
    } else if demand_token(tokens, "status_approval")
        || agent.agent_status.as_deref() == Some("blocked")
    {
        AgentDemand::Approval
    } else {
        AgentDemand::None
    }
}

/// The activity axis. A state Herdr does not name and no token describes is
/// reported as unknown rather than folded into idle (engineering rule 4).
fn agent_activity(agent: &SessionAgentPayload) -> AgentActivity {
    let tokens = &agent.tokens;
    if present_token(tokens, "status_working") || agent.agent_status.as_deref() == Some("working") {
        AgentActivity::Working
    } else if matches!(agent.agent_status.as_deref(), Some("idle") | Some("done"))
        || present_token(tokens, "status_idle")
        || demand_token(tokens, "status_done")
    {
        AgentActivity::Stopped
    } else {
        AgentActivity::Unknown
    }
}

fn projected_last_activity(agent: &SessionAgentPayload, pane_id: &str) -> Result<String, String> {
    match agent.tokens.get("activity") {
        None => agent
            .state_change_seq
            .map(|sequence| format!("{sequence:020}"))
            .ok_or_else(|| {
                format!("agent {pane_id} has neither an activity token nor state_change_seq")
            }),
        Some(Value::String(value)) => {
            let value = value.trim();
            if value.len() == 13 && value.bytes().all(|byte| byte.is_ascii_digit()) {
                Ok(value.to_owned())
            } else {
                Err(format!("agent {pane_id} has an invalid activity token"))
            }
        }
        Some(_) => Err(format!("agent {pane_id} has an invalid activity token")),
    }
}

/// A label plugin status token in either form. `status_question_new` is the
/// unread form and `status_question` the read one; both say the question
/// exists. Only Hide's own pane record decides whether it has been read, so the
/// `_new` suffix is an input to the demand axis and never to the read axis.
/// The legacy boolean form (`status_question: true`) still counts.
fn demand_token(tokens: &BTreeMap<String, Value>, name: &str) -> bool {
    present_token(tokens, &format!("{name}_new")) || present_token(tokens, name)
}

fn present_token(tokens: &BTreeMap<String, Value>, name: &str) -> bool {
    tokens
        .get(name)
        .is_some_and(|value| value.as_bool() != Some(false) && !value.is_null())
}

fn token_string(tokens: &BTreeMap<String, Value>, name: &str) -> Option<String> {
    tokens
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn collapse_whitespace(value: String) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn valid_elapsed(value: &str) -> bool {
    let Some((digits, suffix)) = value.split_at_checked(value.len().saturating_sub(1)) else {
        return false;
    };
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && matches!(suffix, "s" | "m" | "h" | "d")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn workspace_summary_uses_physical_ownership_priority_and_unique_panes() {
        use crate::model::{CheckoutSnapshot, PaneSnapshot, TabSnapshot, WorkspaceSnapshot};
        let checkout = |id: &str, panes: &[&str]| CheckoutSnapshot {
            id: id.to_owned(),
            tabs: vec![TabSnapshot {
                panes: panes
                    .iter()
                    .map(|id| PaneSnapshot {
                        id: (*id).to_owned(),
                        content: Default::default(),
                        herdr_label: None,
                        terminal_title: None,
                        workspace_label: None,
                        cwd: "/fixture".to_owned(),
                        status_label: "Unknown".to_owned(),
                        requires_close_confirmation: false,
                        summary: None,
                        activity_at_unix_ms: None,
                        fork: Default::default(),
                        ports: vec![],
                        children: None,
                        lineage_path: Vec::new(),
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
        }];
        let mut agents = project_agents(payload(json!([
            {"pane_id":"error", "state_change_seq":1, "agent_status":"idle", "tokens":{"status_error":"×"}},
            {"pane_id":"question", "state_change_seq":1, "agent_status":"idle", "tokens":{"status_question":"?"}},
            {"pane_id":"done", "state_change_seq":1, "agent_status":"idle"},
            {"pane_id":"working", "state_change_seq":1, "agent_status":"working"},
            {"pane_id":"unknown", "state_change_seq":1, "agent_status":"unknown"},
            {"pane_id":"child", "state_change_seq":1, "agent_status":"blocked", "spawned_from_pane_id":"working"}
        ])))
        .agents;
        let error = agents.iter_mut().find(|a| a.pane_id == "error").unwrap();
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
        serde_json::from_value(json!({"agents": agents})).expect("valid fixture")
    }

    /// AC1. Every token form and every Herdr lifecycle lands on the pair the
    /// PRD names. The read and unread forms of a demand token classify the
    /// same, because whether it has been read is Hide's own judgment.
    #[test]
    fn axes_classify_every_token_form_and_lifecycle() {
        let cases = [
            (
                json!({"status_question_new": "?"}),
                "question",
                "unknown",
                "?",
            ),
            (json!({"status_question": "?"}), "question", "unknown", "?"),
            (
                json!({"status_approval_new": "!"}),
                "approval",
                "unknown",
                "!",
            ),
            (json!({"status_approval": "!"}), "approval", "unknown", "!"),
            (
                json!({"status_error_new": "\u{d7}"}),
                "error",
                "unknown",
                "\u{d7}",
            ),
            (
                json!({"status_error": "\u{d7}"}),
                "error",
                "unknown",
                "\u{d7}",
            ),
            (
                json!({"status_working": "\u{25cf}"}),
                "none",
                "working",
                "\u{25cf}",
            ),
            (
                json!({"status_done_new": "\u{25cf}"}),
                "none",
                "stopped",
                "✓",
            ),
            (json!({"status_idle": "\u{25cb}"}), "none", "stopped", "✓"),
            (json!({"status_unknown": "~"}), "none", "unknown", "~"),
        ];
        let agents = cases
            .iter()
            .enumerate()
            .map(|(index, (tokens, _, _, _))| {
                let mut tokens = tokens.as_object().expect("token object").clone();
                tokens.insert("activity".to_owned(), json!(format!("{index:013}")));
                json!({
                    "pane_id": format!("pane-{index}"),
                    "workspace_label": "Fixture",
                    "agent": "codex",
                    "agent_status": "unknown",
                    "tokens": tokens
                })
            })
            .collect::<Vec<_>>();
        let projected = project_agents(payload(json!(agents))).agents;
        let by_pane = projected
            .iter()
            .map(|agent| (agent.pane_id.as_str(), agent))
            .collect::<BTreeMap<_, _>>();
        for (index, (_, demand, activity, symbol)) in cases.iter().enumerate() {
            let agent = by_pane[format!("pane-{index}").as_str()];
            assert_eq!(&agent.demand, demand, "demand for pane-{index}");
            assert_eq!(&agent.activity, activity, "activity for pane-{index}");
            assert_eq!(&agent.symbol, symbol, "symbol for pane-{index}");
        }
    }

    /// AC1. Herdr's own lifecycle words, with no plugin token at all. Blocked
    /// is an approval, and `done` and `idle` produce the same activity: the
    /// difference between them is Herdr's tab-scoped read judgment, which Hide
    /// does not use.
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
                    (agent.demand.as_str(), agent.activity.as_str()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(axes["working"], ("none", "working"));
        assert_eq!(axes["blocked"], ("approval", "unknown"));
        assert_eq!(axes["done"], ("none", "stopped"));
        assert_eq!(axes["idle"], ("none", "stopped"));
        assert_eq!(axes["unknown"], ("none", "unknown"));
        assert_eq!(
            axes["done"], axes["idle"],
            "done and idle differ only by Herdr's tab-scoped seen, which Hide does not read"
        );
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
                true,
                Ownership::Operator
            ),
            AgentGroup::NeedsYou
        );
        assert_eq!(
            agent_group_for(
                AgentDemand::Approval,
                AgentActivity::Unknown,
                false,
                false,
                Ownership::Operator
            ),
            AgentGroup::Seen
        );
    }

    /// AC10. Closing a pane needs confirmation exactly where the PRD says:
    /// working, or in Needs You or Done. A read idle pane closes without one.
    #[test]
    fn axes_require_close_confirmation_for_working_needs_you_and_done() {
        let cases = [
            (
                AgentDemand::None,
                AgentActivity::Working,
                false,
                false,
                true,
            ),
            (
                AgentDemand::Question,
                AgentActivity::Stopped,
                true,
                false,
                true,
            ),
            (
                AgentDemand::Approval,
                AgentActivity::Unknown,
                false,
                true,
                true,
            ),
            (AgentDemand::None, AgentActivity::Stopped, true, false, true),
            (
                AgentDemand::None,
                AgentActivity::Stopped,
                false,
                false,
                false,
            ),
            (
                AgentDemand::Question,
                AgentActivity::Stopped,
                false,
                false,
                false,
            ),
            (
                AgentDemand::None,
                AgentActivity::Unknown,
                true,
                false,
                false,
            ),
        ];
        for (demand, activity, unread, blocked, expected) in cases {
            let group = agent_group_for(demand, activity, unread, blocked, Ownership::Operator);
            assert_eq!(
                agent_requires_close_confirmation(activity, group),
                expected,
                "{demand:?} {activity:?} unread={unread} blocked={blocked}"
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
            .join("../macos")
            .canonicalize()
            .expect("the shell sits beside the core");
        let mut offenders = Vec::new();
        let mut scanned = 0_usize;
        let mut stack = vec![shell.join("Sources"), shell.join("Tests")];
        while let Some(entry) = stack.pop() {
            if entry.is_dir() {
                for child in std::fs::read_dir(&entry).expect("readable directory") {
                    stack.push(child.expect("readable entry").path());
                }
                continue;
            }
            if entry.extension().and_then(|value| value.to_str()) != Some("swift") {
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
    fn axes_status_labels_are_short_human_words() {
        let labels = [
            (
                AgentDemand::Question,
                AgentActivity::Unknown,
                true,
                "Question",
            ),
            (
                AgentDemand::Approval,
                AgentActivity::Unknown,
                true,
                "Approval",
            ),
            (AgentDemand::Error, AgentActivity::Unknown, true, "Error"),
            (AgentDemand::None, AgentActivity::Working, true, "Working"),
            (AgentDemand::None, AgentActivity::Stopped, true, "Done"),
            (AgentDemand::None, AgentActivity::Stopped, false, "Idle"),
            (AgentDemand::None, AgentActivity::Unknown, true, "Unknown"),
        ];
        for (demand, activity, unread, expected) in labels {
            let label = agent_status_label(demand, activity, unread);
            assert_eq!(label, expected);
            assert!(!label.contains('_'), "{label} leaks an axis value");
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
        // The operator has looked at `read-idle`, so it drops out of Done.
        records.insert(
            "read-idle".to_owned(),
            PaneReadRecord {
                state_change_seq: Some(1),
                demand: "none".to_owned(),
                activity: "stopped".to_owned(),
            },
        );
        apply_read_state(&mut agents, &mut records, None, ReadRecordScope::Local);

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
        apply_read_state(&mut agents, &mut records, None, ReadRecordScope::Local);
        assert_eq!(
            agents
                .iter()
                .map(|agent| agent.pane_id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "third"]
        );
    }

    #[test]
    fn summary_is_compact_and_missing_summary_has_an_actionable_label() {
        let projected = project_agents(payload(json!([
            {"pane_id":"long","tokens":{"status_idle":"○","activity":"0000000000001","summary":"  one   two three four five six seven eight nine ten  ","elapsed":"4m"}},
            {"pane_id":"missing","tokens":{"status_idle":"○","activity":"0000000000000"}}
        ]))).agents;
        assert!(projected[0].summary.chars().count() <= 30);
        assert_eq!(projected[0].elapsed, "4m");
        assert_eq!(projected[1].summary, "Check agent-context-labels settings");
        assert_eq!(projected[1].elapsed, "0s");
    }

    #[test]
    fn one_broken_agent_excludes_only_itself_and_names_why() {
        let projection = project_agents(payload(json!([
            {"pane_id":"good","tokens":{"status_working":"●","activity":"0000000000002"}},
            {"pane_id":"bad-activity","tokens":{"status_idle":"○","activity":"oops"}},
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
        assert_eq!(
            projection.excluded[0].pane_id.as_deref(),
            Some("bad-activity")
        );
        assert!(projection.excluded[0].reason.contains("invalid activity"));
        assert_eq!(projection.excluded[1].pane_id, None);
        assert!(projection.excluded[1].reason.contains("pane id"));
    }

    #[test]
    fn official_state_change_sequence_replaces_only_a_missing_activity_token() {
        let projection = project_agents(payload(json!([
            {
                "pane_id":"remote",
                "state_change_seq":218,
                "agent_status":"done",
                "tokens":{"status_done":"●","summary":"finished"}
            },
            {
                "pane_id":"malformed",
                "state_change_seq":219,
                "tokens":{"status_idle":"○","activity":"not-a-time"}
            }
        ])));

        assert_eq!(projection.agents.len(), 1);
        assert_eq!(projection.agents[0].pane_id, "remote");
        assert_eq!(projection.agents[0].last_activity, "00000000000000000218");
        assert_eq!(projection.excluded.len(), 1);
        assert!(projection.excluded[0].reason.contains("invalid activity"));
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

        apply_read_state(&mut agents, &mut records, None, ReadRecordScope::Local);
        assert!(
            agents
                .iter()
                .all(|agent| agent.unread && agent.group == "done"),
            "nothing is read before the operator focuses anything"
        );

        apply_read_state(&mut agents, &mut records, Some("b"), ReadRecordScope::Local);
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
        apply_read_state(&mut agents, &mut records, Some("b"), ReadRecordScope::Local);

        let seen_by_herdr = json!([
            {"pane_id":"a","agent_status":"idle","state_change_seq":9,
             "tokens":{"status_done":"\u{25cf}","activity":"0000000000001"}},
            {"pane_id":"b","agent_status":"idle","state_change_seq":9,
             "tokens":{"status_done":"\u{25cf}","activity":"0000000000001"}}
        ]);
        let mut agents = projected(seen_by_herdr);
        apply_read_state(&mut agents, &mut records, None, ReadRecordScope::Local);
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
        apply_read_state(
            &mut watched,
            &mut records,
            Some("a"),
            ReadRecordScope::Local,
        );
        assert!(!watched[0].unread, "a question on the focused pane is read");
        assert_eq!(watched[0].group, "seen");

        let moved_on = json!([{
            "pane_id":"a","agent_status":"idle","state_change_seq":3,
            "tokens":{"status_question_new":"?","activity":"0000000000003"}
        }]);
        let mut later = projected(moved_on);
        apply_read_state(
            &mut later,
            &mut records,
            Some("elsewhere"),
            ReadRecordScope::Local,
        );
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
        apply_read_state(
            &mut working,
            &mut records,
            Some("a"),
            ReadRecordScope::Local,
        );
        assert!(!working[0].unread);

        let mut asking = projected(json!([{
            "pane_id":"a","agent_status":"working","state_change_seq":7,
            "tokens":{"status_question_new":"?","status_working":"\u{25cf}","activity":"0000000000001"}
        }]));
        apply_read_state(&mut asking, &mut records, None, ReadRecordScope::Local);
        assert!(
            asking[0].unread,
            "a new demand is unread even at the same sequence"
        );
    }

    /// AC4. Records for panes Herdr no longer reports are dropped, but only
    /// when the caller holds a fresh agent list. Applying the same list twice
    /// converges (engineering rule 11).
    #[test]
    fn read_records_are_evicted_only_against_a_fresh_agent_list() {
        let mut agents = projected(json!([finished("a", 1)]));
        let mut records = BTreeMap::new();
        apply_read_state(&mut agents, &mut records, Some("a"), ReadRecordScope::Local);
        let after_first = records.clone();
        assert!(
            apply_read_state(&mut agents, &mut records, Some("a"), ReadRecordScope::Local)
                .is_empty()
        );
        assert_eq!(records, after_first, "a repeated apply changes nothing");

        let mut none: Vec<SidebarAgentSnapshot> = Vec::new();
        apply_read_state(&mut none, &mut records, None, ReadRecordScope::Retain);
        assert!(
            records.contains_key("a"),
            "an empty list from before the first sync must not wipe the record"
        );
        apply_read_state(&mut none, &mut records, None, ReadRecordScope::Local);
        assert!(
            records.is_empty(),
            "a pane Herdr stopped reporting is dropped"
        );
    }

    /// The ordering key falls back to Herdr's own sequence, zero padded to the
    /// activity token's width, when the label plugin sent nothing.
    #[test]
    fn last_activity_falls_back_to_the_herdr_sequence() {
        let projection = project_agents(payload(json!([
            {"pane_id":"working","agent_status":"working","state_change_seq":1}
        ])));
        assert_eq!(projection.agents[0].last_activity, "00000000000000000001");
        assert_eq!(projection.agents[0].state_change_seq, Some(1));
    }

    #[test]
    fn ambient_counts_parse_and_unknown_keys_never_survive() {
        let projection = project_agents(payload(json!([
            {"pane_id":"legacy","tokens":{"status_idle":"○","activity":"0000000000001"}},
            {"pane_id":"counted","tokens":{"status_working":"●","activity":"0000000000002"},
             "ambient":{"subagents_active":2,"background_running":1,"background_failed":0,
                        "task_name":"SENTINEL-do-not-leak","command":"SENTINEL-rm -rf /"}}
        ])));

        assert_eq!(projection.excluded, []);
        let counted = projection
            .agents
            .iter()
            .find(|agent| agent.pane_id == "counted")
            .expect("counted agent");
        let ambient = counted.ambient.expect("ambient present");
        assert_eq!(ambient.subagents_active, 2);
        assert_eq!(ambient.background_running, 1);
        assert_eq!(
            projection
                .agents
                .iter()
                .find(|agent| agent.pane_id == "legacy")
                .and_then(|agent| agent.ambient),
            None,
            "a snapshot without the key stays on the legacy path"
        );
        let serialized = serde_json::to_string(&projection.agents).expect("serialize agents");
        assert!(
            !serialized.contains("SENTINEL"),
            "unknown ambient keys must never reach the projected agent: {serialized}"
        );
    }

    #[test]
    fn a_malformed_ambient_record_excludes_only_that_agent() {
        let projection = project_agents(payload(json!([
            {"pane_id":"broken","tokens":{"status_working":"●","activity":"0000000000002"},
             "ambient":{"subagents_active":"not-a-number"}},
            {"pane_id":"intact","tokens":{"status_idle":"○","activity":"0000000000001"}}
        ])));

        assert_eq!(
            projection
                .agents
                .iter()
                .map(|agent| agent.pane_id.as_str())
                .collect::<Vec<_>>(),
            ["intact"]
        );
        assert_eq!(projection.excluded.len(), 1);
        assert_eq!(projection.excluded[0].pane_id.as_deref(), Some("broken"));
        assert!(projection.excluded[0].reason.contains("subagents_active"));
    }
}
