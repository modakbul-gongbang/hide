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
//!
//! The same print mode answers `/usage`, a local command the CLI resolves
//! against its own login: no model turn, no cost, and the account's weekly
//! window as text in the result frame. That is how the toolbar's Weekly Usage
//! reads Claude Code, so Hide itself never touches the keychain; see
//! [`ClaudeCliBackend::usage_text`].

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::program::Program;
use crate::runner::{self, Environment, Run, Spec};
use crate::{
    AiBackend, AiError, AiRequest, AiResponse, AiUsage, Availability, CancelToken, ModelCatalog,
    ProviderId,
};

/// The default for background features. With thinking off (see
/// [`THINKING_OFF`]) sonnet answered the label prompt in 3.6 to 7 s with about
/// 165 output tokens and followed its rules best of the aliases measured;
/// haiku without thinking was as fast but broke the rules, and haiku with
/// thinking took 56 to 81 s. Measured 2026-10-02 on claude 2.1.287.
pub const DEFAULT_MODEL: &str = "sonnet";

/// The variable and value that turn thinking off for a model turn.
///
/// Print mode thinks by default, and a background answer is a short JSON
/// object: on 2026-10-02 the context label spent a median 2,656 output tokens
/// (at most 9,425) to return about 100, which put its median at 28 s and 17%
/// of requests past their 60 s deadline. There is no flag for it:
/// `--settings '{"alwaysThinkingEnabled":false}'` left the thinking in place,
/// and this variable removed it. It is set on the child whatever the
/// operator's environment says, so a budget exported in their shell cannot
/// bring the latency back.
pub const THINKING_OFF: (&str, &str) = ("MAX_THINKING_TOKENS", "0");

/// How long the account's model list is kept before the CLI is asked again.
/// Settings reads the list while the Hide AI tab is on screen, and a model list
/// is not worth a child process every few seconds.
const MODELS_TTL: Duration = Duration::from_secs(600);
/// The `initialize` request answers from the CLI's own state without a model
/// turn; the bound covers a cold Node start on a loaded machine.
const MODELS_TIMEOUT: Duration = Duration::from_secs(20);

/// The availability probe is a local process that reads a token file; it has
/// no reason to take longer than this, and the router must not stall on it.
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
/// `/usage` is a local command: the measured run answers in under a second,
/// and the bound covers a cold Node start on a loaded machine. Past it the
/// child is killed and the popover keeps its last answer or says so.
const USAGE_TIMEOUT: Duration = Duration::from_secs(30);
/// The only variables a `/usage` child receives, and which they are is
/// `hide-platform`'s rule for a child that must find the account's login
/// (`process::LOGIN_CHILD_VARIABLES`). On macOS and Linux that is `HOME`,
/// `PATH`, `USER`, `LOGNAME` and `TMPDIR`: `USER` is what lets the CLI find
/// its keychain account (without it the CLI prints `/cost` text as if logged
/// out), `HOME` and `PATH` locate the login and Node. On Windows it is the
/// variables Node and the CLI read there instead (`USERPROFILE`,
/// `SystemRoot`, `APPDATA`, the standard system folders, and
/// `CLAUDE_CODE_GIT_BASH_PATH` for the Git Bash the CLI needs). Everything else is withheld on purpose:
/// without `HERDR_ENV` the operator's Herdr and hide hooks exit early, and
/// without `CLAUDECODE` the CLI does not think it is nested.
pub const USAGE_ENVIRONMENT: &[&str] = hide_platform::process::LOGIN_CHILD_VARIABLES;

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
    /// The `PATH` the CLI is looked for on and run with. `None` is this
    /// account's search (`hide_platform::programs`); a caller that has to
    /// choose it, such as a test with a stand-in login shell, gives one built
    /// by `hide_platform::programs::cli_path_with`.
    pub search_path: Option<OsString>,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("claude"),
            model: DEFAULT_MODEL.to_owned(),
            cwd: std::env::temp_dir(),
            search_path: None,
        }
    }
}

/// Why a `/usage` read produced no text. The caller decides what each one
/// means for the screen; no variant carries output, a token, or an account.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "error", content = "kind", rename_all = "snake_case")]
pub enum UsageError {
    /// The binary is not on `PATH` (or the configured path is not a file).
    NotInstalled,
    /// The child outlived [`USAGE_TIMEOUT`] and was killed.
    Timeout,
    Cancelled,
    /// The child could not be started, exited non-zero, or reported
    /// `is_error`; the payload is the diagnostic token only.
    Failed(String),
    /// stdout was not a result frame, or the frame had no `result` text.
    NoResultFrame,
}

