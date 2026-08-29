use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::model::{PaneLayoutDirection, SidebarAgentSnapshot};

#[derive(Debug, Deserialize)]
pub struct SessionSnapshotPayload {
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub layouts: Vec<SessionLayoutPayload>,
    pub agents: Vec<SessionAgentPayload>,
}

#[derive(Debug, Deserialize)]
pub struct SessionLayoutPayload {
    pub workspace_id: String,
    pub tab_id: String,
    pub zoomed: bool,
    pub area: SessionLayoutRect,
    pub focused_pane_id: String,
    pub panes: Vec<SessionLayoutPanePayload>,
    pub splits: Vec<SessionLayoutSplitPayload>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct SessionLayoutRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionLayoutPanePayload {
    pub pane_id: String,
    pub rect: SessionLayoutRect,
}

#[derive(Debug, Deserialize)]
pub struct SessionLayoutSplitPayload {
    pub direction: PaneLayoutDirection,
    pub ratio: f32,
    pub rect: SessionLayoutRect,
}

#[derive(Debug, Deserialize)]
pub struct SessionAgentPayload {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub workspace_label: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub tokens: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentState {
    Question,
    Approval,
    Error,
    Working,
    UnseenCompletion,
    Idle,
    Unknown,
}

impl AgentState {
    fn name(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Approval => "approval",
            Self::Error => "error",
            Self::Working => "working",
            Self::UnseenCompletion => "unseen_completion",
            Self::Idle => "idle",
            Self::Unknown => "unknown",
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Self::Question => "?",
            Self::Approval => "!",
            Self::Error => "×",
            Self::Working | Self::UnseenCompletion => "●",
            Self::Idle => "○",
            Self::Unknown => "~",
        }
    }
}

pub fn project_agents(
    payload: SessionSnapshotPayload,
) -> Result<Vec<SidebarAgentSnapshot>, String> {
    let mut projected = payload
        .agents
        .into_iter()
        .enumerate()
        .map(|(source_index, agent)| project_agent(agent, source_index))
        .collect::<Result<Vec<_>, _>>()?;

    projected.sort_by(|left, right| {
        left.agent
            .sort_rank
            .cmp(&right.agent.sort_rank)
            .then_with(|| right.agent.activity.cmp(&left.agent.activity))
            .then_with(|| left.source_index.cmp(&right.source_index))
    });
    Ok(projected.into_iter().map(|item| item.agent).collect())
}

struct RankedAgent {
    source_index: usize,
    agent: SidebarAgentSnapshot,
}

fn project_agent(agent: SessionAgentPayload, source_index: usize) -> Result<RankedAgent, String> {
    let pane_id = non_empty(agent.pane_id.as_deref().or(agent.id.as_deref()))
        .map(str::to_owned)
        .ok_or_else(|| "session agent is missing a pane id".to_owned())?;
    let sort_rank = token_string(&agent.tokens, "sort_rank")
        .filter(|value| value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| format!("agent {pane_id} has an invalid sort_rank token"))?;
    let activity = token_string(&agent.tokens, "activity")
        .filter(|value| value.len() == 13 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| format!("agent {pane_id} has an invalid activity token"))?;
    let state = authoritative_state(&agent);
    let workspace_label = non_empty(agent.workspace_label.as_deref())
        .or_else(|| {
            agent
                .cwd
                .as_deref()
                .and_then(|cwd| cwd.rsplit('/').find(|segment| !segment.trim().is_empty()))
        })
        .unwrap_or("workspace")
        .to_owned();
    let summary = token_string(&agent.tokens, "summary")
        .map(collapse_whitespace)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(30).collect())
        .unwrap_or_else(|| "Check agent-context-labels settings".to_owned());
    let elapsed = token_string(&agent.tokens, "elapsed")
        .filter(|value| valid_elapsed(value))
        .unwrap_or_else(|| "0s".to_owned());

    Ok(RankedAgent {
        source_index,
        agent: SidebarAgentSnapshot {
            id: agent.id.unwrap_or_else(|| pane_id.clone()),
            pane_id,
            workspace_label,
            agent_kind: non_empty(agent.agent.as_deref())
                .unwrap_or("unknown")
                .to_owned(),
            state: state.name().to_owned(),
            symbol: state.symbol().to_owned(),
            summary,
            elapsed,
            sort_rank,
            activity,
        },
    })
}

