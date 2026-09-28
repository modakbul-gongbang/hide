//! Starting an agent in a pane, the one way every Hide start reaches Herdr.
//!
//! The pinned Herdr's `agent.start` types the agent's command into the pane
//! only while the pane's shell alone holds the terminal: the foreground
//! process group is the shell's own and has no other member (v0.9.1
//! `available_pane_shell_from_job`). It does not wait for that state, it
//! refuses with `agent_pane_busy`, and no event announces the moment the
//! state is reached. A pane Hide has just made, a new worktree's, a fork's
//! split or a restored tab, is still running its shell's startup files then,
//! so a start reads that state through `pane.process_info` and sends
//! `agent.start` only once the shell holds the terminal. The wait is bounded;
//! a shell that never reaches its prompt is a start that did not happen, with
//! nothing typed into the pane.
//!
//! Every call here runs on a worker thread, never under `Mutex<Runtime>`.

use std::thread;
use std::time::{Duration, Instant};

use hide_herdr_client::{
    ApiConnector, ApiError, request_with_connector, request_with_correlation_id,
};
use serde_json::Value;

use crate::wire;

/// How long a pane's shell gets to reach its prompt: the pinned Herdr's own
/// default wait for an agent's readiness.
pub(crate) const SHELL_WAIT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const PANE_BUSY: &str = "agent_pane_busy";

/// Why a start did not settle with Herdr's answer to `agent.start`.
#[derive(Debug)]
pub(crate) enum StartError {
    /// Nothing was typed into the pane: its shell never held the terminal in
    /// time, or its state could not be read.
    NotStarted(String),
    /// Herdr's own answer to `agent.start`: a refusal, or no answer at all.
    Herdr(ApiError),
}

/// Sends `agent.start` with `params` for `pane_id` once the pane's shell
/// holds its terminal, and returns Herdr's answer.
pub(crate) fn start_at_shell(
    connector: &dyn ApiConnector,
    correlation_id: &str,
    pane_id: &str,
    params: Value,
    answer_timeout: Duration,
) -> Result<Value, StartError> {
    start_within(
        connector,
        correlation_id,
        pane_id,
        params,
        answer_timeout,
        SHELL_WAIT,
    )
}

fn start_within(
    connector: &dyn ApiConnector,
    correlation_id: &str,
    pane_id: &str,
    params: Value,
    answer_timeout: Duration,
    wait: Duration,
) -> Result<Value, StartError> {
    let started = Instant::now();
    let deadline = started + wait;
    loop {
        wait_for_shell(connector, correlation_id, pane_id, started, deadline)?;
        match request_with_correlation_id(
            connector,
            correlation_id,
            "agent.start",
            params.clone(),
            answer_timeout,
        ) {
            // A prompt hook can take the terminal between the read and the
            // start. Herdr refused before typing anything, so the same start
            // is sent again once the shell holds the terminal, inside the
            // same deadline.
            Err(ApiError::Remote { code, .. })
                if code == PANE_BUSY && Instant::now() < deadline => {}
            answer => return answer.map_err(StartError::Herdr),
        }
    }
}

