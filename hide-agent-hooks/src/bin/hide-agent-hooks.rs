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

use hide_agent_hooks::counters;
use hide_agent_hooks::diagnosis::Diagnosis;
use hide_agent_hooks::guidance::{self, GuidanceAgent};
use hide_agent_hooks::report;
use hide_agent_hooks::runtime::{AgentRuntime, HookEvent, hook_stdout};
use hide_platform::process::{OwnedChild, OwnerWatch};

const PROMPT_BUDGET: Duration = Duration::from_millis(1_850);
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
    let owner_watch = match OwnerWatch::from_launch() {
        Ok(watch) => watch,
        Err(_) => return ExitCode::from(2),
    };
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("hook") => {
            // An agent beyond Claude Code and Codex has no counters, Memory
            // or letters: its hook prints the session guidance and nothing
            // else (`hide_agent_hooks::guidance`).
            if let Some(agent) = argument_value("--runtime", &arguments)
                .and_then(|value| GuidanceAgent::from_id(&value))
            {
                run_guidance_hook(agent, &arguments);
                return ExitCode::SUCCESS;
            }
            // Cursor loads Claude Code's hooks from `~/.claude/settings.json`
            // beside its own and runs both, so under Cursor Claude Code's hook
            // stays out and Cursor's own guidance hook is the one that speaks
            // (`docs/agent-hooks.md`, Other agents).
            if argument_value("--runtime", &arguments).as_deref() == Some("claude-code")
                && hide_agent_hooks::runtime::run_by_cursor(|name| std::env::var_os(name))
            {
                return ExitCode::SUCCESS;
            }
            if argument_value("--event", &arguments).as_deref()
                == Some(HookEvent::UserPromptSubmit.name())
            {
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
     --event <SessionStart|UserPromptSubmit|SubagentStart|SubagentStop|Stop> \
     [--memory-injection] [--source <install marker>]\n       \
     hide-agent-hooks hook --runtime <gemini-cli|qwen-code|factory-droid|copilot-cli|kiro|cursor|augment|junie> \
     --event SessionStart [--source <install marker>]\n       hide-agent-hooks doctor [--json]"
        .to_owned()
}

/// The guidance hook of an agent that has no other Hide hook: one line of
/// stdout in the field that agent reads, and nothing else. It never fails
/// loudly, for the reason every hook does not.
fn run_guidance_hook(agent: GuidanceAgent, arguments: &[String]) {
    if argument_value("--event", arguments).as_deref() != Some("SessionStart") {
        return;
    }
    let context = guidance::session_context(workspace_context::live_context().as_deref());
    let output = guidance::stdout(agent, &context);
    // PowerShell re-encodes what a Windows hook prints; ASCII survives it.
    let output = if cfg!(windows) && output.starts_with('{') {
        hide_agent_hooks::runtime::ascii_json(&output)
    } else {
        output
    };
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{output}").and_then(|_| stdout.flush());
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
        .any(|argument| argument == "--memory-injection");
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
    let intake = if event == HookEvent::UserPromptSubmit && runtime.is_some() {
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
    let Some(pane_id) = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        // Not inside a Herdr pane: there is no pane to describe.
        return;
    };
    let Ok(counters) = counters::apply(&home, &pane_id, event) else {
        return;
    };
    // `--source` on the command line is the install marker the diagnosis
    // reads out of the hook file; the report's own source is fixed in
    // `report::metadata_source`, because Herdr refuses the marker's `@`.
    let socket_path = report::socket_path(&home);
    let outcome = match &socket_path {
        Ok(path) => report::report(path, &pane_id, counters),
        Err(error) => Err(error.clone()),
    };
    // The outcome is written down rather than surfaced here: a hook's
    // stderr reaches nobody, and the record is what `doctor` and Settings
    // show (engineering rule 10).
    let socket_path = socket_path.unwrap_or_default();
    let _ = report::record_outcome(&home, &pane_id, event, &socket_path, &outcome);
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
