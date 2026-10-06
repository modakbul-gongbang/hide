//! The Factory's judgments (D-13): one tool-less call each, whose input the
//! code bundles and whose answer can only pass, add questions and flags, or
//! choose from a closed list. The provider work belongs to hide-ai; the
//! feature ids, prompts, schemas and parsing belong here.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::{Card, RecoveryAction, SplitPiece};

pub const INTAKE_REVIEW: &str = "factory_intake_review";
pub const DRIFT: &str = "factory_drift";
pub const WATCH: &str = "factory_watch";
pub const ENV_DIAGNOSIS: &str = "factory_env_diagnosis";
pub const CHECK: &str = "factory_check";

pub const SCHEMA_VERSION: &str = "factory.v1";

/// Size caps of a bundled input; a larger part is cut with a marker.
pub const INPUT_LIMIT: usize = 48 * 1024;
pub const ATTACHMENT_LIMIT: usize = 24 * 1024;
pub const DIFF_LIMIT: usize = 24 * 1024;
/// Pending judgments per Factory (D-44).
pub const QUEUE_LIMIT: usize = 16;

/// Order the queue serves: intake review first (D-44).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Intake = 0,
    Factory = 1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "feature", rename_all = "snake_case")]
pub enum JudgmentInput {
    IntakeReview {
        card: Card,
        attachment: Option<String>,
        other_tasks: Vec<OtherTask>,
        repo_files: Vec<String>,
        guide: Option<String>,
    },
    Drift {
        card: Card,
        diff: String,
        decisions: Vec<String>,
    },
    Watch {
        board: Value,
    },
    EnvDiagnosis {
        facts: Value,
        actions: Vec<RecoveryAction>,
    },
    Check {
        instruction: String,
        card: Card,
        diff: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtherTask {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judgment {
    /// Stable per subject and input; resubmitting the same is a duplicate.
    pub id: String,
    pub factory: String,
    pub task: Option<String>,
    pub priority: Priority,
    pub input: JudgmentInput,
}

impl Judgment {
    pub fn feature_id(&self) -> &'static str {
        match self.input {
            JudgmentInput::IntakeReview { .. } => INTAKE_REVIEW,
            JudgmentInput::Drift { .. } => DRIFT,
            JudgmentInput::Watch { .. } => WATCH,
            JudgmentInput::EnvDiagnosis { .. } => ENV_DIAGNOSIS,
            JudgmentInput::Check { .. } => CHECK,
        }
    }

    pub fn system(&self) -> &'static str {
        match self.input {
            JudgmentInput::IntakeReview { .. } => INTAKE_SYSTEM,
            JudgmentInput::Drift { .. } => DRIFT_SYSTEM,
            JudgmentInput::Watch { .. } => WATCH_SYSTEM,
            JudgmentInput::EnvDiagnosis { .. } => ENV_SYSTEM,
            JudgmentInput::Check { .. } => CHECK_SYSTEM,
        }
    }

    pub fn schema(&self) -> Value {
        match self.input {
            JudgmentInput::IntakeReview { .. } => intake_schema(),
            JudgmentInput::Drift { .. } | JudgmentInput::Check { .. } => finding_schema(),
            JudgmentInput::Watch { .. } => watch_schema(),
            JudgmentInput::EnvDiagnosis { .. } => env_schema(),
        }
    }

    /// The bundled input as the provider reads it, within [`INPUT_LIMIT`].
    pub fn render_input(&self) -> String {
        let value = match &self.input {
            JudgmentInput::IntakeReview {
                card,
                attachment,
                other_tasks,
                repo_files,
                guide,
            } => json!({
                "card": card,
                "attachment": attachment.as_deref().map(|text| cut(text, ATTACHMENT_LIMIT)),
                "other_tasks": other_tasks,
                "repo_files": repo_files.iter().take(400).collect::<Vec<_>>(),
                "guide": guide.as_deref().map(|text| cut(text, 8 * 1024)),
            }),
            JudgmentInput::Drift {
                card,
                diff,
                decisions,
            } => json!({"card": card, "diff": cut(diff, DIFF_LIMIT), "decisions": decisions}),
            JudgmentInput::Watch { board } => json!({"board": board}),
            JudgmentInput::EnvDiagnosis { facts, actions } => json!({
                "facts": facts,
                "actions": actions.iter().map(|action| action.as_str()).collect::<Vec<_>>(),
            }),
            JudgmentInput::Check {
                instruction,
                card,
                diff,
            } => json!({
                "instruction": instruction,
                "card": card,
                "diff": diff.as_deref().map(|text| cut(text, DIFF_LIMIT)),
            }),
        };
        cut(&value.to_string(), INPUT_LIMIT)
    }
}

/// Cuts at a UTF-8 boundary and marks the cut.
pub fn cut(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[cut {} bytes]", &text[..end], text.len() - end)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgmentAnswer {
    pub id: String,
    pub factory: String,
    pub task: Option<String>,
    pub outcome: JudgmentOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JudgmentOutcome {
    Answered {
        value: Value,
    },
    /// No provider, a provider failure, or a full queue (B19, B68): the
    /// judgment is never skipped; the engine escalates.
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedQuestion {
    pub text: String,
    pub suggestion: String,
    #[serde(default)]
    pub default_action: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntakeVerdict {
    pub questions: Vec<ProposedQuestion>,
    pub dependencies: Vec<String>,
    pub split: Vec<SplitPiece>,
    pub flags: Vec<String>,
}

impl IntakeVerdict {
    pub fn result(&self) -> crate::model::ReviewResult {
        if !self.split.is_empty() {
            crate::model::ReviewResult::Split
        } else if !self.questions.is_empty() {
            crate::model::ReviewResult::NeedsAnswers
        } else {
            crate::model::ReviewResult::Ready
        }
    }
}

pub fn parse_intake(value: &Value) -> Result<IntakeVerdict, String> {
    let questions = parse_questions(&value["questions"])?;
    let dependencies = string_list(&value["dependencies"])?;
    let flags = string_list(&value["flags"])?;
    let mut split = Vec::new();
    for piece in value["split"].as_array().into_iter().flatten() {
        let title = text_field(piece, "title")?;
        let goal = text_field(piece, "goal")?;
        let criteria = string_list(&piece["criteria"])?;
        if criteria.is_empty() {
            return Err("split_piece_without_criteria".into());
        }
        let after = piece["after"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|n| n as usize)
                    .collect()
            })
            .unwrap_or_default();
        split.push(SplitPiece {
            title,
            goal,
            criteria,
            after,
        });
    }
    if split.len() == 1 {
        return Err("split_needs_two_pieces".into());
    }
    Ok(IntakeVerdict {
        questions,
        dependencies,
        split,
        flags,
    })
}

/// Drift and user checks: pass, or add questions and flags (D-44).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub pass: bool,
    pub questions: Vec<ProposedQuestion>,
    pub flags: Vec<String>,
}

pub fn parse_finding(value: &Value) -> Result<Finding, String> {
    let pass = value["pass"].as_bool().ok_or("pass_missing")?;
    let mut questions = parse_questions(&value["questions"])?;
    for question in &mut questions {
        if question.default_action.is_none() {
            // A check may only make work slower: every question it adds
            // carries a default so the worker keeps going (D-44).
            question.default_action = Some(question.suggestion.clone());
        }
    }
    let flags = string_list(&value["flags"])?;
    Ok(Finding {
        pass: pass && questions.is_empty() && flags.is_empty(),
        questions,
        flags,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub text: String,
    /// A warning without a proposed action goes to the log only (B69).
    pub action: Option<String>,
    pub task: Option<String>,
}

pub fn parse_watch(value: &Value) -> Result<Vec<Warning>, String> {
    let mut warnings = Vec::new();
    for item in value["warnings"].as_array().into_iter().flatten() {
        warnings.push(Warning {
            text: text_field(item, "text")?,
            action: item["action"]
                .as_str()
                .map(str::trim)
                .filter(|action| !action.is_empty())
                .map(str::to_owned),
            task: item["task"].as_str().map(str::to_owned),
        });
    }
    Ok(warnings)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnosis {
    pub cause: String,
    /// Only an action from the closed list (D-54); anything else is a
    /// proposal for a person.
    pub action: Option<RecoveryAction>,
    pub proposal: Option<(String, String)>,
}

pub fn parse_env(value: &Value) -> Result<Diagnosis, String> {
    let cause = text_field(value, "cause")?;
    let action = match value["action"].as_str() {
        None | Some("") | Some("none") => None,
        Some(name) => Some(
            RecoveryAction::ALL
                .into_iter()
                .find(|action| action.as_str() == name)
                .ok_or("action_not_in_list")?,
        ),
    };
    let proposal = match (value["command"].as_str(), value["impact"].as_str()) {
        (Some(command), Some(impact)) if !command.trim().is_empty() => {
            Some((command.to_owned(), impact.to_owned()))
        }
        _ => None,
    };
    Ok(Diagnosis {
        cause,
        action,
        proposal,
    })
}

fn parse_questions(value: &Value) -> Result<Vec<ProposedQuestion>, String> {
    let mut questions = Vec::new();
    for item in value.as_array().into_iter().flatten() {
        let text = text_field(item, "text")?;
        let suggestion = text_field(item, "suggestion")?;
        let default_action = item["default_action"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        questions.push(ProposedQuestion {
            text,
            suggestion,
            default_action,
        });
    }
    Ok(questions)
}

fn text_field(value: &Value, key: &str) -> Result<String, String> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("{key}_missing"))
}

fn string_list(value: &Value) -> Result<Vec<String>, String> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => Ok(items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .collect()),
        _ => Err("list_expected".into()),
    }
}

const QUESTION_ITEM: &str = r#"{"type":"object","additionalProperties":false,"required":["text","suggestion","default_action"],"properties":{"text":{"type":"string"},"suggestion":{"type":"string"},"default_action":{"type":"string"}}}"#;

fn question_item() -> Value {
    serde_json::from_str(QUESTION_ITEM).unwrap_or(Value::Null)
}

fn intake_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["questions", "dependencies", "split", "flags"],
        "properties": {
            "questions": {"type": "array", "items": question_item()},
            "dependencies": {"type": "array", "items": {"type": "string"}},
            "split": {"type": "array", "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["title", "goal", "criteria", "after"],
                "properties": {
                    "title": {"type": "string"},
                    "goal": {"type": "string"},
                    "criteria": {"type": "array", "items": {"type": "string"}},
                    "after": {"type": "array", "items": {"type": "integer"}}
                }
            }},
            "flags": {"type": "array", "items": {"type": "string"}}
        }
    })
}

