//! The Sessions tool's groups, lines and order, from the row's own state
//! (docs/status-model.md, The Sessions tool), and each row's own PR summary.
//! Memory and conversation history are separate readers, not session state.
use crate::model::{PullRequestBadge, PullRequestChecks, ReviewDecision, SidebarAgentSnapshot};
use crate::request_view::AgentPullRequestSnapshot;
use serde::{Deserialize, Serialize};

/// How long a resolved session stays under Resolved (PRD D-23).
pub const RESOLVED_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveSource {
    Operator,
    Automatic,
}

/// A record written before the 24-hour window also carried its local date;
/// serde ignores that field on read and the next save drops it.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Resolution {
    pub at_unix_ms: u64,
    pub source: ResolveSource,
    pub session: Option<String>,
    pub activity: String,
}

pub(crate) fn auto_resolvable(agent: &SidebarAgentSnapshot, input_after: Option<u64>) -> bool {
    if agent.activity != "stopped" || agent.blocked || agent.demand != "none" {
        return false;
    }
    let mut assigned = agent
        .request
        .iter()
        .flat_map(|r| &r.pull_requests)
        .filter(|p| p.duty)
        .peekable();
    assigned.peek().is_some()
        && assigned.all(|p| {
            p.badge.is_settled()
                && input_after.is_none_or(|input| p.settled_at_unix_ms.is_some_and(|at| at > input))
        })
}

/// The agent's own state names the group, the same names the sidebar uses
/// (PRD D-16): no PR stage and no tag decides it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    NeedsYou,
    Working,
    Done,
    #[default]
    Idle,
    Resolved,
}

pub const GROUPS: [Group; 5] = [
    Group::NeedsYou,
    Group::Working,
    Group::Done,
    Group::Idle,
    Group::Resolved,
];

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Row {
    pub group: Group,
    /// What the session is doing or has done (PRD B31); absent rather than
    /// invented. A Needs You row draws its `ask` instead.
    pub line: Option<String>,
    /// The line is the remaining work of an unfinished turn (◐).
    pub unfinished: bool,
}

pub(crate) fn row(agent: &SidebarAgentSnapshot) -> Row {
    let label_line = || agent.request.as_ref().and_then(|r| r.line.clone());
    let unfinished = super::axes::stopped_unfinished(agent);
    if agent.resolved.is_some() {
        return Row {
            group: Group::Resolved,
            ..Row::default()
        };
    }
    match agent.group.as_str() {
        // A Needs You row draws its ask; a block has none and draws its
        // cause (agent-blocked-state B1).
        "needs_you" => Row {
            group: Group::NeedsYou,
            line: (agent.demand == "error" && agent.unread)
                .then(|| agent.detail.clone())
                .flatten(),
            unfinished: false,
        },
        "working" => Row {
            group: Group::Working,
            line: if agent.wait == Some(crate::model::AgentWait::Children) {
                agent.descendant_line.clone()
            } else {
                label_line()
            },
            unfinished: false,
        },
        "done" => Row {
            group: Group::Done,
            line: label_line(),
            unfinished: false,
        },
        // A read block rests here with its cause, ahead of the fold
        // (agent-blocked-state B2).
        _ if agent.demand == "error" => Row {
            group: Group::Idle,
            line: agent.detail.clone(),
            unfinished: true,
        },
        _ => Row {
            group: Group::Idle,
            line: label_line().filter(|_| unfinished),
            unfinished,
        },
    }
}

pub(crate) fn mergeable(pull: &AgentPullRequestSnapshot) -> bool {
    pull.checks == PullRequestChecks::Passing
        && matches!(pull.review, None | Some(ReviewDecision::Approved))
}

/// One PR's state as a chip or icon draws it, worst first (PRD D-30).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Failed,
    /// Checks running, or waiting on a review.
    Pending,
    Mergeable,
    Merged,
}

pub(crate) fn pr_state(pull: &AgentPullRequestSnapshot) -> Option<PrState> {
    match pull.badge {
        PullRequestBadge::Closed => None,
        PullRequestBadge::Merged => Some(PrState::Merged),
        PullRequestBadge::Open | PullRequestBadge::Review => {
            Some(if pull.checks == PullRequestChecks::Failed {
                PrState::Failed
            } else if mergeable(pull) {
                PrState::Mergeable
            } else {
                PrState::Pending
            })
        }
    }
}

