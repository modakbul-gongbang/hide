use crate::model::{AgentSnapshot, AgentStatus, AmbientSignal, QuestionOption, QuestionPayload};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HerdrError {
    #[error("socket connection failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("herdr protocol error: {0}")]
    Protocol(String),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct HerdrClient {
    socket_path: String,
}

impl HerdrClient {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value, HerdrError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
        let request = json!({ "id": "herdr-pet", "method": method, "params": params });
        writeln!(stream, "{}", request)?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        let value: Value = serde_json::from_str(&line)?;
        if value.get("error").is_some() {
            return Err(HerdrError::Protocol(value.to_string()));
        }
        Ok(value.get("result").cloned().unwrap_or(value))
    }

    pub fn snapshot(&self) -> Result<Value, HerdrError> {
        self.request("session.snapshot", json!({}))
    }
    pub fn subscribe(&self) -> Result<Value, HerdrError> {
        self.request("events.subscribe", json!({ "subscriptions": ["pane.*"] }))
    }

    pub fn subscribe_stream(&self) -> Result<BufReader<UnixStream>, HerdrError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let request = json!({
            "id": "herdr-pet-events",
            "method": "events.subscribe",
            "params": { "subscriptions": ["pane.*"] }
        });
        writeln!(stream, "{request}")?;
        let mut reader = BufReader::new(stream);
        let mut acknowledgement = String::new();
        reader.read_line(&mut acknowledgement)?;
        let value: Value = serde_json::from_str(&acknowledgement)?;
        if value.get("error").is_some() {
            return Err(HerdrError::Protocol(value.to_string()));
        }
        Ok(reader)
    }
    pub fn agent_focus(&self, id: &str) -> Result<Value, HerdrError> {
        self.request("agent.focus", json!({ "target": id }))
    }

    pub fn agent_send_keys(&self, id: &str, keys: &[&str]) -> Result<Value, HerdrError> {
        self.request("agent.send_keys", json!({ "target": id, "keys": keys }))
    }

    pub fn window_title_set(&self, title: &str) -> Result<Value, HerdrError> {
        self.request("client.window_title.set", json!({ "title": title }))
    }
}

pub fn parse_snapshot(value: &Value, target_id: &str, target_label: &str) -> Vec<AgentSnapshot> {
    let list = value
        .get("agents")
        .or_else(|| value.get("result").and_then(|v| v.get("agents")))
        .or_else(|| {
            value
                .get("result")
                .and_then(|v| v.get("snapshot"))
                .and_then(|v| v.get("agents"))
        })
        .or_else(|| value.get("snapshot").and_then(|v| v.get("agents")))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| value.as_array().cloned().unwrap_or_default());
    list.iter()
        .filter_map(|item| parse_agent(item, target_id, target_label))
        .collect()
}

