//! What the operator needs to do, derived from the five axes.
use super::axes::*;
use super::tally::RowMark;

/// Runtime gates deliberately distinguish a running process from a root
/// waiting on work it delegated. Both prevent sleep and reopening.
pub(crate) fn is_running(agent: &SidebarAgentSnapshot) -> bool {
    agent.activity == "working"
}

pub(crate) fn has_work_in_progress(agent: &SidebarAgentSnapshot) -> bool {
    is_running(agent) || agent.group == "working"
}

pub(crate) fn is_waiting_for_operator(agent: &SidebarAgentSnapshot) -> bool {
    agent.demand != "none" || agent.blocked
}

pub(crate) fn is_seen(agent: &SidebarAgentSnapshot) -> bool {
    agent.group == "seen"
}

pub(crate) fn rest_refusal(agent: &SidebarAgentSnapshot) -> Option<&'static str> {
    if has_work_in_progress(agent) {
        Some("This agent is working")
    } else if is_waiting_for_operator(agent) {
        Some("This agent is waiting for you")
    } else if agent.activity != "stopped" {
        Some("Hide cannot tell what this agent is doing")
    } else {
        None
    }
}
use crate::labels::analysis::LabelEnd;
use crate::model::{AgentStatusCode, PullRequestChecks, SidebarAgentSnapshot};
use crate::request_view::AgentPullRequestSnapshot;
use serde::{Deserialize, Serialize};

/// The four groups the sidebar reads top to bottom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentGroup {
    NeedsYou,
    Done,
    Working,
    Seen,
}

impl AgentGroup {
    pub(crate) fn name(self) -> &'static str {
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
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::NeedsYou => 0,
            Self::Done => 1,
            Self::Working => 2,
            Self::Seen => 3,
        }
    }
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

/// The one short word a row shows. A reported completion the operator has not
/// read is `Done`; an ordinary stopped pane and a read completion are `Idle`.
/// No view ever shows an axis value, so nothing underscored can reach the
/// screen.
/// The status word of a root waiting on its children.
pub(crate) fn agent_status_code(
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
pub(crate) fn agent_requires_close_confirmation(
    activity: AgentActivity,
    demand: AgentDemand,
    blocked: bool,
) -> bool {
    activity == AgentActivity::Working || demand != AgentDemand::None || blocked
}

/// An activity-less pane cannot safely be described as idle in a destructive
/// confirmation. The caller must obtain a fresh status before it can close.
pub(crate) fn agent_requires_close_status_check(
    activity: AgentActivity,
    demand: AgentDemand,
    blocked: bool,
) -> bool {
    activity == AgentActivity::Unknown && demand == AgentDemand::None && !blocked
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
pub(crate) fn agent_second_line(
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
pub(crate) fn sort_agents(agents: &mut [SidebarAgentSnapshot]) {
    agents.sort_by(|left, right| {
        group_of(left)
            .rank()
            .cmp(&group_of(right).rank())
            .then_with(|| right.last_activity.cmp(&left.last_activity))
    });
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

/// Fills in every value the shell draws from the lifecycle axes.
///
/// Called again whenever the read axis moves, so the derived values can never
/// describe a different read state than the row they sit on.
pub(crate) fn derive_from_axes(agent: &mut SidebarAgentSnapshot) {
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

/// Re-derives every drawn value of a row whose sleep mark was just set.
pub fn rederive(agent: &mut SidebarAgentSnapshot) {
    derive_from_axes(agent);
}

/// What a row asks of the operator now, in the order the view draws its
/// groups (D-06).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestVerb {
    /// A question or an approval waits on the operator.
    Answer,
    /// A pull request's checks failed.
    Fix,
    /// A pull request is ready to review or merge.
    Review,
    /// The agent stopped before its work was done (AI `unfinished`).
    Stopped,
    /// A finished turn, or a pull request settled since the last request,
    /// the operator has not looked at.
    Result,
    Working,
    /// Its pull request's checks, its descendants, or something it named.
    Waiting,
    Idle,
}

pub(crate) fn verb_of(
    row: &SidebarAgentSnapshot,
    pull_requests: &[AgentPullRequestSnapshot],
    result_opened: Option<u64>,
) -> RequestVerb {
    if row.demand != "none" {
        return RequestVerb::Answer;
    }
    if row.activity == "working" {
        return RequestVerb::Working;
    }
    let duty = || {
        pull_requests
            .iter()
            .filter(|pull_request| pull_request.live && pull_request.duty)
    };
    let open = |checks: &[PullRequestChecks]| {
        duty().any(|pull_request| {
            !pull_request.badge.is_settled() && checks.contains(&pull_request.checks)
        })
    };
    if open(&[PullRequestChecks::Failed]) {
        return RequestVerb::Fix;
    }
    if open(&[
        PullRequestChecks::Passing,
        PullRequestChecks::None,
        PullRequestChecks::Unknown,
    ]) {
        return RequestVerb::Review;
    }
    // Only the label analysis reads a turn as unfinished (D-33); a row
    // without one never stops here.
    let end = row.row_facts.as_ref().and_then(|facts| facts.end);
    if end == Some(LabelEnd::Unfinished) && !row.waiting_on_descendants {
        return RequestVerb::Stopped;
    }
    if open(&[PullRequestChecks::Pending]) {
        return RequestVerb::Waiting;
    }
    // A settled pull request is a result until the operator opens it.
    let unseen = |pull_request: &AgentPullRequestSnapshot| match result_opened {
        None => true,
        Some(opened) => pull_request
            .settled_at_unix_ms
            .is_some_and(|settled| settled > opened),
    };
    if (row.completed && row.unread)
        || duty().any(|pull_request| pull_request.badge.is_settled() && unseen(pull_request))
    {
        return RequestVerb::Result;
    }
    if row.waiting_on_descendants || end == Some(LabelEnd::Waiting) {
        return RequestVerb::Waiting;
    }
    RequestVerb::Idle
}
