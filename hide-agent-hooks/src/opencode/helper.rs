//! `hide-agent-hooks opencode <operation>`: what Hide's OpenCode plugin asks
//! (`plugin.js`). Each operation reads one JSON object on stdin, prints one on
//! stdout and exits 0; a failure answers `{}`, the plugin then changes nothing,
//! and the cause goes to the same private diagnostic the other hooks use.
//!
//! The operations reuse the Claude Code and Codex hook's parts: the letter
//! intake (`delivery`), the spawn guard and the Factory question guard
//! (`spawn_guard`), the pane counts (`counters`, `report`) and Project Memory
//! (`memory`). What differs is only the transport: OpenCode has no command
//! hook, so the plugin hands over the facts a hook payload would carry.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hide_agent_hooks::delivery::{self, Intake, Prompt};
use hide_agent_hooks::memory::{MemoryRequest, memory_context_until};
use hide_agent_hooks::runtime::HookEvent;
use hide_agent_hooks::spawn_guard::{self as guard, QuestionDecision, Registration};
use hide_agent_hooks::{counters, guidance, report};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::workspace_context;

/// The runtime id the guard log, the Factory guard and Memory know OpenCode by.
const RUNTIME: &str = "opencode";
/// Inside the plugin's 1.85 s prompt budget, which also pays for this process
/// starting.
const PROMPT_BUDGET: Duration = Duration::from_millis(1_650);
/// Inside the plugin's 2.5 s tool budget.
const TOOL_BUDGET: Duration = Duration::from_millis(2_300);
const CONFIRM_BUDGET: Duration = Duration::from_millis(1_800);
const INPUT_LIMIT: u64 = 256 * 1024;

#[derive(Default, Deserialize)]
#[serde(default)]
struct Input {
    session_id: Option<String>,
    prompt: Option<String>,
    cwd: Option<PathBuf>,
    first: bool,
    letters: Vec<String>,
    tool: Option<String>,
    command: Option<String>,
    working: u32,
    done: u32,
    /// Helper calls the plugin skipped at its cap or gave up on since its last
    /// call: recorded here, since the plugin has no log of its own.
    lost: u32,
}

pub fn run(operation: Option<&str>, started: Instant) {
    let Some(home) = hide_platform::host::home_dir().ok() else {
        answer(&json!({}));
        return;
    };
    let mut bytes = Vec::new();
    let input = std::io::stdin()
        .take(INPUT_LIMIT)
        .read_to_end(&mut bytes)
        .ok()
        .and_then(|_| serde_json::from_slice::<Input>(&bytes).ok());
    let Some(input) = input else {
        delivery::diagnose(&home, "format");
        answer(&json!({}));
        return;
    };
    if input.lost > 0 {
        delivery::diagnose(&home, "plugin");
    }
    let output = match operation {
        Some("start") => start(),
        Some("prompt") => prompt(&home, input, started + PROMPT_BUDGET),
        Some("confirm") => confirm(&home, input, started + CONFIRM_BUDGET),
        Some("tool") => tool(&home, input, started + TOOL_BUDGET),
        Some("subagents") => subagents(&home, input),
        _ => json!({}),
    };
    answer(&output);
}

fn answer(value: &Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{value}").and_then(|_| stdout.flush());
}

fn pane() -> Option<String> {
    std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.is_empty())
}

fn session(input: &Input) -> Option<String> {
    input
        .session_id
        .clone()
        .filter(|id| delivery::valid_session(id))
}

/// The guidance a session start gets, read once when OpenCode loads the plugin
/// and attached to each new root session's first prompt.
fn start() -> Value {
    json!({
        "context": guidance::session_context(workspace_context::live_context().as_deref()),
    })
}

