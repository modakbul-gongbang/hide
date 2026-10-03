//! The request view's row (PRD overview-request-view): what the operator
//! asked each agent, what came of it, and what is theirs to do now.
//!
//! Everything here is derived from facts the core already holds, with no AI:
//! the session's own words the label worker read (`labels::facts`), Herdr's
//! state and Hide's read and demand axes on the row, the lineage, and the
//! pull requests GitHub reported. The verb is computed here and nowhere else
//! (D-07), so the web only sorts and draws.
//!
//! A row's pull requests are those of its checkout's branch and those its
//! session made (D-31, D-46). A pull request on several rows gives its duty
//! (fix, review) to one of them: the row on that branch's checkout, else the
//! row whose session printed it first.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::issues::IssueReference;
use crate::labels::analysis::LabelEnd;
use crate::labels::facts::{Reply, Request, Requester};
use crate::model::{
    GithubSnapshot, PullRequestBadge, PullRequestChecks, PullRequestSnapshot, SidebarAgentSnapshot,
};

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

/// What the label worker's facts give a row, laid on by the overlay only
/// while the pane's reference proves the session they were read from.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RowFacts {
    pub(crate) native_title: Option<String>,
    pub(crate) operator_request: Option<Request>,
    pub(crate) other_request: Option<Request>,
    pub(crate) reply: Option<Reply>,
    /// `(lowercase owner/name, number, when its tool printed it)`.
    pub(crate) created_prs: Vec<(String, u64, u64)>,
    /// How the label analysis read the turn's end and its line; `None`
    /// without one (summaries off, a failed analysis, a turn not judged
    /// yet).
    pub(crate) end: Option<LabelEnd>,
    pub(crate) line: Option<String>,
}

/// Who sent the request a row shows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum RequestSender {
    Operator,
    /// An hcoord sender, or the delegated child's parent by its title.
    Named(String),
    /// Something other than Hide's input wrote it.
    Agent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RequestLineSnapshot {
    pub text: String,
    /// The text was longer than the core keeps.
    pub cut: bool,
    pub images: u32,
    pub at_unix_ms: u64,
    pub sender: RequestSender,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReplySnapshot {
    pub text: String,
    pub cut: bool,
    pub at_unix_ms: u64,
}

/// One of a row's pull requests.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgentPullRequestSnapshot {
    pub number: u32,
    pub title: String,
    pub url: String,
    pub badge: PullRequestBadge,
    pub checks: PullRequestChecks,
    pub head_branch: String,
    pub closing_issues: Vec<IssueReference>,
    /// Drawn as the row's chip or counted in its `+N` (D-43): open, or
    /// settled after the operator's last request.
    pub live: bool,
    /// This row holds the pull request's duty (D-31).
    pub duty: bool,
    /// The row's session made it (D-31), as opposed to its branch having it.
    pub created: bool,
    pub settled_at_unix_ms: Option<u64>,
}

/// The request view's part of an agent row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgentRequestSnapshot {
    pub verb: RequestVerb,
    /// When the row took this verb; kept across a restart (D-40).
    pub verb_since_unix_ms: u64,
    /// The agent's own name for its session (D-12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_title: Option<String>,
    /// The label's line for the turn (B18, B47); absent without one, when
    /// the row shows its reply instead (D-12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// How the label read the turn's end, shown with the line when the row
    /// is expanded (D-28); absent without a label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<LabelEnd>,
    pub request: Option<RequestLineSnapshot>,
    /// Who sent a request after the operator's last one (B4).
    pub later_by: Option<RequestSender>,
    pub reply: Option<ReplySnapshot>,
    /// The chip first (D-46), then the other live ones, then settled ones.
    pub pull_requests: Vec<AgentPullRequestSnapshot>,
}

/// A row's verb and when it took it, kept in `core-state.json`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerbRecord {
    pub verb: RequestVerb,
    pub since_unix_ms: u64,
    /// When the operator last opened the row's result (D-29): a pull
    /// request that settled before then is a result already seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_opened_unix_ms: Option<u64>,
}

/// Where a row lives, for its branch's pull requests.
pub(crate) struct RowPlace<'a> {
    pub(crate) branch: Option<&'a str>,
    pub(crate) root_path: &'a str,
}

