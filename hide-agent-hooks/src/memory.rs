//! A hook's Project Memory, asked of the core (PRD core-host-node-move D-12,
//! B14). The core owns the one store and answers for the checkout the hook's
//! pane is attested to, whichever machine the agent runs on; the hook asks
//! through `hide workspace memory`, the same pane credential and node link
//! its letters come by, and fails open: no answer before its deadline is a
//! turn without Memory.

use crate::delivery::{self, Failure};
use crate::runtime::{AgentRuntime, HookEvent, hook_stdout_with_context};
use hide_memory::HOOK_INPUT_LIMIT_BYTES;
use serde::Deserialize;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a hook waits for Memory, from when it starts asking: inside the
/// 2 s every hook has, and never past the letter pull it runs beside.
pub const MEMORY_BUDGET: Duration = Duration::from_millis(1_250);

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HookMemoryOutcome {
    Provided { count: usize },
    Empty,
    Disabled,
    Unavailable,
    Deadline,
    InputTooLarge,
    ProjectUnresolved,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookMemoryResult {
    pub stdout: Option<String>,
    pub outcome: HookMemoryOutcome,
}

#[derive(Debug, Deserialize)]
struct HookInput {
    cwd: Option<PathBuf>,
    prompt: Option<String>,
    session_id: Option<String>,
}

/// Retains only the bounded stdin prefix and stops as soon as overflow is
/// observed. The installed binary additionally puts this read behind the
/// hook's absolute process deadline.
pub fn read_bounded(mut input: impl Read) -> io::Result<(Vec<u8>, bool)> {
    let mut retained = Vec::with_capacity(HOOK_INPUT_LIMIT_BYTES.min(16 * 1024));
    let mut buffer = [0_u8; 8192];
    let mut exceeded = false;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = HOOK_INPUT_LIMIT_BYTES.saturating_sub(retained.len());
        let keep = remaining.min(read);
        retained.extend_from_slice(&buffer[..keep]);
        exceeded |= keep < read;
        if exceeded {
            break;
        }
    }
    Ok((retained, exceeded))
}

/// The hook's output for `bytes`, the runtime's payload, with the Memory the
/// core gives it before `deadline`; a failure is recorded under `home`.
pub fn project_memory_output_until(
    runtime: AgentRuntime,
    event: HookEvent,
    bytes: &[u8],
    exceeded: bool,
    home: &Path,
    deadline: Instant,
) -> HookMemoryResult {
    let projected = if exceeded {
        MemoryContext::without(HookMemoryOutcome::InputTooLarge)
    } else {
        match serde_json::from_slice::<HookInput>(bytes) {
            Ok(input) => memory_context_until(
                MemoryRequest {
                    runtime_id: runtime.memory_id(),
                    event,
                    cwd: input.cwd,
                    prompt: input.prompt,
                    session_id: input.session_id,
                },
                home,
                deadline,
            ),
            Err(_) => MemoryContext::without(HookMemoryOutcome::Unavailable),
        }
    };
    HookMemoryResult {
        stdout: match &projected.context {
            Some(context) => hook_stdout_with_context(runtime, event, Some(context)),
            None => crate::runtime::hook_stdout(runtime, event),
        },
        outcome: projected.outcome,
    }
}

/// What one hook asks Project Memory for, whatever runtime's payload it came in.
pub struct MemoryRequest<'a> {
    /// The provider id Memory keys sessions and receipts by: `claude`,
    /// `codex`, `opencode`, `pi` or `omp`.
    pub runtime_id: &'a str,
    /// `SessionStart` for a session's first context, `UserPromptSubmit` for a
    /// later prompt; any other event asks for nothing.
    pub event: HookEvent,
    pub cwd: Option<PathBuf>,
    pub prompt: Option<String>,
    pub session_id: Option<String>,
}

/// The Memory context for one hook, with its receipt, or none.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MemoryContext {
    pub context: Option<String>,
    pub outcome: HookMemoryOutcome,
}

impl MemoryContext {
    fn without(outcome: HookMemoryOutcome) -> Self {
        Self {
            context: None,
            outcome,
        }
    }
}

/// The core's answer: what `hide_memory::hook::HookContext` serializes to.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
enum Outcome {
    Provided { count: usize },
    Empty,
    Disabled,
    Unavailable,
    Deadline,
    ProjectUnresolved,
}

#[derive(Deserialize)]
struct Answer {
    context: Option<String>,
    #[serde(flatten)]
    outcome: Outcome,
}

