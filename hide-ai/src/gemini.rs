//! `gemini -p`, one child per request.
//!
//! Gemini CLI documents headless mode (`-p`, with stdin appended to the
//! prompt), `--output-format json` (one object `{response, stats, error?}`),
//! exit codes (0 ok, 1 general or API failure, 42 input error, 53 turn limit)
//! and `--approval-mode plan`, its read-only mode: https://geminicli.com/docs/cli/cli-reference/
//! and https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/headless.md.
//! None of that has been run against a real account from this build; the
//! contract is proven against a stand-in CLI only (`AI_PROVIDERS.md`).
//!
//! The system prompt replaces the CLI's own through `GEMINI_SYSTEM_MD`
//! (`docs/cli/system-prompt.md`), and the request runs in a folder of its own
//! so no project `GEMINI.md` is found beside it. The transcript goes on stdin.
//!
//! Gemini CLI has no login probe and no model list command. Its login is
//! therefore known only when a request is tried: an unauthenticated CLI
//! answers `NotAuthenticated`, which the router already remembers and moves
//! past. The models are the four aliases the CLI documents, reported as a
//! fixed list.

use serde_json::Value;

use crate::runner::{self, Environment, PrivateFile, Spec};
use crate::text_cli::{TextCliConfig, classify_failure, json_from_text, system_with_schema};
use crate::{
    AiBackend, AiError, AiRequest, AiResponse, AiUsage, Availability, CancelToken, ModelCatalog,
    ProviderId,
};

const PROGRAM: &str = "gemini";

/// The model aliases `--model` documents; the CLI cannot be asked for more.
pub const MODEL_ALIASES: &[&str] = &["auto", "pro", "flash", "flash-lite"];

/// Appended after the transcript on stdin: the CLI needs a prompt argument,
/// and the instructions themselves are in the system prompt.
const INSTRUCTION: &str = "Follow the system instructions for the input above.";

pub struct GeminiCliBackend {
    config: TextCliConfig,
}

impl GeminiCliBackend {
    pub fn new(config: TextCliConfig) -> Self {
        Self { config }
    }

    /// The exact argument vector one request is started with. The prompt body
    /// is not here: it goes on stdin.
    pub fn print_arguments(model: &str) -> Vec<String> {
        let mut args = vec![
            "-p".to_owned(),
            INSTRUCTION.to_owned(),
            "--output-format".to_owned(),
            "json".to_owned(),
            // The CLI's read-only mode: it plans and reads, and writes or
            // runs nothing.
            "--approval-mode".to_owned(),
            "plan".to_owned(),
        ];
        if !model.is_empty() {
            args.push("--model".to_owned());
            args.push(model.to_owned());
        }
        args
    }
}

impl AiBackend for GeminiCliBackend {
    fn id(&self) -> ProviderId {
        ProviderId::GEMINI
    }

    fn availability(&self) -> Availability {
        match self.config.resolved(PROGRAM) {
            Some(_) => Availability::Ready,
            None => Availability::NotInstalled,
        }
    }

    fn models(&self) -> ModelCatalog {
        match self.config.resolved(PROGRAM) {
            Some(_) => ModelCatalog::Fixed(MODEL_ALIASES.iter().map(|m| (*m).to_owned()).collect()),
            None => ModelCatalog::Unknown {
                reason: "gemini_not_installed".to_owned(),
            },
        }
    }

    fn execute(&self, request: &AiRequest, cancel: &CancelToken) -> Result<AiResponse, AiError> {
        let binary = self
            .config
            .resolved(PROGRAM)
            .ok_or_else(|| AiError::ProviderUnavailable("gemini_not_installed".to_owned()))?;
        let system = PrivateFile::create(
            "system.md",
            &system_with_schema(&request.system, &request.output_schema),
        )
        .map_err(|error| {
            AiError::ProviderUnavailable(format!("gemini_system_file:{}", error.kind()))
        })?;
        let run = runner::run(
            &Spec {
                binary: &binary,
                args: &Self::print_arguments(request.model(&self.config.model)),
                cwd: system.dir(),
                stdin: Some(&request.input),
                environment: Environment::Inherit,
                set: &[("GEMINI_SYSTEM_MD", system.path().display().to_string())],
                deadline: request.deadline,
            },
            cancel,
        )
        .map_err(|error| error.into_error("gemini"))?;
        answer(&run)
    }
}

