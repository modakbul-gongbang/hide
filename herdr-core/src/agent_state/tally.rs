//! Scope totals, marks and representative ordering.
use super::axes::*;
use super::turn::*;
use crate::model::SidebarAgentSnapshot;
use crate::pet::PetSummary;
use crate::sidebar::sync_checkout_purposes;
use std::collections::BTreeMap;

/// The mark a row draws, matching the retired label plugin's symbol table. It is
/// one decision for the row's own symbol and for every count of marks, so a
/// badge that stands for folded rows says what opening them would show.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RowMark {
    Error,
    Approval,
    Question,
    Working,
    Done,
    Idle,
    Unknown,
}

impl RowMark {
    pub(crate) fn of(agent: &SidebarAgentSnapshot) -> Self {
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

    pub(crate) fn symbol(self) -> &'static str {
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
pub(crate) fn child_representative_rank(agent: &SidebarAgentSnapshot) -> (u8, u8) {
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

/// Buckets the projected agent list.
///
/// While the herdr connection is down the last valid agent list is retained
/// (so the pet does not blink to empty) but every retained agent counts as
/// disconnected: a stale yellow "act now" badge for a server that is no
/// longer answering is exactly the failure this prevents.
pub fn summarize(agents: &[SidebarAgentSnapshot], connected: bool) -> PetSummary {
    let mut summary = PetSummary::default();
    for agent in agents {
        if !connected {
            summary.disconnected += 1;
            continue;
        }
        // The groups are decided once, in the projection. The pet counts them
        // through the projection's own enum rather than reading tokens, axes,
        // or group names a second time.
        match crate::agent_state::group_of(agent) {
            AgentGroup::NeedsYou => {
                summary.needs_you += 1;
                if crate::agent_state::demand_of(agent) == AgentDemand::Error {
                    summary.error += 1;
                }
            }
            AgentGroup::Done => summary.done += 1,
            AgentGroup::Working => summary.working += 1,
            AgentGroup::Seen => summary.seen += 1,
        }
    }
    summary
}

/// The in-process subagents Hide's hook reports as working, summed over the
/// listed agents. A count is only meaningful while the server is answering,
/// and only a pane that still holds an agent is counted, so a token left on
/// a pane whose agent exited does not linger in the badge.
pub fn subagents_active(
    agents: &[SidebarAgentSnapshot],
    hook_tokens: &BTreeMap<String, crate::agent_hooks::PaneHookTokens>,
    connected: bool,
) -> u32 {
    if !connected {
        return 0;
    }
    agents
        .iter()
        .filter_map(|agent| hook_tokens.get(&agent.pane_id))
        .filter_map(|tokens| tokens.working)
        .fold(0, u32::saturating_add)
}

/// Whether this agent is one the operator still has to act on.
///
/// Read through the projection's own enum, not by matching the group name a
/// second time: a name compared here is a copy of a vocabulary that lives in
/// one place.
pub fn is_unseen(agent: &SidebarAgentSnapshot) -> bool {
    crate::agent_state::group_of(agent) == AgentGroup::NeedsYou
}

/// The unseen panes in click order: the pane whose unseen state was observed
/// first, then snapshot order.
///
/// `observed` holds the first time each pane was seen unseen. It lives in
/// memory only (D-21), so after a restart every pane carries the same first
/// observation and the snapshot's own order decides - which is the approved
/// fallback, not a defect.
pub fn attention_order(
    agents: &[SidebarAgentSnapshot],
    observed: &BTreeMap<String, u64>,
) -> Vec<String> {
    let mut unseen = agents
        .iter()
        .enumerate()
        .filter(|(_, agent)| is_unseen(agent))
        .map(|(index, agent)| {
            (
                observed.get(&agent.pane_id).copied().unwrap_or(u64::MAX),
                index,
                agent.pane_id.clone(),
            )
        })
        .collect::<Vec<_>>();
    unseen.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    unseen.into_iter().map(|(_, _, pane_id)| pane_id).collect()
}

/// Records the first moment each currently-unseen pane became unseen and
/// forgets panes that are no longer unseen. Running it twice with the same
/// agent list leaves the map unchanged.
pub fn observe_unseen(
    observed: &mut BTreeMap<String, u64>,
    agents: &[SidebarAgentSnapshot],
    now_unix_ms: u64,
) {
    let unseen = agents
        .iter()
        .filter(|agent| is_unseen(agent))
        .map(|agent| agent.pane_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    observed.retain(|pane_id, _| unseen.contains(pane_id.as_str()));
    for pane_id in unseen {
        observed.entry(pane_id.to_owned()).or_insert(now_unix_ms);
    }
}