/// Memory, then the letters waiting for this pane's agent. Nothing is
/// confirmed here: the plugin confirms after OpenCode stored the prompt that
/// carries them (`confirm`), so a letter whose prompt was lost stays pending.
fn prompt(home: &Path, input: Input, deadline: Instant) -> Value {
    let Some(session) = session(&input) else {
        return json!({});
    };
    let memory = memory_context_until(
        MemoryRequest {
            runtime_id: RUNTIME,
            event: if input.first {
                HookEvent::SessionStart
            } else {
                HookEvent::UserPromptSubmit
            },
            cwd: input.cwd,
            prompt: input.prompt,
            session_id: Some(session.clone()),
        },
        home,
        Instant::now() + Duration::from_millis(hide_memory::HOOK_PROCESS_BUDGET_MS),
    );
    let intake = delivery::pull(
        deadline,
        &Prompt {
            bell: false,
            session: Some(session),
        },
    )
    .unwrap_or_else(|failure| {
        delivery::diagnose_failure(home, &failure);
        None
    });
    let mut context = memory.context.unwrap_or_default();
    let mut letters = Vec::new();
    if let Some(Intake { context: text, ids }) = intake {
        context.push_str(&text);
        letters = ids;
    }
    json!({ "context": context, "letters": letters })
}

fn confirm(home: &Path, input: Input, deadline: Instant) -> Value {
    if input.letters.is_empty() {
        return json!({});
    }
    let intake = Intake {
        context: String::new(),
        ids: input.letters,
    };
    match delivery::confirm(&intake, deadline) {
        Ok(()) => json!({ "confirmed": intake.ids }),
        Err(failure) => {
            delivery::diagnose_failure(home, &failure);
            json!({})
        }
    }
}

/// The spawn guard for `bash` and the Factory guard for `question`: `deny`
/// carries the reason the plugin throws, and every other outcome lets the call
/// run, as the Claude Code and Codex guards do.
fn tool(home: &Path, input: Input, deadline: Instant) -> Value {
    let Some(pane) = pane() else {
        return json!({});
    };
    let Some(program) = workspace_context::cli_program() else {
        return json!({});
    };
    match input.tool.as_deref() {
        Some("question") => {
            let Some(session) = session(&input) else {
                return json!({});
            };
            match guard::question_decision(&program, RUNTIME, &session, deadline) {
                QuestionDecision::Worker => {
                    guard::record_question_refusal(home, RUNTIME, &pane, "question");
                    json!({ "deny": guard::QUESTION_REASON })
                }
                QuestionDecision::Allow => json!({}),
                QuestionDecision::Unreachable(cause) => {
                    guard::unreachable(home, RUNTIME, cause);
                    json!({})
                }
            }
        }
        Some("bash") => {
            let Some(command) = input.command.as_deref() else {
                return json!({});
            };
            let Some(launch) = guard::find_launch(command, &|name| std::env::var(name).ok()) else {
                return json!({});
            };
            match guard::registration(&program, deadline) {
                Registration::Registered => {}
                Registration::NotRegistered => return json!({}),
                Registration::Unreachable(cause) => {
                    guard::unreachable(home, RUNTIME, cause);
                    return json!({});
                }
            }
            let cwd = input
                .cwd
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_default();
            let (repo, branch) = guard::checkout_facts(&cwd);
            let delegation =
                guard::spawn_command(&launch, repo.as_deref(), branch.as_deref(), true);
            let handoff = guard::spawn_command(&launch, repo.as_deref(), branch.as_deref(), false);
            // Logged before it is answered; the refusal stands without the log.
            guard::record_refusal(home, RUNTIME, &pane, &launch);
            json!({ "deny": guard::refusal_reason(&delegation, &handoff) })
        }
        _ => json!({}),
    }
}

/// The pane's subagent counts as the plugin keeps them, stored and reported to
/// Herdr the way the Claude Code and Codex hooks report theirs.
fn subagents(home: &Path, input: Input) -> Value {
    let Some(pane) = pane() else {
        return json!({});
    };
    let counts = counters::PaneCounters {
        working: input.working,
        done: input.done,
    };
    if counters::store(home, &pane, counts).is_err() {
        return json!({});
    }
    let socket_path = report::socket_path(home);
    let outcome = match &socket_path {
        Ok(path) => report::report(path, &pane, counts),
        Err(error) => Err(error.clone()),
    };
    let socket_path = socket_path.unwrap_or_default();
    let _ = report::record_outcome(home, &pane, "subagent count", &socket_path, &outcome);
    json!({ "reported": outcome.is_ok() })
}