fn finding_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["pass", "questions", "flags"],
        "properties": {
            "pass": {"type": "boolean"},
            "questions": {"type": "array", "items": question_item()},
            "flags": {"type": "array", "items": {"type": "string"}}
        }
    })
}

fn watch_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["warnings"],
        "properties": {"warnings": {"type": "array", "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["text", "action", "task"],
            "properties": {
                "text": {"type": "string"},
                "action": {"type": "string"},
                "task": {"type": "string"}
            }
        }}}
    })
}

fn env_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["cause", "action", "command", "impact"],
        "properties": {
            "cause": {"type": "string"},
            "action": {"type": "string"},
            "command": {"type": "string"},
            "impact": {"type": "string"}
        }
    })
}

const INTAKE_SYSTEM: &str = "You review a software Task card before it runs, with no knowledge of the conversation that wrote it. You see only the card, its attached PRD, the repository's file list and guide, and the other Tasks of the same Factory. Return JSON only. You may only add: questions a person must answer before the Task can run safely (each with a concrete suggestion and a default action), dependencies on other listed Tasks by id when this Task cannot start until that one is merged, a split when the Task is clearly too large for one pull request (two or more pieces, each with a goal and checkable criteria, `after` naming earlier pieces), and short flags. Ask about untestable completion criteria, hidden decisions, and mismatch between the card and the PRD. Never rewrite the card. Do not add a dependency only because two Tasks touch the same file. Return empty arrays when the card is ready.";