/// Turns one finished child into the answer or the failure it reported.
fn answer(run: &runner::Run) -> Result<AiResponse, AiError> {
    match run.code {
        Some(0) => {}
        // Documented: a malformed input, and the turn limit. Both are settled
        // by the input, so asking again repeats them.
        Some(42) => return Err(AiError::InvalidOutput("gemini_input_error".to_owned())),
        Some(53) => return Err(AiError::InvalidOutput("gemini_turn_limit".to_owned())),
        code => {
            // A failed run prints the CLI's JSON error object on stdout when
            // it got that far, and plain text on stderr when it did not.
            let text = match serde_json::from_str::<Value>(run.stdout.trim()) {
                Ok(frame) => frame
                    .get("error")
                    .map(Value::to_string)
                    .unwrap_or_else(|| run.stderr.clone()),
                Err(_) => run.stderr.clone(),
            };
            return Err(classify_failure("gemini", code, &text));
        }
    }
    let frame = serde_json::from_str::<Value>(run.stdout.trim()).map_err(|_| {
        // The child ran to a clean exit, so the prompt was submitted, and
        // output that is not the CLI's object says nothing about the turn.
        AiError::CompletionUnknown("gemini_output_not_json".to_owned())
    })?;
    if let Some(error) = frame.get("error").filter(|error| !error.is_null()) {
        return Err(classify_failure("gemini", Some(0), &error.to_string()));
    }
    let text = frame
        .get("response")
        .and_then(Value::as_str)
        .ok_or_else(|| AiError::InvalidOutput("gemini_without_response".to_owned()))?;
    let value = json_from_text(text)
        .ok_or_else(|| AiError::InvalidOutput("gemini_answer_not_json".to_owned()))?;
    Ok(AiResponse {
        value,
        usage: AiUsage::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(code: i32, stdout: Value, stderr: &str) -> runner::Run {
        runner::Run {
            code: Some(code),
            stdout: stdout.to_string(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn read_only_plan_mode_and_json_output_are_always_asked_for_and_no_prompt_is_in_argv() {
        let args = GeminiCliBackend::print_arguments("");
        assert!(args.windows(2).any(|p| p == ["--approval-mode", "plan"]));
        assert!(args.windows(2).any(|p| p == ["--output-format", "json"]));
        assert!(!args.iter().any(|arg| arg == "--model"));
        let with_model = GeminiCliBackend::print_arguments("flash");
        assert!(with_model.windows(2).any(|p| p == ["--model", "flash"]));
    }

    #[test]
    fn the_answer_is_the_json_the_model_printed_inside_the_response_field() {
        let ok = run(
            0,
            json!({"response": "```json\n{\"summary\":\"x\"}\n```", "stats": {}}),
            "",
        );
        assert_eq!(answer(&ok).unwrap().value, json!({"summary": "x"}));
        let prose = run(0, json!({"response": "I cannot do that"}), "");
        assert!(matches!(answer(&prose), Err(AiError::InvalidOutput(_))));
        let missing = run(0, json!({"stats": {}}), "");
        assert!(matches!(answer(&missing), Err(AiError::InvalidOutput(_))));
    }

    #[test]
    fn the_documented_exit_codes_map_to_their_classes() {
        assert_eq!(
            answer(&run(42, json!({}), "")).unwrap_err(),
            AiError::InvalidOutput("gemini_input_error".to_owned())
        );
        assert_eq!(
            answer(&run(53, json!({}), "")).unwrap_err(),
            AiError::InvalidOutput("gemini_turn_limit".to_owned())
        );
        assert_eq!(
            answer(&run(
                1,
                json!({}),
                "Please set an Auth method (401 Unauthorized)"
            ))
            .unwrap_err(),
            AiError::NotAuthenticated
        );
        assert_eq!(
            answer(&run(
                1,
                json!({"error": {"code": 429, "message": "quota"}}),
                ""
            ))
            .unwrap_err(),
            AiError::UsageLimited { retry_after: None }
        );
        assert_eq!(
            answer(&run(1, json!({}), "boom")).unwrap_err(),
            AiError::CompletionUnknown("gemini_failed:exit=1".to_owned())
        );
    }

    #[test]
    fn an_error_object_on_a_clean_exit_is_still_a_failure() {
        let frame = run(
            0,
            json!({"response": null, "error": {"message": "503 overloaded"}}),
            "",
        );
        assert_eq!(
            answer(&frame).unwrap_err(),
            AiError::Transient("gemini_upstream".to_owned())
        );
    }
}
