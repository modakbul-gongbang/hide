//! The Factory's judgments (D-13): one tool-less call each, whose input the
//! code bundles and whose answer can only pass, add questions and flags, or
//! choose from a closed list. The provider work belongs to hide-ai; the
//! feature ids, prompts, schemas and parsing belong here.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::{
    Card, DecisionKind, FactoryAi, ObserverProposal, RecoveryAction, SplitPiece, WorkerPick,
};

pub const INTAKE_REVIEW: &str = "factory_intake_review";
pub const DRIFT: &str = "factory_drift";
pub const WATCH: &str = "factory_watch";
pub const ENV_DIAGNOSIS: &str = "factory_env_diagnosis";
pub const CHECK: &str = "factory_check";
/// The Observer: one call per decision request, quiet worker or risk-path
/// merge (D-15).
pub const OBSERVER: &str = "factory_observer";

pub const SCHEMA_VERSION: &str = "factory.v1";

/// Size caps of a bundled input; a larger part is cut with a marker.
pub const INPUT_LIMIT: usize = 48 * 1024;
pub const ATTACHMENT_LIMIT: usize = 24 * 1024;
pub const DIFF_LIMIT: usize = 24 * 1024;
/// The one worker text a diagnosis carries (D-37).
pub const WORKER_TEXT_LIMIT: usize = 4 * 1024;
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
        /// The autonomy scope a worker's proposal claims, by its description:
        /// the review decides whether the card fits it (B30).
        #[serde(default)]
        autonomy_scope: Option<String>,
        /// The Factory's worker candidates the review picks from (D-41);
        /// empty when there is only one.
        #[serde(default)]
        workers: Vec<CandidateNote>,
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
    /// Sort one decision request into A-E (D-14, D-16).
    ObserverClassify {
        request: DecisionRequest,
        card: Card,
        decisions: Vec<String>,
        attachment: Option<String>,
    },
    /// Read a worker resting without a report (D-23, D-37).
    ObserverDiagnose {
        card: Card,
        decisions: Vec<String>,
        attachment: Option<String>,
        worker_text: Option<WorkerText>,
    },
    /// Approve a verified merge whose only gate is a risk path (D-21).
    ObserverMerge {
        card: Card,
        decisions: Vec<String>,
        attachment: Option<String>,
        risk_paths: Vec<String>,
    },
}

/// What the Observer sees of a decision request (D-16).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub question: String,
    pub choices: Vec<String>,
    pub suggestion: String,
    /// The action the worker takes meanwhile; `None` for a block, which
    /// cannot proceed without the answer.
    pub default_action: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateNote {
    pub index: usize,
    pub agent: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub description: String,
}

/// Which of a worker's texts a diagnosis read (D-37).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerTextSource {
    UserTurn,
    LastAnswer,
    Screen,
}

impl WorkerTextSource {
    pub const ALL: [Self; 3] = [Self::UserTurn, Self::LastAnswer, Self::Screen];
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerText {
    pub source: WorkerTextSource,
    pub text: String,
}

impl WorkerText {
    /// The text within [`WORKER_TEXT_LIMIT`]: a user turn keeps its start,
    /// an answer or a screen its end, cut at a character boundary (D-37).
    pub fn bounded(source: WorkerTextSource, text: &str) -> Self {
        let text = if text.len() <= WORKER_TEXT_LIMIT {
            text.to_owned()
        } else if source == WorkerTextSource::UserTurn {
            let mut end = WORKER_TEXT_LIMIT;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text[..end].to_owned()
        } else {
            let mut start = text.len() - WORKER_TEXT_LIMIT;
            while !text.is_char_boundary(start) {
                start += 1;
            }
            text[start..].to_owned()
        };
        Self { source, text }
    }
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
    /// The Factory AI it runs on; `None` is the app's Hide AI (D-40).
    #[serde(default)]
    pub ai: Option<FactoryAi>,
}

impl Judgment {
    pub fn feature_id(&self) -> &'static str {
        match self.input {
            JudgmentInput::IntakeReview { .. } => INTAKE_REVIEW,
            JudgmentInput::Drift { .. } => DRIFT,
            JudgmentInput::Watch { .. } => WATCH,
            JudgmentInput::EnvDiagnosis { .. } => ENV_DIAGNOSIS,
            JudgmentInput::Check { .. } => CHECK,
            JudgmentInput::ObserverClassify { .. }
            | JudgmentInput::ObserverDiagnose { .. }
            | JudgmentInput::ObserverMerge { .. } => OBSERVER,
        }
    }

