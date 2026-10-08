//! Drives the Gemini CLI, Grok and Pi backends against scripted stand-ins for
//! the real CLIs, so each contract is exercised end to end without a login, a
//! network or a paid request. None of those CLIs is installed where this is
//! built: what these tests prove is what the backend sends and how it reads the
//! documented answers, not that a real CLI accepts it (`AI_PROVIDERS.md`).
//!
//! Each test writes its own stand-in script into a private folder, with its
//! answers embedded, so tests share no environment and run in parallel.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hide_ai::{
    AiBackend, AiError, AiRequest, Availability, CancelToken, GeminiCliBackend, GrokCliBackend,
    ModelCatalog, PiCliBackend, RequestId, TextCliConfig,
};
use serde_json::{Value, json};

/// The stand-in: records what it was started with, then answers the first
/// rule whose `when` words are all among its arguments.
const SCRIPT: &str = r#"#!/usr/bin/env python3
import json, os, sys, time
SCRIPT = json.loads(r'''__SCRIPT__''')
argv = sys.argv[1:]
stdin = sys.stdin.read()
files = {}
for flag in SCRIPT.get("file_args", []):
    if flag in argv:
        path = argv[argv.index(flag) + 1]
        with open(path, encoding="utf-8") as handle:
            files[flag] = {"text": handle.read(), "mode": oct(os.stat(path).st_mode & 0o777),
                           "dir_mode": oct(os.stat(os.path.dirname(path)).st_mode & 0o777)}
for name in SCRIPT.get("file_env", []):
    path = os.environ.get(name)
    if path:
        with open(path, encoding="utf-8") as handle:
            files[name] = {"text": handle.read(), "path": path}
record = {
    "argv": argv,
    "stdin": stdin,
    "cwd": os.getcwd(),
    "env": {key: os.environ.get(key) for key in SCRIPT.get("env_keys", [])},
    "files": files,
}
with open(SCRIPT["record"], "a", encoding="utf-8") as handle:
    handle.write(json.dumps(record) + "\n")
for rule in SCRIPT["rules"]:
    if all(word in argv for word in rule.get("when", [])):
        time.sleep(rule.get("sleep", 0))
        sys.stdout.write(rule.get("stdout", ""))
        sys.stderr.write(rule.get("stderr", ""))
        if "flood" in rule:
            sys.stdout.write("x" * rule["flood"])
        sys.exit(rule.get("exit", 0))
sys.exit(99)
"#;

struct Fake {
    /// Removed with the stand-in; a name no other test can share.
    _folder: tempfile::TempDir,
    dir: PathBuf,
    binary: PathBuf,
    record: PathBuf,
}

impl Fake {
    fn new(name: &str, script: Value) -> Self {
        let folder = tempfile::Builder::new()
            .prefix(&format!("hide-ai-fake-{name}-"))
            .tempdir()
            .unwrap();
        let dir = folder.path().to_path_buf();
        let record = dir.join("record.jsonl");
        let mut script = script;
        script["record"] = json!(record);
        let binary = dir.join(name);
        std::fs::write(&binary, SCRIPT.replace("__SCRIPT__", &script.to_string())).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _folder: folder,
            dir,
            binary,
            record,
        }
    }

    fn config(&self, model: &str) -> TextCliConfig {
        TextCliConfig {
            binary: Some(self.binary.clone()),
            model: model.to_owned(),
            cwd: self.dir.clone(),
            search_path: None,
        }
    }

    /// Every start the stand-in recorded, oldest first.
    fn starts(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.record)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// The last recorded start.
    fn last(&self) -> Value {
        self.starts().pop().expect("the stand-in was started")
    }
}

fn schema() -> Value {
    json!({"type": "object", "required": ["summary"], "properties": {"summary": {"type": "string"}}})
}

const TRANSCRIPT: &str = "user: please add compact task labels (secret-transcript-marker)";