fn string_field(item: &Value, key: &str) -> String {
    item.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn parse_agent(item: &Value, target_id: &str, target_label: &str) -> Option<AgentSnapshot> {
    // Herdr 0.8 session snapshots identify a pane by `pane_id`; older
    // agent-list responses used `id`. Either is a valid focus target.
    let id = item
        .get("id")
        .or_else(|| item.get("pane_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if id.is_empty() {
        return None;
    }
    let tokens = item.get("tokens").unwrap_or(&Value::Null);
    let raw_status = string_field(item, "agent_status");
    // Herdr marks a state the user has NOT looked at yet with a `_new` token
    // (`status_question_new: "?"`); the same token without `_new` means the user
    // already checked it and must not re-escalate. Legacy herdr emitted plain
    // booleans (`status_question: true`) for the unseen case, so accept those too.
    let unseen_token = |name: &str| {
        tokens
            .get(format!("{name}_new"))
            .is_some_and(|value| value.as_bool() != Some(false))
            || tokens.get(name).and_then(Value::as_bool).unwrap_or(false)
    };
    let has_token = |name: &str| {
        tokens
            .get(name)
            .is_some_and(|value| value.as_bool() != Some(false))
    };
    let status = if unseen_token("status_question")
        || unseen_token("status_approval")
        || raw_status == "blocked"
    {
        AgentStatus::Attention
    } else if unseen_token("status_error") || raw_status == "error" {
        AgentStatus::Error
    } else if has_token("status_working") || raw_status == "working" {
        AgentStatus::Working
    } else if has_token("status_done") || has_token("status_done_new") || raw_status == "done" {
        AgentStatus::Done
    } else {
        AgentStatus::Idle
    };
    let question = item
        .get("report_metadata")
        .and_then(|m| m.get("herdr_pet_question"))
        .and_then(parse_question);
    let ambient = parse_ambient(item);
    Some(AgentSnapshot {
        id,
        target_id: target_id.into(),
        target_label: target_label.into(),
        project: string_field(item, "cwd")
            .rsplit('/')
            .next()
            .unwrap_or("workspace")
            .into(),
        agent_kind: string_field(item, "agent"),
        cwd: string_field(item, "cwd"),
        summary: tokens
            .get("summary")
            .and_then(Value::as_str)
            .or_else(|| item.get("summary").and_then(Value::as_str))
            .unwrap_or("No active summary")
            .into(),
        status,
        elapsed_seconds: tokens
            .get("elapsed")
            .and_then(Value::as_u64)
            .or_else(|| item.get("elapsed_seconds").and_then(Value::as_u64))
            .unwrap_or(0),
        pane_id: item
            .get("pane_id")
            .and_then(Value::as_str)
            .map(String::from),
        tab_id: item.get("tab_id").and_then(Value::as_str).map(String::from),
        workspace_id: item
            .get("workspace_id")
            .and_then(Value::as_str)
            .map(String::from),
        focused: item
            .get("focused")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        session_id: item
            .get("agent_session")
            .and_then(|session| session.get("value"))
            .and_then(Value::as_str)
            .map(String::from),
        updated_at_ms: item
            .get("updated_at_ms")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        question,
        disconnected_reason: None,
        ambient: ambient.0,
        ambient_compat_warning: ambient.1,
    })
}

/// Parses the optional `ambient` object a Herdr server may attach to a pane.
/// Returns `(None, false)` when the key is absent entirely (legacy server or
/// authorization off), `(Some(signal), false)` when it parsed cleanly, and
/// `(None, true)` when the key is present but its shape could not be read
/// (unknown/broken record) - the caller then keeps whatever ambient value it
/// last saw for this pane instead of losing it (D-27). Never partially
/// extracts a broken object: any subcount with the wrong type invalidates
/// the whole object.
fn parse_ambient(item: &Value) -> (Option<AmbientSignal>, bool) {
    let Some(raw) = item.get("ambient") else {
        return (None, false);
    };
    if raw.is_null() {
        return (None, false);
    }
    let Some(object) = raw.as_object() else {
        return (None, true);
    };
    let count = |key: &str| -> Result<u32, ()> {
        match object.get(key) {
            None => Ok(0),
            Some(value) => value.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or(()),
        }
    };
    match (
        count("subagents_active"),
        count("background_running"),
        count("background_failed"),
    ) {
        (Ok(subagents_active), Ok(background_running), Ok(background_failed)) => (
            Some(AmbientSignal {
                subagents_active,
                background_running,
                background_failed,
            }),
            false,
        ),
        _ => (None, true),
    }
}

fn parse_question(value: &Value) -> Option<QuestionPayload> {
    Some(QuestionPayload {
        question: value
            .get("question")
            .or_else(|| value.get("prompt"))
            .and_then(Value::as_str)?
            .into(),
        options: value
            .get("options")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|option| {
                        Some(QuestionOption {
                            label: option
                                .get("label")
                                .or_else(|| option.get("title"))
                                .and_then(Value::as_str)?
                                .into(),
                            description: option
                                .get("description")
                                .and_then(Value::as_str)
                                .map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        allow_free_text: value
            .get("allow_free_text")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

/// Join hook-dropped question files (`<dir>/<session_id>.json`) onto agents that
/// are waiting for input. Files describe questions the local Claude Code hook saw,
/// so only agents whose herdr session id matches pick one up, and only while the
/// agent is actually in the attention state.
pub fn attach_session_questions(agents: &mut [AgentSnapshot], dir: &std::path::Path) {
    for agent in agents.iter_mut() {
        if agent.status != AgentStatus::Attention || agent.question.is_some() {
            continue;
        }
        let Some(session_id) = agent.session_id.as_deref() else {
            continue;
        };
        if session_id.contains(['/', '\\']) || session_id.contains("..") {
            continue;
        }
        let path = dir.join(format!("{session_id}.json"));
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(&contents) {
            agent.question = parse_question(&value);
        }
    }
}

#[derive(Default)]
pub struct EventDeduper {
    fingerprints: HashMap<String, String>,
}

pub struct EventDebouncer {
    window: Duration,
    last_emitted: HashMap<String, Instant>,
}

impl EventDebouncer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last_emitted: HashMap::new(),
        }
    }

    pub fn allow(&mut self, id: &str, now: Instant) -> bool {
        match self.last_emitted.get(id) {
            Some(previous) if now.duration_since(*previous) < self.window => false,
            _ => {
                self.last_emitted.insert(id.into(), now);
                true
            }
        }
    }
}

impl EventDeduper {
    pub fn changed(&mut self, id: &str, value: &Value) -> bool {
        let fingerprint = value.to_string();
        self.fingerprints
            .insert(id.into(), fingerprint.clone())
            .as_deref()
            != Some(&fingerprint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_question_and_token_status() {
        let value = json!({ "agents": [{ "id": "a", "agent_status": "idle", "agent": "codex", "cwd": "/tmp/modakbul", "tokens": { "status_question": true, "summary": "Choose a plan", "elapsed": 12 }, "report_metadata": { "herdr_pet_question": { "question": "Ship it?", "options": [{ "label": "Yes" }] } } }] });
        let agents = parse_snapshot(&value, "local", "Local");
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].status, AgentStatus::Attention);
        assert_eq!(agents[0].question.as_ref().unwrap().options[0].label, "Yes");
    }

    #[test]
    fn parses_recorded_agent_list_fixture() {
        let value: Value =
            serde_json::from_str(include_str!("../tests/fixtures/agent-list.json")).unwrap();
        let agents = parse_snapshot(&value, "local", "Local Mac");
        assert_eq!(agents.len(), 3);
        assert_eq!(agents[0].status, AgentStatus::Attention);
        assert_eq!(agents[1].status, AgentStatus::Working);
        assert_eq!(agents[2].status, AgentStatus::Error);
        assert_eq!(agents[0].question.as_ref().unwrap().options.len(), 2);
    }

    #[test]
    fn legacy_snapshot_without_ambient_key_parses_unaffected() {
        let value = json!({ "agents": [{ "id": "a", "agent_status": "working", "agent": "claude", "cwd": "/tmp/x" }] });
        let agents = parse_snapshot(&value, "local", "Local");
        assert_eq!(agents[0].ambient, None);
        assert!(!agents[0].ambient_compat_warning);
        assert_eq!(agents[0].status, AgentStatus::Working);
    }

    #[test]
    fn valid_ambient_object_parses_into_counts() {
        let value = json!({ "agents": [{ "id": "a", "agent_status": "working", "agent": "claude", "cwd": "/tmp/x",
            "ambient": { "subagents_active": 2, "background_running": 1, "background_failed": 0 } }] });
        let agents = parse_snapshot(&value, "local", "Local");
        let ambient = agents[0].ambient.expect("ambient present");
        assert_eq!(ambient.subagents_active, 2);
        assert_eq!(ambient.background_running, 1);
        assert_eq!(ambient.background_failed, 0);
        assert!(!agents[0].ambient_compat_warning);
    }

    #[test]
    fn malformed_ambient_object_sets_warning_without_panicking() {
        let value = json!({ "agents": [{ "id": "a", "agent_status": "working", "agent": "claude", "cwd": "/tmp/x",
            "ambient": { "subagents_active": "not-a-number" } }] });
        let agents = parse_snapshot(&value, "local", "Local");
        assert_eq!(agents[0].ambient, None);
        assert!(agents[0].ambient_compat_warning);
        assert_eq!(
            agents[0].status,
            AgentStatus::Working,
            "malformed ambient must not affect unrelated field parsing"
        );
    }

    #[test]
    fn unknown_ambient_keys_and_sentinel_content_never_survive_parsing() {
        let value = json!({ "agents": [{ "id": "a", "agent_status": "working", "agent": "claude", "cwd": "/tmp/x",
            "ambient": {
                "subagents_active": 1,
                "background_running": 0,
                "background_failed": 0,
                "task_name": "SENTINEL-do-not-leak this prompt text",
                "command": "SENTINEL-rm -rf /"
            } }] });
        let agents = parse_snapshot(&value, "local", "Local");
        let ambient = agents[0].ambient.expect("ambient present");
        assert_eq!(ambient.subagents_active, 1);
        let serialized = serde_json::to_string(&agents[0]).unwrap();
        assert!(
            !serialized.contains("SENTINEL"),
            "unknown ambient keys must never reach the serialized AgentSnapshot: {serialized}"
        );
    }

    #[test]
    fn seen_question_token_does_not_reescalate() {
        let value = json!({ "agents": [
            { "id": "seen", "agent_status": "idle", "agent": "claude", "cwd": "/tmp/seen",
              "tokens": { "status_question": "?" } },
            { "id": "unseen", "agent_status": "done", "agent": "claude", "cwd": "/tmp/unseen",
              "tokens": { "status_question_new": "?" } },
            { "id": "seen-done", "agent_status": "done", "agent": "claude", "cwd": "/tmp/done",
              "tokens": { "status_done": "●" } }
        ] });
        let agents = parse_snapshot(&value, "local", "Local");
        assert_eq!(
            agents[0].status,
            AgentStatus::Idle,
            "checked question stays quiet"
        );
        assert_eq!(
            agents[1].status,
            AgentStatus::Attention,
            "unseen question escalates"
        );
        assert_eq!(agents[2].status, AgentStatus::Done);
    }

    #[test]
    fn session_question_file_attaches_only_to_waiting_agents() {
        let value = json!({ "agents": [
            { "id": "a", "agent_status": "done", "agent": "claude", "cwd": "/tmp/one",
              "agent_session": { "value": "sess-waiting" },
              "tokens": { "status_question_new": "?", "summary": "질문 대기" } },
            { "id": "b", "agent_status": "idle", "agent": "claude", "cwd": "/tmp/two",
              "agent_session": { "value": "sess-idle" }, "tokens": { "status_idle": "○" } }
        ] });
        let mut agents = parse_snapshot(&value, "local", "Local");
        assert_eq!(agents[0].status, AgentStatus::Attention);
        assert_eq!(agents[0].session_id.as_deref(), Some("sess-waiting"));

        let dir = std::env::temp_dir().join("herdr-pet-question-test");
        std::fs::create_dir_all(&dir).unwrap();
        for session in ["sess-waiting", "sess-idle"] {
            std::fs::write(
                dir.join(format!("{session}.json")),
                r#"{"question":"진행할까요?","options":[{"label":"진행"},{"label":"중단"}]}"#,
            )
            .unwrap();
        }
        attach_session_questions(&mut agents, &dir);
        let question = agents[0]
            .question
            .as_ref()
            .expect("attention agent gets question");
        assert_eq!(question.question, "진행할까요?");
        assert_eq!(question.options.len(), 2);
        assert!(
            agents[1].question.is_none(),
            "idle agent must stay question-free"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_events_are_filtered() {
        let mut deduper = EventDeduper::default();
        let value = json!({ "agent_status": "working" });
        assert!(deduper.changed("a", &value));
        assert!(!deduper.changed("a", &value));
    }

    #[test]
    fn output_spam_is_debounced_but_later_state_can_emit() {
        let start = Instant::now();
        let mut debouncer = EventDebouncer::new(Duration::from_millis(50));
        assert!(debouncer.allow("a", start));
        assert!(!debouncer.allow("a", start + Duration::from_millis(10)));
        assert!(debouncer.allow("a", start + Duration::from_millis(60)));
    }
}