    pub fn system(&self) -> &'static str {
        match self.input {
            JudgmentInput::IntakeReview { .. } => INTAKE_SYSTEM,
            JudgmentInput::Drift { .. } => DRIFT_SYSTEM,
            JudgmentInput::Watch { .. } => WATCH_SYSTEM,
            JudgmentInput::EnvDiagnosis { .. } => ENV_SYSTEM,
            JudgmentInput::Check { .. } => CHECK_SYSTEM,
            JudgmentInput::ObserverClassify { .. } => CLASSIFY_SYSTEM,
            JudgmentInput::ObserverDiagnose { .. } => DIAGNOSE_SYSTEM,
            JudgmentInput::ObserverMerge { .. } => MERGE_SYSTEM,
        }
    }

    pub fn schema(&self) -> Value {
        match self.input {
            JudgmentInput::IntakeReview { .. } => intake_schema(),
            JudgmentInput::Drift { .. } | JudgmentInput::Check { .. } => finding_schema(),
            JudgmentInput::Watch { .. } => watch_schema(),
            JudgmentInput::EnvDiagnosis { .. } => env_schema(),
            JudgmentInput::ObserverClassify { .. } => classify_schema(),
            JudgmentInput::ObserverDiagnose { .. } => diagnose_schema(),
            JudgmentInput::ObserverMerge { .. } => merge_schema(),
        }
    }

    /// Whether this is an Observer call, which the daily cap counts (D-34).
    pub fn observer(&self) -> bool {
        self.feature_id() == OBSERVER
    }

    /// The bundled input as the provider reads it, within [`INPUT_LIMIT`].
    pub fn render_input(&self) -> String {
        let mut value = match &self.input {
            JudgmentInput::IntakeReview {
                card,
                attachment,
                other_tasks,
                repo_files,
                guide,
                autonomy_scope,
                workers,
            } => json!({
                "card": card,
                "attachment": attachment.as_deref().map(|text| cut(text, ATTACHMENT_LIMIT)),
                "other_tasks": other_tasks,
                "repo_files": repo_files.iter().take(400).collect::<Vec<_>>(),
                "guide": guide.as_deref().map(|text| cut(text, 8 * 1024)),
                "autonomy_scope": autonomy_scope,
                "workers": workers,
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
            JudgmentInput::ObserverClassify {
                request,
                card,
                decisions,
                attachment,
            } => json!({
                "request": request,
                "card": card,
                "decisions": decisions,
                "attachment": attachment.as_deref().map(|text| cut(text, ATTACHMENT_LIMIT)),
            }),
            JudgmentInput::ObserverDiagnose {
                card,
                decisions,
                attachment,
                worker_text,
            } => json!({
                "card": card,
                "decisions": decisions,
                "attachment": attachment.as_deref().map(|text| cut(text, ATTACHMENT_LIMIT)),
                "worker_text": worker_text,
            }),
            JudgmentInput::ObserverMerge {
                card,
                decisions,
                attachment,
                risk_paths,
            } => json!({
                "card": card,
                "decisions": decisions,
                "attachment": attachment.as_deref().map(|text| cut(text, ATTACHMENT_LIMIT)),
                "risk_paths": risk_paths,
            }),
        };
        // Display metadata adds no input or provider work.
        if let Some(card) = value.get_mut("card").and_then(Value::as_object_mut) {
            card.remove("summary");
        }
        if let Some(factories) = value
            .pointer_mut("/board/factories")
            .and_then(Value::as_array_mut)
        {
            for factory in factories {
                if let Some(columns) = factory.get_mut("columns").and_then(Value::as_array_mut) {
                    for column in columns {
                        strip_display_cards(&mut column["cards"]);
                    }
                }
                strip_display_cards(&mut factory["cancelled"]);
            }
        }
        cut(&value.to_string(), INPUT_LIMIT)
    }
}

fn strip_display_cards(cards: &mut Value) {
    for card in cards
        .as_array_mut()
        .into_iter()
        .flatten()
        .filter_map(Value::as_object_mut)
    {
        for field in [
            "summary",
            "issue",
            "issue_url",
            "pr",
            "worker_runtime",
            "resume_at",
            "waiting_group",
            "stage",
        ] {
            card.remove(field);
        }
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
    pub summary: Option<String>,
    pub questions: Vec<ProposedQuestion>,
    pub dependencies: Vec<String>,
    pub split: Vec<SplitPiece>,
    pub flags: Vec<String>,
    /// Whether the card fits the claimed autonomy scope; `None` when no
    /// scope was claimed or the review did not say.
    pub fits_scope: Option<bool>,
    /// The worker candidate it picked, when it was offered a choice.
    pub worker: Option<WorkerPick>,
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
        summary: value["summary"].as_str().map(crate::model::short_summary),
        questions,
        dependencies,
        split,
        flags,
        fits_scope: value["fits_scope"].as_bool(),
        worker: value["worker"].as_u64().map(|index| WorkerPick {
            index: index as usize,
            reason: value["worker_reason"]
                .as_str()
                .map(|reason| cut(reason.trim(), 300))
                .unwrap_or_default(),
        }),
    })
}

