//! `claude -p --output-format json`, one child per request.
//!
//! Print mode is Claude Code's only official structured-output path: there is
//! no `app-server` equivalent in `claude --help`. The flag set below is not a
//! preference. Measured on claude 2.1.267, a bare `claude -p` loads the whole
//! agent harness into the system prompt (32,903 cached input tokens) and
//! answers the wrong question, reviewing the transcript instead of
//! classifying it; `--system-prompt` with `--tools ''` and
//! `--setting-sources ''` strips that to 1,188 input tokens and returns a
//! schema-validated `structured_output`. Dropping any of the three is not a
//! cost regression, it is a wrong answer. The measurements are recorded in
//! `agents/runs/hide-ai-claude-backend/claude-print-mode-measurements.md`.
//!
//! One child per request, never a persistent one: a second request to a live
//! child reuses its `session_id` and accumulates the conversation.
//!
//! `--bare` is unusable here. It reads `ANTHROPIC_API_KEY` only and never the
//! OAuth keychain, so it is incompatible with the subscription login that is
//! the user's own credential.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::codex::resolve_binary;
use crate::{
    AiBackend, AiError, AiRequest, AiResponse, AiUsage, Availability, CancelToken, ModelCatalog,
    ProviderId,
};

/// The user's decision for background features: the cheapest alias that
/// answered the measured classification correctly.
pub const DEFAULT_MODEL: &str = "haiku";

/// The aliases `--model` accepts, cheapest first.
///
/// This is the one model list in the crate that a provider is not asked for,
/// because Claude Code has no command that answers the question: `--help`
/// documents `--model` with three of the four aliases as examples and there is
/// no list subcommand. Parsing that sentence would be a worse contract than
/// naming the aliases here, so the gap is recorded in `AI_PROVIDERS.md`
/// instead, and an account that cannot use the chosen alias still discovers it
/// as a request failure. It stays inside the provider boundary: no list of
/// Claude models exists in the core or the shell.
pub const MODEL_ALIASES: &[&str] = &["haiku", "sonnet", "opus", "fable"];

/// The availability probe is a local process that reads a token file; it has
/// no reason to take longer than this, and the router must not stall on it.
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a child that has closed stdout is given to exit before it is
/// killed. Its answer is already in hand at that point.
const EXIT_GRACE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug)]
pub struct ClaudeConfig {
    /// `claude` on `PATH` by default; an explicit path is used as given.
    pub binary: PathBuf,
    /// A model alias (`haiku`, `sonnet`, `opus`, `fable`) or a full name.
    pub model: String,
    /// Working directory for the child. A neutral directory keeps a project's
    /// own CLAUDE.md out of the prompt; `--setting-sources ''` does not stop
    /// CLAUDE.md discovery, the directory does.
    pub cwd: PathBuf,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("claude"),
            model: DEFAULT_MODEL.to_owned(),
            cwd: std::env::temp_dir(),
        }
    }
}

/// Drives the installed Claude Code CLI in print mode, one child per request.
pub struct ClaudeCliBackend {
    config: ClaudeConfig,
}

impl ClaudeCliBackend {
    pub fn new(config: ClaudeConfig) -> Self {
        Self { config }
    }

    /// The exact argument vector one request is started with. The prompt body
    /// is not here: it goes on stdin, so no transcript reaches an argument
    /// list, a process listing, or the argv length limit.
    pub fn print_arguments(model: &str, system: &str, output_schema: &Value) -> Vec<String> {
        vec![
            "-p".to_owned(),
            "--model".to_owned(),
            model.to_owned(),
            // The three that decide whether the answer is right at all.
            "--system-prompt".to_owned(),
            system.to_owned(),
            "--tools".to_owned(),
            String::new(),
            "--setting-sources".to_owned(),
            String::new(),
            // Nothing else may enter the turn or outlive it.
            "--strict-mcp-config".to_owned(),
            "--disable-slash-commands".to_owned(),
            "--no-session-persistence".to_owned(),
            "--permission-prompts".to_owned(),
            "none".to_owned(),
            "--json-schema".to_owned(),
            output_schema.to_string(),
            "--output-format".to_owned(),
            "json".to_owned(),
        ]
    }

    /// The argument vector the availability probe is started with.
    pub fn auth_arguments() -> Vec<String> {
        vec!["auth".to_owned(), "status".to_owned(), "--json".to_owned()]
    }

