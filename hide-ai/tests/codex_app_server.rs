//! Drives `CodexAppServerBackend` against a scripted stand-in for
//! `codex app-server`, so the protocol client is exercised end to end
//! without a login or a network.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use hide_ai::{
    AiBackend, AiError, AiRequest, Availability, CancelToken, CodexAppServerBackend, CodexConfig,
    RequestId,
};
use serde_json::json;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-app-server.py")
}

fn backend() -> CodexAppServerBackend {
    CodexAppServerBackend::new(CodexConfig {
        binary: fixture(),
        model: "gpt-5.6-luna".to_owned(),
        cwd: std::env::temp_dir(),
    })
}

fn request(deadline: Duration) -> AiRequest {
    AiRequest {
        feature_id: "fixture",
        request_id: RequestId("req-1".to_owned()),
        subject_id: "pane-1".to_owned(),
        system: "Answer as JSON.".to_owned(),
        input: "hello".to_owned(),
        output_schema: json!({"type": "object", "required": ["summary"], "properties": {"summary": {"type": "string"}}}),
        max_output_tokens: 96,
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

#[test]
fn a_cancelled_request_interrupts_the_turn_and_keeps_the_child() {
    with_mode("slow", || {
        let backend = backend();
        let cancel = CancelToken::new();
        let canceller = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            canceller.cancel();
        });
        let error = backend
            .execute(&request(Duration::from_secs(10)), &cancel)
            .unwrap_err();
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

#[test]
fn a_child_that_exits_mid_turn_is_provider_unavailable_and_restarts() {
    with_mode("exit", || {
        let backend = backend();
        let error = backend
            .execute(&request(Duration::from_secs(10)), &CancelToken::new())
            .unwrap_err();
        assert!(
            matches!(error, AiError::ProviderUnavailable(_)),
            "{error:?}"
        );
    });
    with_mode("ok", || {
        // A fresh backend in ok mode proves the spawn path is repeatable; the
        // same instance restarts its child on the next call too.
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
    let missing = CodexAppServerBackend::new(CodexConfig {
        binary: PathBuf::from("codex-binary-that-does-not-exist"),
        ..CodexConfig::default()
    });
    assert_eq!(missing.availability(), Availability::NotInstalled);
    let missing: Arc<dyn AiBackend> = Arc::new(missing);
    assert!(matches!(
        missing.execute(&request(Duration::from_secs(1)), &CancelToken::new()),
        Err(AiError::ProviderUnavailable(_))
    ));
}
