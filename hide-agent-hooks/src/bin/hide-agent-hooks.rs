//! Hide's agent-hook helper.
//!
//! Two jobs, and a hard line between them:
//!
//! - `hook` is what the installed hook entry runs. It counts one event for
//!   the pane it fired in and reports the pane's totals to Herdr. It never
//!   writes a runtime's configuration and never fails loudly: an agent turn
//!   must not break because Hide could not count something.
//! - `doctor` prints the same hook-install judgement the Settings diagnosis
//!   shows, so the state can be read without launching the app (PRD B30).
//!
//! Installing and removing are deliberately absent. They change a file the
//! operator owns, and the only paths authorised to do that are Hide's first
//! run and the Settings action the operator pressed (PRD D-25, D-31, B28).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use hide_agent_adapter::HookDialect;
use hide_agent_hooks::basic;
use hide_agent_hooks::counters::{self, Change};
use hide_agent_hooks::diagnosis::Diagnosis;
use hide_agent_hooks::guidance::{self, GuidanceAgent};
use hide_agent_hooks::report;
use hide_agent_hooks::runtime::{AgentRuntime, HookEvent, hook_stdout};
use hide_platform::process::{OwnedChild, OwnerWatch};

const PROMPT_BUDGET: Duration = Duration::from_millis(1_850);
/// How long the spawn guard waits for the daemon's answer, inside the entry's
/// own eight second timeout. It is spent only for a call that starts an agent
/// through Herdr, so an ordinary shell call never waits on it.
const GUARD_BUDGET: Duration = Duration::from_millis(2_500);
/// How long the spawn guard waits for the tool call's payload.
const GUARD_PAYLOAD_BUDGET: Duration = Duration::from_millis(500);
const INTAKE_BUDGET: Duration = Duration::from_millis(1_650);
/// How long a prompt hook waits for the runtime's payload to learn whether
/// the prompt was Hide's bell. A producer that never closes stdin costs this
/// and no more, and the prompt is then treated as the operator's.
const PROMPT_PAYLOAD_BUDGET: Duration = Duration::from_millis(500);

#[path = "../workspace_context.rs"]
mod workspace_context;

fn main() -> ExitCode {
    let started = Instant::now();
    // Guarded launches acknowledge ownership before parsing, filesystem work
    // or runtime stdin. Keep this guard until all output and intake finish.
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let owner_watch = match OwnerWatch::from_launch() {
        Ok(watch) => watch,
        // Exit 2 from a `PreToolUse` hook refuses the tool call. The outer hook
        // is what an agent runs, and it must never end that way; the inner one
        // is only ever started by it. Cursor's permission hooks get their allow,
        // since Cursor reads output that is not a valid answer as a refusal.
        Err(_) if arguments.first().map(String::as_str) == Some("hook") => {
            if cursor_permission_hook(&arguments) {
                print_line(basic::CURSOR_ALLOW);
            }
            return ExitCode::SUCCESS;
        }
        Err(_) => return ExitCode::from(2),
    };
    match arguments.first().map(String::as_str) {
        Some("hook") => {
            // Grok and Cursor have no Memory or letters: their hooks carry the
            // spawn guard, the subagent count and Cursor's session guidance
            // (`hide_agent_hooks::guidance`, `hide_agent_hooks::basic`).
            if let Some(agent) = argument_value("--runtime", &arguments)
                .and_then(|value| GuidanceAgent::from_id(&value))
            {
                run_basic_hook(agent, &arguments, started);
                return ExitCode::SUCCESS;
            }
            // An entry an earlier build wrote for an agent Hide no longer
            // supports runs nothing: the kit's retirement takes it out, and
            // until then it must not count a pane or print guidance.
            if argument_value("--runtime", &arguments)
                .is_some_and(|value| GuidanceAgent::is_retired_id(&value))
            {
                return ExitCode::SUCCESS;
            }
            // Cursor and Grok load Claude Code's hooks from
            // `~/.claude/settings.json` beside their own and run both, so under
            // them Claude Code's hook stays out and their own hook is the one
            // that speaks, once (`docs/agent-hooks.md`, Other agents). OpenCode
            // runs it too and has no hook of Hide's: it still speaks there, but
            // takes no letters (`run_hook`).
            if argument_value("--runtime", &arguments).and_then(|value| AgentRuntime::parse(&value))
                == Some(AgentRuntime::ClaudeCode)
                && hide_agent_hooks::runtime::silences_claude_hook(|name| std::env::var_os(name))
            {
                return ExitCode::SUCCESS;
            }
            let event = argument_value("--event", &arguments);
            if event.as_deref() == Some(HookEvent::PreToolUse.name()) {
                // A guard that fails answers like one with nothing to say:
                // the call runs (PRD herdr-spawn-guard D-04). A panic is one
                // such failure, and it must not end the hook with a code the
                // agent reads as a refusal.
                let _ = std::panic::catch_unwind(|| run_spawn_guard(&arguments, started));
            } else if event.as_deref() == Some(HookEvent::UserPromptSubmit.name()) {
                run_prompt_hook(&arguments, started + PROMPT_BUDGET);
            } else {
                run_hook(&arguments, started);
            }
            // A hook that could not report is not a failed turn. Its visible
            // outcome is the pane reading as uninstrumented (PRD B32).
            ExitCode::SUCCESS
        }
        Some("hook-inner") if owner_watch.is_some() => {
            run_hook(&arguments, started);
            ExitCode::SUCCESS
        }
        // An internal operation must never bypass its parent deadline.
        Some("hook-inner") => ExitCode::from(2),
        Some("doctor") => match run_doctor(&arguments) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("{}", usage());
            ExitCode::from(2)
        }
    }
}

