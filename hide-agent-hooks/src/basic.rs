//! What Grok's and Cursor's own hook events say, and what Hide answers them
//! (PRD grok-cursor-hooks).
//!
//! Both agents document a refusal of a shell call and the start and end of a
//! subagent, in their own payload and answer shapes. This module reads those
//! payloads into the facts the spawn guard and the pane count need and
//! writes the answers; the guard itself (`crate::spawn_guard`) and the count
//! (`crate::counters`) are the same ones Claude Code's and Codex's hooks use.
//!
//! | | Grok | Cursor |
//! | --- | --- | --- |
//! | Shell call | `toolName` `run_terminal_command` (`Bash` in a matcher), `toolInput.command`, `cwd` | `tool_name` `Shell`, `tool_input.command`, `tool_input.working_directory`, `cwd` |
//! | Refusal | Claude Code's `hookSpecificOutput.permissionDecision` | `{"permission": "deny", "agent_message": ...}` |
//! | Nothing to refuse | no output | `{"permission": "allow"}`, on every path (D-04) |
//! | Turn end | `Stop`: `subagentType` inside a subagent, `backgroundTasks` still running | `stop` |
//!
//! Cursor's `preToolUse` and `subagentStart` are permission hooks: an answer
//! that is not valid JSON of their schema blocks the action even when the
//! hook exits zero, so for them every path that does not refuse prints
//! [`CURSOR_ALLOW`], including a helper that fails or runs out of time.

use serde::Deserialize;
use serde_json::Value;

use crate::runtime::HookEvent;

/// Cursor's answer that lets the action proceed.
pub const CURSOR_ALLOW: &str = r#"{"permission":"allow"}"#;

/// Whether Cursor reads `event`'s answer as a permission, so that anything
/// but a valid answer refuses the action.
pub fn cursor_permission_event(event: HookEvent) -> bool {
    matches!(event, HookEvent::PreToolUse | HookEvent::SubagentStart)
}

/// Cursor's refusal of a tool call: the reason goes back to the agent.
pub fn cursor_deny(reason: &str) -> String {
    serde_json::json!({ "permission": "deny", "agent_message": reason }).to_string()
}

/// What a Grok `Stop` says about the pane's subagents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrokStop {
    /// The stop is a subagent's own (`subagentType`) or the payload could not
    /// be read: the pane's count is left as it is.
    Ignore,
    /// The main session's turn ended with this many background subagents
    /// still running.
    Running(u32),
}

/// Reads a Grok `Stop` payload. Every `backgroundTasks` entry is in flight
/// (the list is empty when nothing is), and only the `subagent` ones are the
/// pane's subagents; shells and monitors are not.
pub fn grok_stop(payload: &[u8], truncated: bool) -> GrokStop {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Stop {
        subagent_type: Option<Value>,
        #[serde(default)]
        background_tasks: Vec<Task>,
    }
    #[derive(Deserialize)]
    struct Task {
        #[serde(rename = "type")]
        kind: Option<String>,
    }
    if truncated {
        return GrokStop::Ignore;
    }
    let Ok(stop) = serde_json::from_slice::<Stop>(payload) else {
        return GrokStop::Ignore;
    };
    // An empty or null type is no subagent's: the main session's stop.
    if stop
        .subagent_type
        .is_some_and(|kind| !kind.is_null() && kind.as_str() != Some(""))
    {
        return GrokStop::Ignore;
    }
    let running = stop
        .background_tasks
        .iter()
        .filter(|task| task.kind.as_deref() == Some("subagent"))
        .count();
    GrokStop::Running(u32::try_from(running).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_reads_its_permission_answers_as_valid_json_of_its_schema() {
        let allow: Value = serde_json::from_str(CURSOR_ALLOW).unwrap();
        assert_eq!(allow, serde_json::json!({ "permission": "allow" }));
        let deny: Value = serde_json::from_str(&cursor_deny("use hide agent spawn")).unwrap();
        assert_eq!(
            deny,
            serde_json::json!({ "permission": "deny", "agent_message": "use hide agent spawn" })
        );
        assert!(cursor_permission_event(HookEvent::PreToolUse));
        assert!(cursor_permission_event(HookEvent::SubagentStart));
        for event in [
            HookEvent::SessionStart,
            HookEvent::SubagentStop,
            HookEvent::Stop,
        ] {
            assert!(!cursor_permission_event(event));
        }
    }

    #[test]
    fn a_grok_stop_counts_only_the_background_subagents_of_the_main_session() {
        let stop =
            br#"{"hook_event_name":"Stop","sessionId":"s","reason":"end_turn","backgroundTasks":[
            {"id":"a","type":"subagent","status":"running","agentType":"explore"},
            {"id":"b","type":"shell","status":"running","command":"npm run dev"},
            {"id":"c","type":"subagent","status":"running"},
            {"id":"d","type":"monitor","status":"running"}],"sessionCrons":[]}"#;
        assert_eq!(grok_stop(stop, false), GrokStop::Running(2));
        assert_eq!(
            grok_stop(br#"{"sessionId":"s","backgroundTasks":[]}"#, false),
            GrokStop::Running(0)
        );
        assert_eq!(
            grok_stop(br#"{"sessionId":"s"}"#, false),
            GrokStop::Running(0)
        );
        let child = br#"{"sessionId":"child","subagentType":"explore","backgroundTasks":[]}"#;
        assert_eq!(grok_stop(child, false), GrokStop::Ignore);
        assert_eq!(
            grok_stop(
                br#"{"sessionId":"s","subagentType":"","backgroundTasks":[]}"#,
                false
            ),
            GrokStop::Running(0)
        );
        assert_eq!(grok_stop(b"not json", false), GrokStop::Ignore);
        assert_eq!(grok_stop(stop, true), GrokStop::Ignore);
    }
}
