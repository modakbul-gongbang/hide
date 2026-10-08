//! Quiet pane identity and one operator-facing band, from existing core facts.
//! No clock, I/O, delivery authority or terminal geometry lives here.
use crate::model::{PaneSnapshot, SidebarAgentSnapshot, TerminalPaneSnapshot};
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Pr {
        workspace_id: String,
        number: u32,
        checks: crate::model::PullRequestChecks,
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
    workspace_id: &str,
    offline: Option<&str>,
) -> Header {
    let pull = agent
        .and_then(|agent| agent.request.as_ref())
        .and_then(|request| {
            request
                .pull_requests
                .iter()
                .find(|pull| pull.live && !pull.badge.is_settled())
        })
        .map(|pull| Action::Pr {
            workspace_id: workspace_id.to_owned(),
            number: pull.number,
            checks: pull.checks,
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
    let tag = Some(super::sessions::tag(agent, agent.state.verb));
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
        Some(Tag::Fix) => ("fix", "error", pull.clone()),
        Some(Tag::Review) => ("review", "warning", pull.clone()),
        Some(Tag::Merge) => ("merge", "success", pull.clone()),
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
        ),
    }
}
