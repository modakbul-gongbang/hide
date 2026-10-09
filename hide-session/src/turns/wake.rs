//! Background work a Claude Code session started and that will wake the agent
//! again when it ends (PRD agent-blocked-state B18, D-26).
//!
//! Only a record the agent's own process wrote proves a device: the start
//! text its tool returned, the `task-notification` that ends it, a `TaskStop`
//! call, and the `SessionStart` hook record that marks a new process (a
//! device never outlives the process that started it). Anything the records
//! do not show is not a device.

use serde_json::Value;

use super::{NATIVE_ID_LIMIT_BYTES, TurnMark, WakeMark, native::TOOL_MARK_LIMIT};
use crate::SkipReason;

/// The wake marks one Claude Code record carries; empty for most records.
pub(crate) fn claude(item: &Value) -> Result<Option<TurnMark>, SkipReason> {
    let mut marks = Vec::new();
    match item.get("type").and_then(Value::as_str) {
        Some("attachment") => attachment(item, &mut marks),
        Some("queue-operation")
            if item.get("operation").and_then(Value::as_str) == Some("enqueue") =>
        {
            if let Some(text) = item.get("content").and_then(Value::as_str) {
                notifications(text, &mut marks);
            }
        }
        Some("user") => user(item, &mut marks),
        Some("assistant") => assistant(item, &mut marks),
        _ => {}
    }
    Ok((!marks.is_empty()).then_some(TurnMark::Wake(marks)))
}

/// Keeps a record's marks bounded. A record with more than the limit, or a
/// task id too long to keep, is one this reader cannot follow, so what it
/// says about devices is lost (`WakeMark::Lost`); it never fails the read the
/// turn and label depend on.
fn push(marks: &mut Vec<WakeMark>, mark: Option<WakeMark>) {
    match mark {
        Some(mark) if marks.len() < TOOL_MARK_LIMIT => marks.push(mark),
        _ if marks.last() == Some(&WakeMark::Lost) => {}
        _ => marks.push(WakeMark::Lost),
    }
}

fn attachment(item: &Value, marks: &mut Vec<WakeMark>) {
    let attachment = &item["attachment"];
    match attachment.get("type").and_then(Value::as_str) {
        Some("hook_success") => {
            if matches!(
                attachment.get("hookName").and_then(Value::as_str),
                Some("SessionStart:startup" | "SessionStart:resume")
            ) {
                push(marks, Some(WakeMark::Boot));
            }
        }
        Some("queued_command") => {
            if let Some(text) = attachment.get("prompt").and_then(Value::as_str) {
                notifications(text, marks);
            }
        }
        _ => {}
    }
}

/// Every `<task-notification>` in `text` that names a finished task. A
/// `running` status, or a notification with none, is an event of a task that
/// goes on.
fn notifications(text: &str, marks: &mut Vec<WakeMark>) {
    for part in text.split("<task-notification>").skip(1) {
        let part = part.split("</task-notification>").next().unwrap_or(part);
        let (Some(id), Some(status)) = (tag(part, "task-id"), tag(part, "status")) else {
            continue;
        };
        if matches!(status.trim(), "completed" | "failed" | "killed" | "stopped") {
            push(marks, id_of(id.trim()).map(|id| WakeMark::Ended { id }));
        }
    }
}

fn tag<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let rest = &text[text.find(&open)? + open.len()..];
    Some(&rest[..rest.find(&format!("</{name}>"))?])
}

fn id_of(id: &str) -> Option<String> {
    (!id.is_empty() && id.len() <= NATIVE_ID_LIMIT_BYTES).then(|| id.to_owned())
}

/// The task id that begins at the start of `text`.
fn token(text: &str) -> Option<&str> {
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(text.len());
    (end > 0).then(|| &text[..end])
}

fn after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    Some(&text[text.find(marker)? + marker.len()..])
}

fn user(item: &Value, marks: &mut Vec<WakeMark>) {
    let Some(blocks) = item.pointer("/message/content").and_then(Value::as_array) else {
        return;
    };
    let at = || crate::timestamp_ms(item.get("timestamp")).ok();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("tool_result")
            || block.get("is_error").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let texts: Vec<&str> = match block.get("content") {
            Some(Value::String(text)) => vec![text],
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect(),
            _ => continue,
        };
        for text in texts {
            if let Some(id) = after(text, "Command running in background with ID: ")
                .or_else(|| after(text, "was moved to the background (ID: "))
                .and_then(token)
            {
                push(
                    marks,
                    id_of(id).map(|id| WakeMark::Started {
                        id,
                        expires_at_unix_ms: None,
                    }),
                );
            } else if let Some(rest) = after(text, "Monitor started (task ")
                && let Some(id) = token(rest)
            {
                // A monitor that announces an expiry but has no time to count
                // it from cannot be proven alive.
                let expires = match after(rest, "expires in ").and_then(duration_ms) {
                    Some(ms) => at().map(|at| Some(at.saturating_add(ms))),
                    None => Some(None),
                };
                let Some(expires) = expires else {
                    push(marks, None);
                    continue;
                };
                push(
                    marks,
                    id_of(id).map(|id| WakeMark::Started {
                        id,
                        expires_at_unix_ms: expires,
                    }),
                );
            } else if text.starts_with("Async agent launched successfully")
                && let Some(id) = after(text, "agentId: ").and_then(token)
            {
                push(
                    marks,
                    id_of(id).map(|id| WakeMark::Started {
                        id,
                        expires_at_unix_ms: None,
                    }),
                );
            }
        }
    }
}

