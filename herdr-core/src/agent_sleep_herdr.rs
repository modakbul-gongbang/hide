//! The one place agent sleep relies on Herdr and on the operating system
//! (PRD agent-sleep D-20).
//!
//! Herdr has no hibernate: the pinned release ends nothing on request and
//! forgets an agent whose process ended. So sleeping is Hide ending the
//! pane's foreground process group with SIGTERM, which returns the pane to
//! its shell, and waking is Herdr's own `agent.start` in that same pane with
//! the provider's resume arguments. When Herdr offers a hibernate or an
//! exit mark of its own, this file is what changes; the decision, the
//! records and the screens in `agent_sleep.rs` and the runtime stay.
//!
//! Every call here runs on a worker thread, never under `Mutex<Runtime>`.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use hide_herdr_client::{ApiConnector, ApiError, request_with_connector};
use hide_platform::process;

use crate::agent_sleep::WakeMode;
use crate::agent_start::StartError;
use crate::live::LiveContext;
use crate::wire;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an ended agent has to hand the terminal back to its shell before
/// the sleep is given up and the agent is left awake (B8).
const END_TIMEOUT: Duration = Duration::from_secs(10);
const END_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// `agent.start` waits this long for the agent to be ready; the read waits a
/// little longer so a slow start is not reported as an unanswered one.
const AGENT_START_TIMEOUT_MS: u64 = 120_000;

/// Why an agent was not ended. `Busy` is the agent having started working, or
/// waiting for the operator, since the decision was made: nothing was
/// signalled, and a Reopen says so as such. Everything else is `Failed`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EndError {
    Busy(String),
    Failed(String),
}

impl std::fmt::Display for EndError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy(message) | Self::Failed(message) => formatter.write_str(message),
        }
    }
}

/// Ends the agent in `pane_id` and returns the state change sequence Herdr
/// reported for it just before, which is how a later listing of the same
/// agent is told from a new one.
///
/// It reads the agent again first, because the decision was made on a list
/// up to a second old and a parent's prompt may have set it working since.
/// The signal goes to the pane's foreground process group only, and only
/// when that group is not the shell's: the agent and what it started in its
/// own group (MCP servers, tool processes) end, the shell and every other
/// pane stay (B7).
#[allow(clippy::disallowed_methods)] // a production wait, not test code
pub(crate) fn end_agent(
    connector: &dyn ApiConnector,
    pane_id: &str,
    kind: &str,
) -> Result<u64, EndError> {
    let state = request_with_connector(
        connector,
        "agent.get",
        wire::agent_target_params(pane_id).map_err(EndError::Failed)?,
        REQUEST_TIMEOUT,
    )
    .map_err(|error| format!("agent.get failed: {error}"))
    .and_then(wire::agent_state)
    .map_err(EndError::Failed)?;
    if !matches!(state.status.as_str(), "idle" | "done") {
        return Err(EndError::Busy(format!("the agent is {} now", state.status)));
    }
    if !state
        .agent
        .as_deref()
        .is_some_and(|agent| agent.eq_ignore_ascii_case(kind))
    {
        return Err(EndError::Failed(format!(
            "the pane now runs {} rather than {kind}",
            state.agent.as_deref().unwrap_or("no agent")
        )));
    }
    let group = process_group(connector, pane_id).map_err(EndError::Failed)?;
    let (Some(shell), Some(foreground)) = (group.shell_pid, group.foreground_process_group_id)
    else {
        return Err(EndError::Failed(
            "Herdr reported no shell or foreground process group for the pane".into(),
        ));
    };
    // kill(-0) signals Hide's own group and kill(-1) every process the user
    // owns, so a group id that is not a real group never reaches the signal.
    if foreground <= 1 || shell <= 1 {
        return Err(EndError::Failed(format!(
            "Herdr reported process group {foreground} and shell {shell}, which are not a pane's"
        )));
    }
    if foreground == shell {
        return Err(EndError::Failed(
            "the pane's shell already holds the terminal".into(),
        ));
    }
    // The group Herdr reported as the pane's foreground gets SIGTERM.
    process::terminate_group(foreground).map_err(|error| {
        EndError::Failed(format!("ending process group {foreground} failed: {error}"))
    })?;
    let deadline = Instant::now() + END_TIMEOUT;
    loop {
        thread::sleep(END_POLL_INTERVAL);
        let group = process_group(connector, pane_id).map_err(EndError::Failed)?;
        if group.foreground_process_group_id == Some(shell) {
            return Ok(state.state_change_seq);
        }
        if Instant::now() >= deadline {
            return Err(EndError::Failed(format!(
                "the agent did not hand the terminal back to its shell within {} s",
                END_TIMEOUT.as_secs()
            )));
        }
    }
}

fn process_group(
    connector: &dyn ApiConnector,
    pane_id: &str,
) -> Result<wire::PaneProcessGroup, String> {
    request_with_connector(
        connector,
        "pane.process_info",
        wire::pane_process_info_params(pane_id)?,
        REQUEST_TIMEOUT,
    )
    .map_err(|error| format!("pane.process_info failed: {error}"))
    .and_then(wire::pane_process_group)
}

