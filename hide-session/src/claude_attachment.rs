//! What a Claude Code `attachment` record says, judged in one place.
//!
//! Claude Code writes the things it adds to a session itself, outside any
//! message, as `{"type":"attachment","attachment":{"type":...}}`. The wake
//! reader (`turns::wake`) takes a process start and a finished task from the
//! kinds below, and the conversation reader (`parse_claude_line`) takes a
//! hook's added context from [`Attachment::HookContext`]; both ask this
//! module, so the shape is not judged twice. Any other kind is not read.

use serde_json::Value;

pub(crate) enum Attachment<'a> {
    /// A hook that ran; its `stdout` repeats what a hook context says, so this
    /// kind is a process boundary and never a source of text.
    HookSuccess { hook_name: Option<&'a str> },
    /// A command the agent queued for itself, such as a task notification.
    QueuedCommand { prompt: Option<&'a str> },
    /// The context a hook returned to the model, which Claude Code itself
    /// wrote: one string per hook, or a single string.
    HookContext { content: &'a Value },
}

impl<'a> Attachment<'a> {
    /// The attachment a record holds, or `None` for a record that is not an
    /// attachment or one of a kind nothing here reads.
    pub(crate) fn of(item: &'a Value) -> Option<Self> {
        if item.get("type").and_then(Value::as_str) != Some("attachment") {
            return None;
        }
        let attachment = item.get("attachment")?;
        match attachment.get("type").and_then(Value::as_str)? {
            "hook_success" => Some(Self::HookSuccess {
                hook_name: attachment.get("hookName").and_then(Value::as_str),
            }),
            "queued_command" => Some(Self::QueuedCommand {
                prompt: attachment.get("prompt").and_then(Value::as_str),
            }),
            "hook_additional_context" => Some(Self::HookContext {
                content: attachment.get("content")?,
            }),
            _ => None,
        }
    }
}

/// The text of a hook context, its strings joined by a newline; `None` when
/// it holds no text.
pub(crate) fn hook_context_text(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    (!text.trim().is_empty()).then_some(text)
}
