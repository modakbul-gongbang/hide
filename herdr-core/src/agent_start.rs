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
//! The shell alone holds the terminal between its startup files' commands
//! too, before its line editor runs, and until then the terminal is in
//! canonical mode: the kernel keeps an unfinished typed line and drops what
//! passes its limit (1,024 bytes on macOS), Enter included, so a long start
//! line (a first prompt) would be cut and never run. A start whose line could
//! be cut therefore also waits, inside the same bound, until the pane's node
//! reads the shell's terminal taking keys rather than lines (`line_input`),
//! which is the line editor reading. A line shorter than every supported
//! terminal's limit reaches the shell whole either way and asks no node.
//!
//! A start that puts an agent back under the name of the one just ended in
//! the same pane (a wake, a Reopen) also meets `agent_name_taken` until
//! Herdr has forgotten the ended agent; [`start_at_shell_reusing_name`] sends
//! that start again on exactly that refusal, inside a bounded wait. The other
//! starts take that refusal as the answer, since their name is someone else's.
//!
//! Every call here runs on a worker thread, never under `Mutex<Runtime>`.

use std::thread;
use std::time::{Duration, Instant};

use hide_herdr_client::{
    ApiConnector, ApiError, request_with_connector, request_with_correlation_id,
};
use hide_node_link::process::LineInput;
use hide_node_link::protocol::Call;
use serde_json::Value;

use crate::node_access::NodeLink;
use crate::wire;

/// How long a pane's shell gets to reach its prompt: the pinned Herdr's own
/// default wait for an agent's readiness.
pub(crate) const SHELL_WAIT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const PANE_BUSY: &str = "agent_pane_busy";
const NAME_TAKEN: &str = "agent_name_taken";
/// The fewest bytes of an unfinished line a supported system's terminal
/// keeps before the shell reads it: macOS's `MAX_INPUT` (Linux keeps 4,096,
/// and a Windows console cuts nothing). A start line shorter than this
/// reaches the shell whole once the shell holds the terminal.
const SHORTEST_LINE_LIMIT: usize = 1024;
/// How long Herdr gets to let go of the name of an agent that was just
/// ended: it forgets the agent a moment after its process is gone, and
/// refuses `agent.start` under that name until it has.
pub(crate) const NAME_RELEASE_WAIT: Duration = Duration::from_secs(10);

/// Why a start did not settle with Herdr's answer to `agent.start`.
#[derive(Debug)]
pub(crate) enum StartError {
    /// Nothing was typed into the pane: its shell never held the terminal in
    /// time, or its state could not be read.
    NotStarted(String),
    /// Herdr's own answer to `agent.start`: a refusal, or no answer at all.
    Herdr(ApiError),
}

