//! `hide-agent-hooks <agent> <operation>`: what Hide's script file in an
//! agent's own folder asks (`hide_agent_hooks::plugin`): OpenCode's plugin
//! (`opencode`), and Pi's and omp's extension (`pi`, `omp`). Each operation
//! reads one JSON object on stdin, prints one on stdout and exits 0; a failure
//! answers `{}`, the script then changes nothing, and the cause goes to the
//! same private diagnostic the other hooks use.
//!
//! The operations reuse the Claude Code and Codex hook's parts: the letter
//! intake (`delivery`), the spawn guard and the Factory question guard
//! (`spawn_guard`), the pane counts (`counters`, `report`) and Project Memory
//! (`memory`). What differs is only the transport: these agents have no
//! command hook, so the script hands over the facts a hook payload would
//! carry. What each agent gets follows its adapter declaration.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hide_agent_adapter::{AgentAdapter, Capability, PluginDialect};
use hide_agent_hooks::delivery::{self, Intake, Prompt};
use hide_agent_hooks::memory::{MemoryRequest, memory_context_until};
use hide_agent_hooks::runtime::HookEvent;
use hide_agent_hooks::spawn_guard::{self as guard, QuestionDecision, Registration};
use hide_agent_hooks::{counters, guidance, report};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::workspace_context;

/// Inside the script's 1.85 s prompt budget, which also pays for this process
/// starting.
const PROMPT_BUDGET: Duration = Duration::from_millis(1_650);
/// Inside the script's 2.5 s tool budget.
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
    /// Helper calls the script skipped at its cap or gave up on since its last
    /// call: recorded here, since the script has no log of its own.
    lost: u32,
    /// The version of the script that asks; Pi's and omp's extension sends it,
    /// and one this build does not write is answered with nothing (B13).
    version: Option<u32>,
}

/// The agent a script speaks for: its adapter row and the runtime id the
/// guard log, the Factory guard and Memory know it by.
struct Agent {
    adapter: &'static AgentAdapter,
    dialect: PluginDialect,
}

impl Agent {
    fn parse(name: &str) -> Option<Self> {
        let adapter = hide_agent_adapter::ADAPTERS
            .iter()
            .find(|row| row.id == name)?;
        match adapter.hook {
            hide_agent_adapter::HookInstall::Plugin(dialect) => Some(Self { adapter, dialect }),
            _ => None,
        }
    }

    fn runtime(&self) -> &'static str {
        self.adapter.id
    }

    /// Whether `version` is the script this build writes. OpenCode's plugin
    /// sends none and is judged by the kit alone.
    fn current(&self, version: Option<u32>) -> bool {
        match self.dialect {
            PluginDialect::OpenCode => true,
            PluginDialect::Pi | PluginDialect::Omp => {
                version == Some(hide_agent_hooks::pi_extension::VERSION)
            }
        }
    }

    /// Whether `tool` is the agent's own question tool, which a Factory worker
    /// is refused.
    fn question_tool(&self, tool: &str) -> bool {
        matches!(self.adapter.factory.direct_ask, Capability::Available(ask) if ask.tools.contains(&tool))
    }
}

/// Answers one operation of the script of `agent` (`opencode`, `pi`, `omp`).
pub fn run(agent: &str, operation: Option<&str>, started: Instant) {
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
    let Some(agent) = Agent::parse(agent).filter(|agent| agent.current(input.version)) else {
        answer(&json!({}));
        return;
    };
    let output = match operation {
        Some("start") => start(),
        Some("prompt") => prompt(&home, &agent, input, started + PROMPT_BUDGET),
        Some("confirm") => confirm(&home, input, started + CONFIRM_BUDGET),
        Some("tool") => tool(&home, &agent, input, started + TOOL_BUDGET),
        Some("subagents") if agent.adapter.subagent_counts.is_some() => subagents(&home, input),
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

/// The guidance a session start gets, read once when the agent loads the
/// script or starts its session, and attached to each new root session's first
/// prompt.
fn start() -> Value {
    json!({
        "context": guidance::session_context(workspace_context::live_context().as_deref()),
    })
}

/// Memory, then the letters waiting for this pane's agent. Nothing is
/// confirmed here: the script confirms after the agent stored the prompt that
/// carries them (`confirm`), so a letter whose prompt was lost stays pending.
fn prompt(home: &Path, agent: &Agent, input: Input, deadline: Instant) -> Value {
    let Some(session) = session(&input) else {
        return json!({});
    };
    let memory = if agent.adapter.memory.is_some() {
        memory_context_until(
            MemoryRequest {
                runtime_id: agent.runtime(),
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
        )
        .context
    } else {
        None
    };
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
    let mut context = memory.unwrap_or_default();
    let mut letters = Vec::new();
    if let Some(Intake { context: text, ids }) = intake {
        context.push_str(&text);
        letters = ids;
    }
    json!({ "context": context, "letters": letters })
}

fn confirm(home: &Path, input: Input, deadline: Instant) -> Value {
    if input.letters.is_empty() || !delivery::valid_letter_ids(&input.letters) {
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

/// The spawn guard for `bash` and the Factory guard for the agent's question
/// tool (OpenCode's `question`, omp's `ask`): `deny` carries the reason the
/// script refuses with, and every other outcome lets the call run, as the
/// Claude Code and Codex guards do.
fn tool(home: &Path, agent: &Agent, input: Input, deadline: Instant) -> Value {
    let runtime = agent.runtime();
    let Some(pane) = pane() else {
        return json!({});
    };
    let Some(program) = workspace_context::cli_program() else {
        return json!({});
    };
    match input.tool.as_deref() {
        Some(name) if agent.question_tool(name) => {
            let Some(session) = session(&input) else {
                return json!({});
            };
            match guard::question_decision(&program, runtime, &session, deadline) {
                QuestionDecision::Worker => {
                    guard::record_question_refusal(home, runtime, &pane, name);
                    json!({ "deny": guard::QUESTION_REASON })
                }
                QuestionDecision::Allow => json!({}),
                QuestionDecision::Unreachable(cause) => {
                    guard::unreachable(home, runtime, cause);
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
                    guard::unreachable(home, runtime, cause);
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
            guard::record_refusal(home, runtime, &pane, &launch);
            json!({ "deny": guard::refusal_reason(&delegation, &handoff) })
        }
        _ => json!({}),
    }
}

/// The pane's subagent counts as the script keeps them, stored and reported to
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
