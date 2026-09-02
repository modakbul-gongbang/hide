use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::model::{AmbientSignal, PaneLayoutDirection, SidebarAgentSnapshot};

#[derive(Clone, Debug, Deserialize)]
pub struct SessionSnapshotPayload {
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub tabs: Vec<SessionTabPayload>,
    #[serde(default)]
    pub layouts: Vec<SessionLayoutPayload>,
    pub agents: Vec<SessionAgentPayload>,
    #[serde(default)]
    pub panes: Vec<SessionPanePayload>,
    #[serde(default)]
    pub workspaces: Vec<SessionWorkspacePayload>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionWorkspacePayload {
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionTabPayload {
    pub tab_id: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
pub struct SessionLayoutSplitPayload {
    pub direction: PaneLayoutDirection,
    pub ratio: f32,
    pub rect: SessionLayoutRect,
}

#[derive(Clone, Debug, Deserialize)]
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
    /// Herdr's record of the conversation this agent is running, when it has
    /// one. `kind` says whether `value` is an id or a path; only an id can be
    /// handed to an agent's own fork command.
    #[serde(default)]
    pub agent_session: Option<SessionAgentSessionPayload>,
    /// The pane this agent was spawned from, as Herdr's own lineage records it.
    /// Present only on an agent started through `agent.new` with a source pane.
    #[serde(default)]
    pub spawned_from_pane_id: Option<String>,
    #[serde(default)]
    pub state_change_seq: Option<u64>,
    #[serde(default)]
    pub tokens: BTreeMap<String, Value>,
    /// Passed through verbatim; the strict shape check lives in
    /// [`parse_ambient`] so a broken record can never partially survive.
    #[serde(default)]
    pub ambient: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SessionAgentSessionPayload {
    pub kind: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionPanePayload {
    pub pane_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
    /// The name the user gave this pane in Herdr, when they gave it one.
    #[serde(default)]
    pub label: Option<String>,
    /// What the program running in the pane set the terminal title to.
    #[serde(default)]
    pub terminal_title: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentState {
    Question,
    Approval,
    Blocked,
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
            Self::Blocked => "blocked",
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
            Self::Blocked => "●",
            Self::Error => "×",
            Self::Working | Self::UnseenCompletion => "●",
            Self::Idle => "○",
            Self::Unknown => "~",
        }
    }
}

/// One agent that could not be read out of an otherwise valid snapshot.
///
/// A single broken record excludes only itself; the surrounding agents are
/// still projected, and the exclusion is reported rather than swallowed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentExclusion {
    pub source_index: usize,
    pub pane_id: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentProjection {
    pub agents: Vec<SidebarAgentSnapshot>,
    pub excluded: Vec<AgentExclusion>,
}

pub fn project_agents(payload: SessionSnapshotPayload) -> AgentProjection {
    let mut projected = Vec::with_capacity(payload.agents.len());
    let mut excluded = Vec::new();
    for (source_index, agent) in payload.agents.into_iter().enumerate() {
        let pane_id =
            non_empty(agent.pane_id.as_deref().or(agent.id.as_deref())).map(str::to_owned);
        match project_agent(agent, source_index) {
            Ok(ranked) => projected.push(ranked),
            Err(reason) => excluded.push(AgentExclusion {
                source_index,
                pane_id,
                reason,
            }),
        }
    }

    projected.sort_by(|left, right| {
        left.agent
            .sort_rank
            .cmp(&right.agent.sort_rank)
            .then_with(|| right.agent.activity.cmp(&left.agent.activity))
            .then_with(|| left.source_index.cmp(&right.source_index))
    });
    AgentProjection {
        agents: projected.into_iter().map(|item| item.agent).collect(),
        excluded,
    }
}

struct RankedAgent {
    source_index: usize,
    agent: SidebarAgentSnapshot,
}

fn project_agent(agent: SessionAgentPayload, source_index: usize) -> Result<RankedAgent, String> {
    let pane_id = non_empty(agent.pane_id.as_deref().or(agent.id.as_deref()))
        .map(str::to_owned)
        .ok_or_else(|| "session agent is missing a pane id".to_owned())?;
    let sort_rank = projected_sort_rank(&agent.tokens, &pane_id)?;
    let activity = projected_activity(&agent, &pane_id)?;
    let state = authoritative_state(&agent);
    let ambient = match agent.ambient.as_ref() {
        Some(raw) => parse_ambient(raw)?,
        None => None,
    };
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
            ambient,
            session_id: agent
                .agent_session
                .as_ref()
                .filter(|session| session.kind == "id")
                .map(|session| session.value.clone())
                .filter(|value| !value.trim().is_empty()),
            spawned_from_pane_id: non_empty(agent.spawned_from_pane_id.as_deref())
                .map(str::to_owned),
        },
    })
}