/// Lays the request block on every row. `place` answers a local row's
/// checkout; a device's rows have none (their pull requests are not read).
/// `verbs` keeps each pane's verb and since; returns whether it changed.
pub(crate) fn apply<'a>(
    rows: &mut [SidebarAgentSnapshot],
    place: impl Fn(&str) -> Option<RowPlace<'a>>,
    github: &GithubSnapshot,
    verbs: &mut BTreeMap<String, VerbRecord>,
    now_unix_ms: u64,
) -> bool {
    let names: HashMap<String, String> = rows
        .iter()
        .map(|row| (row.pane_id.clone(), row.identity_label.clone()))
        .collect();
    let mut linked: Vec<Vec<Linked<'_>>> = rows
        .iter()
        .map(|row| linked_pull_requests(row, place(&row.pane_id), github))
        .collect();
    assign_duty(&mut linked);
    let mut verbs_changed = false;
    for (row, linked) in rows.iter_mut().zip(linked) {
        let facts = row.row_facts.clone().unwrap_or_default();
        let parent = row
            .lineage_parent_pane_id
            .as_ref()
            .and_then(|parent| names.get(parent));
        let resolve = |request: &Request| sender(request, row.delegated, parent);
        let operator = facts
            .operator_request
            .as_ref()
            .map(|request| (request, resolve(request)));
        let other = facts
            .other_request
            .as_ref()
            .map(|request| (request, resolve(request)));
        let operator_at = operator
            .as_ref()
            .filter(|(_, sender)| *sender == RequestSender::Operator)
            .map(|(request, _)| request.at_unix_ms);
        let (shown, later_by) = match (operator, other) {
            (Some((request, RequestSender::Operator)), other) => (
                Some((request, RequestSender::Operator)),
                other
                    .filter(|(later, _)| later.at_unix_ms > request.at_unix_ms)
                    .map(|(_, sender)| sender),
            ),
            (one, two) => (
                [one, two]
                    .into_iter()
                    .flatten()
                    .max_by_key(|(request, _)| request.at_unix_ms),
                None,
            ),
        };
        let pull_requests = shown_pull_requests(linked, operator_at);
        let opened = verbs
            .get(&row.pane_id)
            .and_then(|record| record.result_opened_unix_ms);
        let verb = verb_of(row, &pull_requests, opened);
        let record = verbs.get(&row.pane_id).filter(|record| record.verb == verb);
        let since = match record {
            Some(record) => record.since_unix_ms,
            None => {
                verbs.insert(
                    row.pane_id.clone(),
                    VerbRecord {
                        verb,
                        since_unix_ms: now_unix_ms,
                        result_opened_unix_ms: opened,
                    },
                );
                verbs_changed = true;
                now_unix_ms
            }
        };
        row.request = Some(AgentRequestSnapshot {
            verb,
            verb_since_unix_ms: since,
            native_title: facts
                .native_title
                .as_deref()
                .and_then(|title| shown_name(title, MAX_TITLE_CHARS)),
            line: facts.line.clone(),
            end: facts.end,
            request: shown.map(|(request, sender)| RequestLineSnapshot {
                text: request.text.clone(),
                cut: request.cut,
                images: request.images,
                at_unix_ms: request.at_unix_ms,
                sender,
            }),
            later_by,
            reply: facts.reply.as_ref().map(|reply| ReplySnapshot {
                text: reply.text.clone(),
                cut: reply.cut,
                at_unix_ms: reply.at_unix_ms,
            }),
            pull_requests,
        });
    }
    verbs_changed
}

/// Drops the verb records of the panes `gone` names. The caller names
/// only panes an authoritative topology no longer has, so a list that is
/// empty for a moment (a Herdr reconnect, before the first session) never
/// restarts every wait. Returns whether any went.
pub(crate) fn prune_verbs(
    verbs: &mut BTreeMap<String, VerbRecord>,
    gone: impl Fn(&str) -> bool,
) -> bool {
    let before = verbs.len();
    verbs.retain(|pane, _| !gone(pane));
    verbs.len() != before
}

/// Marks the row's result opened now, when its verb is `result`; a settled
/// pull request it showed is then seen. Returns whether it was marked.
pub(crate) fn open_result(
    verbs: &mut BTreeMap<String, VerbRecord>,
    pane_id: &str,
    now_unix_ms: u64,
) -> bool {
    match verbs.get_mut(pane_id) {
        Some(record) if record.verb == RequestVerb::Result => {
            record.result_opened_unix_ms = Some(now_unix_ms);
            true
        }
        _ => false,
    }
}

fn sender(request: &Request, delegated: bool, parent: Option<&String>) -> RequestSender {
    let parents_first = request.first && delegated;
    match &request.requester {
        Requester::Operator => RequestSender::Operator,
        Requester::Unobserved if parents_first => {
            parent.map_or(RequestSender::Agent, |name| named(name))
        }
        Requester::Unobserved => RequestSender::Operator,
        Requester::Named(name) => named(name),
        Requester::Agent if parents_first => {
            parent.map_or(RequestSender::Agent, |name| named(name))
        }
        Requester::Agent => RequestSender::Agent,
    }
}

/// The longest sender name a row shows; hcoord names are short handles.
const MAX_SENDER_CHARS: usize = 64;
/// The longest agent-written session title a row carries.
const MAX_TITLE_CHARS: usize = 200;
/// What the row writes for the operator and for an unnamed agent, so no
/// sender can pass as either (an hcoord name is whatever its sender chose).
const RESERVED_SENDERS: [&str; 3] = ["나", "에이전트", "operator"];

fn named(name: &str) -> RequestSender {
    match shown_name(name, MAX_SENDER_CHARS) {
        Some(name)
            if !RESERVED_SENDERS
                .iter()
                .any(|reserved| name.eq_ignore_ascii_case(reserved)) =>
        {
            RequestSender::Named(name)
        }
        _ => RequestSender::Agent,
    }
}

/// A name another program wrote, as one line the row can show: control and
/// bidirectional formatting characters dropped, at most `max` characters,
/// `None` when nothing is left.
fn shown_name(text: &str, max: usize) -> Option<String> {
    let bidi = |c: char| matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    let kept: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|&c| !bidi(c))
        .collect();
    let kept: String = kept
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect();
    (!kept.is_empty()).then_some(kept)
}

struct Linked<'a> {
    pull_request: &'a PullRequestSnapshot,
    on_branch: bool,
    /// When the session's tool printed it, for a pull request it made.
    sighted_at: Option<u64>,
    duty: bool,
}

fn linked_pull_requests<'a>(
    row: &SidebarAgentSnapshot,
    place: Option<RowPlace<'_>>,
    github: &'a GithubSnapshot,
) -> Vec<Linked<'a>> {
    let Some(place) = place else {
        return Vec::new();
    };
    let mut linked: Vec<Linked<'a>> = Vec::new();
    if let (Some(branch), Some(project)) = (place.branch, github.project(place.root_path)) {
        linked.extend(
            project
                .pull_requests
                .iter()
                .filter(|pull_request| pull_request.head_branch == branch)
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
fn assign_duty(rows: &mut [Vec<Linked<'_>>]) {
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

fn shown_pull_requests(
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

fn verb_of(
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

#[cfg(test)]
mod tests;