/// The PRs this row holds the duty of, never its descendants' (PRD D-18,
/// D-39): how many, the worst state and how many share it, and each PR as an
/// index into `request.pull_requests`, worst first. Closed PRs are not counted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PrSummary {
    pub count: usize,
    pub worst: PrState,
    pub worst_count: usize,
    pub pulls: Vec<OwnPr>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OwnPr {
    pub index: usize,
    pub state: PrState,
}

pub(crate) fn own_prs(agent: &SidebarAgentSnapshot) -> Option<PrSummary> {
    let mut pulls: Vec<OwnPr> = agent
        .request
        .iter()
        .flat_map(|request| request.pull_requests.iter().enumerate())
        .filter(|(_, pull)| pull.duty && pull.live)
        .filter_map(|(index, pull)| pr_state(pull).map(|state| OwnPr { index, state }))
        .collect();
    pulls.sort_by_key(|pull| pull.state);
    let worst = pulls.first()?.state;
    Some(PrSummary {
        count: pulls.len(),
        worst,
        worst_count: pulls.iter().filter(|pull| pull.state == worst).count(),
        pulls,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Section {
    pub group: Group,
    /// Member indices, in the order the panel draws them.
    pub members: Vec<usize>,
    /// Idle rows with neither a PR nor unfinished work, folded as "N more".
    pub more: Vec<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Scope {
    /// Nonempty groups only, in `GROUPS` order.
    pub groups: Vec<Section>,
}

pub(crate) fn scope<'a>(members: impl Iterator<Item = (usize, &'a SidebarAgentSnapshot)>) -> Scope {
    let members: Vec<_> = members
        .filter(|(_, row)| {
            if row.resolved.is_some() {
                row.resolved_recent
            } else {
                !row.delegated
            }
        })
        .collect();
    let mut result = Scope::default();
    for group in GROUPS {
        let mut rows: Vec<_> = members
            .iter()
            .filter(|(_, row)| row.state.session.group == group)
            .copied()
            .collect();
        let failed = |row: &SidebarAgentSnapshot| {
            row.state
                .pr
                .as_ref()
                .is_some_and(|pr| pr.worst == PrState::Failed)
        };
        let idle_rank = |row: &SidebarAgentSnapshot| {
            if row.state.session.unfinished {
                0
            } else {
                match row.state.pr.as_ref().map(|pr| pr.worst) {
                    Some(PrState::Failed) => 1,
                    Some(PrState::Mergeable) => 2,
                    Some(PrState::Pending) => 3,
                    Some(PrState::Merged) => 4,
                    None => 5,
                }
            }
        };
        rows.sort_by(|(a_index, a), (b_index, b)| {
            let recent = || {
                b.last_activity
                    .cmp(&a.last_activity)
                    .then_with(|| a_index.cmp(b_index))
            };
            match group {
                Group::NeedsYou => a
                    .state
                    .request_since
                    .cmp(&b.state.request_since)
                    .then_with(|| a_index.cmp(b_index)),
                Group::Working => failed(b).cmp(&failed(a)).then_with(recent),
                Group::Done => recent(),
                Group::Idle => idle_rank(a).cmp(&idle_rank(b)).then_with(recent),
                Group::Resolved => {
                    let at =
                        |row: &SidebarAgentSnapshot| row.resolved.as_ref().map(|r| r.at_unix_ms);
                    at(b).cmp(&at(a)).then_with(|| a_index.cmp(b_index))
                }
            }
        });
        if rows.is_empty() {
            continue;
        }
        let (shown, more): (Vec<_>, Vec<_>) = rows
            .into_iter()
            .partition(|(_, row)| group != Group::Idle || idle_rank(row) < 5);
        result.groups.push(Section {
            group,
            members: shown.into_iter().map(|(member, _)| member).collect(),
            more: more.into_iter().map(|(member, _)| member).collect(),
        });
    }
    result
}