/// The Observer's reading of a decision request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classification {
    pub kind: DecisionKind,
    /// Unsure of the kind, or a permission signal anywhere: a person's.
    pub ambiguous: bool,
    pub permission_signal: bool,
    pub answer: String,
    pub proposal: Option<ObserverProposal>,
    pub reason: String,
}

pub fn parse_classification(value: &Value, card: &Card) -> Result<Classification, String> {
    let kind = value["kind"]
        .as_str()
        .and_then(DecisionKind::parse)
        .ok_or("kind_missing")?;
    let proposal = match value["proposal"]["type"].as_str() {
        None | Some("none") => None,
        Some(kind @ ("card_fix" | "new_task")) => {
            let proposal = &value["proposal"];
            let title = text_field(proposal, "title")?;
            let goal = text_field(proposal, "goal")?;
            let criteria = string_list(&proposal["criteria"])?;
            if criteria.is_empty() {
                return Err("proposal_without_criteria".into());
            }
            Some(if kind == "card_fix" {
                ObserverProposal::CardFix {
                    card: Box::new(Card {
                        title,
                        summary: None,
                        goal,
                        criteria,
                        ..card.clone()
                    }),
                }
            } else {
                ObserverProposal::NewTask {
                    card: Box::new(Card {
                        title,
                        goal,
                        criteria,
                        ..Card::default()
                    }),
                    prerequisite: proposal["prerequisite"].as_bool().unwrap_or(false),
                }
            })
        }
        Some(_) => return Err("proposal_type_unknown".into()),
    };
    Ok(Classification {
        kind,
        ambiguous: value["ambiguous"].as_bool().ok_or("ambiguous_missing")?,
        permission_signal: value["permission_signal"]
            .as_bool()
            .ok_or("permission_signal_missing")?,
        answer: value["answer"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned(),
        proposal,
        reason: cut(value["reason"].as_str().unwrap_or_default().trim(), 300),
    })
}

/// What a diagnosis found (D-23).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The worker was asking: the engine raises the request for it.
    Question {
        text: String,
        suggestion: String,
        choices: Vec<String>,
        classification: Classification,
    },
    ForgotDone,
    /// Stuck, or nothing to tell: a person's.
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerDiagnosis {
    pub verdict: Verdict,
    pub reason: String,
}

pub fn parse_diagnosis(value: &Value, card: &Card) -> Result<WorkerDiagnosis, String> {
    let reason = cut(value["reason"].as_str().unwrap_or_default().trim(), 300);
    let verdict = match value["verdict"].as_str() {
        Some("question") => {
            let question = &value["question"];
            let choices = string_list(&question["choices"])?;
            Verdict::Question {
                text: text_field(question, "text")?,
                suggestion: text_field(question, "suggestion")?,
                choices: valid_choices(choices).map_err(|(reason, _)| reason)?,
                classification: parse_classification(question, card)?,
            }
        }
        Some("forgot_done") => Verdict::ForgotDone,
        Some("stuck" | "unknown") => Verdict::Stopped,
        _ => return Err("verdict_missing".into()),
    };
    Ok(WorkerDiagnosis { verdict, reason })
}

pub fn parse_merge(value: &Value) -> Result<(bool, String), String> {
    let approve = value["approve"].as_bool().ok_or("approve_missing")?;
    Ok((
        approve,
        cut(value["reason"].as_str().unwrap_or_default().trim(), 300),
    ))
}

/// The most choices a decision request carries, and the longest one (D-13).
pub const CHOICE_LIMIT: usize = 5;
pub const CHOICE_CHARS: usize = 120;

/// Choices trimmed, empty ones dropped; refused past the caps with the
/// reason code and its limit.
pub fn valid_choices(choices: Vec<String>) -> Result<Vec<String>, (String, usize)> {
    let choices: Vec<String> = choices
        .into_iter()
        .map(|choice| choice.trim().to_owned())
        .filter(|choice| !choice.is_empty())
        .collect();
    if choices.len() > CHOICE_LIMIT {
        return Err(("too_many_choices".into(), CHOICE_LIMIT));
    }
    if choices
        .iter()
        .any(|choice| choice.chars().count() > CHOICE_CHARS)
    {
        return Err(("choice_too_long".into(), CHOICE_CHARS));
    }
    Ok(choices)
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
        "required": ["summary", "questions", "dependencies", "split", "flags", "fits_scope", "worker", "worker_reason"],
        "properties": {
            "summary": {"type": "string", "maxLength": 60},
            "worker": {"type": "integer", "minimum": 0},
            "worker_reason": {"type": "string", "maxLength": 200},
            "questions": {"type": "array", "items": question_item()},
            "dependencies": {"type": "array", "items": {"type": "string"}},
            "fits_scope": {"type": ["boolean", "null"]},
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

const CLASSIFY_ITEM: &str = r#"{"kind":{"type":"string","enum":["A","B","C","D","E"]},"ambiguous":{"type":"boolean"},"permission_signal":{"type":"boolean"},"answer":{"type":"string"},"reason":{"type":"string","maxLength":200},"proposal":{"type":"object","additionalProperties":false,"required":["type","title","goal","criteria","prerequisite"],"properties":{"type":{"type":"string","enum":["none","card_fix","new_task"]},"title":{"type":"string"},"goal":{"type":"string"},"criteria":{"type":"array","items":{"type":"string"}},"prerequisite":{"type":"boolean"}}}}"#;

fn classify_properties() -> Value {
    serde_json::from_str(CLASSIFY_ITEM).unwrap_or(Value::Null)
}

fn classify_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "ambiguous", "permission_signal", "answer", "proposal", "reason"],
        "properties": classify_properties(),
    })
}