fn wait_for_shell(
    connector: &dyn ApiConnector,
    correlation_id: &str,
    pane_id: &str,
    started: Instant,
    deadline: Instant,
) -> Result<(), StartError> {
    let params = wire::pane_process_info_params(pane_id).map_err(StartError::NotStarted)?;
    loop {
        let group = request_with_connector(
            connector,
            "pane.process_info",
            params.clone(),
            READ_TIMEOUT,
        )
        .map_err(|error| format!("pane.process_info failed: {error}"))
        .and_then(wire::pane_process_group)
        .map_err(|message| {
            StartError::NotStarted(format!(
                "The pane's shell could not be read, so the agent was not started: {message}"
            ))
        })?;
        if shell_holds_terminal(&group) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            crate::diagnostic!(serde_json::json!({
                "component": "agent_start",
                "kind": "shell_wait.timeout",
                "request": correlation_id,
                "pane_id": pane_id,
                "waited_ms": started.elapsed().as_millis() as u64,
                "shell_pid": group.shell_pid,
                "foreground_process_group_id": group.foreground_process_group_id,
                "foreground_pids": group.foreground_pids,
            }));
            return Err(StartError::NotStarted(format!(
                "The pane's shell did not reach its prompt within {} s, so the agent was not started.",
                SHELL_WAIT.as_secs()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Herdr's condition for `agent.start`: the foreground group is the shell's
/// own and holds nothing but the shell.
fn shell_holds_terminal(group: &wire::PaneProcessGroup) -> bool {
    match group.shell_pid {
        Some(shell) if shell > 1 => {
            group.foreground_process_group_id == Some(shell)
                && group.foreground_pids.iter().all(|pid| *pid == shell)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::{Value, json};

    use super::*;
    use crate::fake_herdr::FakeHerdr;

    const SHELL: u32 = 4100;

    fn process_info(foreground: u32, pids: &[u32]) -> Value {
        let processes: Vec<Value> = pids
            .iter()
            .map(|pid| json!({"pid": pid, "name": if *pid == SHELL { "zsh" } else { "direnv" }}))
            .collect();
        json!({"type": "pane_process_info", "process_info": {
            "pane_id": "w1:p1", "shell_pid": SHELL, "foreground_process_group_id": foreground,
            "foreground_processes": processes
        }})
    }

    fn started() -> Value {
        json!({"type": "agent_started", "argv": [], "agent": {
            "pane_id": "w1:p1", "terminal_id": "term_1", "workspace_id": "w1",
            "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1
        }})
    }

    fn start(herdr: &FakeHerdr, wait: Duration) -> Result<Value, StartError> {
        start_within(
            &herdr.connector(),
            "test:start",
            "w1:p1",
            json!({"pane_id": "w1:p1"}),
            Duration::from_secs(5),
            wait,
        )
    }

    #[test]
    fn the_start_waits_until_the_shell_alone_holds_the_terminal() {
        let reads = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&reads);
        let herdr = FakeHerdr::start("agent-start-wait", move |method, _| match method {
            // Startup files first run a command in their own group, then one
            // in the shell's group, and only then leave the shell alone.
            "pane.process_info" => match seen.fetch_add(1, Ordering::SeqCst) {
                0 => process_info(4200, &[4200]),
                1 => process_info(SHELL, &[SHELL, 4201]),
                _ => process_info(SHELL, &[SHELL]),
            },
            "agent.start" => started(),
            other => panic!("unexpected {other}"),
        });
        assert!(start(&herdr, Duration::from_secs(5)).is_ok());
        assert_eq!(
            herdr.methods(),
            [
                "pane.process_info",
                "pane.process_info",
                "pane.process_info",
                "agent.start"
            ]
        );
    }

    #[test]
    fn a_start_refused_as_busy_waits_for_the_shell_and_is_sent_again() {
        let starts = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&starts);
        let herdr =
            FakeHerdr::start_with_errors("agent-start-busy", move |method, _| match method {
                "pane.process_info" => Ok(process_info(SHELL, &[SHELL])),
                "agent.start" if seen.fetch_add(1, Ordering::SeqCst) == 0 => Err((
                    PANE_BUSY.into(),
                    "agent target pane w1:p1 is not an available shell".into(),
                )),
                "agent.start" => Ok(started()),
                other => panic!("unexpected {other}"),
            });
        assert!(start(&herdr, Duration::from_secs(5)).is_ok());
        assert_eq!(
            herdr.methods(),
            [
                "pane.process_info",
                "agent.start",
                "pane.process_info",
                "agent.start"
            ]
        );
    }

    #[test]
    fn a_shell_that_never_reaches_its_prompt_types_nothing_and_says_so() {
        let herdr = FakeHerdr::start("agent-start-never", |method, _| match method {
            "pane.process_info" => process_info(4200, &[4200]),
            other => panic!("unexpected {other}"),
        });
        let Err(StartError::NotStarted(message)) = start(&herdr, Duration::from_millis(300)) else {
            panic!("a shell that never holds the terminal is not a start");
        };
        assert!(message.contains("did not reach its prompt"), "{message}");
        assert!(!herdr.methods().iter().any(|method| method == "agent.start"));
    }

    #[test]
    fn another_refusal_is_herdrs_answer_at_once() {
        let herdr = FakeHerdr::start_with_errors("agent-start-refused", |method, _| match method {
            "pane.process_info" => Ok(process_info(SHELL, &[SHELL])),
            "agent.start" => Err((
                "agent_name_taken".into(),
                "agent name one is already used".into(),
            )),
            other => panic!("unexpected {other}"),
        });
        let Err(StartError::Herdr(ApiError::Remote { code, .. })) =
            start(&herdr, Duration::from_secs(5))
        else {
            panic!("a refusal other than a busy pane is not retried");
        };
        assert_eq!(code, "agent_name_taken");
        assert_eq!(herdr.methods(), ["pane.process_info", "agent.start"]);
    }
}
