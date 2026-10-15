//! Drives `CodexAppServerBackend` against a scripted stand-in for
//! `codex app-server`, so the protocol client is exercised end to end
//! without a login or a network.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hide_ai::{
    AiBackend, AiError, AiLogEvent, AiLogSink, AiRequest, AiRouter, AiUsageSink, Availability,
    CancelToken, CodexAppServerBackend, CodexConfig, ModelTokens, NoopLogSink, ProviderId,
    RequestId, RouterConfig,
};
use serde_json::json;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-app-server.py")
}

fn backend() -> CodexAppServerBackend {
    CodexAppServerBackend::new(
        CodexConfig {
            binary: fixture(),
            model: "gpt-5.6-luna".to_owned(),
            cwd: std::env::temp_dir(),
            search_path: None,
        },
        Arc::new(NoopLogSink),
    )
}

fn request(deadline: Duration) -> AiRequest {
    AiRequest {
        feature_id: "fixture".into(),
        request_id: RequestId("req-1".to_owned()),
        subject_id: "pane-1".to_owned(),
        system: "Answer as JSON.".to_owned(),
        input: "hello".to_owned(),
        output_schema: json!({"type": "object", "required": ["summary"], "properties": {"summary": {"type": "string"}}}),
        deadline,
        schema_version: "fixture.v1".into(),
        pick: None,
    }
}

fn with_mode<T>(mode: &str, body: impl FnOnce() -> T) -> T {
    let _guard = crate::FAKE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: FAKE_ENV serialises every writer of a FAKE_* variable in this
    // binary, and the fixture reads it once at spawn.
    unsafe { std::env::set_var("FAKE_MODE", mode) };
    let out = body();
    unsafe { std::env::remove_var("FAKE_MODE") };
    out
}

#[test]
fn a_completed_turn_returns_the_message_as_json_with_usage() {
    with_mode("ok", || {
        let backend = backend();
        assert_eq!(backend.availability(), Availability::Ready);
        let response = backend
            .execute(&request(Duration::from_secs(10)), &CancelToken::new())
            .unwrap();
        assert_eq!(response.value["summary"], "fixture");
        assert_eq!(response.usage.output_tokens, Some(56));
        assert_eq!(response.usage.input_tokens, Some(1234));
    });
}

/// The ledger counts the thread's running total with the cache categories
/// taken out of `inputTokens`, under the model the server resolved.
#[test]
fn a_completed_turn_reports_the_thread_total_as_disjoint_ledger_tokens() {
    with_mode("ok", || {
        let response = backend()
            .execute(&request(Duration::from_secs(10)), &CancelToken::new())
            .unwrap();
        let [tokens] = response.usage.models.as_slice() else {
            panic!("{:?}", response.usage.models);
        };
        assert_eq!(tokens.model, "gpt-5.6-luna");
        assert_eq!(
            (
                tokens.input,
                tokens.output,
                tokens.cache_read,
                tokens.cache_write
            ),
            (1000, 56, 200, 34)
        );
        assert_eq!((tokens.cache_write_1h, tokens.reasoning), (0, Some(12)));
        assert!(!tokens.lower_bound);
    });
}

#[test]
fn the_child_is_started_with_tools_disabled_and_stdio_listen() {
    with_mode("ok", || {
        let record = std::env::temp_dir().join(format!("hide-ai-args-{}.json", std::process::id()));
        unsafe { std::env::set_var("FAKE_ARGS_FILE", &record) };
        let _ = backend().availability();
        unsafe { std::env::remove_var("FAKE_ARGS_FILE") };
        let args: Vec<String> = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
        let _ = std::fs::remove_file(&record);
        assert_eq!(args, CodexAppServerBackend::spawn_arguments());
        assert_eq!(args.first().map(String::as_str), Some("app-server"));
        assert!(args.windows(2).any(|w| w == ["--disable", "shell_tool"]));
        assert!(args.windows(2).any(|w| w == ["--listen", "stdio://"]));
        assert!(!args.iter().any(|a| a == "exec"));
    });
}

