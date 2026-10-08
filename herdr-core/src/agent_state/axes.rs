//! Lifecycle, completion, read and ownership axes, including lineage.
use super::turn::{derive_from_axes, sort_agents};
use crate::model::{PaneReadRecord, SidebarAgentSnapshot};
use crate::sidebar::SessionAgentPayload;
use std::collections::{BTreeMap, BTreeSet, HashSet};

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
    pub(crate) fn name(self) -> &'static str {
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

/// Reads the demand, activity and read axes back off a row.
///
/// `derive_from_axes` is the only writer of those fields and writes them from
/// these same enums, so the round trip is total.
pub(crate) fn axes_of(agent: &SidebarAgentSnapshot) -> (AgentDemand, AgentActivity, bool) {
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

/// The demand axis of a projected row, for callers outside this module.
///
/// It exists so nothing has to compare the published axis name against a
/// string literal of its own: the vocabulary lives in [`AgentDemand`] and is
/// read back through it.
pub fn demand_of(agent: &SidebarAgentSnapshot) -> AgentDemand {
    axes_of(agent).0
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
pub(crate) fn derive_read_state(
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
pub(crate) fn agent_demand(agent: &SessionAgentPayload) -> AgentDemand {
    if agent
        .facts
        .as_ref()
        .and_then(|facts| facts.user_turn.as_ref())
        .is_some_and(|turn| turn.kind == hide_session::turns::UserTurnKind::Question)
        || agent.label.as_ref().is_some_and(|label| label.question)
    {
        AgentDemand::Question
    } else if agent_blocked(agent) {
        AgentDemand::Approval
    } else {
        AgentDemand::None
    }
}

/// Session plan approval holds the same state as Herdr's blocked prompt.
/// A current-state read cannot hold an agent that is already working again.
pub(crate) fn agent_blocked(agent: &SessionAgentPayload) -> bool {
    agent.agent_status.as_deref() == Some("blocked")
        || (agent.agent_status.as_deref() != Some("working")
            && agent
                .facts
                .as_ref()
                .is_some_and(|facts| facts.awaiting_operator))
}

/// The activity axis. A state Herdr does not name is reported as unknown
/// rather than folded into idle (engineering rule 4).
pub(crate) fn agent_activity(agent: &SessionAgentPayload) -> AgentActivity {
    match agent.agent_status.as_deref() {
        Some("working") => AgentActivity::Working,
        Some("idle") | Some("done") => AgentActivity::Stopped,
        _ => AgentActivity::Unknown,
    }
}

/// Whether this stopped pane has actually completed work: Herdr's `done`.
/// It is deliberately separate from activity and read state; Herdr's
/// tab-scoped seen value never decides Hide's pane-level read axis.
pub(crate) fn agent_completed(agent: &SessionAgentPayload) -> bool {
    agent.agent_status.as_deref() == Some("done")
}

/// A tab holding only delegated agent children stays out of the operator strip.
/// Ordinary terminal panes do not turn it back into an operator-owned tab.
pub(crate) fn tab_is_delegated(
    panes: &[crate::model::PaneSnapshot],
    agents: &[SidebarAgentSnapshot],
) -> bool {
    let mut holds_an_agent = false;
    let mut all_delegated = true;
    for pane in panes {
        let Some(agent) = agents.iter().find(|a| a.pane_id == pane.id) else {
            continue;
        };
        holds_an_agent = true;
        all_delegated &= agent.delegated;
    }
    holds_an_agent && all_delegated
}