fn diagnose_schema() -> Value {
    let mut question = classify_properties();
    question["text"] = json!({"type": "string"});
    question["suggestion"] = json!({"type": "string"});
    question["choices"] = json!({"type": "array", "items": {"type": "string", "maxLength": CHOICE_CHARS}, "maxItems": CHOICE_LIMIT});
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["verdict", "question", "reason"],
        "properties": {
            "verdict": {"type": "string", "enum": ["question", "forgot_done", "stuck", "unknown"]},
            "reason": {"type": "string", "maxLength": 200},
            "question": {
                "type": "object",
                "additionalProperties": false,
                "required": ["text", "suggestion", "choices", "kind", "ambiguous", "permission_signal", "answer", "proposal", "reason"],
                "properties": question,
            }
        }
    })
}

fn merge_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["approve", "reason"],
        "properties": {
            "approve": {"type": "boolean"},
            "reason": {"type": "string", "maxLength": 200}
        }
    })
}

const CLASSIFY_SYSTEM: &str = concat!(
    "You sort one decision request a Factory worker raised about its Task. You see the request (question, choices, suggestion, and the default action or none for a blocking request), the Task card, its recorded decisions and an excerpt of its PRD. Return JSON only. You never decide who answers; you only classify and suggest. ",
    "Kinds: A the answer is already in the card, the PRD excerpt or the recorded decisions; B a technical choice inside the card's scope; C a product or taste choice a person owns; D a permission: cost, sign-in or credentials, deletion, security, an effect outside the repository, anything outside the card's scope, or anything irreversible; E the card itself is wrong or incomplete. Set ambiguous true when you are not sure of the kind, and permission_signal true when any part of the request touches a D topic, whatever kind you chose. answer is the answer you would give (one of the choices when they fit), empty for D. For E, propose either card_fix (the corrected title, goal and criteria of this card) or new_task (a separate Task, prerequisite true when this Task cannot finish without it); otherwise proposal type none with empty fields. reason is one line in Korean."
);

