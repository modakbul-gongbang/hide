//! The Sessions tool's groups and tags, derived once from the row's verb.
//! Memory and conversation history are separate readers, not session state.
use crate::model::{PullRequestChecks, ReviewDecision, SidebarAgentSnapshot};
use serde::Serialize;

use super::RequestVerb;

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
    let tag = match verb {
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
            if duties.peek().is_some()
                && duties.all(|pull| {
                    pull.checks == PullRequestChecks::Passing
                        && matches!(pull.review, None | Some(ReviewDecision::Approved))
                })
            {
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
            }) => Tag::CiWait,
        RequestVerb::Waiting => Tag::Waiting,
        RequestVerb::Idle => Tag::Idle,
    };
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
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            counts: GROUPS.into_iter().map(|group| (group, 0)).collect(),
            groups: Vec::new(),
        }
    }
}

pub(crate) fn scope<'a>(
    members: impl Iterator<Item = (usize, &'a SidebarAgentSnapshot)>,
) -> Scope {
    let members: Vec<_> = members.filter(|(_, row)| !row.delegated).collect();
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
