//! Quiet pane identity and one operator-facing band, from existing core facts.
//! No clock, I/O, delivery authority or terminal geometry lives here.
use crate::model::{PaneSnapshot, SidebarAgentSnapshot, TerminalPaneSnapshot, WorkspaceSnapshot};
use serde::Serialize;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Header {
    pub band: Option<Band>,
    pub working: bool,
    pub pull: Option<Action>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Band {
    pub kind: String,
    pub tone: &'static str,
    pub reason: Option<String>,
    pub since_unix_ms: Option<u64>,
    pub action: Option<Action>,
    pub more: usize,
    pub exit_code: Option<i32>,
    pub child_tag: Option<super::sessions::Tag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facts: Option<ReasonFacts>,
}

/// Facts already read by the core. Missing commands/check names are explicit;
/// a generated progress sentence cannot stand in for either source.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReasonFacts {
    ApprovalCommandUnavailable,
    PullRequest {
        checks: crate::model::PullRequestChecks,
        review: Option<crate::model::ReviewDecision>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Pr {
        workspace_id: String,
        url: String,
        number: u32,
        checks: crate::model::PullRequestChecks,
        tone: &'static str,
    },
    Child {
        pane_id: String,
        label: String,
    },
}

pub(crate) fn of(
    pane: &PaneSnapshot,
    agent: Option<&SidebarAgentSnapshot>,
    transport: Option<&TerminalPaneSnapshot>,
    workspace: &WorkspaceSnapshot,
    offline: Option<&str>,
) -> Header {
    let tag = agent.map(|agent| super::sessions::tag(agent, agent.state.verb));
    let links = agent
        .and_then(|agent| agent.request.as_ref())
        .map(|request| &request.pull_requests);
    let live = |pull: &&crate::request_view::AgentPullRequestSnapshot| {
        pull.live && !pull.badge.is_settled()
    };
    let action = |pull: &crate::request_view::AgentPullRequestSnapshot| Action::Pr {
        workspace_id: workspace.id.clone(),
        url: pull.url.clone(),
        number: pull.number,
        checks: pull.checks,
        tone: workspace
            .pull_requests
            .iter()
            .find(|pr| pr.url == pull.url)
            .map_or("pr", pr_tone),
    };
    let pull = links.and_then(|links| links.iter().find(live)).map(action);
    let duty = links.and_then(|links| {
        links.iter().filter(live).find(|pull| {
            use super::sessions::Tag;
            use crate::model::PullRequestChecks;
            match tag {
                Some(Tag::Fix) => pull.duty && pull.checks == PullRequestChecks::Failed,
                Some(Tag::Review | Tag::Merge) => {
                    pull.duty
                        && matches!(
                            pull.checks,
                            PullRequestChecks::Passing
                                | PullRequestChecks::None
                                | PullRequestChecks::Unknown
                        )
                }
                _ => false,
            }
        })
    });
    let band = |kind: &str, tone, reason, since_unix_ms, action, more, exit_code| {
        Some(Band {
            kind: kind.to_owned(),
            tone,
            reason,
            since_unix_ms,
            action,
            more,
            exit_code,
            child_tag: None,
            facts: None,
        })
    };
    let unavailable = if let Some(name) = offline {
        band(
            "device_offline",
            "muted",
            Some(name.to_owned()),
            None,
            None,
            0,
            None,
        )
    } else if let Some(sleep) = &pane.sleep {
        band(
            &sleep.state,
            if sleep.state == "failed" {
                "error"
            } else {
                "muted"
            },
            sleep.reason.clone(),
            Some(sleep.since_unix_ms),
            None,
            0,
            None,
        )
    } else if let Some(transport) = transport {
        if transport.scroll_held_elsewhere {
            band("controlled_elsewhere", "muted", None, None, None, 0, None)
        } else if transport.closed || transport.transport_state == "ended" {
            let failed = transport.exit_code.is_some_and(|code| code != 0);
            band(
                if failed { "exit" } else { "terminated" },
                if failed { "error" } else { "muted" },
                None,
                None,
                None,
                0,
                transport.exit_code,
            )
        } else {
            match transport.transport_state.as_str() {
                "released" => band("disconnected", "muted", None, None, None, 0, None),
                "closing" => band("closing", "muted", None, None, None, 0, None),
                "unavailable" => band("unavailable", "error", None, None, None, 0, None),
                "starting" | "connecting" | "reconnecting" => {
                    band("starting", "muted", None, None, None, 0, None)
                }
                _ => None,
            }
        }
    } else {
        None
    };
    if unavailable.is_some() {
        return Header {
            band: unavailable,
            pull,
            working: false,
        };
    }
    let Some(agent) = agent else {
        return Header::default();
    };
    let demand = matches!(
        tag,
        Some(super::sessions::Tag::Approval | super::sessions::Tag::Answer)
    );
    if !demand && let Some(child) = agent.raised_children.first() {
        return Header {
            pull,
            working: false,
            band: band(
                "raised_child",
                "warning",
                Some(
                    [Some(child.title.clone()), child.reason.clone()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · "),
                ),
                child.since_unix_ms,
                Some(Action::Child {
                    pane_id: child.pane_id.clone(),
                    label: child.title.clone(),
                }),
                agent.raised_children.len().saturating_sub(1),
                None,
            )
            .map(|mut band| {
                band.child_tag = Some(child.tag);
                band
            }),
        };
    }
    use super::sessions::Tag;
    let (kind, tone, action) = match tag {
        Some(Tag::Approval) => ("approval", "warning", None),
        Some(Tag::Answer) => ("answer", "warning", None),
        Some(Tag::Fix) => ("fix", "error", duty.map(action)),
        Some(Tag::Review) => {
            let action = duty.map(action);
            ("review", pull_tone(&action), action)
        }
        Some(Tag::Merge) => {
            let action = duty.map(action);
            ("merge", pull_tone(&action), action)
        }
        Some(Tag::Stopped) => ("stopped", "warning", None),
        Some(Tag::Result) => ("result", "success", None),
        _ => {
            return Header {
                band: None,
                working: agent.state.verb == super::RequestVerb::Working,
                pull,
            };
        }
    };
    Header {
        pull,
        working: false,
        band: band(
            kind,
            tone,
            agent
                .request
                .as_ref()
                .and_then(|request| request.line.clone()),
            agent.state.request_since,
            action,
            0,
            None,
        )
        .map(|mut band| {
            band.facts = match tag {
                Some(Tag::Approval) => Some(ReasonFacts::ApprovalCommandUnavailable),
                Some(Tag::Fix | Tag::Review | Tag::Merge) => {
                    duty.map(|pull| ReasonFacts::PullRequest {
                        checks: pull.checks,
                        review: pull.review,
                    })
                }
                _ => None,
            };
            if band.facts.is_some() {
                band.reason = None;
            }
            band
        }),
    }
}

fn pull_tone(pull: &Option<Action>) -> &'static str {
    match pull {
        Some(Action::Pr { tone, .. }) => tone,
        _ => "pr",
    }
}

fn pr_tone(pr: &crate::model::PullRequestSnapshot) -> &'static str {
    use crate::model::{PullRequestBadge, ReviewDecision};
    match (pr.badge, pr.review, pr.is_draft) {
        (PullRequestBadge::Review, Some(ReviewDecision::ChangesRequested), _) => "error",
        (PullRequestBadge::Review, Some(ReviewDecision::Approved), _) => "success",
        (PullRequestBadge::Review, _, _) | (_, _, true) => "muted",
        _ => "pr",
    }
}