fn usage() -> String {
    "usage: hide-agent-hooks hook --runtime <claude-code|codex> \
     --event <SessionStart|UserPromptSubmit|SubagentStart|SubagentStop|Stop|PreToolUse> \
     [--memory-injection] [--source <install marker>]\n       \
     hide-agent-hooks hook --runtime <grok|cursor> \
     --event <SessionStart|PreToolUse|SubagentStart|SubagentStop|Stop> \
     [--source <install marker>]\n       hide-agent-hooks doctor [--json]"
        .to_owned()
}

/// Whether this is a Cursor permission hook (`preToolUse`, `subagentStart`)
/// outside Grok: one whose every answer but a refusal must be Cursor's allow.
fn cursor_permission_hook(arguments: &[String]) -> bool {
    argument_value("--runtime", arguments).and_then(|value| GuidanceAgent::from_id(&value))
        == Some(GuidanceAgent::Cursor)
        && argument_value("--event", arguments)
            .and_then(|value| HookEvent::parse(&value))
            .is_some_and(basic::cursor_permission_event)
        && !inside_grok()
}

fn inside_grok() -> bool {
    hide_agent_hooks::runtime::inside_grok(|name| std::env::var_os(name))
}

/// The hook of Grok or Cursor (PRD grok-cursor-hooks): the spawn guard on a
/// shell call, the subagent count on the subagent and turn events, and
/// Cursor's guidance at session start. It never fails loudly, and Cursor's
/// permission events get an answer on every path.
fn run_basic_hook(agent: GuidanceAgent, arguments: &[String], started: Instant) {
    let Some(dialect) = agent.dialect() else {
        return;
    };
    let Some(event) =
        argument_value("--event", arguments).and_then(|value| HookEvent::parse(&value))
    else {
        return;
    };
    // Grok runs Cursor's `hooks.json` beside its own file; there only Grok's
    // own hook speaks, so nothing is refused or counted twice (D-05).
    if dialect == HookDialect::Cursor && inside_grok() {
        return;
    }
    let answer = std::panic::catch_unwind(|| match event {
        HookEvent::PreToolUse => guard_refusal(dialect, agent.id(), started).map(|reason| {
            match dialect {
                HookDialect::Cursor => basic::cursor_deny(&reason),
                // Grok takes Claude Code's refusal envelope.
                _ => hide_agent_hooks::spawn_guard::deny_output(&reason),
            }
        }),
        HookEvent::SessionStart => guidance::stdout(
            agent,
            &guidance::session_context(workspace_context::live_context().as_deref()),
        ),
        _ => None,
    })
    .ok()
    .flatten();
    let answer = match answer {
        None if dialect == HookDialect::Cursor && basic::cursor_permission_event(event) => {
            Some(basic::CURSOR_ALLOW.to_owned())
        }
        answer => answer,
    };
    if let Some(answer) = answer {
        print_line(&answer);
    }
    let _ = std::panic::catch_unwind(|| count_basic(dialect, event, started));
}