/// Action proof after shell readiness, repeated before every actual start
/// attempt. No proof is cached across startup waits or a busy-pane refusal.
/// `node` is the node that runs the pane, asked how its shell's terminal
/// takes input when the start line could be cut; none when that node cannot
/// be reached, which only a start with such a line minds.
pub(crate) fn start_at_shell_checked(
    connector: &dyn ApiConnector,
    node: Option<&dyn NodeLink>,
    correlation_id: &str,
    pane_id: &str,
    params: Value,
    answer_timeout: Duration,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Value, StartError> {
    start_within(
        connector,
        node,
        correlation_id,
        pane_id,
        params,
        answer_timeout,
        SHELL_WAIT,
        Duration::ZERO,
        check,
    )
}

/// A start for an agent that goes back under the name of the one
/// just ended in this pane: `agent_name_taken` is answered by sending the
/// start again until Herdr has released the name, within [`NAME_RELEASE_WAIT`].
pub(crate) fn start_at_shell_reusing_name(
    connector: &dyn ApiConnector,
    node: Option<&dyn NodeLink>,
    correlation_id: &str,
    pane_id: &str,
    params: Value,
    answer_timeout: Duration,
) -> Result<Value, StartError> {
    start_within(
        connector,
        node,
        correlation_id,
        pane_id,
        params,
        answer_timeout,
        SHELL_WAIT,
        NAME_RELEASE_WAIT,
        &|| Ok(()),
    )
}

#[allow(clippy::disallowed_methods)] // a production wait, not test code
#[allow(clippy::too_many_arguments)] // one bounded start with shell/name waits and effect admission
fn start_within(
    connector: &dyn ApiConnector,
    node: Option<&dyn NodeLink>,
    correlation_id: &str,
    pane_id: &str,
    params: Value,
    answer_timeout: Duration,
    wait: Duration,
    name_release_wait: Duration,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Value, StartError> {
    let started = Instant::now();
    let deadline = started + wait;
    let name_deadline = started + name_release_wait;
    let shell = Shell {
        connector,
        node,
        correlation_id,
        pane_id,
        line: wire::agent_start_line_bytes(&params),
    };
    let mut released = false;
    loop {
        shell.wait(started, deadline)?;
        check().map_err(StartError::NotStarted)?;
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
            // Herdr has not yet forgotten the agent this start replaces. It
            // refused before typing anything, so the same start is sent again
            // until it has, inside the name wait.
            Err(ApiError::Remote { code, .. })
                if code == NAME_TAKEN && Instant::now() < name_deadline =>
            {
                thread::sleep(POLL_INTERVAL);
            }
            // An earlier start in this pane under this name never showed its
            // agent, and Herdr still holds the name for it while the shell
            // has the terminal back. That start is this one's to replace: the
            // name is given back once and the start sent again.
            Err(ApiError::Remote { code, message }) if code == NAME_TAKEN && !released => {
                released = true;
                if !shell.release_unstarted(&params) {
                    return Err(StartError::Herdr(ApiError::Remote { code, message }));
                }
            }
            answer => return answer.map_err(StartError::Herdr),
        }
    }
}

/// The pane a start waits on, and how many bytes its start line types.
struct Shell<'a> {
    connector: &'a dyn ApiConnector,
    node: Option<&'a dyn NodeLink>,
    correlation_id: &'a str,
    pane_id: &'a str,
    line: usize,
}