/// What a wake needs, gathered under the lock before the worker starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WakeRequest {
    pub(crate) pane_id: String,
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) mode: WakeMode,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) codex_daemon: crate::codex_launch::CodexDaemon,
}

/// How a wake ended. `reason` is what the pane says (B14); `detail` is what
/// only the diagnostic carries, Herdr's own code and message among it (B19).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WakeOutcome {
    Started,
    Failed { reason: String, detail: String },
}

/// Starts the agent again in the same pane, through Herdr's `agent.start`
/// once the pane's shell holds its terminal (`agent_start`); Herdr answers
/// once the agent is ready.
pub(crate) fn start_agent(connector: &dyn ApiConnector, request: &WakeRequest) -> WakeOutcome {
    if let Some(cwd) = request.cwd.as_deref()
        && !Path::new(cwd).is_dir()
    {
        return WakeOutcome::Failed {
            reason: format!("The working folder {cwd} no longer exists."),
            detail: format!("{cwd} is not a directory"),
        };
    }
    let params = match wire::agent_start_params(
        &request.pane_id,
        &request.name,
        &request.kind,
        request.args.clone(),
        request.codex_daemon,
    ) {
        Ok(params) => params,
        Err(detail) => {
            return WakeOutcome::Failed {
                reason: detail.clone(),
                detail,
            };
        }
    };
    match crate::agent_start::start_at_shell_reusing_name(
        connector,
        &format!("herdr-core:agent-sleep:{}:wake", request.pane_id),
        &request.pane_id,
        params,
        Duration::from_millis(AGENT_START_TIMEOUT_MS + 5_000),
    ) {
        Ok(value) => match wire::started_agent(value) {
            Ok(_) => WakeOutcome::Started,
            Err(detail) => WakeOutcome::Failed {
                reason: "The agent did not become ready in time.".to_owned(),
                detail,
            },
        },
        Err(StartError::NotStarted(detail)) => WakeOutcome::Failed {
            reason: refused_reason(request.mode).to_owned(),
            detail,
        },
        Err(StartError::Herdr(ApiError::Remote { code, message })) => WakeOutcome::Failed {
            reason: refused_reason(request.mode).to_owned(),
            detail: format!("{code}: {message}"),
        },
        Err(StartError::Herdr(error)) => WakeOutcome::Failed {
            reason: "The agent did not become ready in time.".to_owned(),
            detail: error.to_string(),
        },
    }
}

fn refused_reason(mode: WakeMode) -> &'static str {
    match mode {
        WakeMode::Resume => "The conversation couldn\u{2019}t be resumed.",
        WakeMode::Fresh => "A new session couldn\u{2019}t be started.",
    }
}

pub(crate) fn spawn_end(context: LiveContext, pane_id: String, kind: String) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-agent-sleep".into())
        .spawn(move || {
            let result = end_agent(context.api_connector.as_ref(), &pane_id, &kind)
                .map_err(|error| error.to_string());
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_agent_sleep_end(&pane_id, result),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("agent sleep worker could not be started: {error}"))
}