/// The count a Grok or Cursor event makes. Grok's `Stop` names the
/// background subagents still running and comes from inside a subagent too,
/// which leaves the count alone; Cursor's subagents end with the turn.
fn count_basic(dialect: HookDialect, event: HookEvent, started: Instant) {
    let change = match (dialect, event) {
        (_, HookEvent::SessionStart) => Change::Reset,
        (_, HookEvent::SubagentStart) => Change::Started,
        (_, HookEvent::SubagentStop) => Change::Stopped,
        (HookDialect::Grok, HookEvent::Stop) => {
            let Some((payload, truncated)) =
                read_stdin_before_deadline(started + GUARD_PAYLOAD_BUDGET)
            else {
                return;
            };
            match basic::grok_stop(&payload, truncated) {
                basic::GrokStop::Ignore => return,
                basic::GrokStop::Running(running) => Change::Settled { running },
            }
        }
        (_, HookEvent::Stop) => Change::Settled { running: 0 },
        (_, HookEvent::UserPromptSubmit | HookEvent::PreToolUse) => return,
    };
    let Some(home) = home_directory() else { return };
    report_count(&home, event, change);
}

fn print_line(text: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{text}").and_then(|_| stdout.flush());
}

/// The spawn guard (`hide_agent_hooks::spawn_guard`) of Claude Code's and
/// Codex's hook: it prints Claude Code's refusal for a shell call that starts
/// an agent through Herdr, and nothing on every other path.
fn run_spawn_guard(arguments: &[String], started: Instant) {
    let Some(runtime) =
        argument_value("--runtime", arguments).and_then(|value| AgentRuntime::parse(&value))
    else {
        return;
    };
    if runtime.dialect().adapter().spawn_guard.is_none() {
        return;
    }
    // Another agent that runs Claude Code's hooks (Grok, OpenCode, Cursor)
    // speaks through its own hook, or Hide has not verified its contract.
    if hide_agent_hooks::runtime::ForeignOrigin::detect(|name| std::env::var_os(name)).is_some() {
        return;
    }
    if let Some(reason) = guard_refusal(runtime.dialect(), runtime.id(), started) {
        print_line(&hide_agent_hooks::spawn_guard::deny_output(&reason));
    }
}