/// Reads a pane's optional `ambient` object.
///
/// `null` means the server sent nothing for this pane. Any other unreadable
/// shape excludes the whole record rather than partially extracting it, and
/// unknown keys are dropped so nothing but the three counts can ever reach
/// app state (see docs/ambient-signals.md).
fn parse_ambient(raw: &Value) -> Result<Option<AmbientSignal>, String> {
    if raw.is_null() {
        return Ok(None);
    }
    let object = raw
        .as_object()
        .ok_or_else(|| "ambient signal is not an object".to_owned())?;
    let count = |key: &str| -> Result<u32, String> {
        match object.get(key) {
            None => Ok(0),
            Some(value) => value
                .as_u64()
                .and_then(|number| u32::try_from(number).ok())
                .ok_or_else(|| format!("ambient signal {key} is not a count")),
        }
    };
    Ok(Some(AmbientSignal {
        subagents_active: count("subagents_active")?,
        background_running: count("background_running")?,
        background_failed: count("background_failed")?,
    }))
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
    } else if agent.agent_status.as_deref() == Some("blocked") {
        AgentState::Blocked
    } else if present_token(tokens, "status_working")
        || agent.agent_status.as_deref() == Some("working")
    {
        AgentState::Working
    } else if agent.agent_status.as_deref() == Some("done") {
        AgentState::UnseenCompletion
    } else if present_token(tokens, "status_idle") || agent.agent_status.as_deref() == Some("idle")
    {
        AgentState::Idle
    } else {
        AgentState::Unknown
    }
}

/// Optional presentation tokens come from plugins, not from the Socket API
/// contract. An agent without one remains visible after explicitly ranked
/// agents, while a present malformed token is still rejected and reported.
fn projected_sort_rank(tokens: &BTreeMap<String, Value>, pane_id: &str) -> Result<String, String> {
    match tokens.get("sort_rank") {
        None => Ok("99".to_owned()),
        Some(Value::String(value)) => {
            let value = value.trim();
            if value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()) {
                Ok(value.to_owned())
            } else {
                Err(format!("agent {pane_id} has an invalid sort_rank token"))
            }
        }
        Some(_) => Err(format!("agent {pane_id} has an invalid sort_rank token")),
    }
}

fn projected_activity(agent: &SessionAgentPayload, pane_id: &str) -> Result<String, String> {
    match agent.tokens.get("activity") {
        None => agent
            .state_change_seq
            .map(|sequence| format!("{sequence:020}"))
            .ok_or_else(|| {
                format!("agent {pane_id} has neither an activity token nor state_change_seq")
            }),
        Some(Value::String(value)) => {
            let value = value.trim();
            if value.len() == 13 && value.bytes().all(|byte| byte.is_ascii_digit()) {
                Ok(value.to_owned())
            } else {
                Err(format!("agent {pane_id} has an invalid activity token"))
            }
        }
        Some(_) => Err(format!("agent {pane_id} has an invalid activity token")),
    }
}

