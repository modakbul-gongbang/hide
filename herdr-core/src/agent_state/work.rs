//! Association of a session with PRs and issues, and the one holder of each duty.
use crate::model::{GithubSnapshot, PullRequestChecks, PullRequestSnapshot, SidebarAgentSnapshot};
use crate::request_view::{AgentPullRequestSnapshot, RowPlace};
use std::collections::HashMap;

pub(crate) struct Linked<'a> {
    pull_request: &'a PullRequestSnapshot,
    on_branch: bool,
    /// When the session's tool printed it, for a pull request it made.
    sighted_at: Option<u64>,
    duty: bool,
}

pub(crate) fn linked_pull_requests<'a>(
    row: &SidebarAgentSnapshot,
    place: Option<RowPlace<'_>>,
    github: &'a GithubSnapshot,
) -> Vec<Linked<'a>> {
    let Some(place) = place else {
        return Vec::new();
    };
    let mut linked: Vec<Linked<'a>> = Vec::new();
    if let Some(project) = github.project(place.root_path) {
        linked.extend(
            crate::github::pull_request_for_checkout(
                &project.pull_requests,
                place.branch,
                place.head_sha,
            )
            .map(|pull_request| Linked {
                pull_request,
                on_branch: true,
                sighted_at: None,
                duty: false,
            }),
        );
    }
    let created = row
        .row_facts
        .as_ref()
        .map(|facts| facts.created_prs.as_slice())
        .unwrap_or_default();
    for (repository, number, sighted_at) in created {
        let found = github
            .projects
            .iter()
            .flat_map(|project| project.pull_requests.iter())
            .find(|pull_request| {
                u64::from(pull_request.number) == *number
                    && hide_session::pull_request_addresses(&pull_request.url)
                        .first()
                        .is_some_and(|(repo, _)| repo.eq_ignore_ascii_case(repository))
            });
        let Some(pull_request) = found else {
            continue;
        };
        match linked
            .iter_mut()
            .find(|known| known.pull_request.url == pull_request.url)
        {
            Some(known) => known.sighted_at = Some(*sighted_at),
            None => linked.push(Linked {
                pull_request,
                on_branch: false,
                sighted_at: Some(*sighted_at),
                duty: false,
            }),
        }
    }
    linked
}

/// Gives each pull request's duty to one row: the first on its branch's
/// checkout, else the one whose session printed it first.
pub(crate) fn assign_duty(rows: &mut [Vec<Linked<'_>>]) {
    let mut holder: HashMap<&str, (usize, usize, bool, u64)> = HashMap::new();
    for (row, linked) in rows.iter().enumerate() {
        for (index, link) in linked.iter().enumerate() {
            let rank = (link.on_branch, link.sighted_at.unwrap_or(u64::MAX));
            let better = |held: &(usize, usize, bool, u64)| {
                (rank.0 && !held.2) || (rank.0 == held.2 && !rank.0 && rank.1 < held.3)
            };
            match holder.get(link.pull_request.url.as_str()) {
                Some(held) if !better(held) => {}
                _ => {
                    holder.insert(&link.pull_request.url, (row, index, rank.0, rank.1));
                }
            }
        }
    }
    let chosen: Vec<(usize, usize)> = holder
        .values()
        .map(|(row, index, _, _)| (*row, *index))
        .collect();
    for (row, index) in chosen {
        rows[row][index].duty = true;
    }
}

fn settled_at(pull_request: &PullRequestSnapshot) -> Option<u64> {
    pull_request
        .merged_at_unix_ms
        .or(pull_request.closed_at_unix_ms)
}

fn is_open(pull_request: &PullRequestSnapshot) -> bool {
    !pull_request.badge.is_settled()
}

/// The order a row's live pull requests are looked at in (D-46).
fn urgency(pull_request: &AgentPullRequestSnapshot) -> u8 {
    match (pull_request.badge.is_settled(), pull_request.checks) {
        (false, PullRequestChecks::Failed) => 0,
        (false, PullRequestChecks::Passing | PullRequestChecks::None) => 1,
        (false, PullRequestChecks::Pending) => 2,
        _ => 3,
    }
}

pub(crate) fn shown_pull_requests(
    linked: Vec<Linked<'_>>,
    operator_at: Option<u64>,
) -> Vec<AgentPullRequestSnapshot> {
    let mut shown: Vec<(AgentPullRequestSnapshot, u64)> = linked
        .into_iter()
        .map(|link| {
            let pull_request = link.pull_request;
            let settled = settled_at(pull_request);
            let live = is_open(pull_request)
                || matches!((settled, operator_at), (Some(settled), Some(asked)) if settled > asked);
            let recency = pull_request
                .created_at_unix_ms
                .or(pull_request.updated_at_unix_ms)
                .unwrap_or(0);
            (
                AgentPullRequestSnapshot {
                    number: pull_request.number,
                    title: pull_request.title.clone(),
                    url: pull_request.url.clone(),
                    badge: pull_request.badge,
                    checks: pull_request.checks,
                    head_branch: pull_request.head_branch.clone(),
                    closing_issues: pull_request.closing_issues.clone(),
                    live,
                    duty: link.duty,
                    created: link.sighted_at.is_some(),
                    settled_at_unix_ms: settled,
                },
                recency,
            )
        })
        .collect();
    shown.sort_by(|(left, left_recency), (right, right_recency)| {
        right
            .live
            .cmp(&left.live)
            .then_with(|| urgency(left).cmp(&urgency(right)))
            .then_with(|| right_recency.cmp(left_recency))
            .then_with(|| right.number.cmp(&left.number))
    });
    shown
        .into_iter()
        .map(|(pull_request, _)| pull_request)
        .collect()
}