const DIAGNOSE_SYSTEM: &str = concat!(
    "A Factory worker stopped working without reporting through hide factory done, ask or block, and did not report after being reminded. You see its Task card, recorded decisions, an excerpt of its PRD and at most one of its texts: its pending user-turn question, its last answer, or the end of its screen. Return JSON only. verdict is question when the worker was asking a person something (fill question with the request it was asking: text, suggestion, up to five short choices, and its classification), forgot_done when the work looks finished and only the report is missing, stuck when it is blocked on something it cannot resolve, unknown when the text does not say. Fill question with empty strings, an empty list, kind A, false flags and proposal type none unless verdict is question. reason is one line in Korean a person reads under the stop. ",
    "Kinds: A the answer is already in the card, the PRD excerpt or the recorded decisions; B a technical choice inside the card's scope; C a product or taste choice a person owns; D a permission: cost, sign-in or credentials, deletion, security, an effect outside the repository, anything outside the card's scope, or anything irreversible; E the card itself is wrong or incomplete. Set ambiguous true when you are not sure of the kind, and permission_signal true when any part of the request touches a D topic, whatever kind you chose. answer is the answer you would give (one of the choices when they fit), empty for D. For E, propose either card_fix (the corrected title, goal and criteria of this card) or new_task (a separate Task, prerequisite true when this Task cannot finish without it); otherwise proposal type none with empty fields."
);

const MERGE_SYSTEM: &str = "A verified Factory Task changed files under paths its operator marked risky, and that is the only reason it waits for a merge. You see its card, recorded decisions, an excerpt of its PRD and the risk path patterns. Return JSON only: approve true only when the card plainly asks for a change in those paths and the decisions show nothing outside its scope; otherwise false. reason is one line in Korean.";

const INTAKE_SYSTEM: &str = "summary에는 Task의 목표를 60자 이내 한 줄로 요약하세요. 제목을 반복하지 마세요. You review a software Task card before it runs, with no knowledge of the conversation that wrote it. You see only the card, its attached PRD, the repository's file list and guide, and the other Tasks of the same Factory. Return JSON only. You may only add: questions a person must answer before the Task can run safely (each with a concrete suggestion and a default action), dependencies on other listed Tasks by id when this Task cannot start until that one is merged, a split when the Task is clearly too large for one pull request (two or more pieces, each with a goal and checkable criteria, `after` naming earlier pieces), and short flags. Ask about untestable completion criteria, hidden decisions, and mismatch between the card and the PRD. Never rewrite the card. Do not add a dependency only because two Tasks touch the same file. Return empty arrays when the card is ready. When autonomy_scope is given, set fits_scope to true only when the card plainly falls within that description and false otherwise; when it is null, set fits_scope to null. When workers lists candidates, set worker to the index of the one whose description fits this card best and worker_reason to one line in Korean saying why; otherwise set worker to 0 and worker_reason to an empty string.";

const DRIFT_SYSTEM: &str = "You compare a finished change with the Task card it claims to complete: its goal, completion criteria and out-of-scope list, and the decisions the worker recorded. Return JSON only. pass is true when the diff does what the card asks and nothing it rules out. When it drifts, add questions for a person, each with a suggestion and a default action that keeps the change as narrow as the card; add flags for a reported breaking change or a public contract change. You cannot send work back and you cannot approve anything wider than the card.";

const WATCH_SYSTEM: &str = "You read a Factory board summary: recent events, Task states, waits and decision records. Return JSON only: warnings a person should act on, each naming the Task id it is about when there is one and an action the person can take. A warning with no action is still allowed but goes to a log. Never propose merging, removing dependencies or widening scope on your own. Do not restate anything a person already sees as a question, a merge wait or a stop in the inbox, and do not report finished Tasks or ordinary progress: those are not a person's turn. Warn only about what a person cannot see there, such as work that stopped moving, a chain blocked by one wait, or a pattern of failures.";

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