/// `30m`, `90s`, `2h` at the start of `text`.
fn duration_ms(text: &str) -> Option<u64> {
    let digits = text.find(|c: char| !c.is_ascii_digit())?;
    let value: u64 = text[..digits].parse().ok()?;
    let unit = match text[digits..].chars().next()? {
        's' => 1_000,
        'm' => 60_000,
        'h' => 3_600_000,
        _ => return None,
    };
    value.checked_mul(unit)
}

fn assistant(item: &Value, marks: &mut Vec<WakeMark>) {
    let Some(blocks) = item.pointer("/message/content").and_then(Value::as_array) else {
        return;
    };
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("tool_use")
            && block.get("name").and_then(Value::as_str) == Some("TaskStop")
            && let Some(id) = block.pointer("/input/task_id").and_then(Value::as_str)
        {
            push(marks, id_of(id).map(|id| WakeMark::Ended { id }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn marks(item: Value) -> Vec<WakeMark> {
        match claude(&item).unwrap() {
            Some(TurnMark::Wake(marks)) => marks,
            None => Vec::new(),
            other => panic!("{other:?}"),
        }
    }

    fn result(text: &str) -> Value {
        json!({"type": "user", "timestamp": "2026-10-07T13:15:43.912Z",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": text}]}})
    }

    fn started(id: &str) -> WakeMark {
        WakeMark::Started {
            id: id.into(),
            expires_at_unix_ms: None,
        }
    }

    #[test]
    fn each_way_claude_starts_background_work_names_its_task() {
        assert_eq!(
            marks(result(
                "Command running in background with ID: bz0nfp0pu. Output is being written to: /x"
            )),
            [started("bz0nfp0pu")]
        );
        assert_eq!(
            marks(result(
                "Command did not complete within its 600s timeout and was moved to the background (ID: bjbx2pcf9). Output"
            )),
            [started("bjbx2pcf9")]
        );
        let launched = json!({"type": "user", "message": {"content": [{"type": "tool_result",
            "content": [{"type": "text", "text": "Async agent launched successfully. (internal)\nagentId: a7359e5355eca4579 (internal ID)\nThe agent is working"}]}]}});
        assert_eq!(marks(launched), [started("a7359e5355eca4579")]);
    }

    #[test]
    fn a_monitor_expires_after_the_time_it_announces() {
        let at = crate::parse_rfc3339_unix_ms("2026-10-07T13:15:43.912Z").unwrap();
        assert_eq!(
            marks(result(
                "Monitor started (task bcwkc73y4, expires in 30m unless the source ends first; you get one notice"
            )),
            [WakeMark::Started {
                id: "bcwkc73y4".into(),
                expires_at_unix_ms: Some(at + 30 * 60_000)
            }]
        );
    }

    #[test]
    fn a_failed_start_and_ordinary_results_name_no_task() {
        let mut failed = result("Command running in background with ID: x");
        failed["message"]["content"][0]["is_error"] = json!(true);
        assert!(marks(failed).is_empty());
        assert!(marks(result("Compiling hide v0.1.0")).is_empty());
    }

    #[test]
    fn a_task_ends_by_notification_in_either_record_or_by_task_stop() {
        let notification = |status: &str| {
            format!(
                "<task-notification>\n<task-id>bz0nfp0pu</task-id>\n<status>{status}</status>\n</task-notification>"
            )
        };
        let ended = WakeMark::Ended {
            id: "bz0nfp0pu".into(),
        };
        for status in ["completed", "failed", "killed", "stopped"] {
            assert_eq!(
                marks(
                    json!({"type": "queue-operation", "operation": "enqueue", "content": notification(status)})
                ),
                [ended.clone()]
            );
            assert_eq!(
                marks(
                    json!({"type": "attachment", "attachment": {"type": "queued_command", "prompt": notification(status)}})
                ),
                [ended.clone()]
            );
        }
        assert!(marks(json!({"type": "queue-operation", "operation": "enqueue", "content": notification("running")})).is_empty());
        assert!(marks(json!({"type": "queue-operation", "operation": "dequeue", "content": notification("completed")})).is_empty());
        assert_eq!(
            marks(
                json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "TaskStop", "input": {"task_id": "bz0nfp0pu"}}]}})
            ),
            [ended]
        );
    }

    #[test]
    fn only_a_session_start_hook_of_a_new_process_is_a_boundary() {
        let hook = |name: &str| json!({"type": "attachment", "attachment": {"type": "hook_success", "hookName": name}});
        assert_eq!(marks(hook("SessionStart:startup")), [WakeMark::Boot]);
        assert_eq!(marks(hook("SessionStart:resume")), [WakeMark::Boot]);
        assert!(marks(hook("SessionStart:clear")).is_empty());
        assert!(marks(hook("SessionStart:compact")).is_empty());
        assert!(marks(hook("PreToolUse:Bash")).is_empty());
    }
}