pub(crate) fn requires_close_confirmation(state: &str) -> bool {
    matches!(
        state,
        "working" | "blocked" | "question" | "approval" | "error" | "unseen_completion"
    )
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
        let projected = project_agents(payload(json!(agents))).agents;
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
        .agents;
        assert_eq!(seen[0].state, "idle");
    }

    #[test]
    fn sort_rank_is_primary_and_activity_descending_breaks_ties() {
        let projected = project_agents(payload(json!([
            {"pane_id":"older","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000001"}},
            {"pane_id":"later-rank","tokens":{"status_working":"●","sort_rank":"04","activity":"9999999999999"}},
            {"pane_id":"newer","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000002"}},
            {"pane_id":"first-rank","tokens":{"status_error_new":"×","sort_rank":"00","activity":"0000000000000"}}
        ]))).agents;
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
        ]))).agents;
        assert!(projected[0].summary.chars().count() <= 30);
        assert_eq!(projected[0].elapsed, "4m");
        assert_eq!(projected[1].summary, "Check agent-context-labels settings");
        assert_eq!(projected[1].elapsed, "0s");
    }

    #[test]
    fn one_broken_agent_excludes_only_itself_and_names_why() {
        let projection = project_agents(payload(json!([
            {"pane_id":"good","tokens":{"status_working":"●","sort_rank":"05","activity":"0000000000002"}},
            {"pane_id":"bad-rank","tokens":{"status_idle":"○","sort_rank":"oops","activity":"0000000000001"}},
            {"tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000000"}},
            {"pane_id":"also-good","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000003"}}
        ])));

        assert_eq!(
            projection
                .agents
                .iter()
                .map(|agent| agent.pane_id.as_str())
                .collect::<Vec<_>>(),
            ["good", "also-good"]
        );
        assert_eq!(projection.excluded.len(), 2);
        assert_eq!(projection.excluded[0].pane_id.as_deref(), Some("bad-rank"));
        assert!(projection.excluded[0].reason.contains("sort_rank"));
        assert_eq!(projection.excluded[1].pane_id, None);
        assert!(projection.excluded[1].reason.contains("pane id"));
    }

    #[test]
    fn official_state_change_sequence_replaces_only_a_missing_activity_token() {
        let projection = project_agents(payload(json!([
            {
                "pane_id":"remote",
                "state_change_seq":218,
                "agent_status":"done",
                "tokens":{"status_done":"●","sort_rank":"05","summary":"finished"}
            },
            {
                "pane_id":"malformed",
                "state_change_seq":219,
                "tokens":{"status_idle":"○","sort_rank":"10","activity":"not-a-time"}
            }
        ])));

        assert_eq!(projection.agents.len(), 1);
        assert_eq!(projection.agents[0].pane_id, "remote");
        assert_eq!(projection.agents[0].activity, "00000000000000000218");
        assert_eq!(projection.excluded.len(), 1);
        assert!(projection.excluded[0].reason.contains("invalid activity"));
    }

    #[test]
    fn official_statuses_project_without_optional_plugin_tokens() {
        let projection = project_agents(payload(json!([
            {"pane_id":"working","agent_status":"working","state_change_seq":1},
            {"pane_id":"blocked","agent_status":"blocked","state_change_seq":2},
            {"pane_id":"done","agent_status":"done","state_change_seq":3},
            {"pane_id":"idle","agent_status":"idle","state_change_seq":4},
            {"pane_id":"unknown","agent_status":"unknown","state_change_seq":5}
        ])));

        assert!(projection.excluded.is_empty());
        let states = projection
            .agents
            .iter()
            .map(|agent| {
                (
                    agent.pane_id.as_str(),
                    (agent.state.as_str(), agent.symbol.as_str()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(states["working"], ("working", "●"));
        assert_eq!(states["blocked"], ("blocked", "●"));
        assert_eq!(states["done"], ("unseen_completion", "●"));
        assert_eq!(states["idle"], ("idle", "○"));
        assert_eq!(states["unknown"], ("unknown", "~"));
        assert!(
            projection
                .agents
                .iter()
                .all(|agent| agent.sort_rank == "99")
        );
        assert_eq!(
            projection
                .agents
                .iter()
                .find(|agent| agent.pane_id == "working")
                .expect("working agent")
                .activity,
            "00000000000000000001"
        );
    }

    #[test]
    fn ambient_counts_parse_and_unknown_keys_never_survive() {
        let projection = project_agents(payload(json!([
            {"pane_id":"legacy","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000001"}},
            {"pane_id":"counted","tokens":{"status_working":"●","sort_rank":"05","activity":"0000000000002"},
             "ambient":{"subagents_active":2,"background_running":1,"background_failed":0,
                        "task_name":"SENTINEL-do-not-leak","command":"SENTINEL-rm -rf /"}}
        ])));

        assert_eq!(projection.excluded, []);
        let counted = projection
            .agents
            .iter()
            .find(|agent| agent.pane_id == "counted")
            .expect("counted agent");
        let ambient = counted.ambient.expect("ambient present");
        assert_eq!(ambient.subagents_active, 2);
        assert_eq!(ambient.background_running, 1);
        assert_eq!(
            projection
                .agents
                .iter()
                .find(|agent| agent.pane_id == "legacy")
                .and_then(|agent| agent.ambient),
            None,
            "a snapshot without the key stays on the legacy path"
        );
        let serialized = serde_json::to_string(&projection.agents).expect("serialize agents");
        assert!(
            !serialized.contains("SENTINEL"),
            "unknown ambient keys must never reach the projected agent: {serialized}"
        );
    }

    #[test]
    fn a_malformed_ambient_record_excludes_only_that_agent() {
        let projection = project_agents(payload(json!([
            {"pane_id":"broken","tokens":{"status_working":"●","sort_rank":"05","activity":"0000000000002"},
             "ambient":{"subagents_active":"not-a-number"}},
            {"pane_id":"intact","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000001"}}
        ])));

        assert_eq!(
            projection
                .agents
                .iter()
                .map(|agent| agent.pane_id.as_str())
                .collect::<Vec<_>>(),
            ["intact"]
        );
        assert_eq!(projection.excluded.len(), 1);
        assert_eq!(projection.excluded[0].pane_id.as_deref(), Some("broken"));
        assert!(projection.excluded[0].reason.contains("subagents_active"));
    }
}