    fn resolved_binary(&self) -> Option<PathBuf> {
        resolve_binary(&self.config.binary)
    }
}

impl AiBackend for ClaudeCliBackend {
    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    /// `claude auth status --json` is the contract for the login state. The
    /// credentials file is never read: the CLI's own answer is the record.
    fn availability(&self) -> Availability {
        let Some(binary) = self.resolved_binary() else {
            return Availability::NotInstalled;
        };
        let run = match run(
            &binary,
            &Self::auth_arguments(),
            &self.config.cwd,
            None,
            AUTH_TIMEOUT,
            &CancelToken::new(),
        ) {
            Ok(run) => run,
            Err(error) => {
                return Availability::Unavailable {
                    reason: error.diagnostic("auth"),
                };
            }
        };
        let status = match serde_json::from_str::<Value>(run.stdout.trim()) {
            Ok(status) => status,
            Err(_) => {
                return Availability::Unavailable {
                    reason: format!("auth_status_unreadable:exit={}", run.exit()),
                };
            }
        };
        match status.get("loggedIn").and_then(Value::as_bool) {
            Some(true) => Availability::Ready,
            Some(false) => Availability::NeedsLogin,
            // The field is the contract; without it nothing is known, and a
            // guess either way would be a silent default over an unknown.
            None => Availability::Unavailable {
                reason: "auth_status_without_logged_in".to_owned(),
            },
        }
    }

    /// The documented aliases, which is all the CLI offers; see
    /// [`MODEL_ALIASES`]. A CLI that is not installed answers nothing, the
    /// same as its availability does.
    fn models(&self) -> ModelCatalog {
        if self.resolved_binary().is_none() {
            return ModelCatalog::Unknown {
                reason: "claude_not_installed".to_owned(),
            };
        }
        ModelCatalog::Offered(
            MODEL_ALIASES
                .iter()
                .map(|alias| (*alias).to_owned())
                .collect(),
        )
    }

    fn execute(&self, request: &AiRequest, cancel: &CancelToken) -> Result<AiResponse, AiError> {
        let binary = self
            .resolved_binary()
            .ok_or_else(|| AiError::ProviderUnavailable("claude_not_installed".to_owned()))?;
        let run = run(
            &binary,
            &Self::print_arguments(&self.config.model, &request.system, &request.output_schema),
            &self.config.cwd,
            Some(&request.input),
            request.deadline,
            cancel,
        )
        .map_err(|error| error.before_submission())?;
        answer(&run)
    }
}

/// One finished child: whatever it wrote to stdout, and how it ended.
struct Run {
    code: Option<i32>,
    stdout: String,
}

impl Run {
    /// The exit status as a diagnostic token; `signal` when a signal ended it.
    fn exit(&self) -> String {
        self.code
            .map_or_else(|| "signal".to_owned(), |code| code.to_string())
    }

    fn succeeded(&self) -> bool {
        self.code == Some(0)
    }
}

/// Why a child produced no output. Every variant here happened before the
/// prompt reached the model: the child is killed before its stdin closes, so
/// an EOF can never submit a partial prompt.
enum RunError {
    Spawn(std::io::ErrorKind),
    NoPipe(&'static str),
    Write(std::io::ErrorKind),
    Deadline,
    Cancelled,
}

impl RunError {
    fn diagnostic(&self, stage: &str) -> String {
        match self {
            Self::Spawn(kind) => format!("{stage}_spawn_failed:{kind}"),
            Self::NoPipe(pipe) => format!("{stage}_no_{pipe}"),
            Self::Write(kind) => format!("{stage}_stdin_write_failed:{kind}"),
            Self::Deadline => format!("{stage}_deadline"),
            Self::Cancelled => format!("{stage}_cancelled"),
        }
    }