impl Shell<'_> {
    /// Returns once the shell alone holds the terminal and its terminal
    /// would hand the start line over whole.
    #[allow(clippy::disallowed_methods)] // a production wait, not test code
    fn wait(&self, started: Instant, deadline: Instant) -> Result<(), StartError> {
        let params =
            wire::pane_process_info_params(self.pane_id).map_err(StartError::NotStarted)?;
        loop {
            let group = request_with_connector(
                self.connector,
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
            // The limit of a terminal that still holds lines, read only while
            // the shell holds it.
            let held = match group.shell_pid.filter(|_| group.shell_holds_terminal()) {
                Some(shell) => match self.line_cut_at(shell)? {
                    None => return Ok(()),
                    held => held,
                },
                None => None,
            };
            if Instant::now() >= deadline {
                crate::diagnostic!(serde_json::json!({
                    "component": "agent_start",
                    "kind": "shell_wait.timeout",
                    "request": self.correlation_id,
                    "pane_id": self.pane_id,
                    "waited_ms": started.elapsed().as_millis() as u64,
                    "shell_pid": group.shell_pid,
                    "foreground_process_group_id": group.foreground_process_group_id,
                    "foreground_pids": group.foreground_pids,
                    "line_bytes": self.line,
                    "line_limit": held,
                }));
                return Err(StartError::NotStarted(match held {
                    Some(limit) => format!(
                        "The pane's shell did not start reading its command line within {} s, so the agent was not started: until it does, its terminal keeps only {limit} bytes of a typed line, and this start's line is up to {} bytes. Check what the shell's startup files wait on, then retry.",
                        SHELL_WAIT.as_secs(),
                        self.line,
                    ),
                    None => format!(
                        "The pane's shell did not reach its prompt within {} s, so the agent was not started.",
                        SHELL_WAIT.as_secs()
                    ),
                }));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Gives back the start's name when Herdr holds it in this pane for a
    /// start that never showed its agent; true when it did. Called after
    /// Herdr refused the name, which it checks before anything is typed, and
    /// right after the shell was read holding the terminal, so no agent of
    /// that start runs here.
    fn release_unstarted(&self, params: &Value) -> bool {
        let (Some(name), Some(kind)) = (params["name"].as_str(), params["kind"].as_str()) else {
            return false;
        };
        let held = request_with_connector(
            self.connector,
            "agent.list",
            wire::empty_params(),
            READ_TIMEOUT,
        )
        .map_err(|error| format!("agent.list failed: {error}"))
        .and_then(|value| wire::holds_unstarted_launch(value, self.pane_id, name));
        let release = match held {
            Ok(true) => wire::pane_release_agent_params(self.pane_id, kind).and_then(|params| {
                request_with_connector(self.connector, "pane.release_agent", params, READ_TIMEOUT)
                    .map_err(|error| format!("pane.release_agent failed: {error}"))
            }),
            Ok(false) => return false,
            Err(reason) => Err(reason),
        };
        crate::diagnostic!(serde_json::json!({
            "component": "agent_start",
            "kind": "unstarted_name.release",
            "request": self.correlation_id,
            "pane_id": self.pane_id,
            "released": release.is_ok(),
            "reason": release.as_ref().err(),
        }));
        release.is_ok()
    }

    /// `None` when the start line reaches `shell` whole now; the limit of a
    /// terminal that would cut it, while the shell's line editor is not
    /// reading yet.
    fn line_cut_at(&self, shell: u32) -> Result<Option<usize>, StartError> {
        if self.line < SHORTEST_LINE_LIMIT {
            return Ok(None);
        }
        let refused = |reason: String| {
            StartError::NotStarted(format!(
                "The agent was not started: its start line is up to {} bytes, more than a terminal keeps before the shell's line editor reads it, and {reason}.",
                self.line
            ))
        };
        let node = self.node.ok_or_else(|| {
            refused(
                "the machine that runs the pane cannot be asked whether its shell is reading"
                    .into(),
            )
        })?;
        match crate::node_access::call_as::<LineInput>(
            node,
            Call::LineInput { pid: shell },
            READ_TIMEOUT,
        ) {
            Ok(LineInput::Keys | LineInput::Console) => Ok(None),
            Ok(LineInput::Lines { limit }) if self.line < limit as usize => Ok(None),
            Ok(LineInput::Lines { limit }) => Ok(Some(limit as usize)),
            Err(error) => Err(refused(format!(
                "the pane shell's terminal could not be read ({error})"
            ))),
        }
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
            None,
            "test:start",
            "w1:p1",
            json!({"pane_id": "w1:p1"}),
            Duration::from_secs(5),
            wait,
            Duration::ZERO,
            &|| Ok(()),
        )
    }

    fn start_reusing_name(herdr: &FakeHerdr, name_wait: Duration) -> Result<Value, StartError> {
        start_within(
            &herdr.connector(),
            None,
            "test:start",
            "w1:p1",
            json!({"pane_id": "w1:p1"}),
            Duration::from_secs(5),
            Duration::from_secs(5),
            name_wait,
            &|| Ok(()),
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
    fn a_changed_session_after_readiness_or_a_busy_refusal_never_types_another_start() {
        for previously_admitted in [false, true] {
            let herdr = FakeHerdr::start_with_errors("checked-start", |method, _| match method {
                "pane.process_info" => Ok(process_info(SHELL, &[SHELL])),
                "agent.start" => Err((PANE_BUSY.into(), "busy before typing".into())),
                other => panic!("unexpected {other}"),
            });
            let checks = std::cell::Cell::new(0);
            let answer = start_within(
                &herdr.connector(),
                None,
                "checked",
                "w1:p1",
                json!({"pane_id":"w1:p1"}),
                Duration::from_secs(5),
                Duration::from_secs(5),
                Duration::ZERO,
                &|| {
                    let count = checks.get();
                    checks.set(count + 1);
                    if previously_admitted && count == 0 {
                        Ok(())
                    } else {
                        Err("session_route_unconfirmed".into())
                    }
                },
            );
            assert!(
                matches!(answer, Err(StartError::NotStarted(reason)) if reason == "session_route_unconfirmed")
            );
            let typed_attempts = herdr
                .methods()
                .iter()
                .filter(|method| *method == "agent.start")
                .count();
            assert_eq!(typed_attempts, usize::from(previously_admitted));
        }
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

    /// Herdr forgets an ended agent a moment after its process is gone, so a
    /// start under the same name is refused until then and must be sent
    /// again, once the refusal is gone, and not before.
    #[test]
    fn a_start_under_the_ended_agents_name_is_sent_again_until_herdr_lets_go() {
        let starts = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&starts);
        let herdr =
            FakeHerdr::start_with_errors("agent-start-name", move |method, _| match method {
                "pane.process_info" => Ok(process_info(SHELL, &[SHELL])),
                "agent.start" if seen.fetch_add(1, Ordering::SeqCst) < 2 => {
                    Err((NAME_TAKEN.into(), "agent name two is already used".into()))
                }
                "agent.start" => Ok(started()),
                other => panic!("unexpected {other}"),
            });
        assert!(start_reusing_name(&herdr, Duration::from_secs(5)).is_ok());
        assert_eq!(starts.load(Ordering::SeqCst), 3);
    }

    /// Herdr's agent list with `name` held on `pane`, as a start that never
    /// showed its agent, or as one whose agent runs.
    fn holding(pane: &str, agent: Option<&str>) -> Value {
        let mut held = json!({
            "pane_id": pane, "terminal_id": "term_1", "workspace_id": "w1", "tab_id": "w1:t1",
            "name": "factory-hide-t1", "focused": false, "agent_status": "unknown",
            "revision": 0, "state_change_seq": 0, "launch_pending": true
        });
        if let Some(agent) = agent {
            held["agent"] = json!(agent);
        }
        json!({"type": "agent_list", "agents": [held]})
    }

    fn start_named(herdr: &FakeHerdr) -> Result<Value, StartError> {
        start_within(
            &herdr.connector(),
            None,
            "test:start",
            "w1:p1",
            json!({"pane_id": "w1:p1", "name": "factory-hide-t1", "kind": "claude"}),
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::ZERO,
            &|| Ok(()),
        )
    }

    /// A start in this pane that never showed its agent (its line was cut,
    /// or its program exited at once) leaves Herdr holding the name with the
    /// shell back at its prompt; the next start under that name gives it
    /// back and starts, rather than being refused for good.
    #[test]
    fn a_name_held_by_an_unstarted_launch_in_the_same_pane_is_given_back() {
        let starts = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&starts);
        let herdr =
            FakeHerdr::start_with_errors("agent-start-unstarted", move |method, _| match method {
                "pane.process_info" => Ok(process_info(SHELL, &[SHELL])),
                "agent.start" if seen.fetch_add(1, Ordering::SeqCst) == 0 => Err((
                    NAME_TAKEN.into(),
                    "agent name factory-hide-t1 is already used".into(),
                )),
                "agent.start" => Ok(started()),
                "agent.list" => Ok(holding("w1:p1", None)),
                "pane.release_agent" => Ok(json!({"type": "ok"})),
                other => panic!("unexpected {other}"),
            });
        assert!(start_named(&herdr).is_ok());
        let calls = herdr.calls();
        let (_, release) = calls
            .iter()
            .find(|(method, _)| method == "pane.release_agent")
            .expect("the held name is given back");
        assert_eq!(release["pane_id"], "w1:p1");
        assert_eq!(release["agent"], "claude");
        assert_eq!(
            herdr.methods().last().map(String::as_str),
            Some("agent.start")
        );
        assert_eq!(starts.load(Ordering::SeqCst), 2);
    }

    /// A name held in another pane, or by an agent that shows, is someone
    /// else's: the refusal is the answer and nothing is given back.
    #[test]
    fn a_name_held_elsewhere_or_by_a_running_agent_is_not_given_back() {
        for (pane, agent) in [("w1:p2", None), ("w1:p1", Some("claude"))] {
            let herdr =
                FakeHerdr::start_with_errors("agent-start-held", move |method, _| match method {
                    "pane.process_info" => Ok(process_info(SHELL, &[SHELL])),
                    "agent.start" => Err((
                        NAME_TAKEN.into(),
                        "agent name factory-hide-t1 is already used".into(),
                    )),
                    "agent.list" => Ok(holding(pane, agent)),
                    other => panic!("unexpected {other}"),
                });
            let Err(StartError::Herdr(ApiError::Remote { code, .. })) = start_named(&herdr) else {
                panic!("a name someone else holds is refused");
            };
            assert_eq!(code, NAME_TAKEN);
            assert_eq!(
                herdr.methods(),
                ["pane.process_info", "agent.start", "agent.list"]
            );
        }
    }

    /// The wait is bounded, and an ordinary start never waits for a name: it
    /// belongs to someone else.
    #[test]
    fn a_name_that_is_never_released_is_herdrs_answer_and_an_ordinary_start_does_not_wait() {
        let herdr =
            FakeHerdr::start_with_errors("agent-start-name-held", |method, _| match method {
                "pane.process_info" => Ok(process_info(SHELL, &[SHELL])),
                "agent.start" => Err((NAME_TAKEN.into(), "agent name two is already used".into())),
                other => panic!("unexpected {other}"),
            });
        let Err(StartError::Herdr(ApiError::Remote { code, .. })) =
            start_reusing_name(&herdr, Duration::from_millis(300))
        else {
            panic!("a name that is never released is refused");
        };
        assert_eq!(code, NAME_TAKEN);
        let before = herdr.methods().len();
        let Err(StartError::Herdr(ApiError::Remote { code, .. })) =
            start(&herdr, Duration::from_secs(5))
        else {
            panic!("an ordinary start takes the refusal as the answer");
        };
        assert_eq!(code, NAME_TAKEN);
        assert_eq!(
            herdr.methods().len() - before,
            2,
            "one process read and one start, nothing sent again"
        );
    }

    /// The pane's node, answering how the shell's terminal takes input with
    /// the next of `answers` (the last one repeats), and counting the reads.
    struct Terminal {
        answers: Vec<Result<LineInput, String>>,
        reads: AtomicUsize,
    }

    impl Terminal {
        fn answering(answers: Vec<Result<LineInput, String>>) -> Self {
            Self {
                answers,
                reads: AtomicUsize::new(0),
            }
        }
    }

    impl NodeLink for Terminal {
        fn call(
            &self,
            call: Call,
            _: Duration,
        ) -> Result<crate::node_access::LinkAnswer, crate::node_access::LinkError> {
            let Call::LineInput { pid } = call else {
                panic!("a start asks its node only how the shell's terminal reads");
            };
            assert_eq!(pid, SHELL, "the shell Herdr named is the one read");
            let read = self.reads.fetch_add(1, Ordering::SeqCst);
            match &self.answers[read.min(self.answers.len() - 1)] {
                Ok(input) => Ok(crate::node_access::LinkAnswer::Parsed(
                    serde_json::to_value(input).unwrap(),
                )),
                Err(reason) => Err(crate::node_access::LinkError::NotConnected(reason.clone())),
            }
        }
    }

    fn shell_alone(name: &str) -> FakeHerdr {
        FakeHerdr::start(name, |method, _| match method {
            "pane.process_info" => process_info(SHELL, &[SHELL]),
            "agent.start" => started(),
            other => panic!("unexpected {other}"),
        })
    }

    /// A first prompt of `bytes` bytes, the argument Herdr types last.
    fn start_with_prompt(
        herdr: &FakeHerdr,
        node: Option<&dyn NodeLink>,
        bytes: usize,
        wait: Duration,
    ) -> Result<Value, StartError> {
        start_within(
            &herdr.connector(),
            node,
            "test:start",
            "w1:p1",
            json!({"pane_id": "w1:p1", "args": ["--", "가".repeat(bytes / 3)]}),
            Duration::from_secs(5),
            wait,
            Duration::ZERO,
            &|| Ok(()),
        )
    }

    fn typed(herdr: &FakeHerdr) -> usize {
        herdr
            .methods()
            .iter()
            .filter(|method| *method == "agent.start")
            .count()
    }

    /// A shell that holds the terminal while its startup files still run
    /// keeps a typed line in canonical mode, where macOS drops what passes
    /// 1,024 bytes; the start waits until the line editor reads keys.
    #[test]
    fn a_long_start_line_waits_for_the_shells_line_editor() {
        let herdr = shell_alone("agent-start-editor");
        let terminal = Terminal::answering(vec![
            Ok(LineInput::Lines { limit: 1024 }),
            Ok(LineInput::Lines { limit: 1024 }),
            Ok(LineInput::Keys),
        ]);
        assert!(start_with_prompt(&herdr, Some(&terminal), 4800, Duration::from_secs(5)).is_ok());
        assert_eq!(terminal.reads.load(Ordering::SeqCst), 3);
        assert_eq!(typed(&herdr), 1);
        assert_eq!(
            herdr.methods().last().map(String::as_str),
            Some("agent.start")
        );
    }

    /// A line that fits the terminal's own limit reaches the shell whole
    /// even before the line editor reads (a Linux terminal keeps 4,096
    /// bytes), and a console cuts no line.
    #[test]
    fn a_long_line_the_terminal_keeps_whole_starts_without_waiting() {
        for input in [LineInput::Lines { limit: 4096 }, LineInput::Console] {
            let herdr = shell_alone("agent-start-fits");
            let terminal = Terminal::answering(vec![Ok(input)]);
            assert!(
                start_with_prompt(&herdr, Some(&terminal), 2400, Duration::from_secs(5)).is_ok()
            );
            assert_eq!(terminal.reads.load(Ordering::SeqCst), 1);
            assert_eq!(typed(&herdr), 1);
        }
    }

    /// A line shorter than every terminal's limit is typed as soon as the
    /// shell holds the terminal, with no node to ask: a shell that has no
    /// line editor still starts a plain command.
    #[test]
    fn a_short_start_line_asks_no_node() {
        let herdr = shell_alone("agent-start-short");
        let terminal = Terminal::answering(vec![Ok(LineInput::Lines { limit: 1024 })]);
        assert!(start_with_prompt(&herdr, Some(&terminal), 600, Duration::from_secs(5)).is_ok());
        assert_eq!(terminal.reads.load(Ordering::SeqCst), 0);
        assert!(start_with_prompt(&herdr, None, 600, Duration::from_secs(5)).is_ok());
        assert_eq!(typed(&herdr), 2);
    }

    /// The wait is the shell wait: past it a terminal still holding lines
    /// gets nothing typed, never a cut line, and the reason names the limit.
    #[test]
    fn a_terminal_that_never_leaves_line_mode_gets_nothing_typed() {
        let herdr = shell_alone("agent-start-canonical");
        let terminal = Terminal::answering(vec![Ok(LineInput::Lines { limit: 1024 })]);
        let Err(StartError::NotStarted(message)) =
            start_with_prompt(&herdr, Some(&terminal), 4800, Duration::from_millis(300))
        else {
            panic!("a line the terminal would cut is not typed");
        };
        assert!(message.contains("1024 bytes"), "{message}");
        assert_eq!(typed(&herdr), 0);
    }

    /// Without a node to ask, or with one that cannot read the terminal, a
    /// long line is not typed on a guess.
    #[test]
    fn a_long_line_whose_terminal_cannot_be_read_gets_nothing_typed() {
        let herdr = shell_alone("agent-start-unread");
        let Err(StartError::NotStarted(message)) =
            start_with_prompt(&herdr, None, 4800, Duration::from_secs(5))
        else {
            panic!("no node, no long line");
        };
        assert!(message.contains("cannot be asked"), "{message}");
        let terminal = Terminal::answering(vec![Err("the helper is gone".into())]);
        let Err(StartError::NotStarted(message)) =
            start_with_prompt(&herdr, Some(&terminal), 4800, Duration::from_secs(5))
        else {
            panic!("an unread terminal is not a reading one");
        };
        assert!(message.contains("the helper is gone"), "{message}");
        assert_eq!(typed(&herdr), 0);
    }
}