/// Drives the installed Claude Code CLI in print mode, one child per request.
pub struct ClaudeCliBackend {
    config: ClaudeConfig,
    /// The account's model list and when it was read.
    models: Mutex<Option<(Instant, Vec<String>)>>,
}

impl ClaudeCliBackend {
    pub fn new(config: ClaudeConfig) -> Self {
        Self {
            config,
            models: Mutex::new(None),
        }
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

    /// The argument vector a weekly-usage read is started with. `/usage` is
    /// answered locally, so none of the model-turn flags apply; the one that
    /// matters is `--no-session-persistence`, which keeps the read out of
    /// `~/.claude/projects/` and every resume list.
    pub fn usage_arguments() -> Vec<String> {
        vec![
            "-p".to_owned(),
            "/usage".to_owned(),
            "--output-format".to_owned(),
            "json".to_owned(),
            "--no-session-persistence".to_owned(),
        ]
    }

    /// Runs `/usage` and returns the result frame's `result` text: the CLI's
    /// own rendering of the account's limits, which the caller parses.
    ///
    /// The child gets [`USAGE_ENVIRONMENT`] and nothing else, and runs in the
    /// configured `cwd`. The text is returned whatever it says: a CLI that
    /// cannot read its login still exits 0 with `is_error: false` and prints
    /// `/cost` text instead, so "logged out" is the caller's reading of the
    /// text, not a class this function can name.
    pub fn usage_text(&self, cancel: &CancelToken) -> Result<String, UsageError> {
        let binary = self.resolved_binary().ok_or(UsageError::NotInstalled)?;
        let run = runner::run(
            &Spec {
                binary: &binary,
                args: &Self::usage_arguments(),
                cwd: &self.config.cwd,
                stdin: None,
                environment: Environment::Login,
                set: &[],
                deadline: USAGE_TIMEOUT,
            },
            cancel,
        )
        .map_err(|error| match error {
            runner::RunError::Deadline => UsageError::Timeout,
            runner::RunError::Cancelled => UsageError::Cancelled,
            other => UsageError::Failed(other.diagnostic("usage")),
        })?;
        // A non-zero exit is the child failing, whatever it printed on the
        // way down; only a child that finished is held to the frame shape.
        if !run.succeeded() {
            return Err(UsageError::Failed(format!("usage_exit_{}", run.exit())));
        }
        let frame = match serde_json::from_str::<Value>(run.stdout.trim()) {
            Ok(frame) if frame.get("type").and_then(Value::as_str) == Some("result") => frame,
            _ => return Err(UsageError::NoResultFrame),
        };
        if frame.get("is_error").and_then(Value::as_bool) == Some(true) {
            return Err(UsageError::Failed("usage_is_error".to_owned()));
        }
        frame
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(UsageError::NoResultFrame)
    }

    fn resolved_binary(&self) -> Option<Program> {
        Program::resolve(&self.config.binary, self.config.search_path.as_deref())
    }

    /// The argument vector the account's model list is asked with: the
    /// stream-json control protocol with only an `initialize` request, so no
    /// model turn runs and nothing is persisted.
    pub fn models_arguments() -> Vec<String> {
        [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--tools",
            "",
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--disable-slash-commands",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    /// The one line written to the child's stdin for [`Self::models_arguments`].
    const INITIALIZE_REQUEST: &'static str = r#"{"type":"control_request","request_id":"hide-models","request":{"subtype":"initialize"}}"#;

    /// Asks the CLI which models the logged-in account can use.
    ///
    /// The `initialize` answer also carries the account (an email, an
    /// organization, the plan): only the model names are read from it, and
    /// neither the answer nor any part of it is logged or returned.
    fn read_models(&self, binary: &Program) -> Result<Vec<String>, String> {
        let mut input = Self::INITIALIZE_REQUEST.to_owned();
        input.push('\n');
        let run = runner::run(
            &Spec {
                binary,
                args: &Self::models_arguments(),
                cwd: &self.config.cwd,
                stdin: Some(&input),
                environment: Environment::Inherit,
                set: &[],
                deadline: MODELS_TIMEOUT,
            },
            &CancelToken::new(),
        )
        .map_err(|error| error.diagnostic("claude_models"))?;
        parse_models(&run.stdout)
            .ok_or_else(|| format!("claude_models_unreadable:exit={}", run.exit()))
    }
}

/// The model names in the `initialize` control response, in the order the CLI
/// lists them, without `default`: that is the CLI choosing for the account,
/// which the shell offers as "CLI default" (no `--model`) instead.
fn parse_models(stdout: &str) -> Option<Vec<String>> {
    let frame = stdout.lines().find_map(|line| {
        let frame = serde_json::from_str::<Value>(line.trim()).ok()?;
        (frame.get("type").and_then(Value::as_str) == Some("control_response")).then_some(frame)
    })?;
    let response = frame.get("response")?;
    if response.get("subtype").and_then(Value::as_str) != Some("success") {
        return None;
    }
    let models = response.get("response")?.get("models")?.as_array()?;
    let names: Vec<String> = models
        .iter()
        .filter_map(|model| model.get("value").and_then(Value::as_str))
        .filter(|name| *name != "default")
        .map(str::to_owned)
        .collect();
    (!names.is_empty()).then_some(names)
}

impl AiBackend for ClaudeCliBackend {
    fn id(&self) -> ProviderId {
        ProviderId::CLAUDE
    }

    /// `claude auth status --json` is the contract for the login state. The
    /// credentials file is never read: the CLI's own answer is the record.
    fn availability(&self) -> Availability {
        let Some(binary) = self.resolved_binary() else {
            return Availability::NotInstalled;
        };
        let run = match runner::run(
            &Spec {
                binary: &binary,
                args: &Self::auth_arguments(),
                cwd: &self.config.cwd,
                stdin: None,
                environment: Environment::Inherit,
                set: &[],
                deadline: AUTH_TIMEOUT,
            },
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

    /// The models the logged-in account can use, asked of the CLI itself (the
    /// `initialize` control request) and kept for ten minutes. A CLI that is
    /// not installed or does not answer reports why instead of a list the
    /// account may not be able to use (B36).
    fn models(&self) -> ModelCatalog {
        let Some(binary) = self.resolved_binary() else {
            return ModelCatalog::Unknown {
                reason: "claude_not_installed".to_owned(),
            };
        };
        let mut cached = self.models.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((read_at, models)) = cached.as_ref()
            && read_at.elapsed() < MODELS_TTL
        {
            return ModelCatalog::Offered(models.clone());
        }
        match self.read_models(&binary) {
            Ok(models) => {
                *cached = Some((Instant::now(), models.clone()));
                ModelCatalog::Offered(models)
            }
            Err(reason) => ModelCatalog::Unknown { reason },
        }
    }

    fn execute(&self, request: &AiRequest, cancel: &CancelToken) -> Result<AiResponse, AiError> {
        let binary = self
            .resolved_binary()
            .ok_or_else(|| AiError::ProviderUnavailable("claude_not_installed".to_owned()))?;
        let run = runner::run(
            &Spec {
                binary: &binary,
                args: &Self::print_arguments(
                    &self.config.model,
                    &request.system,
                    &request.output_schema,
                ),
                cwd: &self.config.cwd,
                stdin: Some(&request.input),
                environment: Environment::Inherit,
                // Print mode thinks by default; see `THINKING_OFF`.
                set: &[(THINKING_OFF.0, THINKING_OFF.1.to_owned())],
                deadline: request.deadline,
            },
            cancel,
        )
        .map_err(|error| error.into_error("claude"))?;
        answer(&run)
    }
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
            stderr: String::new(),
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
                stderr: String::new(),
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
            binary: PathBuf::from("/nonexistent/claude-binary-that-does-not-exist"),
            ..ClaudeConfig::default()
        });
        assert_eq!(backend.availability(), Availability::NotInstalled);
        let request = AiRequest {
            feature_id: "test".into(),
            request_id: crate::RequestId("r".to_owned()),
            subject_id: "s".to_owned(),
            system: "sys".to_owned(),
            input: "in".to_owned(),
            output_schema: schema(),
            deadline: Duration::from_secs(1),
            schema_version: "v1".into(),
        };
        assert_eq!(
            backend.execute(&request, &CancelToken::new()).unwrap_err(),
            AiError::ProviderUnavailable("claude_not_installed".to_owned())
        );
    }

    #[test]
    fn the_models_are_the_values_of_the_initialize_answer_without_the_account() {
        let answer = json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": "hide-models",
                "response": {
                    "models": [
                        {"value": "default", "displayName": "Default (recommended)"},
                        {"value": "opus", "resolvedModel": "claude-opus-5-5"},
                        {"value": "sonnet"}
                    ],
                    "account": {"email": "someone@example.invalid"}
                }
            }
        });
        let stdout = format!("{}\n", answer);
        let models = parse_models(&stdout).unwrap();
        assert_eq!(models, ["opus", "sonnet"]);
        assert!(!models.iter().any(|model| model.contains("example")));
    }

    #[test]
    fn an_answer_without_models_is_not_a_list() {
        for stdout in [
            "",
            "not json",
            r#"{"type":"system","subtype":"init"}"#,
            r#"{"type":"control_response","response":{"subtype":"error","error":"x"}}"#,
            r#"{"type":"control_response","response":{"subtype":"success","response":{"models":[]}}}"#,
        ] {
            assert_eq!(parse_models(stdout), None, "{stdout}");
        }
    }
}
