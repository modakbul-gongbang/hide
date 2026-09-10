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

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use hide_agent_hooks::counters;
use hide_agent_hooks::diagnosis::Diagnosis;
use hide_agent_hooks::report;
use hide_agent_hooks::runtime::{HookEvent, hook_source_id};

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
    "usage: hide-agent-hooks hook --event <SessionStart|SubagentStart|SubagentStop|Stop> \
     [--source <id>]\n       hide-agent-hooks doctor [--json]"
        .to_owned()
}

fn run_hook(arguments: &[String]) {
    // The runtimes deliver the event payload on stdin. Nothing here needs it,
    // but leaving it unread makes the agent's write block once the pipe
    // fills, so it is drained and discarded.
    let mut discarded = Vec::new();
    let _ = std::io::stdin().take(1 << 16).read_to_end(&mut discarded);

    let Some(event) =
        argument_value("--event", arguments).and_then(|value| HookEvent::parse(&value))
    else {
        return;
    };
    let Some(pane_id) = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        // Not inside a Herdr pane: there is no pane to describe.
        return;
    };
    let Some(home) = home_directory() else {
        return;
    };
    let Ok(counters) = counters::apply(&home, &pane_id, event) else {
        return;
    };
    let source = argument_value("--source", arguments).unwrap_or_else(hook_source_id);
    let _ = report::report(&pane_id, &source, counters);
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