/// [`MemoryContext`] for `request` from the core, bounded by `deadline` and
/// failing open; a failure to ask is recorded under `home` (`memory_hook`
/// `memory.unavailable` or `memory.deadline`).
pub fn memory_context_until(
    request: MemoryRequest<'_>,
    home: &Path,
    deadline: Instant,
) -> MemoryContext {
    let MemoryRequest {
        runtime_id,
        event,
        cwd,
        prompt,
        session_id,
    } = request;
    if !matches!(event, HookEvent::SessionStart | HookEvent::UserPromptSubmit) {
        return MemoryContext::without(HookMemoryOutcome::Unavailable);
    }
    let Some(session) = session_id.filter(|session| delivery::valid_session(session)) else {
        return MemoryContext::without(HookMemoryOutcome::Unavailable);
    };
    // The core relates the folder to the checkout's root as this machine's
    // Project read spells it, which is its canonical path.
    let cwd = cwd.map(|cwd| {
        std::fs::canonicalize(&cwd)
            .unwrap_or(cwd)
            .to_string_lossy()
            .into_owned()
    });
    let mut arguments = vec![
        "workspace",
        "memory",
        "--event",
        event.name(),
        "--runtime",
        runtime_id,
        "--session",
        &session,
    ];
    if let Some(cwd) = cwd.as_deref().filter(|cwd| !cwd.is_empty()) {
        arguments.extend(["--cwd", cwd]);
    }
    let prompt = match event {
        HookEvent::UserPromptSubmit => prompt.as_deref().map(hide_memory::hook::prompt_prefix),
        _ => None,
    };
    let answer =
        delivery::run_cli_with_input(&arguments, prompt.unwrap_or_default().as_bytes(), deadline)
            .and_then(|value| serde_json::from_value::<Answer>(value).map_err(|_| "format".into()));
    let answer = match answer {
        Ok(answer) => answer,
        Err(failure) => {
            let deadline = failure.code == "deadline";
            record(home, &failure);
            return MemoryContext::without(if deadline {
                HookMemoryOutcome::Deadline
            } else {
                HookMemoryOutcome::Unavailable
            });
        }
    };
    MemoryContext {
        context: answer.context,
        outcome: match answer.outcome {
            Outcome::Provided { count } => HookMemoryOutcome::Provided { count },
            Outcome::Empty => HookMemoryOutcome::Empty,
            Outcome::Disabled => HookMemoryOutcome::Disabled,
            Outcome::Unavailable => HookMemoryOutcome::Unavailable,
            Outcome::Deadline => HookMemoryOutcome::Deadline,
            Outcome::ProjectUnresolved => HookMemoryOutcome::ProjectUnresolved,
        },
    }
}

fn record(home: &Path, failure: &Failure) {
    let (cause, kind) = if failure.code == "deadline" {
        ("memory_deadline", "memory.deadline")
    } else {
        ("memory_unavailable", "memory.unavailable")
    };
    if delivery::claim(home, cause) {
        eprintln!(
            "{}",
            serde_json::json!({"component":"memory_hook","kind":kind,"code":failure.code,"diagnostic_saved":true})
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_input_is_drained_but_never_parsed() {
        let bytes = vec![b'x'; HOOK_INPUT_LIMIT_BYTES + 7];
        let (retained, exceeded) = read_bounded(bytes.as_slice()).unwrap();
        assert_eq!(retained.len(), HOOK_INPUT_LIMIT_BYTES);
        assert!(exceeded);
    }

    #[test]
    fn the_core_s_answer_reads_as_the_hook_s_outcome() {
        let answer = |value: serde_json::Value| serde_json::from_value::<Answer>(value).unwrap();
        let provided = answer(serde_json::json!({
            "context": "Project Memory:\n- Keep it.\n", "outcome": "provided", "count": 1,
        }));
        assert!(matches!(provided.outcome, Outcome::Provided { count: 1 }));
        assert_eq!(
            provided.context.as_deref(),
            Some("Project Memory:\n- Keep it.\n")
        );
        let none = answer(serde_json::json!({"context": null, "outcome": "project_unresolved"}));
        assert!(matches!(none.outcome, Outcome::ProjectUnresolved));
        assert!(none.context.is_none());
    }

    #[test]
    fn current_runtime_prompt_fixtures_parse_and_use_the_verified_context_envelope() {
        for (runtime, bytes) in [
            (
                AgentRuntime::ClaudeCode,
                include_bytes!("../tests/fixtures/claude-user-prompt-submit.json").as_slice(),
            ),
            (
                AgentRuntime::Codex,
                include_bytes!("../tests/fixtures/codex-user-prompt-submit.json").as_slice(),
            ),
        ] {
            let input: HookInput = serde_json::from_slice(bytes).unwrap();
            assert_eq!(
                input.cwd.as_deref(),
                Some(Path::new("/tmp/fixture-project"))
            );
            assert_eq!(
                input.prompt.as_deref(),
                Some("Inspect the project memory boundary")
            );
            let output = hook_stdout_with_context(
                runtime,
                HookEvent::UserPromptSubmit,
                Some("Project Memory:\n- Keep the boundary local.\n"),
            )
            .unwrap();
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert_eq!(
                value["hookSpecificOutput"]["hookEventName"],
                "UserPromptSubmit"
            );
            assert_eq!(
                value["hookSpecificOutput"]["additionalContext"],
                "Project Memory:\n- Keep the boundary local.\n"
            );
        }
    }

    #[test]
    fn an_empty_session_start_receipt_follows_the_base_context_without_a_zero_item_message() {
        let receipt = "<hide-memory-receipt event=\"SessionStart\" count=\"0\" items=\"\" auth=\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" />\n";
        for runtime in [AgentRuntime::ClaudeCode, AgentRuntime::Codex] {
            let output =
                hook_stdout_with_context(runtime, HookEvent::SessionStart, Some(receipt)).unwrap();
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            let delivered = value["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap();
            assert!(delivered.ends_with(receipt), "{delivered}");
            assert!(!delivered.contains("Project Memory ready 0"));
        }
    }

    #[test]
    fn an_expired_hook_keeps_only_its_base_context() {
        let temp = tempfile::tempdir().unwrap();
        let payload = serde_json::to_vec(&serde_json::json!({
            "cwd": temp.path(),
            "session_id": "expired-session"
        }))
        .unwrap();

        let result = project_memory_output_until(
            AgentRuntime::Codex,
            HookEvent::SessionStart,
            &payload,
            false,
            temp.path(),
            Instant::now(),
        );

        assert_eq!(result.outcome, HookMemoryOutcome::Deadline);
        let output: serde_json::Value =
            serde_json::from_str(result.stdout.as_ref().unwrap()).unwrap();
        assert_eq!(
            output["hookSpecificOutput"]["additionalContext"],
            crate::runtime::PURPOSE_CONTEXT
        );
    }
}
