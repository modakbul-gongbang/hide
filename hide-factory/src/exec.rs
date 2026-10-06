//! Short subprocesses (`git`, `gh`) behind one seam, and the reading of their
//! failures into the structured environment signals (D-31 rule 2). Every run
//! has a deadline and a capped output (`hide_platform::process::run_to_end`);
//! tests replace the runner with a recording fake.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use hide_platform::process::{RunFailure, run_to_end};

use crate::adapters::{EnvSignal, Failure};

/// The longest a single `git` or `gh` call may take.
pub const CALL_DEADLINE: Duration = Duration::from_secs(120);

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

pub trait Runner: Send {
    /// Runs `program` with `args` in `cwd`; `Err` only when it could not run
    /// or answer at all.
    fn run(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<Output, Failure>;
}

/// The real runner. `stop` ends a call in flight when the engine shuts down.
pub struct SystemRunner {
    pub stop: Arc<AtomicBool>,
}

impl Runner for SystemRunner {
    fn run(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<Output, Failure> {
        let mut command = Command::new(program);
        command.args(args);
        // A pager or a prompt would wait forever for a person.
        command
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GH_PAGER", "cat");
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        match run_to_end(&mut command, CALL_DEADLINE, &self.stop) {
            Ok(finished) => Ok(Output {
                code: finished.code,
                stdout: finished.stdout,
                stderr: finished.stderr,
            }),
            Err(RunFailure::TimedOut) => Err(if program == "gh" {
                Failure::environment(program, EnvSignal::Network, "timed out")
            } else {
                Failure::task(program, "timed out")
            }),
            Err(RunFailure::Stopped) => Err(Failure::task(program, "stopped")),
            Err(RunFailure::Start(error)) | Err(RunFailure::Wait(error)) => {
                Err(Failure::task(program, error.to_string()))
            }
        }
    }
}

/// The shell a verify command or quick check runs under, and its flag.
pub fn shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("/bin/sh", "-c")
    }
}

/// Runs and requires success; a failure is read for its signal.
pub fn checked(
    runner: &mut dyn Runner,
    stage: &str,
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String, Failure> {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let output = runner.run(program, &args, cwd)?;
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
        "remote end hung up",
        "early eof",
        "unexpected disconnect",
        "could not read from remote repository",
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
            (
                "fatal: the remote end hung up unexpectedly",
                128,
                Some(EnvSignal::Network),
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