fn authoritative_state(agent: &SessionAgentPayload) -> AgentState {
    let tokens = &agent.tokens;
    if unseen_token(tokens, "status_error") {
        AgentState::Error
    } else if unseen_token(tokens, "status_question") {
        AgentState::Question
    } else if unseen_token(tokens, "status_approval") {
        AgentState::Approval
    } else if present_token(tokens, "status_done_new") {
        AgentState::UnseenCompletion
    } else if present_token(tokens, "status_working")
        || agent.agent_status.as_deref() == Some("working")
    {
        AgentState::Working
    } else if present_token(tokens, "status_idle")
        || matches!(agent.agent_status.as_deref(), Some("idle" | "done"))
    {
        AgentState::Idle
    } else {
        AgentState::Unknown
    }
}

fn unseen_token(tokens: &BTreeMap<String, Value>, name: &str) -> bool {
    present_token(tokens, &format!("{name}_new"))
        || tokens.get(name).and_then(Value::as_bool) == Some(true)
}

fn present_token(tokens: &BTreeMap<String, Value>, name: &str) -> bool {
    tokens
        .get(name)
        .is_some_and(|value| value.as_bool() != Some(false) && !value.is_null())
}

fn token_string(tokens: &BTreeMap<String, Value>, name: &str) -> Option<String> {
    tokens
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn collapse_whitespace(value: String) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn valid_elapsed(value: &str) -> bool {
    let Some((digits, suffix)) = value.split_at_checked(value.len().saturating_sub(1)) else {
        return false;
    };
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && matches!(suffix, "s" | "m" | "h" | "d")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn payload(agents: Value) -> SessionSnapshotPayload {
        serde_json::from_value(json!({"agents": agents})).expect("valid fixture")
    }

    #[test]
    fn seven_authoritative_states_keep_fixed_symbols_and_seen_tokens_stay_idle() {
        let states = [
            (json!({"status_question_new": "?"}), "question", "?"),
            (json!({"status_approval_new": "!"}), "approval", "!"),
            (json!({"status_error_new": "×"}), "error", "×"),
            (json!({"status_working": "●"}), "working", "●"),
            (json!({"status_done_new": "●"}), "unseen_completion", "●"),
            (json!({"status_idle": "○"}), "idle", "○"),
            (json!({"status_unknown": "~"}), "unknown", "~"),
        ];
        let agents = states
            .iter()
            .enumerate()
            .map(|(index, (tokens, _, _))| {
                let mut tokens = tokens.as_object().expect("token object").clone();
                tokens.insert("sort_rank".to_owned(), json!(format!("0{index}")));
                tokens.insert("activity".to_owned(), json!(format!("{:013}", index)));
                json!({
                    "pane_id": format!("pane-{index}"),
                    "workspace_label": "Fixture",
                    "agent": "codex",
                    "agent_status": "unknown",
                    "tokens": tokens
                })
            })
            .collect::<Vec<_>>();
        let projected = project_agents(payload(json!(agents))).expect("project states");
        for (agent, (_, state, symbol)) in projected.iter().zip(states) {
            assert_eq!(agent.state, state);
            assert_eq!(agent.symbol, symbol);
        }

        let seen = project_agents(payload(json!([{
            "pane_id": "seen",
            "workspace_label": "Fixture",
            "agent": "codex",
            "agent_status": "idle",
            "tokens": {
                "status_question": "?",
                "sort_rank": "10",
                "activity": "0000000000001"
            }
        }])))
        .expect("project seen state");
        assert_eq!(seen[0].state, "idle");
    }

    #[test]
    fn sort_rank_is_primary_and_activity_descending_breaks_ties() {
        let projected = project_agents(payload(json!([
            {"pane_id":"older","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000001"}},
            {"pane_id":"later-rank","tokens":{"status_working":"●","sort_rank":"04","activity":"9999999999999"}},
            {"pane_id":"newer","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000002"}},
            {"pane_id":"first-rank","tokens":{"status_error_new":"×","sort_rank":"00","activity":"0000000000000"}}
        ]))).expect("project sorted agents");
        assert_eq!(
            projected
                .iter()
                .map(|agent| agent.pane_id.as_str())
                .collect::<Vec<_>>(),
            ["first-rank", "later-rank", "newer", "older"]
        );
    }

    #[test]
    fn summary_is_compact_and_missing_summary_has_an_actionable_label() {
        let projected = project_agents(payload(json!([
            {"pane_id":"long","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000001","summary":"  one   two three four five six seven eight nine ten  ","elapsed":"4m"}},
            {"pane_id":"missing","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000000"}}
        ]))).expect("project summaries");
        assert!(projected[0].summary.chars().count() <= 30);
        assert_eq!(projected[0].elapsed, "4m");
        assert_eq!(projected[1].summary, "Check agent-context-labels settings");
        assert_eq!(projected[1].elapsed, "0s");
    }
}
