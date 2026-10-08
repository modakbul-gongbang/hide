//! Native Claude/Codex question and plan records. Text is never inferred
//! from a label, title, or an ordinary assistant sentence.

use serde_json::Value;

use super::{NATIVE_ID_LIMIT_BYTES, ToolTurnMark, TurnMark, TurnMode, UserTurnContent};
use crate::SkipReason;

/// A single native record is already bounded by the session line limit.
/// Bound the retained collection as well, including unrelated tool results.
const TOOL_MARK_LIMIT: usize = 64;

type Mark = Result<Option<TurnMark>, SkipReason>;
pub(crate) type Parser = fn(&Value) -> Mark;

fn id(value: Option<&Value>) -> Result<Option<String>, SkipReason> {
    let Some(value) = value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if value.len() > NATIVE_ID_LIMIT_BYTES {
        return Err(SkipReason::UserTurnCapacity);
    }
    Ok(Some(value.to_owned()))
}

fn question_content(input: &Value) -> Option<UserTurnContent> {
    let questions = input.get("questions")?.as_array()?;
    if questions.is_empty()
        || questions
            .iter()
            .any(|question| question.get("question").and_then(Value::as_str).is_none())
    {
        return None;
    }
    if questions.iter().any(|question| {
        question.get("options").is_some_and(|options| {
            options.as_array().is_none_or(|options| {
                options
                    .iter()
                    .any(|choice| choice.get("label").and_then(Value::as_str).is_none())
            })
        })
    }) {
        return None;
    }
    Some(UserTurnContent::questions(questions.iter().map(
        |question| {
            let choices = question
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|choice| choice.get("label").and_then(Value::as_str));
            (
                question["question"]
                    .as_str()
                    .expect("question texts checked"),
                choices,
            )
        },
    )))
}

pub(crate) fn claude(item: &Value) -> Mark {
    let role = item.get("type").and_then(Value::as_str);
    let Some(blocks) = item.pointer("/message/content").and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut marks = Vec::new();
    for block in blocks {
        let kind = block.get("type").and_then(Value::as_str);
        let mark = match (role, kind) {
            (Some("assistant"), Some("tool_use"))
                if block.get("name").and_then(Value::as_str) == Some("AskUserQuestion") =>
            {
                let call = id(block.get("id"))?.ok_or(SkipReason::UserTurnInvalid)?;
                Some(ToolTurnMark::Asked {
                    call,
                    content: block.get("input").and_then(question_content),
                })
            }
            (Some("user"), Some("tool_result")) => {
                id(block.get("tool_use_id"))?.map(|call| ToolTurnMark::Answered { call })
            }
            _ => None,
        };
        if let Some(mark) = mark {
            if marks.len() == TOOL_MARK_LIMIT {
                return Err(SkipReason::UserTurnCapacity);
            }
            marks.push(mark);
        }
    }
    Ok((!marks.is_empty()).then_some(TurnMark::Tools(marks)))
}

/// Codex 0.160.1 emits task lifecycle records and native function-call
/// records. A completed plan-mode turn holds approval; an unanswered native
/// request_user_input holds a question while the turn is still running.
pub(crate) fn codex(item: &Value) -> Mark {
    let Some(payload) = item.get("payload") else {
        return Ok(None);
    };
    let kind = payload.get("type").and_then(Value::as_str);
    let turn = || id(payload.get("turn_id"));
    let mark = match item.get("type").and_then(Value::as_str) {
        Some("event_msg") => match kind {
            Some("task_started") => Some(TurnMark::Started {
                turn: turn()?,
                mode: match payload
                    .get("collaboration_mode_kind")
                    .and_then(Value::as_str)
                {
                    Some("plan") => TurnMode::Plan,
                    Some("default") => TurnMode::Other,
                    _ => TurnMode::Unknown,
                },
            }),
            Some("item_completed")
                if payload.pointer("/item/type").and_then(Value::as_str) == Some("Plan") =>
            {
                Some(
                    match payload.pointer("/item/text").and_then(Value::as_str) {
                        Some(text) => TurnMark::PlanContent {
                            turn: turn()?,
                            content: UserTurnContent::new(text, []),
                        },
                        None => TurnMark::Plan { turn: turn()? },
                    },
                )
            }
            Some("task_complete") => Some(TurnMark::Completed { turn: turn()? }),
            Some("turn_aborted") => Some(TurnMark::Aborted { turn: turn()? }),
            _ => None,
        },
        Some("response_item") => match kind {
            Some("function_call")
                if payload.get("name").and_then(Value::as_str) == Some("request_user_input") =>
            {
                let call = id(payload.get("call_id"))?.ok_or(SkipReason::UserTurnInvalid)?;
                let arguments = payload
                    .get("arguments")
                    .and_then(Value::as_str)
                    .and_then(|arguments| serde_json::from_str::<Value>(arguments).ok());
                Some(TurnMark::Tools(vec![ToolTurnMark::Asked {
                    call,
                    content: arguments.as_ref().and_then(question_content),
                }]))
            }
            Some("function_call_output") => id(payload.get("call_id"))?
                .map(|call| TurnMark::Tools(vec![ToolTurnMark::Answered { call }])),
            Some("message") if payload.get("role").and_then(Value::as_str) == Some("assistant") => {
                // This explicit native wrapper is also written when the
                // structured Plan item is absent from the read's prefix.
                payload
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|block| block.get("text").and_then(Value::as_str))
                    .find_map(|text| {
                        let body = text
                            .trim()
                            .strip_prefix("<proposed_plan>")?
                            .strip_suffix("</proposed_plan>")?
                            .trim();
                        Some(TurnMark::PlanContent {
                            turn: None,
                            content: UserTurnContent::new(body, []),
                        })
                    })
            }
            _ => None,
        },
        _ => None,
    };
    Ok(mark)
}