/// Waits until the slow fake has created `marker`, which it does once a turn
/// has started. The fake is another process, so the file is polled.
#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn wait_for_turn(marker: &std::path::Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !marker.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the turn never started: {}",
            marker.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_cancelled_request_interrupts_the_turn_and_keeps_the_child() {
    with_mode("slow", || {
        let marker =
            std::env::temp_dir().join(format!("hide-ai-turn-started-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        // SAFETY: `with_mode` holds the lock every writer of FAKE_* takes.
        unsafe { std::env::set_var("FAKE_STARTED_FILE", &marker) };
        let backend = backend();
        let cancel = CancelToken::new();
        let canceller = cancel.clone();
        // The cancel is raised once the turn is running, so it is a running
        // turn that is interrupted, not a start that is abandoned.
        let raised = {
            let marker = marker.clone();
            std::thread::spawn(move || {
                wait_for_turn(&marker);
                canceller.cancel();
            })
        };
        let error = backend
            .execute(&request(Duration::from_secs(10)), &cancel)
            .unwrap_err();
        unsafe { std::env::remove_var("FAKE_STARTED_FILE") };
        let _ = std::fs::remove_file(&marker);
        raised.join().unwrap();
        assert_eq!(error, AiError::Cancelled);
        // The same child answers the next availability probe.
        assert_eq!(backend.availability(), Availability::Ready);
    });
}

#[test]
fn an_expired_deadline_interrupts_and_reports_timeout() {
    with_mode("slow", || {
        let error = backend()
            .execute(&request(Duration::from_millis(400)), &CancelToken::new())
            .unwrap_err();
        assert_eq!(error, AiError::Timeout);
    });
}

#[test]
fn a_usage_limit_carries_the_reset_window() {
    with_mode("usage_limit", || {
        let error = backend()
            .execute(&request(Duration::from_secs(10)), &CancelToken::new())
            .unwrap_err();
        match error {
            AiError::UsageLimited { retry_after } => {
                let wait = retry_after.expect("reset time from account/rateLimits/read");
                assert!(
                    wait <= Duration::from_secs(120) && wait > Duration::from_secs(60),
                    "{wait:?}"
                );
            }
            other => panic!("expected usage limit, got {other:?}"),
        }
    });
}

#[test]
fn a_non_json_message_is_invalid_output() {
    with_mode("garbage", || {
        let error = backend()
            .execute(&request(Duration::from_secs(10)), &CancelToken::new())
            .unwrap_err();
        assert!(matches!(error, AiError::InvalidOutput(_)), "{error:?}");
    });
}

#[derive(Debug, Default)]
struct Attempts {
    calls: Mutex<Vec<Vec<ModelTokens>>>,
    started: Mutex<usize>,
}

impl AiUsageSink for Attempts {
    fn attempt(&self, _: &str, _: ProviderId, models: &[ModelTokens]) {
        self.calls.lock().unwrap().push(models.to_vec());
    }
}

impl AiLogSink for Attempts {
    fn log(&self, event: AiLogEvent) {
        if event.event == "ai.attempt" {
            *self.started.lock().unwrap() += 1;
        }
    }
}

#[test]
fn a_started_turn_that_ends_without_an_answer_is_one_unknown_call_without_retry() {
    for mode in [
        "interrupted_after_usage",
        "stream_disconnected",
        "stream_disconnected_500",
        "generic_failure",
        "unknown_failure",
    ] {
        with_mode(mode, || {
            let attempts = Arc::new(Attempts::default());
            let router = AiRouter::new(
                vec![Arc::new(backend())],
                RouterConfig {
                    priority: vec![ProviderId::CODEX],
                    backoff_base: Duration::ZERO,
                    usage_sink: Some(attempts.clone()),
                    ..RouterConfig::default()
                },
                attempts.clone(),
            );
            let error = router
                .execute(&request(Duration::from_secs(10)), &CancelToken::new())
                .unwrap_err();
            assert!(
                matches!(error, AiError::Cancelled | AiError::CompletionUnknown(_)),
                "{mode}: {error:?}"
            );
            assert_eq!(*attempts.started.lock().unwrap(), 1, "{mode}: retried");
            let calls = attempts.calls.lock().unwrap();
            assert_eq!(calls.len(), 1, "{mode}: unknown calls");
            assert!(calls[0].is_empty(), "incomplete turn usage is not a total");
        });
    }
}

#[test]
fn a_definite_input_or_pre_response_connection_refusal_spends_no_unknown_call() {
    for (mode, expected_attempts) in [("bad_request", 2), ("connection_failed", 4)] {
        with_mode(mode, || {
            let attempts = Arc::new(Attempts::default());
            let router = AiRouter::new(
                vec![Arc::new(backend())],
                RouterConfig {
                    priority: vec![ProviderId::CODEX],
                    backoff_base: Duration::ZERO,
                    usage_sink: Some(attempts.clone()),
                    ..RouterConfig::default()
                },
                attempts.clone(),
            );
            let error = router
                .execute(&request(Duration::from_secs(10)), &CancelToken::new())
                .unwrap_err();
            match mode {
                "bad_request" => assert!(matches!(error, AiError::Rejected(_)), "{error:?}"),
                _ => assert!(matches!(error, AiError::Transient(_)), "{error:?}"),
            }
            assert_eq!(*attempts.started.lock().unwrap(), expected_attempts);
            assert!(attempts.calls.lock().unwrap().is_empty(), "{mode}");
        });
    }
}

/// Losing the child before `turn/start` is written is a refusal; losing it
/// once `turn/start` is on the wire, answered or not, leaves the turn's fate
/// unknown. Both restart the child on the next request.
#[test]
fn a_child_lost_around_turn_start_is_classified_by_what_was_submitted() {
    with_mode("exit_at_thread_start", || {
        let error = backend()
            .execute(&request(Duration::from_secs(10)), &CancelToken::new())
            .unwrap_err();
        assert!(
            matches!(error, AiError::ProviderUnavailable(_)),
            "{error:?}"
        );
    });
    with_mode("exit", || {
        let error = backend()
            .execute(&request(Duration::from_secs(10)), &CancelToken::new())
            .unwrap_err();
        assert!(matches!(error, AiError::CompletionUnknown(_)), "{error:?}");
    });
    with_mode("exit_after_turn_start", || {
        let backend = backend();
        let error = backend
            .execute(&request(Duration::from_secs(10)), &CancelToken::new())
            .unwrap_err();
        assert!(matches!(error, AiError::CompletionUnknown(_)), "{error:?}");
        // The child is gone; the same instance starts a fresh one for the
        // next request instead of reporting the dead one again. The fixture
        // exits again in this mode, so a fresh child is what the second
        // completion-unknown error proves.
        let again = backend
            .execute(&request(Duration::from_secs(10)), &CancelToken::new())
            .unwrap_err();
        assert!(matches!(again, AiError::CompletionUnknown(_)), "{again:?}");
    });
    with_mode("ok", || {
        assert!(
            backend()
                .execute(&request(Duration::from_secs(10)), &CancelToken::new())
                .is_ok()
        );
    });
}

#[test]
fn availability_reports_login_model_and_install_state() {
    with_mode("no_account", || {
        assert_eq!(backend().availability(), Availability::NeedsLogin);
    });
    with_mode("no_model", || {
        assert!(matches!(
            backend().availability(),
            Availability::Unavailable { .. }
        ));
    });
    let missing = CodexAppServerBackend::new(
        CodexConfig {
            binary: PathBuf::from("/nonexistent/codex-binary-that-does-not-exist"),
            ..CodexConfig::default()
        },
        Arc::new(NoopLogSink),
    );
    assert_eq!(missing.availability(), Availability::NotInstalled);
    let missing: Arc<dyn AiBackend> = Arc::new(missing);
    assert!(matches!(
        missing.execute(&request(Duration::from_secs(1)), &CancelToken::new()),
        Err(AiError::ProviderUnavailable(_))
    ));
}