/// The guard's decision for one tool call: the reason it is refused, or
/// `None` to let it run (not a launch, not in a registered checkout, the
/// daemon unreachable, any failure of its own). A refusal and an unreachable
/// daemon are written to the guard's log here; the caller prints the answer
/// in its runtime's form.
fn guard_refusal(dialect: HookDialect, runtime_id: &str, started: Instant) -> Option<String> {
    use hide_agent_hooks::spawn_guard::{self as guard, Registration};

    // Outside a Herdr pane there is nothing to redirect.
    let pane = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.is_empty())?;
    let (payload, truncated) = read_stdin_before_deadline(started + GUARD_PAYLOAD_BUDGET)?;
    // Question tools have no shell command and take their own readonly route.
    if let Some(question) = guard::may_hold_question(&payload)
        .then(|| guard::read_question(&payload, truncated, dialect))
        .flatten()
    {
        let home = home_directory()?;
        let program = workspace_context::cli_program()?;
        return match guard::question_decision(
            &program,
            runtime_id,
            &question.session,
            started + GUARD_BUDGET,
        ) {
            guard::QuestionDecision::Worker => {
                guard::record_question_refusal(&home, runtime_id, &pane, question.tool);
                Some(guard::QUESTION_REASON.to_owned())
            }
            guard::QuestionDecision::Allow => None,
            guard::QuestionDecision::Unreachable(cause) => {
                guard::unreachable(&home, runtime_id, cause);
                None
            }
        };
    }
    // Every shell call comes through here: this one search decides the rest.
    if !guard::may_hold_launch(&payload) {
        return None;
    }
    let call = guard::read_call(&payload, truncated, dialect)?;
    let launch = guard::find_launch(&call.command, &|name| std::env::var(name).ok())?;
    let home = home_directory()?;
    let program = workspace_context::cli_program()?;
    match guard::registration(&program, Instant::now() + GUARD_BUDGET) {
        Registration::Registered => {}
        Registration::NotRegistered => return None,
        Registration::Unreachable(cause) => {
            guard::unreachable(&home, runtime_id, cause);
            return None;
        }
    }
    let cwd = call
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let (repo, branch) = guard::checkout_facts(&cwd);
    let delegation = guard::spawn_command(&launch, repo.as_deref(), branch.as_deref(), true);
    let handoff = guard::spawn_command(&launch, repo.as_deref(), branch.as_deref(), false);
    // The refusal is logged before it is printed, and the refusal stands even
    // when the log cannot be written.
    guard::record_refusal(&home, runtime_id, &pane, &launch);
    Some(guard::refusal_reason(&delegation, &handoff))
}

fn run_prompt_hook(arguments: &[String], deadline: Instant) {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let mut command = Command::new(executable);
    command
        .arg("hook-inner")
        .args(&arguments[1..])
        .stdin(Stdio::inherit())
        // The inner flush must reach the runtime before it confirms intake.
        // Buffering and re-emitting here would acknowledge undelivered output.
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Ok(mut child) = OwnedChild::spawn_guarded(command, deadline) {
        let _ = child.capture_until(deadline, 64 * 1024);
    }
    // No filesystem or stream write follows the deadline. Failure keeps the
    // manual-inbox fallback; diagnostics run inside the supervised operation.
}

