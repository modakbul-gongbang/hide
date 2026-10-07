//! The Factory's machine work, done by the core's own node over its link
//! (PRD core-host-node D-01, `hide_node_link::factory`), and the reading of
//! a failed run into the structured environment signals (D-31 rule 2). The
//! node runs each command with a deadline and capped output; tests answer
//! the link with a fake.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hide_node_link::factory::{FactoryCall, FactoryGh, FactoryGit, RUN_DEADLINE_MS, RunAnswer};
use hide_node_link::protocol::Call;
use hide_node_link::{LinkError, NodeLink, call_as, call_as_with_progress};
use serde::de::DeserializeOwned;

use crate::adapters::{EnvSignal, Failure};

/// How long the link waits past a run's own deadline before it gives up on
/// the answer.
const LINK_SLACK: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// The node the Factory's projects live on. `stop` ends a run in flight
/// when the engine shuts down.
#[derive(Clone)]
pub struct Machine {
    node: Arc<dyn NodeLink>,
    stop: Arc<AtomicBool>,
}

impl Machine {
    pub fn new(node: Arc<dyn NodeLink>, stop: Arc<AtomicBool>) -> Self {
        Self { node, stop }
    }

    /// One git command in `cwd`; `Err` only when it could not run or answer
    /// at all.
    pub fn git(&self, cwd: &Path, command: FactoryGit) -> Result<Output, Failure> {
        self.run(
            "git",
            FactoryCall::Git {
                cwd: cwd.to_string_lossy().into_owned(),
                command,
            },
        )
    }

    pub fn gh(&self, command: FactoryGh) -> Result<Output, Failure> {
        self.run("gh", FactoryCall::Gh { command })
    }

    /// The project's quick check under the node's shell.
    pub fn check(&self, cwd: &Path, text: &str) -> Result<Output, Failure> {
        self.run(
            "quick_check",
            FactoryCall::Check {
                cwd: cwd.to_string_lossy().into_owned(),
                text: text.to_owned(),
            },
        )
    }

    fn run(&self, program: &str, call: FactoryCall) -> Result<Output, Failure> {
        let answer: RunAnswer = call_as_with_progress(
            self.node.as_ref(),
            Call::Factory { call },
            Duration::from_millis(RUN_DEADLINE_MS) + LINK_SLACK,
            |_: serde_json::Value| !self.stop.load(Ordering::Acquire),
        )
        .map_err(|error| Failure::task(program, error.to_string()))?;
        read_run(program, answer)
    }

    /// Any other Factory request, answered as `T`.
    pub fn call<T: DeserializeOwned>(&self, stage: &str, call: FactoryCall) -> Result<T, Failure> {
        call_as(
            self.node.as_ref(),
            Call::Factory { call },
            Duration::from_millis(RUN_DEADLINE_MS) + LINK_SLACK,
        )
        .map_err(|error| Failure::task(stage, error.to_string()))
    }

    /// A Factory read whose link failure stays apart from the node's answer,
    /// so a caller can ask again rather than read it as the answer.
    pub fn read<T: DeserializeOwned>(&self, call: FactoryCall) -> Result<T, LinkError> {
        call_as(
            self.node.as_ref(),
            Call::Factory { call },
            Duration::from_millis(RUN_DEADLINE_MS) + LINK_SLACK,
        )
    }

    /// Any other node request, answered as `T`.
    pub fn node_call<T: DeserializeOwned>(&self, stage: &str, call: Call) -> Result<T, Failure> {
        call_as(
            self.node.as_ref(),
            call,
            Duration::from_millis(RUN_DEADLINE_MS) + LINK_SLACK,
        )
        .map_err(|error| Failure::task(stage, error.to_string()))
    }
}

/// How a run ended, as the Factory reads it.
pub fn read_run(program: &str, answer: RunAnswer) -> Result<Output, Failure> {
    match answer {
        RunAnswer::Finished {
            code,
            stdout,
            stderr,
        } => Ok(Output {
            code,
            stdout,
            stderr,
        }),
        RunAnswer::TimedOut if program == "gh" => Err(Failure::environment(
            program,
            EnvSignal::Network,
            "timed out",
        )),
        RunAnswer::TimedOut => Err(Failure::task(program, "timed out")),
        RunAnswer::Stopped => Err(Failure::task(program, "stopped")),
        RunAnswer::Unstarted { reason } => Err(Failure::task(program, reason)),
    }
}