    /// The classification for a request the model never saw.
    fn before_submission(self) -> AiError {
        match self {
            Self::Deadline => AiError::Timeout,
            Self::Cancelled => AiError::Cancelled,
            other => AiError::ProviderUnavailable(other.diagnostic("claude")),
        }
    }
}

/// Runs one child to completion under a deadline, collecting its stdout.
///
/// stdout is drained by its own thread, so a child that writes more than a
/// pipe buffer cannot deadlock against the waiter, and its answer is in hand
/// before the exit status is read.
fn run(
    binary: &Path,
    args: &[String],
    cwd: &Path,
    stdin_text: Option<&str>,
    deadline: Duration,
    cancel: &CancelToken,
) -> Result<Run, RunError> {
    let mut child = Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .stdin(if stdin_text.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| RunError::Spawn(error.kind()))?;

    let Some(mut stdout) = child.stdout.take() else {
        kill(&mut child);
        return Err(RunError::NoPipe("stdout"));
    };
    let (sender, incoming) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = sender.send(text);
    });

    if let Some(text) = stdin_text {
        let Some(mut stdin) = child.stdin.take() else {
            kill(&mut child);
            return Err(RunError::NoPipe("stdin"));
        };
        if let Err(error) = stdin
            .write_all(text.as_bytes())
            .and_then(|()| stdin.flush())
        {
            // Kill before the pipe drops: closing a half-written stdin is an
            // EOF, and print mode submits whatever it read at EOF.
            let kind = error.kind();
            kill(&mut child);
            drop(stdin);
            return Err(RunError::Write(kind));
        }
        // The prompt is complete; EOF is what starts the turn.
        drop(stdin);
    }

    let until = Instant::now() + deadline;
    loop {
        match incoming.try_recv() {
            Ok(stdout) => {
                return Ok(Run {
                    code: wait_briefly(&mut child),
                    stdout,
                });
            }
            // The reader thread always sends exactly once, so a closed
            // channel means it panicked; treat that as no output rather than
            // waiting for a send that will never come.
            Err(TryRecvError::Disconnected) => {
                return Ok(Run {
                    code: wait_briefly(&mut child),
                    stdout: String::new(),
                });
            }
            Err(TryRecvError::Empty) => {}
        }
        if cancel.is_cancelled() {
            kill(&mut child);
            return Err(RunError::Cancelled);
        }
        let now = Instant::now();
        if now >= until {
            kill(&mut child);
            return Err(RunError::Deadline);
        }
        std::thread::sleep(POLL.min(until - now));
    }
}

/// The exit status of a child that has already closed stdout, or `None` when
/// it had to be killed to stop waiting for it.
fn wait_briefly(child: &mut Child) -> Option<i32> {
    let until = Instant::now() + EXIT_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) => {}
            Err(_) => return None,
        }
        let now = Instant::now();
        if now >= until {
            kill(child);
            return None;
        }
        std::thread::sleep(POLL.min(until - now));
    }
}

fn kill(child: &mut Child) {
    // A print-mode child holds no state worth draining, so killing it is
    // always safe.
    let _ = child.kill();
    let _ = child.wait();
}

/// Turns one finished child into the answer or the failure it reported.
fn answer(run: &Run) -> Result<AiResponse, AiError> {
    let frame = match serde_json::from_str::<Value>(run.stdout.trim()) {
        Ok(frame) if frame.get("type").and_then(Value::as_str) == Some("result") => frame,
        // The child ran, so the prompt was submitted, and without a result
        // frame nothing says whether the turn completed.
        _ => {
            return Err(AiError::CompletionUnknown(format!(
                "claude_no_result_frame:exit={}",
                run.exit()
            )));
        }
    };
    let failed = frame.get("is_error").and_then(Value::as_bool) == Some(true) || !run.succeeded();
    if failed {
        return Err(classify(&frame, run));
    }
    // The schema-bound answer is `structured_output`, never the `result`
    // string: without a schema print mode returns the object inside a
    // ```json fence, and parsing that back would accept an unvalidated shape.
    let value = frame
        .get("structured_output")
        .filter(|value| !value.is_null())
        .cloned()
        .ok_or_else(|| {
            AiError::InvalidOutput("claude_result_without_structured_output".to_owned())
        })?;
    Ok(AiResponse {
        value,
        usage: usage(&frame),
    })
}