fn run_hook(arguments: &[String], started: Instant) {
    let Some(event) =
        argument_value("--event", arguments).and_then(|value| HookEvent::parse(&value))
    else {
        return;
    };
    let runtime =
        argument_value("--runtime", arguments).and_then(|value| AgentRuntime::parse(&value));
    let Some(home) = home_directory() else { return };
    let deadline = Instant::now() + Duration::from_millis(hide_memory::HOOK_PROCESS_BUDGET_MS);
    let delivery_deadline = started + INTAKE_BUDGET;
    let memory_injection = arguments
        .iter()
        .any(|argument| argument == "--memory-injection")
        && runtime.is_some_and(|runtime| runtime.dialect().adapter().memory.is_some());
    let prompt_hook =
        runtime.is_some_and(|runtime| runtime.dialect().adapter().prompt_hook.is_some());
    // The prompt event reads its payload for the bell test even without
    // Memory; a Memory read that follows works from the same bytes.
    let payload = if event == HookEvent::UserPromptSubmit && runtime.is_some() {
        read_stdin_before_deadline(started + PROMPT_PAYLOAD_BUDGET)
    } else if memory_injection {
        read_stdin_before_deadline(deadline)
    } else {
        None
    };
    let prompt = match (&payload, event) {
        (Some((bytes, truncated)), HookEvent::UserPromptSubmit) => {
            hide_agent_hooks::delivery::read_prompt(bytes, *truncated)
        }
        _ => hide_agent_hooks::delivery::Prompt {
            bell: false,
            session: None,
        },
    };
    let mut output = if let Some(runtime) = runtime.filter(|_| memory_injection) {
        memory_output_before_deadline(runtime, event, home.clone(), deadline, payload)
    } else {
        runtime.and_then(|runtime| hook_stdout(runtime, event))
    };
    // The probe is not gated on a pane id: hided binds a caller outside any
    // pane (a Codex shared daemon, a plain terminal) to the registered
    // checkout holding its cwd, and refuses everything else itself.
    if event == HookEvent::SessionStart
        && let Some(context) = workspace_context::live_context()
    {
        output = output.map(|value| {
            hide_agent_hooks::runtime::append_session_context(&value, &context).unwrap_or(value)
        });
    }
    // Claude Code's hook inside Grok or OpenCode leaves the letters where
    // they are: the session that runs it is not the pane's Claude Code.
    let takes_letters = runtime != Some(AgentRuntime::ClaudeCode)
        || hide_agent_hooks::runtime::takes_letters(|name| std::env::var_os(name));
    let intake = if event == HookEvent::UserPromptSubmit && prompt_hook && takes_letters {
        match hide_agent_hooks::delivery::pull(delivery_deadline, &prompt) {
            Ok(intake) => intake,
            Err(failure) => {
                hide_agent_hooks::delivery::diagnose_failure(&home, &failure);
                None
            }
        }
    } else {
        None
    };
    if let Some(intake) = &intake {
        output = match output {
            Some(existing) => {
                hide_agent_hooks::runtime::append_session_context(&existing, &intake.context)
            }
            None => runtime.and_then(|runtime| {
                hide_agent_hooks::runtime::hook_stdout_with_context(
                    runtime,
                    event,
                    Some(&intake.context),
                )
            }),
        };
    }
    let mut flushed = false;
    if let Some(output) = output {
        // PowerShell runs the hook on Windows and re-encodes what it prints.
        let output = if cfg!(windows) {
            hide_agent_hooks::runtime::ascii_json(&output)
        } else {
            output
        };
        let mut stdout = std::io::stdout().lock();
        flushed = writeln!(stdout, "{output}")
            .and_then(|_| stdout.flush())
            .is_ok();
    }
    if event == HookEvent::UserPromptSubmit {
        // A count-only answer has no letter to confirm.
        if let Some(intake) = intake.filter(|intake| !intake.ids.is_empty()) {
            if flushed {
                if let Err(failure) =
                    hide_agent_hooks::delivery::confirm(&intake, delivery_deadline)
                {
                    hide_agent_hooks::delivery::diagnose_failure(&home, &failure);
                }
            } else {
                hide_agent_hooks::delivery::diagnose(&home, "stdout");
            }
        }
        return;
    }
    if runtime.is_some_and(|runtime| runtime.dialect().adapter().subagent_counts.is_none()) {
        return;
    }
    report_count(&home, event, Change::of(event));
}

/// Applies `change` to this pane's count and reports the pane's totals to
/// Herdr. Outside a Herdr pane there is no pane to describe.
fn report_count(home: &std::path::Path, event: HookEvent, change: Change) {
    let Some(pane_id) = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    // `--source` on the command line is the install marker the diagnosis
    // reads out of the hook file; the report's own source is fixed in
    // `report::metadata_source`, because Herdr refuses the marker's `@`.
    let socket_path = report::socket_path(home);
    let outcome = match (counters::change(home, &pane_id, change), &socket_path) {
        (Ok(counters), Ok(path)) => report_latest(home, path, &pane_id, counters),
        (Err(error), _) => Err(hide_herdr_client::ApiError::Transport(format!(
            "the pane's count could not be changed: {error}"
        ))),
        (_, Err(error)) => Err(error.clone()),
    };
    // The outcome is written down rather than surfaced here: a hook's
    // stderr reaches nobody, and the record is what `doctor` and Settings
    // show (engineering rule 10).
    let socket_path = socket_path.unwrap_or_default();
    let _ = report::record_outcome(home, &pane_id, event, &socket_path, &outcome);
}

/// Reports `counters` and, when another event of the pane changed the record
/// while this report was on its way, the record as it is now: two events
/// that report in the other order than they counted would otherwise leave
/// Herdr with the older count until the pane's next event.
fn report_latest(
    home: &std::path::Path,
    socket_path: &std::path::Path,
    pane_id: &str,
    counters: counters::PaneCounters,
) -> Result<(), hide_herdr_client::ApiError> {
    report::report(socket_path, pane_id, counters)?;
    // Only a record actually read can be newer; one that cannot be read now
    // is left to the event that is writing it, which reports it itself.
    match counters::read_settled(home, pane_id) {
        Ok(now) if now != counters => report::report(socket_path, pane_id, now),
        _ => Ok(()),
    }
}