/// A run that must succeed; a failure is read for its signal.
pub fn checked(stage: &str, output: Result<Output, Failure>) -> Result<String, Failure> {
    let output = output?;
    if output.ok() {
        return Ok(output.stdout);
    }
    Err(classify(stage, &output))
}

/// Reads a failed call's output for a structured environment signal; anything
/// else is the Task's (B58).
pub fn classify(stage: &str, output: &Output) -> Failure {
    let text = format!("{}\n{}", output.stderr, output.stdout);
    let lower = text.to_lowercase();
    let detail = last_line(&output.stderr).unwrap_or_else(|| format!("exit {:?}", output.code));
    let signal = if lower.contains("no space left on device") {
        Some(EnvSignal::DiskFull)
    } else if output.code == Some(137) {
        Some(EnvSignal::OutOfMemory)
    } else if lower.contains("http 401")
        || lower.contains("gh auth login")
        || lower.contains("authentication required")
    {
        Some(EnvSignal::GithubAuth)
    } else if lower.contains("http 429") || lower.contains("rate limit") {
        Some(EnvSignal::GithubRateLimit)
    } else if lower.contains("http 403")
        || lower.contains("required scopes")
        || lower.contains("must have admin rights")
    {
        Some(EnvSignal::GithubForbidden)
    } else if ["http 500", "http 502", "http 503", "http 504"]
        .iter()
        .any(|code| lower.contains(code))
        || lower.contains("the requested url returned error: 5")
    {
        Some(EnvSignal::GithubServer)
    } else if [
        "could not resolve host",
        "connection refused",
        "network is unreachable",
        "connection reset",
        "tls handshake",
        "i/o timeout",
        "operation timed out",
        "unexpected disconnect",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        Some(EnvSignal::Network)
    } else {
        None
    };
    let mut failure = match signal {
        Some(signal) => Failure::environment(stage, signal, detail),
        None => Failure::task(stage, detail),
    };
    if signal == Some(EnvSignal::GithubForbidden) {
        failure.missing_scope = missing_scope(&text);
    }
    failure
}

/// gh names a missing scope as `['scope']` or `"scope" scope`.
fn missing_scope(text: &str) -> Option<String> {
    let start = text.find("['")? + 2;
    let end = text[start..].find('\'')? + start;
    Some(text[start..end].to_owned()).filter(|scope| !scope.is_empty() && scope.len() < 40)
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(300).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(stderr: &str, code: i32) -> Output {
        Output {
            code: Some(code),
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn structured_signals_are_read_and_everything_else_is_the_tasks() {
        let cases = [
            ("HTTP 401: Bad credentials", 1, Some(EnvSignal::GithubAuth)),
            (
                "HTTP 403: Resource not accessible",
                1,
                Some(EnvSignal::GithubForbidden),
            ),
            (
                "API rate limit exceeded",
                1,
                Some(EnvSignal::GithubRateLimit),
            ),
            ("HTTP 502: Bad Gateway", 1, Some(EnvSignal::GithubServer)),
            (
                "fatal: unable to access: Could not resolve host: github.com",
                128,
                Some(EnvSignal::Network),
            ),
            (
                "write error: No space left on device",
                1,
                Some(EnvSignal::DiskFull),
            ),
            ("", 137, Some(EnvSignal::OutOfMemory)),
            (
                "fatal: unable to access 'https://github.com/o/r/': The requested URL returned error: 502",
                128,
                Some(EnvSignal::GithubServer),
            ),
            (
                "ssh: connect to host github.com port 22: Operation timed out\nfatal: Could not read from remote repository.",
                128,
                Some(EnvSignal::Network),
            ),
            // These close lasting refusals too (a missing key, a gone
            // repository, a pack too large): they reach a person.
            (
                "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
                128,
                None,
            ),
            (
                "error: RPC failed; HTTP 413\nfatal: the remote end hung up unexpectedly",
                128,
                None,
            ),
            (
                " ! [remote rejected] HEAD -> factory/1-x (protected branch hook declined)",
                1,
                None,
            ),
            ("error: test failed, to rerun pass --lib", 101, None),
        ];
        for (stderr, code, signal) in cases {
            assert_eq!(
                classify("stage", &failed(stderr, code)).signal,
                signal,
                "{stderr}"
            );
        }
        let scope = classify(
            "stage",
            &failed(
                "HTTP 403: requires one of the following scopes: ['workflow']",
                1,
            ),
        );
        assert_eq!(scope.missing_scope.as_deref(), Some("workflow"));
    }
}