fn request() -> AiRequest {
    AiRequest {
        feature_id: "fixture".into(),
        request_id: RequestId("req-1".to_owned()),
        subject_id: "pane-1".to_owned(),
        system: "Summarise the transcript.".to_owned(),
        input: TRANSCRIPT.to_owned(),
        output_schema: schema(),
        deadline: Duration::from_secs(30),
        schema_version: "fixture.v1".into(),
        pick: None,
    }
}

fn argv(start: &Value) -> Vec<String> {
    start["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect()
}

fn assert_transcript_not_in_argv(start: &Value) {
    assert!(
        !argv(start)
            .iter()
            .any(|arg| arg.contains("secret-transcript-marker")),
        "the transcript reached the argument vector: {:?}",
        argv(start)
    );
}

fn missing(config: TextCliConfig) -> TextCliConfig {
    TextCliConfig {
        binary: Some(PathBuf::from("/nonexistent/hide-ai-no-such-cli")),
        ..config
    }
}

// ---- Gemini CLI ----

fn gemini_fake(rules: Value) -> Fake {
    Fake::new(
        "gemini",
        json!({"rules": rules, "file_env": ["GEMINI_SYSTEM_MD"]}),
    )
}

#[test]
fn gemini_asks_in_plan_mode_with_the_transcript_on_stdin_and_the_system_prompt_in_a_private_file() {
    let fake = gemini_fake(json!([{
        "stdout": json!({"response": "{\"summary\":\"ok\"}", "stats": {}}).to_string()
    }]));
    let backend = GeminiCliBackend::new(fake.config("flash"));
    let response = backend.execute(&request(), &CancelToken::new()).unwrap();
    assert_eq!(response.value, json!({"summary": "ok"}));

    let start = fake.last();
    let args = argv(&start);
    assert!(
        args.windows(2).any(|p| p == ["--approval-mode", "plan"]),
        "{args:?}"
    );
    assert!(
        args.windows(2).any(|p| p == ["--output-format", "json"]),
        "{args:?}"
    );
    assert!(
        args.windows(2).any(|p| p == ["--model", "flash"]),
        "{args:?}"
    );
    assert_transcript_not_in_argv(&start);
    assert_eq!(start["stdin"], TRANSCRIPT);

    let system = start["files"]["GEMINI_SYSTEM_MD"]["text"].as_str().unwrap();
    assert!(system.starts_with("Summarise the transcript."), "{system}");
    assert!(
        system.contains("\"required\":[\"summary\"]"),
        "the schema is in the system prompt"
    );
    let system_path = Path::new(start["files"]["GEMINI_SYSTEM_MD"]["path"].as_str().unwrap());
    assert!(
        !system_path.exists(),
        "the system prompt file is gone once the request ended"
    );
    // The folder is gone with the file, so the two are compared by name: the
    // child reports its real path, the backend the one it created.
    assert_eq!(
        Path::new(start["cwd"].as_str().unwrap()).file_name(),
        system_path.parent().unwrap().file_name(),
        "the request runs in the folder of its own, not in a project"
    );
}

#[test]
fn gemini_default_model_sends_no_model_flag() {
    let fake =
        gemini_fake(json!([{"stdout": json!({"response": "{\"summary\":\"ok\"}"}).to_string()}]));
    GeminiCliBackend::new(fake.config(""))
        .execute(&request(), &CancelToken::new())
        .unwrap();
    assert!(!argv(&fake.last()).iter().any(|arg| arg == "--model"));
}

#[test]
fn gemini_reports_a_fixed_model_list_and_login_is_known_only_by_trying() {
    let fake = gemini_fake(json!([]));
    let backend = GeminiCliBackend::new(fake.config(""));
    assert_eq!(backend.availability(), Availability::Ready);
    let catalog = backend.models();
    assert!(
        catalog.is_fixed(),
        "a list the CLI cannot be asked for says so"
    );
    assert_eq!(catalog.offered(), ["auto", "pro", "flash", "flash-lite"]);
    assert!(
        fake.starts().is_empty(),
        "neither read asked the CLI anything"
    );

    let absent = GeminiCliBackend::new(missing(fake.config("")));
    assert_eq!(absent.availability(), Availability::NotInstalled);
    assert!(matches!(absent.models(), ModelCatalog::Unknown { .. }));
}

#[test]
fn gemini_failures_map_from_the_documented_exit_codes_and_errors() {
    for (rule, expected) in [
        (
            json!({"exit": 42, "stderr": "bad input"}),
            AiError::InvalidOutput("gemini_input_error".into()),
        ),
        (
            json!({"exit": 53, "stderr": "turn limit"}),
            AiError::InvalidOutput("gemini_turn_limit".into()),
        ),
        (
            json!({"exit": 1, "stderr": "401 Unauthorized"}),
            AiError::NotAuthenticated,
        ),
        (
            json!({"exit": 1, "stderr": "429 quota exceeded"}),
            AiError::UsageLimited { retry_after: None },
        ),
        (
            json!({"exit": 1, "stderr": "503 unavailable"}),
            AiError::Transient("gemini_upstream".into()),
        ),
        (
            json!({"exit": 7, "stderr": "boom"}),
            AiError::CompletionUnknown("gemini_failed:exit=7".into()),
        ),
    ] {
        let fake = gemini_fake(json!([rule]));
        let error = GeminiCliBackend::new(fake.config(""))
            .execute(&request(), &CancelToken::new())
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

// ---- Grok ----

fn grok_fake(rules: Value) -> Fake {
    Fake::new(
        "grok",
        json!({
            "rules": rules,
            "file_args": ["--prompt-file"],
            "env_keys": ["GROK_MEMORY", "GROK_DISABLE_AUTOUPDATER"],
        }),
    )
}

#[test]
fn grok_reads_the_transcript_from_a_private_file_with_every_tool_off() {
    let fake = grok_fake(json!([{
        "stdout": json!({"text": "{\"summary\":\"ok\"}", "stopReason": "end_turn"}).to_string()
    }]));
    let backend = GrokCliBackend::new(fake.config("grok-4.6"));
    let response = backend.execute(&request(), &CancelToken::new()).unwrap();
    assert_eq!(response.value, json!({"summary": "ok"}));

    let start = fake.last();
    let args = argv(&start);
    assert_transcript_not_in_argv(&start);
    assert_eq!(
        start["stdin"], "",
        "headless Grok reads no stdin, so none is written"
    );
    assert_eq!(start["files"]["--prompt-file"]["text"], TRANSCRIPT);
    assert_eq!(
        start["files"]["--prompt-file"]["mode"], "0o600",
        "the prompt file is owner-only"
    );
    assert_eq!(start["files"]["--prompt-file"]["dir_mode"], "0o700");
    let path = &args[args.iter().position(|a| a == "--prompt-file").unwrap() + 1];
    assert!(
        !Path::new(path).exists(),
        "the prompt file is gone once the request ended"
    );
    assert!(args.windows(2).any(|p| p == ["--tools", ""]), "{args:?}");
    for flag in ["--no-subagents", "--disable-web-search"] {
        assert!(args.iter().any(|arg| arg == flag), "{flag}");
    }
    assert!(
        args.windows(2).any(|p| p == ["--max-turns", "1"]),
        "{args:?}"
    );
    assert!(
        args.windows(2).any(|p| p == ["--model", "grok-4.6"]),
        "{args:?}"
    );
    assert_eq!(start["env"]["GROK_MEMORY"], "0");
    assert_eq!(start["env"]["GROK_DISABLE_AUTOUPDATER"], "1");
}

#[test]
fn grok_login_and_models_come_from_what_grok_models_prints() {
    let signed_in = grok_fake(json!([{
        "when": ["models"],
        "stdout": "* grok-4.6 (default)\n- grok-4.5\n"
    }]));
    let backend = GrokCliBackend::new(signed_in.config(""));
    assert_eq!(backend.availability(), Availability::Ready);
    assert_eq!(
        backend.models(),
        ModelCatalog::Offered(vec!["grok-4.6".to_owned(), "grok-4.5".to_owned()])
    );

    let signed_out =
        grok_fake(json!([{"when": ["models"], "stdout": "You are not authenticated.\n"}]));
    let backend = GrokCliBackend::new(signed_out.config(""));
    assert_eq!(backend.availability(), Availability::NeedsLogin);
    assert_eq!(
        backend.models(),
        ModelCatalog::Unknown {
            reason: "grok_not_authenticated".to_owned()
        }
    );

    let garbled = grok_fake(json!([{"when": ["models"], "stdout": "usage: grok models\n"}]));
    assert!(matches!(
        GrokCliBackend::new(garbled.config("")).availability(),
        Availability::Unavailable { .. }
    ));
    assert_eq!(
        GrokCliBackend::new(missing(signed_in.config(""))).availability(),
        Availability::NotInstalled
    );
}

#[test]
fn grok_failures_are_classified_from_its_error_object_and_exit() {
    for (rule, expected) in [
        (
            json!({"exit": 1, "stdout": json!({"type": "error", "message": "401 unauthorized"}).to_string()}),
            AiError::NotAuthenticated,
        ),
        (
            json!({"exit": 1, "stderr": "429 too many requests"}),
            AiError::UsageLimited { retry_after: None },
        ),
        (
            json!({"exit": 0, "stdout": "not json at all"}),
            AiError::CompletionUnknown("grok_output_not_json".into()),
        ),
    ] {
        let fake = grok_fake(json!([rule]));
        let error = GrokCliBackend::new(fake.config(""))
            .execute(&request(), &CancelToken::new())
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

// ---- Pi ----

fn pi_fake(rules: Value) -> Fake {
    Fake::new("pi", json!({"rules": rules}))
}

#[test]
fn pi_runs_with_everything_off_and_the_transcript_on_stdin() {
    let fake = pi_fake(json!([{"stdout": "```json\n{\"summary\":\"ok\"}\n```\n"}]));
    let backend = PiCliBackend::new(fake.config("anthropic/claude-x"));
    let response = backend.execute(&request(), &CancelToken::new()).unwrap();
    assert_eq!(response.value, json!({"summary": "ok"}));

    let start = fake.last();
    let args = argv(&start);
    assert_transcript_not_in_argv(&start);
    assert_eq!(start["stdin"], TRANSCRIPT);
    assert_eq!(args.first().map(String::as_str), Some("-p"));
    for flag in [
        "--no-tools",
        "--no-session",
        "--no-extensions",
        "--no-mcp",
        "--no-skills",
        "--no-prompt-templates",
        "--no-context-files",
    ] {
        assert!(
            args.iter().any(|arg| arg == flag),
            "{flag} missing from {args:?}"
        );
    }
    assert!(
        args.windows(2).any(|p| p == ["--thinking", "off"]),
        "{args:?}"
    );
    assert!(
        args.windows(2)
            .any(|p| p == ["--model", "anthropic/claude-x"]),
        "{args:?}"
    );
    let system = &args[args.iter().position(|a| a == "--system-prompt").unwrap() + 1];
    assert!(system.starts_with("Summarise the transcript."));
    assert!(system.contains("\"required\":[\"summary\"]"));
}

#[test]
fn pi_login_for_a_named_provider_is_the_auth_check_exit_code() {
    for (exit, expected) in [
        (0, Availability::Ready),
        (1, Availability::NeedsLogin),
        (2, Availability::NeedsLogin),
    ] {
        let fake =
            pi_fake(json!([{"when": ["auth", "check", "--provider", "anthropic"], "exit": exit}]));
        let backend = PiCliBackend::new(fake.config("anthropic/claude-x"));
        assert_eq!(backend.availability(), expected, "exit {exit}");
        let start = fake.last();
        assert!(
            argv(&start)
                .windows(2)
                .any(|p| p == ["--provider", "anthropic"])
        );
    }
    let broken = pi_fake(json!([{"when": ["auth"], "exit": 9}]));
    assert!(matches!(
        PiCliBackend::new(broken.config("anthropic/x")).availability(),
        Availability::Unavailable { .. }
    ));
}

#[test]
fn pi_with_its_own_default_model_is_ready_when_it_lists_models_and_offers_that_list() {
    let fake = pi_fake(
        json!([{"when": ["--list-models"], "stdout": "anthropic/claude-x  200k\nopenai/gpt-y  128k\n"}]),
    );
    let backend = PiCliBackend::new(fake.config(""));
    assert_eq!(backend.availability(), Availability::Ready);
    assert_eq!(
        backend.models(),
        ModelCatalog::Offered(vec![
            "anthropic/claude-x".to_owned(),
            "openai/gpt-y".to_owned()
        ])
    );

    let none = pi_fake(json!([{"when": ["--list-models"], "stdout": ""}]));
    let backend = PiCliBackend::new(none.config(""));
    assert_eq!(backend.availability(), Availability::NeedsLogin);
    assert!(matches!(backend.models(), ModelCatalog::Unknown { .. }));
    assert_eq!(
        PiCliBackend::new(missing(none.config(""))).availability(),
        Availability::NotInstalled
    );
}

#[test]
fn pi_failures_and_runaway_output_are_reported_not_buffered() {
    for (rule, expected) in [
        (
            json!({"exit": 1, "stderr": "No API key found"}),
            AiError::NotAuthenticated,
        ),
        (
            json!({"exit": 3, "stderr": "odd"}),
            AiError::CompletionUnknown("pi_failed:exit=3".into()),
        ),
        (
            json!({"exit": 0, "stdout": "I will not"}),
            AiError::InvalidOutput("pi_answer_not_json".into()),
        ),
        (
            json!({"exit": 0, "flood": 9 * 1024 * 1024}),
            AiError::InvalidOutput("pi_output_too_large".into()),
        ),
    ] {
        let fake = pi_fake(json!([rule]));
        let error = PiCliBackend::new(fake.config(""))
            .execute(&request(), &CancelToken::new())
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[test]
fn a_child_that_outlives_the_deadline_is_ended_and_reported_as_a_timeout() {
    let fake = pi_fake(json!([{"stdout": "{}", "sleep": 30}]));
    let mut slow = request();
    slow.deadline = Duration::from_millis(300);
    let error = PiCliBackend::new(fake.config(""))
        .execute(&slow, &CancelToken::new())
        .unwrap_err();
    assert_eq!(error, AiError::Timeout);
}

/// Blocks until the stand-in has recorded a start. The limit only ends a wait
/// on a child that never starts; it is not measured.
fn wait_for_start(fake: &Fake) {
    let limit = std::time::Instant::now() + Duration::from_secs(30);
    while fake.starts().is_empty() {
        assert!(
            std::time::Instant::now() < limit,
            "the stand-in never started"
        );
        std::thread::park_timeout(Duration::from_millis(10));
    }
}

#[test]
fn a_raised_cancel_token_stops_the_child_and_reports_cancelled() {
    let fake = pi_fake(json!([{"stdout": "{}", "sleep": 30}]));
    let backend = PiCliBackend::new(fake.config(""));
    let cancel = CancelToken::new();
    let outcome = std::thread::scope(|scope| {
        let running = scope.spawn(|| backend.execute(&request(), &cancel));
        wait_for_start(&fake);
        cancel.cancel();
        running.join().unwrap()
    });
    assert_eq!(outcome.unwrap_err(), AiError::Cancelled);
}