/// Maps a failed result frame to the caller's failure classes.
///
/// The mapping is measured, not inferred: each `api_error_status` below was
/// observed end to end by answering the CLI's own API request with that
/// status and reading the frame it printed. `terminal_reason` and `subtype`
/// cover the failures that never reach the API. Nothing reads the prose in
/// `result`, which is localised and carries no class.
fn classify(frame: &Value, run: &Run) -> AiError {
    if let Some(status) = frame.get("api_error_status").and_then(Value::as_u64) {
        return match status {
            401 | 403 => AiError::NotAuthenticated,
            // Print mode names the reset window only inside the localised
            // `result` sentence, so no reset time is claimed here and the
            // router's default cooldown applies.
            429 => AiError::UsageLimited { retry_after: None },
            500.. => AiError::Transient(format!("claude_api_{status}")),
            // A refused request is settled by its input, like a schema the
            // API rejects; asking again with the same input repeats it.
            _ => AiError::InvalidOutput(format!("claude_api_{status}")),
        };
    }
    let terminal_reason = frame.get("terminal_reason").and_then(Value::as_str);
    if let Some(reason @ ("blocking_limit" | "prompt_too_long" | "rapid_refill_breaker")) =
        terminal_reason
    {
        // The transcript did not fit. Deterministic in the input.
        return AiError::InvalidOutput(format!("claude_context_limit:{reason}"));
    }
    match frame.get("subtype").and_then(Value::as_str) {
        Some("error_max_structured_output_retries") => {
            AiError::InvalidOutput("claude_structured_output_retries".to_owned())
        }
        Some("error_max_turns") => AiError::InvalidOutput("claude_max_turns".to_owned()),
        // Anything else is a refusal that may clear: the frame is the CLI's
        // own settled outcome, so nothing ran to completion, and the token
        // that was actually seen travels with the error.
        subtype => AiError::Transient(format!(
            "claude_{}",
            terminal_reason
                .or(subtype)
                .map_or_else(|| format!("exit_{}", run.exit()), str::to_owned)
        )),
    }
}

