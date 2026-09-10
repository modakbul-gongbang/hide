//! Drives `ClaudeCliBackend` against a scripted stand-in for the installed
//! `claude` CLI, so the print-mode client is exercised end to end without a
//! login, a network, or a paid request.

use std::path::PathBuf;
use std::time::Duration;

use hide_ai::{
    AiBackend, AiError, AiRequest, AiRouter, Availability, CancelToken, ClaudeCliBackend,
    ClaudeConfig, NoopLogSink, ProviderId, RequestId, RouterConfig,
};
use serde_json::{Value, json};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.py")
}

fn backend() -> ClaudeCliBackend {
    ClaudeCliBackend::new(ClaudeConfig {
        binary: fixture(),
        model: "haiku".to_owned(),
        cwd: std::env::temp_dir(),
    })
}

fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "attention"],
        "properties": {
            "summary": {"type": "string"},
            "attention": {"type": "string", "enum": ["question", "none"]}
        }
    })
}

fn request(deadline: Duration) -> AiRequest {
    AiRequest {
        feature_id: "fixture",
        request_id: RequestId("req-1".to_owned()),
        subject_id: "pane-1".to_owned(),
        system: "Classify the transcript. Answer only with the schema.".to_owned(),
        input: "user: add compact task labels".to_owned(),
        output_schema: schema(),
        deadline,
        schema_version: "fixture.v1",
    }
}

/// The fixture reads its behaviour from the environment; tests that need a
/// mode run serially under this lock so they never see each other's value.
static MODE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_mode<T>(mode: &str, body: impl FnOnce() -> T) -> T {
    let _guard = MODE.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: the lock above serialises every writer of FAKE_MODE in this
    // process, and the fixture reads it once at spawn.
    unsafe { std::env::set_var("FAKE_MODE", mode) };
    let out = body();
    unsafe { std::env::remove_var("FAKE_MODE") };
    out
}

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("hide-ai-{name}-{}.txt", std::process::id()))
}

#[test]
fn a_completed_turn_returns_structured_output_with_usage() {
    with_mode("ok", || {
        let backend = backend();
        assert_eq!(backend.availability(), Availability::Ready);
        let response = backend
            .execute(&request(Duration::from_secs(30)), &CancelToken::new())
            .unwrap();
        assert_eq!(response.value["summary"], "fixture");
        assert_eq!(response.usage.input_tokens, Some(1188));
        assert_eq!(response.usage.output_tokens, Some(468));
    });
}

/// The three flags that decide whether the answer is right at all have to
/// reach the child, not merely the vector the backend builds. Dropping one
/// produces a confident wrong answer, so the vector is the assertion.
#[test]
fn the_child_receives_the_system_prompt_and_the_emptied_tool_and_setting_sources() {
    with_mode("ok", || {
        let args_file = scratch("claude-args");
        let stdin_file = scratch("claude-stdin");
        unsafe {
            std::env::set_var("FAKE_ARGS_FILE", &args_file);
            std::env::set_var("FAKE_STDIN_FILE", &stdin_file);
        }
        let request = request(Duration::from_secs(30));
        backend().execute(&request, &CancelToken::new()).unwrap();
        unsafe {
            std::env::remove_var("FAKE_ARGS_FILE");
            std::env::remove_var("FAKE_STDIN_FILE");
        }
        let args: Vec<String> =
            serde_json::from_slice(&std::fs::read(&args_file).unwrap()).unwrap();
        let prompt = std::fs::read_to_string(&stdin_file).unwrap();
        let _ = std::fs::remove_file(&args_file);
        let _ = std::fs::remove_file(&stdin_file);

        assert_eq!(
            args,
            ClaudeCliBackend::print_arguments("haiku", &request.system, &request.output_schema)
        );
        assert_eq!(args.first().map(String::as_str), Some("-p"));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--system-prompt", request.system.as_str()]),
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
                .any(|pair| pair == ["--output-format", "json"]),
            "{args:?}"
        );
        assert!(args.iter().any(|arg| arg == "--no-session-persistence"));
        assert!(args.iter().any(|arg| arg == "--strict-mcp-config"));
        assert!(args.iter().any(|arg| arg == "--disable-slash-commands"));
        assert!(!args.iter().any(|arg| arg == "--bare"));

        // The transcript travels on stdin only.
        assert_eq!(prompt, request.input);
        assert!(
            !args.iter().any(|arg| arg.contains("compact task labels")),
            "{args:?}"
        );
    });
}

/// `result` carries the same JSON the schema bound, and it is still not the
/// answer: without `structured_output` nothing validated the shape.
#[test]
fn a_result_frame_without_structured_output_is_invalid_output() {
    for mode in ["no_structured_output", "null_structured_output"] {
        with_mode(mode, || {
            let error = backend()
                .execute(&request(Duration::from_secs(30)), &CancelToken::new())
                .unwrap_err();
            assert_eq!(
                error,
                AiError::InvalidOutput("claude_result_without_structured_output".to_owned()),
                "{mode}"
            );
        });
    }
}