const DRIFT_SYSTEM: &str = "You compare a finished change with the Task card it claims to complete: its goal, completion criteria and out-of-scope list, and the decisions the worker recorded. Return JSON only. pass is true when the diff does what the card asks and nothing it rules out. When it drifts, add questions for a person, each with a suggestion and a default action that keeps the change as narrow as the card; add flags for a reported breaking change or a public contract change. You cannot send work back and you cannot approve anything wider than the card.";

const WATCH_SYSTEM: &str = "You read a Factory board summary: recent events, Task states, waits and decision records. Return JSON only: warnings a person should act on, each naming the Task id it is about when there is one and an action the person can take. A warning with no action is still allowed but goes to a log. Never propose merging, removing dependencies or widening scope on your own.";

const ENV_SYSTEM: &str = "You diagnose an environment problem from facts the code collected (disk usage by owner, failure signals, which stage failed, how many Tasks). Return JSON only: a one-line cause, and either one action from the given list (or \"none\"), or an exact shell command with its impact for a person to approve. Never choose an action that is not in the list; login, deletion outside the Factory and installing tools are always a proposal.";

const CHECK_SYSTEM: &str = "You run one natural-language check a person configured on a Factory Task. Return JSON only. pass is true when the instruction is satisfied by the card and change you see. Otherwise add questions (with a suggestion and a default action) or flags. You cannot change the card or the change.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_question_without_a_default_takes_its_suggestion() {
        let finding = parse_finding(&json!({
            "pass": true,
            "questions": [{"text": "Docs updated?", "suggestion": "Leave docs", "default_action": ""}],
            "flags": []
        }))
        .unwrap();
        assert!(!finding.pass, "a question turns a pass into a finding");
        assert_eq!(
            finding.questions[0].default_action.as_deref(),
            Some("Leave docs")
        );
    }

    #[test]
    fn a_diagnosis_cannot_pick_an_action_outside_the_closed_list() {
        assert!(
            parse_env(&json!({"cause":"disk","action":"rm_rf_home","command":"","impact":""}))
                .is_err()
        );
        let diagnosis = parse_env(
            &json!({"cause":"disk","action":"remove_finished_worktrees","command":"","impact":""}),
        )
        .unwrap();
        assert_eq!(
            diagnosis.action,
            Some(RecoveryAction::RemoveFinishedWorktrees)
        );
    }

    #[test]
    fn a_split_needs_at_least_two_pieces_with_criteria() {
        let one = json!({"questions":[],"dependencies":[],"flags":[],"split":[{"title":"a","goal":"g","criteria":["c"],"after":[]}]});
        assert!(parse_intake(&one).is_err());
        let two = json!({"questions":[],"dependencies":[],"flags":[],"split":[
            {"title":"a","goal":"g","criteria":["c"],"after":[]},
            {"title":"b","goal":"g","criteria":["c"],"after":[0]}]});
        let verdict = parse_intake(&two).unwrap();
        assert_eq!(verdict.result(), crate::model::ReviewResult::Split);
        assert_eq!(verdict.split[1].after, vec![0]);
    }

    #[test]
    fn inputs_are_cut_at_a_character_boundary() {
        let text = "가".repeat(10);
        let cut_text = cut(&text, 7);
        assert!(cut_text.starts_with("가가"));
        assert!(cut_text.contains("[cut"));
    }
}