pub(crate) fn spawn_wake(context: LiveContext, request: WakeRequest) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-agent-wake".into())
        .spawn(move || {
            let outcome = start_agent(context.api_connector.as_ref(), &request);
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = match runtime.lock() {
                Ok(mut guard) => guard.ingest_agent_wake(&request.pane_id, request.mode, outcome),
                Err(_) => return,
            };
            drop(runtime);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("agent wake worker could not be started: {error}"))
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use serde_json::{Value, json};

    use super::*;
    use crate::fake_herdr::FakeHerdr;

    fn agent_info(status: &str, seq: u64) -> Value {
        json!({"type": "agent_info", "agent": {
            "pane_id": "w1:p1", "tab_id": "w1:t1", "workspace_id": "w1", "terminal_id": "term_1",
            "agent": "claude", "agent_status": status, "state_change_seq": seq,
            "focused": false, "interactive_ready": true, "revision": 0
        }})
    }

    fn process_info(shell: u32, foreground: u32) -> Value {
        json!({"type": "pane_process_info", "process_info": {
            "pane_id": "w1:p1", "shell_pid": shell, "foreground_process_group_id": foreground,
            "foreground_processes": []
        }})
    }

    #[test]
    fn ending_signals_the_foreground_group_and_waits_for_the_shell() {
        let mut child =
            process::OwnedChild::spawn(Command::new("/bin/sleep").arg("60")).expect("sleep starts");
        let pid = child.id();
        let exited = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&exited);
        let herdr = FakeHerdr::start("agent-sleep-end", move |method, _| match method {
            "agent.get" => agent_info("idle", 7),
            "pane.process_info" => {
                // The shell (this test process stands in for it) takes the
                // terminal back only once the agent's group is really gone.
                let shell = std::process::id();
                let gone = !process::is_alive(pid) || seen.load(Ordering::SeqCst);
                process_info(shell, if gone { shell } else { pid })
            }
            other => panic!("unexpected {other}"),
        });
        let reaper = thread::spawn(move || {
            let _ = child.wait();
            exited.store(true, Ordering::SeqCst);
        });
        assert_eq!(end_agent(&herdr.connector(), "w1:p1", "claude"), Ok(7));
        reaper.join().expect("reaped");
        assert!(!process::is_alive(pid));
    }

    #[test]
    fn a_pane_whose_shell_holds_the_terminal_is_left_alone() {
        let herdr = FakeHerdr::start("agent-sleep-shell", |method, _| match method {
            "agent.get" => agent_info("idle", 3),
            "pane.process_info" => process_info(42, 42),
            other => panic!("unexpected {other}"),
        });
        let error = end_agent(&herdr.connector(), "w1:p1", "claude")
            .expect_err("nothing to end")
            .to_string();
        assert!(error.contains("shell already holds"), "{error}");
        assert_eq!(herdr.methods(), ["agent.get", "pane.process_info"]);
    }

    #[test]
    fn a_group_that_would_signal_every_process_is_refused() {
        for (shell, foreground) in [(42, 0), (42, 1), (0, 42), (1, 42)] {
            let herdr = FakeHerdr::start("agent-sleep-broadcast", move |method, _| match method {
                "agent.get" => agent_info("idle", 3),
                "pane.process_info" => process_info(shell, foreground),
                other => panic!("unexpected {other}"),
            });
            let error = end_agent(&herdr.connector(), "w1:p1", "claude")
                .expect_err("refused")
                .to_string();
            assert!(error.contains("not a pane's"), "{error}");
            assert_eq!(herdr.methods(), ["agent.get", "pane.process_info"]);
        }
    }

    #[test]
    fn an_agent_that_started_working_is_not_ended() {
        let herdr = FakeHerdr::start("agent-sleep-working", |method, _| match method {
            "agent.get" => agent_info("working", 3),
            other => panic!("unexpected {other}"),
        });
        let error = end_agent(&herdr.connector(), "w1:p1", "claude").expect_err("working");
        assert!(matches!(error, EndError::Busy(ref message) if message.contains("working")));
        assert_eq!(herdr.methods(), ["agent.get"]);
    }

    #[test]
    fn an_unknown_codex_wake_keeps_the_next_action_and_sends_no_start() {
        let herdr = FakeHerdr::start("codex-wake-capability", |method, _| {
            panic!("unexpected {method}")
        });
        let outcome = start_agent(
            &herdr.connector(),
            &WakeRequest {
                codex_daemon: crate::codex_launch::CodexDaemon::Unknown,
                pane_id: "w1:p1".into(),
                kind: "codex".into(),
                name: "one".into(),
                mode: WakeMode::Resume,
                args: vec!["resume".into(), "abc".into()],
                cwd: None,
            },
        );
        let WakeOutcome::Failed { reason, .. } = outcome else {
            panic!("wake started")
        };
        assert!(reason.contains("Settings"), "{reason}");
        assert!(herdr.methods().is_empty());
    }

    #[test]
    fn a_refused_wake_names_a_plain_reason_and_keeps_herdrs_words_for_the_log() {
        let herdr = FakeHerdr::start_with_errors("agent-wake-refused", |method, _| match method {
            "pane.process_info" => Ok(process_info(42, 42)),
            "agent.start" => Err((
                "agent_name_taken".into(),
                "agent name one is already used".into(),
            )),
            other => panic!("unexpected {other}"),
        });
        let outcome = start_agent(
            &herdr.connector(),
            &WakeRequest {
                codex_daemon: Default::default(),
                pane_id: "w1:p1".into(),
                kind: "claude".into(),
                name: "one".into(),
                mode: WakeMode::Resume,
                args: vec!["--resume".into(), "abc".into()],
                cwd: None,
            },
        );
        assert_eq!(
            outcome,
            WakeOutcome::Failed {
                reason: "The conversation couldn\u{2019}t be resumed.".into(),
                detail: "agent_name_taken: agent name one is already used".into(),
            }
        );
    }

    #[test]
    fn a_wake_into_a_folder_that_is_gone_asks_herdr_nothing() {
        let herdr = FakeHerdr::start("agent-wake-cwd", |method, _| panic!("unexpected {method}"));
        let outcome = start_agent(
            &herdr.connector(),
            &WakeRequest {
                codex_daemon: Default::default(),
                pane_id: "w1:p1".into(),
                kind: "codex".into(),
                name: "wake-w1-p1".into(),
                mode: WakeMode::Resume,
                args: Vec::new(),
                cwd: Some("/nonexistent/agent-sleep-test".into()),
            },
        );
        assert!(matches!(
            outcome,
            WakeOutcome::Failed { ref reason, .. } if reason == "The working folder /nonexistent/agent-sleep-test no longer exists."
        ));
        assert!(herdr.methods().is_empty());
    }
}