/// The child ran, so the prompt was submitted; with no result frame nothing
/// says whether the turn completed.
#[test]
fn a_child_that_prints_no_result_frame_is_completion_unknown() {
    for mode in ["no_result_frame", "init_frame_only"] {
        with_mode(mode, || {
            match backend().execute(&request(Duration::from_secs(30)), &CancelToken::new()) {
                Err(AiError::CompletionUnknown(reason)) => {
                    assert!(reason.contains("exit=1"), "{mode}: {reason}");
                }
                other => panic!("{mode}: {other:?}"),
            }
        });
    }
}

/// A completion-unknown outcome is spent: no second attempt here, and no
/// attempt on the other provider.
#[test]
fn a_completion_unknown_outcome_is_never_retried_or_moved() {
    with_mode("no_result_frame", || {
        let args_file = scratch("claude-unknown-args");
        unsafe { std::env::set_var("FAKE_ARGS_FILE", &args_file) };
        let router = AiRouter::new(
            vec![std::sync::Arc::new(backend())],
            RouterConfig {
                priority: vec![ProviderId::Claude],
                ..RouterConfig::default()
            },
            std::sync::Arc::new(NoopLogSink),
        );
        let error = router
            .execute(&request(Duration::from_secs(30)), &CancelToken::new())
            .unwrap_err();
        unsafe { std::env::remove_var("FAKE_ARGS_FILE") };
        let _ = std::fs::remove_file(&args_file);
        assert!(matches!(error, AiError::CompletionUnknown(_)), "{error:?}");
        // The provider was not parked by it: the next intent is new.
        let state = router.provider_state().unwrap();
        assert_eq!(state.active, Some(ProviderId::Claude));
        assert_eq!(state.degraded, None);
    });
}

/// Each row was observed by answering the CLI's own API request with that
/// status; the fixture reproduces the frame that came back.
#[test]
fn each_measured_failure_shape_maps_to_its_class() {
    let cases: [(&str, AiError); 7] = [
        ("api_401", AiError::NotAuthenticated),
        ("api_403", AiError::NotAuthenticated),
        ("api_429", AiError::UsageLimited { retry_after: None }),
        ("api_500", AiError::Transient("claude_api_500".to_owned())),
        (
            "api_400",
            AiError::InvalidOutput("claude_api_400".to_owned()),
        ),
        (
            "structured_output_retries",
            AiError::InvalidOutput("claude_structured_output_retries".to_owned()),
        ),
        (
            "context_limit",
            AiError::InvalidOutput("claude_context_limit:prompt_too_long".to_owned()),
        ),
    ];
    for (mode, expected) in cases {
        with_mode(mode, || {
            let error = backend()
                .execute(&request(Duration::from_secs(30)), &CancelToken::new())
                .unwrap_err();
            assert_eq!(error, expected, "{mode}");
        });
    }
}

#[test]
fn an_expired_deadline_kills_the_child_and_reports_timeout() {
    with_mode("slow", || {
        let started = std::time::Instant::now();
        let error = backend()
            .execute(&request(Duration::from_millis(400)), &CancelToken::new())
            .unwrap_err();
        assert_eq!(error, AiError::Timeout);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the child was not killed"
        );
    });
}

#[test]
fn a_cancelled_request_kills_the_child() {
    with_mode("slow", || {
        let cancel = CancelToken::new();
        let canceller = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            canceller.cancel();
        });
        let error = backend()
            .execute(&request(Duration::from_secs(30)), &cancel)
            .unwrap_err();
        assert_eq!(error, AiError::Cancelled);
    });
}

#[test]
fn availability_reports_login_install_and_probe_state() {
    with_mode("ok", || {
        assert_eq!(backend().availability(), Availability::Ready)
    });
    with_mode("no_account", || {
        assert_eq!(backend().availability(), Availability::NeedsLogin);
    });
    with_mode("auth_broken", || match backend().availability() {
        Availability::Unavailable { reason } => {
            assert!(reason.contains("auth_status_unreadable"), "{reason}");
        }
        other => panic!("{other:?}"),
    });
    // Without the field nothing is known, and neither answer is invented.
    with_mode("auth_without_field", || {
        assert_eq!(
            backend().availability(),
            Availability::Unavailable {
                reason: "auth_status_without_logged_in".to_owned()
            }
        );
    });
    let missing = ClaudeCliBackend::new(ClaudeConfig {
        binary: PathBuf::from("claude-binary-that-does-not-exist"),
        ..ClaudeConfig::default()
    });
    assert_eq!(missing.availability(), Availability::NotInstalled);
    assert_eq!(
        missing
            .execute(&request(Duration::from_secs(1)), &CancelToken::new())
            .unwrap_err(),
        AiError::ProviderUnavailable("claude_not_installed".to_owned())
    );
}