fn memory_output_before_deadline(
    runtime: AgentRuntime,
    event: HookEvent,
    home: PathBuf,
    deadline: Instant,
    payload: Option<(Vec<u8>, bool)>,
) -> Option<String> {
    let Some((payload, exceeded)) = payload else {
        return hook_stdout(runtime, event);
    };
    hide_agent_hooks::memory::project_memory_output_until(
        runtime, event, &payload, exceeded, &home, deadline,
    )
    .stdout
    .or_else(|| hook_stdout(runtime, event))
}

#[cfg(unix)]
#[allow(clippy::disallowed_methods)] // a production wait, not test code
fn read_stdin_before_deadline(deadline: Instant) -> Option<(Vec<u8>, bool)> {
    use std::os::fd::AsRawFd;

    let stdin = std::io::stdin();
    let input = stdin.lock();
    let descriptor_number = input.as_raw_fd();
    // SAFETY: F_GETFL does not dereference a pointer and the descriptor is
    // borrowed from the live stdin lock.
    let flags = unsafe { libc::fcntl(descriptor_number, libc::F_GETFL) };
    if flags < 0 {
        return Some((Vec::new(), false));
    }
    // SAFETY: F_SETFL changes only this process's descriptor flags. The hook
    // owns stdin for its whole short lifetime, so no other reader observes it.
    if unsafe { libc::fcntl(descriptor_number, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Some((Vec::new(), false));
    }
    let mut retained = Vec::with_capacity(hide_memory::HOOK_INPUT_LIMIT_BYTES.min(16 * 1024));
    let mut buffer = [0_u8; 8192];
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        // SAFETY: `buffer` is a writable byte array and `descriptor_number`
        // is the live stdin descriptor held by `input`.
        let read = unsafe {
            libc::read(
                descriptor_number,
                buffer.as_mut_ptr().cast::<libc::c_void>(),
                buffer.len(),
            )
        };
        if read < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                std::thread::sleep(remaining.min(Duration::from_millis(2)));
                continue;
            }
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Some((Vec::new(), false));
        }
        let read = read as usize;
        if read == 0 {
            return Some((retained, false));
        }
        let remaining_capacity = hide_memory::HOOK_INPUT_LIMIT_BYTES.saturating_sub(retained.len());
        let keep = remaining_capacity.min(read);
        retained.extend_from_slice(&buffer[..keep]);
        if keep < read {
            return Some((retained, true));
        }
    }
}

/// Windows has no nonblocking read of an anonymous pipe, so a thread reads
/// and this waits for it until the deadline; a producer that never closes
/// stdin leaves that thread blocked, and the process exits without it.
#[cfg(not(unix))]
fn read_stdin_before_deadline(deadline: Instant) -> Option<(Vec<u8>, bool)> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(hide_agent_hooks::memory::read_bounded(std::io::stdin()));
    });
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Ok(read)) => Some(read),
        Ok(Err(_)) => Some((Vec::new(), false)),
        Err(_) => None,
    }
}

fn run_doctor(arguments: &[String]) -> Result<String, String> {
    let home = home_directory().ok_or_else(|| "HOME is not set".to_owned())?;
    let diagnosis = Diagnosis::read(&home);
    if arguments.iter().any(|argument| argument == "--json") {
        serde_json::to_string_pretty(&diagnosis)
            .map_err(|error| format!("diagnosis could not be encoded: {error}"))
    } else {
        Ok(diagnosis.render())
    }
}

fn home_directory() -> Option<PathBuf> {
    hide_platform::host::home_dir().ok()
}

fn argument_value(flag: &str, arguments: &[String]) -> Option<String> {
    let index = arguments.iter().position(|argument| argument == flag)?;
    arguments.get(index + 1).cloned()
}
