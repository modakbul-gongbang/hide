//! The Sessions tool's groups and tags, derived once from the row's verb.
//! Memory and conversation history are separate readers, not session state.
use crate::model::{PullRequestChecks, ReviewDecision, SidebarAgentSnapshot};
use serde::{Deserialize, Serialize};

use super::RequestVerb;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveSource {
    Operator,
    Automatic,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Resolution {
    pub at_unix_ms: u64,
    pub source: ResolveSource,
    pub local_date: String,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    MyTurn,
    ReviewMerge,
    InProgress,
    #[default]
    Resting,
    ResolvedToday,
}

pub const GROUPS: [Group; 5] = [
    Group::MyTurn,
    Group::ReviewMerge,
    Group::InProgress,
    Group::Resting,
    Group::ResolvedToday,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    Answer,
    Approval,
    Fix,
    Review,
    Merge,
    Stopped,
    Result,
    Working,
    CiWait,
    Waiting,
    Idle,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Row {
    pub group: Group,
    /// An unlabelled row keeps its outline without inventing a task line.
    pub tag: Option<Tag>,
}

pub(crate) fn row(agent: &SidebarAgentSnapshot, verb: RequestVerb) -> Row {
    if agent.resolved.is_some() {
        return Row {
            group: Group::ResolvedToday,
            tag: None,
        };
    }
    if agent.escalation.is_some() {
        return Row {
            group: Group::MyTurn,
            tag: Some(if agent.blocked {
                Tag::Approval
            } else if agent
                .escalation
                .as_ref()
                .is_some_and(|e| e.cause != super::escalation::Cause::ObserverUnconfirmed)
                || agent.demand == "question"
            {
                Tag::Answer
            } else {
                Tag::Stopped
            }),
        };
    }
    let tag = tag(agent, verb);
    let group = match verb {
        RequestVerb::Answer | RequestVerb::Fix | RequestVerb::Stopped | RequestVerb::Result => {
            Group::MyTurn
        }
        RequestVerb::Review => Group::ReviewMerge,
        RequestVerb::Working | RequestVerb::Waiting => Group::InProgress,
        RequestVerb::Idle => Group::Resting,
    };
    Row {
        group,
        tag: agent
            .request
            .as_ref()
            .and_then(|request| request.line.as_ref().map(|_| tag)),
    }
}

pub(super) fn mergeable(pull: &crate::request_view::AgentPullRequestSnapshot) -> bool {
    pull.checks == PullRequestChecks::Passing
        && matches!(pull.review, None | Some(ReviewDecision::Approved))
}

pub(crate) fn tag(agent: &SidebarAgentSnapshot, verb: RequestVerb) -> Tag {
    match verb {
        RequestVerb::Answer if agent.blocked => Tag::Approval,
        RequestVerb::Answer => Tag::Answer,
        RequestVerb::Fix => Tag::Fix,
        RequestVerb::Review => {
            let mut duties = agent
                .request
                .iter()
                .flat_map(|request| &request.pull_requests)
                .filter(|pull| pull.live && pull.duty && !pull.badge.is_settled())
                .peekable();
            if duties.peek().is_some() && duties.all(mergeable) {
                Tag::Merge
            } else {
                Tag::Review
            }
        }
        RequestVerb::Stopped => Tag::Stopped,
        RequestVerb::Result => Tag::Result,
        RequestVerb::Working => Tag::Working,
        RequestVerb::Waiting
            if agent.request.iter().any(|request| {
                request.pull_requests.iter().any(|pull| {
                    pull.live
                        && pull.duty
                        && !pull.badge.is_settled()
                        && pull.checks == PullRequestChecks::Pending
                })
            }) =>
        {
            Tag::CiWait
        }
        RequestVerb::Waiting => Tag::Waiting,
        RequestVerb::Idle => Tag::Idle,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Section {
    pub group: Group,
    /// Member indices, in the order the panel draws them.
    pub members: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Scope {
    pub counts: std::collections::BTreeMap<Group, usize>,
    pub groups: Vec<Section>,
    pub closed_prs: Vec<ClosedPr>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ClosedPr {
    pub project_id: String,
    pub number: u32,
    pub tag: Tag,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            counts: GROUPS.into_iter().map(|group| (group, 0)).collect(),
            groups: Vec::new(),
            closed_prs: Vec::new(),
        }
    }
}

pub(crate) fn add_closed_prs(
    scope: &mut Scope,
    project: &crate::model::WorkspaceSnapshot,
    numbers: Option<&std::collections::BTreeSet<u32>>,
    agents: &[&SidebarAgentSnapshot],
) {
    let before = scope.closed_prs.len();
    let live_urls: std::collections::HashSet<_> = agents
        .iter()
        .flat_map(|agent| {
            agent
                .request
                .iter()
                .flat_map(|request| &request.pull_requests)
        })
        .filter(|pull| pull.live)
        .map(|pull| pull.url.as_str())
        .collect();
    for pr in &project.pull_requests {
        if pr.badge.is_settled()
            || !numbers.is_some_and(|numbers| numbers.contains(&pr.number))
            || live_urls.contains(pr.url.as_str())
        {
            continue;
        }
        scope.closed_prs.push(ClosedPr {
            project_id: project.id.clone(),
            number: pr.number,
            tag: if pr.checks == PullRequestChecks::Passing
                && matches!(pr.review, None | Some(ReviewDecision::Approved))
            {
                Tag::Merge
            } else {
                Tag::Review
            },
        });
    }
    if !scope.closed_prs.is_empty() {
        *scope
            .counts
            .get_mut(&Group::ReviewMerge)
            .expect("all session groups counted") += scope.closed_prs.len() - before;
        if !scope
            .groups
            .iter()
            .any(|section| section.group == Group::ReviewMerge)
        {
            let at = scope
                .groups
                .iter()
                .position(|section| section.group > Group::ReviewMerge)
                .unwrap_or(scope.groups.len());
            scope.groups.insert(
                at,
                Section {
                    group: Group::ReviewMerge,
                    members: Vec::new(),
                },
            );
        }
    }
}

pub(crate) fn scope<'a>(members: impl Iterator<Item = (usize, &'a SidebarAgentSnapshot)>) -> Scope {
    let members: Vec<_> = members
        .filter(|(_, row)| {
            if row.resolved.is_some() {
                row.resolved_today
            } else {
                !row.delegated || row.escalation.is_some()
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
        rows.sort_by(|(a_index, a), (b_index, b)| {
            if matches!(group, Group::MyTurn | Group::ReviewMerge) {
                a.state
                    .request_since
                    .cmp(&b.state.request_since)
                    .then_with(|| a_index.cmp(b_index))
            } else {
                b.last_activity
                    .cmp(&a.last_activity)
                    .then_with(|| a_index.cmp(b_index))
            }
        });
        result.counts.insert(group, rows.len());
        if !rows.is_empty() {
            result.groups.push(Section {
                group,
                members: rows.into_iter().map(|(member, _)| member).collect(),
            });
        }
    }
    result
}
