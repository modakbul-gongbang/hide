//! The one move from store schema 1 to 2 (D-39): notices leave the person's
//! list for the activity log, unrelated findings become follow-up
//! candidates, an open card confirmation is confirmed, and every recovery
//! is turned on. It works on the stored JSON, since the kinds it removes no
//! longer load, and never runs backwards.

use serde_json::{Value, json};

use crate::judgment::cut;
use crate::model::{RecoveryAction, UnixMs};

/// Who a migrated answer names as its relay.
pub const MIGRATION: &str = "migration";

/// A Task record as schema 2 stores it, and the command to-dos its open
/// proposals hand to its Factory.
pub fn task(mut task: Value, now: UnixMs) -> (Value, Vec<Proposal>) {
    let mut activity: Vec<Value> = take_array(&mut task, "activity");
    let mut proposals = Vec::new();
    let mut acknowledged: Vec<String> = Vec::new();
    let mut questions = Vec::new();
    for mut question in take_array(&mut task, "questions") {
        let kind = question["kind"]["kind"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let open = question["answer"].is_null();
        let text = question["text"].as_str().unwrap_or_default().to_owned();
        let at = question["asked_at"].as_u64().unwrap_or(now);
        match kind.as_str() {
            "notice" => {
                activity.push(json!({"at": at, "kind": "note", "text": text}));
                if let Some(answer) = question["answer"]["text"].as_str() {
                    acknowledged.push(format!("{} -> {answer}", cut(&text, 200)));
                }
            }
            "proposal" => {
                activity.push(json!({"at": at, "kind": "note", "text": text}));
                let command = question["kind"]["command"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                // A recovery action runs by itself now; only a command a
                // person runs stays a person's.
                let action = RecoveryAction::ALL
                    .into_iter()
                    .any(|action| action.as_str() == command);
                if open && !action && !command.trim().is_empty() {
                    proposals.push(Proposal {
                        command,
                        impact: question["kind"]["impact"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        cause: text,
                        at,
                    });
                }
            }
            _ => {
                if let Some(fields) = question.as_object_mut() {
                    fields.remove("notice");
                    fields.remove("refers_to");
                }
                if open && kind == "confirm_card" {
                    question["answer"] = json!({
                        "text": "confirm",
                        "chose": "confirm",
                        "relayed_by": MIGRATION,
                        "at": now,
                    });
                }
                questions.push(question);
            }
        }
    }
    task["questions"] = Value::Array(questions);
    let mut report = None;
    let mut decisions = Vec::new();
    for decision in take_array(&mut task, "decisions") {
        let text = decision["text"].as_str().unwrap_or_default();
        let by = decision["by"].as_str().unwrap_or_default();
        let at = decision["at"].as_u64().unwrap_or(now);
        // A notice's acknowledgement is not a decision (D-35).
        if acknowledged.iter().any(|ack| ack == text) {
            continue;
        }
        // A done summary is the worker's report, not a decision (D-36).
        if let Some(result) = text.strip_prefix("done: ")
            && by.starts_with("worker:")
        {
            let entry = json!({"result": result});
            activity.push(json!({"at": at, "kind": "report", "report": entry}));
            report = Some(entry);
            continue;
        }
        decisions.push(decision);
    }
    task["decisions"] = Value::Array(decisions);
    if let Some(report) = report {
        task["report"] = report;
    }
    for discovery in task["discoveries"].as_array_mut().into_iter().flatten() {
        if discovery["class"] == "unrelated" && discovery["follow_up"].is_null() {
            let at = discovery["at"].as_u64().unwrap_or(now);
            discovery["follow_up"] = json!({"state": "open", "at": at});
        }
    }
    activity.sort_by_key(|entry| entry["at"].as_u64().unwrap_or(0));
    let over = activity
        .len()
        .saturating_sub(crate::model::TASK_ACTIVITY_LIMIT);
    activity.drain(..over);
    task["activity"] = Value::Array(activity);
    (task, proposals)
}

/// A command an open proposal asked a person to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub command: String,
    pub impact: String,
    pub cause: String,
    pub at: UnixMs,
}

/// A Factory record as schema 2 stores it: every recovery on (D-30), and
/// the commands its Tasks' open proposals named as its to-dos.
pub fn factory(mut factory: Value, proposals: &[Proposal]) -> Value {
    factory["config"]["recovery"] = Value::Array(
        RecoveryAction::ALL
            .iter()
            .map(|action| Value::from(action.as_str()))
            .collect(),
    );
    let mut next = factory["next_command"].as_u64().unwrap_or(0);
    let mut commands = take_array(&mut factory, "commands");
    for proposal in proposals {
        next += 1;
        commands.push(json!({
            "id": format!("C{next}"),
            "command": proposal.command,
            "impact": proposal.impact,
            "cause": proposal.cause,
            "at": proposal.at,
        }));
    }
    factory["commands"] = Value::Array(commands);
    factory["next_command"] = Value::from(next);
    factory
}

fn take_array(value: &mut Value, key: &str) -> Vec<Value> {
    match value.as_object_mut().and_then(|fields| fields.remove(key)) {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}