fn usage(frame: &Value) -> AiUsage {
    let usage = frame.get("usage");
    AiUsage {
        input_tokens: usage
            .and_then(|usage| usage.get("input_tokens"))
            .and_then(Value::as_u64),
        output_tokens: usage
            .and_then(|usage| usage.get("output_tokens"))
            .and_then(Value::as_u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({"type": "object", "required": ["summary"]})
    }

    /// The three flags the measurements name are the reason this test exists:
    /// dropping one produces a confident wrong answer, which no output
    /// assertion would catch.
    #[test]
    fn the_argument_vector_carries_the_three_flags_that_decide_correctness() {
        let args = ClaudeCliBackend::print_arguments("haiku", "classify this", &schema());
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--system-prompt", "classify this"]),
            "{args:?}"
        );
        assert!(
            args.windows(2).any(|pair| pair == ["--tools", ""]),
            "{args:?}"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--setting-sources", ""]),
            "{args:?}"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--json-schema", &schema().to_string()]),
            "{args:?}"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-format", "json"]),
            "{args:?}"
        );
        assert_eq!(args.first().map(String::as_str), Some("-p"));
        assert!(args.iter().any(|arg| arg == "--no-session-persistence"));
        assert!(!args.iter().any(|arg| arg == "--bare"));
    }

    /// The transcript is the one thing that must not be in argv.
    #[test]
    fn the_prompt_body_is_absent_from_the_argument_vector() {
        let args = ClaudeCliBackend::print_arguments("haiku", "sys", &schema());
        assert!(
            !args.iter().any(|arg| arg.contains("the user's transcript")),
            "{args:?}"
        );
    }

    fn frame(extra: Value) -> Value {
        let mut frame = json!({"type": "result", "subtype": "success", "is_error": false});
        let Value::Object(extra) = extra else {
            unreachable!("test frames are objects")
        };
        for (key, value) in extra {
            frame[key] = value;
        }
        frame
    }

    fn run_of(frame: &Value, code: i32) -> Run {
        Run {
            code: Some(code),
            stdout: frame.to_string(),
        }
    }

    #[test]
    fn a_result_frame_without_structured_output_is_invalid_output() {
        let ok = frame(json!({"structured_output": {"summary": "x"},
                              "usage": {"input_tokens": 12, "output_tokens": 3}}));
        let response = answer(&run_of(&ok, 0)).unwrap();
        assert_eq!(response.value["summary"], "x");
        assert_eq!(response.usage.input_tokens, Some(12));
        assert_eq!(response.usage.output_tokens, Some(3));

        // The answer text is present and schema-shaped, and is still not the
        // answer: only `structured_output` is.
        let missing = frame(json!({"result": "{\"summary\":\"x\"}"}));
        assert_eq!(
            answer(&run_of(&missing, 0)).unwrap_err(),
            AiError::InvalidOutput("claude_result_without_structured_output".to_owned())
        );
        let null = frame(json!({"structured_output": null}));
        assert!(matches!(
            answer(&run_of(&null, 0)),
            Err(AiError::InvalidOutput(_))
        ));
    }

    #[test]
    fn output_without_a_result_frame_is_completion_unknown() {
        for stdout in ["", "not json", r#"{"type":"system","subtype":"init"}"#] {
            let run = Run {
                code: Some(1),
                stdout: stdout.to_owned(),
            };
            match answer(&run) {
                Err(AiError::CompletionUnknown(reason)) => {
                    assert!(reason.contains("exit=1"), "{reason}");
                }
                other => panic!("{stdout:?} gave {other:?}"),
            }
        }
    }

    /// Every row here was observed by answering the CLI's own API request
    /// with that status; see the measurements file named in this module.
    #[test]
    fn each_measured_api_error_status_maps_to_its_class() {
        let cases: [(u64, AiError); 6] = [
            (401, AiError::NotAuthenticated),
            (403, AiError::NotAuthenticated),
            (429, AiError::UsageLimited { retry_after: None }),
            (500, AiError::Transient("claude_api_500".to_owned())),
            (529, AiError::Transient("claude_api_529".to_owned())),
            (400, AiError::InvalidOutput("claude_api_400".to_owned())),
        ];
        for (status, expected) in cases {
            let failed = frame(json!({
                "is_error": true,
                "terminal_reason": "api_error",
                "api_error_status": status,
                "result": "API Error"
            }));
            assert_eq!(
                answer(&run_of(&failed, 1)).unwrap_err(),
                expected,
                "status {status}"
            );
        }
    }

    #[test]
    fn turn_limits_and_context_limits_are_settled_by_the_input() {
        let retries = frame(json!({
            "is_error": true, "subtype": "error_max_structured_output_retries",
            "terminal_reason": "structured_output_retry_exhausted", "errors": []
        }));
        assert_eq!(
            answer(&run_of(&retries, 1)).unwrap_err(),
            AiError::InvalidOutput("claude_structured_output_retries".to_owned())
        );
        for reason in ["blocking_limit", "prompt_too_long", "rapid_refill_breaker"] {
            let limited = frame(json!({"is_error": true, "terminal_reason": reason}));
            match answer(&run_of(&limited, 1)) {
                Err(AiError::InvalidOutput(detail)) => {
                    assert!(detail.contains(reason), "{detail}");
                }
                other => panic!("{reason} gave {other:?}"),
            }
        }
    }

    /// An unrecognised failure is never a silent default: it reports the
    /// token the CLI actually printed.
    #[test]
    fn an_unrecognised_failure_carries_the_token_it_was_given() {
        let unknown = frame(json!({"is_error": true, "terminal_reason": "model_error"}));
        assert_eq!(
            answer(&run_of(&unknown, 1)).unwrap_err(),
            AiError::Transient("claude_model_error".to_owned())
        );
        // A frame that claims success while the process failed is still a
        // failure, and the exit status is what names it.
        let inconsistent = frame(json!({"structured_output": {"summary": "x"}}));
        assert_eq!(
            answer(&run_of(&inconsistent, 7)).unwrap_err(),
            AiError::Transient("claude_success".to_owned())
        );
    }

    #[test]
    fn a_missing_binary_is_not_installed_and_refuses_before_submitting() {
        let backend = ClaudeCliBackend::new(ClaudeConfig {
            binary: PathBuf::from("claude-binary-that-does-not-exist"),
            ..ClaudeConfig::default()
        });
        assert_eq!(backend.availability(), Availability::NotInstalled);
        let request = AiRequest {
            feature_id: "test",
            request_id: crate::RequestId("r".to_owned()),
            subject_id: "s".to_owned(),
            system: "sys".to_owned(),
            input: "in".to_owned(),
            output_schema: schema(),
            deadline: Duration::from_secs(1),
            schema_version: "v1",
        };
        assert_eq!(
            backend.execute(&request, &CancelToken::new()).unwrap_err(),
            AiError::ProviderUnavailable("claude_not_installed".to_owned())
        );
    }
}
