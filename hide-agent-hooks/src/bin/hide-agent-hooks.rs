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

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use hide_agent_hooks::counters;
use hide_agent_hooks::diagnosis::Diagnosis;
use hide_agent_hooks::report;
use hide_agent_hooks::runtime::{AgentRuntime, HookEvent, hook_stdout};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("hook") => {
            run_hook(&arguments);
            // A hook that could not report is not a failed turn. Its visible
            // outcome is the pane reading as uninstrumented (PRD B32).
            ExitCode::SUCCESS
        }
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
     [--source <install marker>]\n       hide-agent-hooks doctor [--json]"
        .to_owned()
}

fn run_hook(arguments: &[String]) {
    let Some(event) =
        argument_value("--event", arguments).and_then(|value| HookEvent::parse(&value))
    else {
        return;
    };
    let runtime =
        argument_value("--runtime", arguments).and_then(|value| AgentRuntime::parse(&value));
    let Some(home) = home_directory() else { return };
    let deadline = Instant::now() + Duration::from_millis(hide_memory::HOOK_DEADLINE_MS);
    if let Some(runtime) = runtime {
        if let Some(output) = memory_output_before_deadline(runtime, event, home.clone(), deadline)
        {
            println!("{output}");
        }
    } else if let Some(output) = runtime.and_then(|runtime| hook_stdout(runtime, event)) {
        println!("{output}");
    }
    if event == HookEvent::UserPromptSubmit {
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
    let outcome = report::report(&socket_path, &pane_id, counters);
    // The outcome is written down rather than surfaced here: a hook's
    // stderr reaches nobody, and the record is what `doctor` and Settings
    // show (engineering rule 10).
    let _ = report::record_outcome(&home, &pane_id, event, &socket_path, &outcome);
}

fn memory_output_before_deadline(
    runtime: AgentRuntime,
    event: HookEvent,
    home: PathBuf,
    deadline: Instant,
) -> Option<String> {
    let Some((payload, exceeded)) = read_stdin_before_deadline(deadline) else {
        return hook_stdout(runtime, event);
    };
    hide_agent_hooks::memory::project_memory_output_until(
        runtime, event, &payload, exceeded, &home, deadline,
    )
    .stdout
    .or_else(|| hook_stdout(runtime, event))
}

#[cfg(unix)]
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
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return None;
        };
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

#[cfg(not(unix))]
fn read_stdin_before_deadline(_deadline: Instant) -> Option<(Vec<u8>, bool)> {
    hide_agent_hooks::memory::read_bounded(std::io::stdin()).ok()
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
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

fn argument_value(flag: &str, arguments: &[String]) -> Option<String> {
    let index = arguments.iter().position(|argument| argument == flag)?;
    arguments.get(index + 1).cloned()
}
