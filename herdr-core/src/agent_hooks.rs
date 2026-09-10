//! Reading what Hide's hook wrote onto a pane.
//!
//! The hook helper reports through `herdr pane report-metadata`, so its
//! values arrive as ordinary pane tokens on the session snapshot the core
//! already pulls. This module is the only place that reads them, the way
//! [`crate::pane_content`] is the only place that reads the browser pane's
//! tokens (PRD D-08, D-33).
//!
//! Every value is optional and an unreadable one is dropped rather than
//! defaulted. A count Hide cannot read is unknown, and unknown is drawn as
//! unknown; it is never a zero, because a zero is a claim that the agent is
//! working alone (PRD B32, engineering rule 4).

use std::collections::BTreeMap;

use hide_agent_hooks::runtime::{
    AgentRuntime, BLOCKED_TOKEN, DONE_TOKEN, INSTRUMENTED_TOKEN, WORKING_TOKEN,
};
use serde_json::Value;

/// What one pane's tokens say about the session running in it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PaneHookTokens {
    /// The hook version this session reported with. Its presence is the proof
    /// that the session is instrumented at all, which is what separates "this
    /// agent is working alone" from "Hide cannot see this agent's children"
    /// (PRD B23, D-61).
    pub version: Option<u32>,
    pub working: Option<u32>,
    pub done: Option<u32>,
    pub blocked: Option<u32>,
}

impl PaneHookTokens {
    pub fn read(tokens: &BTreeMap<String, Value>) -> Self {
        Self {
            version: count(tokens, INSTRUMENTED_TOKEN),
            working: count(tokens, WORKING_TOKEN),
            done: count(tokens, DONE_TOKEN),
            blocked: count(tokens, BLOCKED_TOKEN),
        }
    }

    /// Whether this pane carries any of Hide's tokens at all.
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

/// Herdr reports every token as a string, so a count is parsed from one. A
/// value that is not a count is dropped, never coerced to zero.
fn count(tokens: &BTreeMap<String, Value>, name: &str) -> Option<u32> {
    match tokens.get(name)? {
        Value::String(raw) => raw.trim().parse().ok(),
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        _ => None,
    }
}

/// The runtime behind an agent kind Herdr detected.
///
/// `None` is an agent Hide has no adapter for. It is the honest answer, and
/// it lands on the single fallback reason rather than being guessed into one
/// of the two Hide does know (PRD B32, D-64).
pub fn runtime_of(agent_kind: &str) -> Option<AgentRuntime> {
    match agent_kind.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claude_code" => Some(AgentRuntime::ClaudeCode),
        "codex" => Some(AgentRuntime::Codex),
        _ => None,
    }
}

/// The prefix every pane id on a remote target carries.
///
/// It is the same rule the read-record ledger scopes itself by; a remote pane
/// is uninstrumented by decision, because Hide does not write to another
/// machine's file system (PRD D-28, D-49).
pub fn is_remote_pane(pane_id: &str) -> bool {
    pane_id.starts_with("remote:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Value::String((*value).to_owned())))
            .collect()
    }

    #[test]
    fn an_instrumented_pane_reports_its_version_and_the_counts_it_has() {
        let read = PaneHookTokens::read(&tokens(&[
            ("hide_hooks", "1"),
            ("hide_sub_working", "2"),
            ("hide_sub_done", "7"),
            ("summary", "unrelated"),
        ]));
        assert_eq!(read.version, Some(1));
        assert_eq!(read.working, Some(2));
        assert_eq!(read.done, Some(7));
        assert_eq!(
            read.blocked, None,
            "a count no adapter reports stays unknown"
        );
    }

    #[test]
    fn a_pane_with_none_of_hides_tokens_is_empty_rather_than_a_row_of_zeroes() {
        let read = PaneHookTokens::read(&tokens(&[("summary", "building"), ("elapsed", "3m")]));
        assert!(read.is_empty());
        assert_eq!((read.version, read.working, read.done), (None, None, None));
    }

    #[test]
    fn an_unreadable_count_is_dropped_and_never_coerced() {
        let read = PaneHookTokens::read(&tokens(&[
            ("hide_hooks", "1"),
            ("hide_sub_working", "many"),
            ("hide_sub_done", "-4"),
        ]));
        assert_eq!(read.version, Some(1));
        assert_eq!(read.working, None);
        assert_eq!(read.done, None);
    }

    #[test]
    fn only_the_two_shipped_adapters_resolve_and_anything_else_says_so() {
        assert_eq!(runtime_of("claude"), Some(AgentRuntime::ClaudeCode));
        assert_eq!(runtime_of("Codex"), Some(AgentRuntime::Codex));
        assert_eq!(runtime_of("unknown"), None);
        assert_eq!(runtime_of(""), None);
    }

    #[test]
    fn a_remote_pane_is_recognised_by_the_same_prefix_the_read_ledger_uses() {
        assert!(is_remote_pane("remote:mini/w1:pA"));
        assert!(!is_remote_pane("w1:pA"));
    }
}
