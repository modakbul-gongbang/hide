//! `codex app-server` over stdio, newline-delimited JSON-RPC 2.0.
//!
//! Protocol facts come from `codex app-server generate-json-schema` for the
//! installed CLI, not from guesswork; the request and notification names used
//! here are listed in `agents/runs/hide-ai-provider-layer/CONTRACT-2026-09-10.md`.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::{
    AiBackend, AiError, AiRequest, AiResponse, AiUsage, Availability, CancelToken, ModelCatalog,
    ProviderId,
};

/// The user's decision for background features.
pub const DEFAULT_MODEL: &str = "gpt-5.6-luna";

/// Features that load tool definitions into every turn. Measured on
/// codex-cli 0.153.4: disabling them takes a label from 26,703 input tokens
/// to 11,898 on this model, and no config key disables the remainder.
const DISABLED_FEATURES: &[&str] = &[
    "shell_tool",
    "view_image",
    "sleep_tool",
    "unified_exec",
    "code_mode_host",
    "apps",
    "plugins",
    "browser_use",
    "browser_use_full_cdp_access",
    "browser_use_external",
    "computer_use",
    "image_generation",
    "multi_agent",
    "skill_search",
    "skill_mcp_dependency_install",
    "tool_suggest",
    "memories",
    "hooks",
    "goals",
    "guardian_approval",
    "mentions_v2",
    "personality",
    "in_app_browser",
    "in_app_chat",
    "in_app_dictation",
    "in_app_local_automation",
    "in_app_updates",
    "remote_plugin",
    "plugin_sharing",
    "fast_mode",
];

const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
const INTERRUPT_GRACE: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
pub struct CodexConfig {
    /// `codex` on `PATH` by default; an explicit path is used as given.
    pub binary: PathBuf,
    pub model: String,
    /// Working directory handed to every thread. A neutral directory keeps a
    /// project's own AGENTS.md out of the prompt.
    pub cwd: PathBuf,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("codex"),
            model: DEFAULT_MODEL.to_owned(),
            cwd: std::env::temp_dir(),
        }
    }
}

enum Line {
    Message(Value),
    Eof,
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    incoming: Receiver<Line>,
    /// Notifications that arrived while a response was awaited.
    pending: VecDeque<Value>,
    next_id: u64,
}

pub struct CodexAppServerBackend {
    config: CodexConfig,
    /// One child, one turn at a time: a background feature never competes
    /// with itself for the account's rate limit.
    session: Mutex<Option<Session>>,
}

impl CodexAppServerBackend {
    pub fn new(config: CodexConfig) -> Self {
        Self {
            config,
            session: Mutex::new(None),
        }
    }

    /// The exact argument vector the child is started with.
    pub fn spawn_arguments() -> Vec<String> {
        let mut args = vec![
            "app-server".to_owned(),
            "-c".to_owned(),
            "mcp_servers={}".to_owned(),
            "-c".to_owned(),
            "web_search=\"disabled\"".to_owned(),
        ];
        for feature in DISABLED_FEATURES {
            args.push("--disable".to_owned());
            args.push((*feature).to_owned());
        }
        args.push("--listen".to_owned());
        args.push("stdio://".to_owned());
        args
    }

    fn resolved_binary(&self) -> Option<PathBuf> {
        resolve_binary(&self.config.binary)
    }

    fn lock(&self) -> MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn ensure_session<'a>(
        &self,
        guard: &'a mut Option<Session>,
    ) -> Result<&'a mut Session, AiError> {
        if guard.as_mut().is_some_and(|session| session.is_alive()) {
            return Ok(guard.as_mut().expect("checked above"));
        }
        let binary = self
            .resolved_binary()
            .ok_or_else(|| AiError::ProviderUnavailable("codex_not_installed".to_owned()))?;
        *guard = Some(Session::spawn(&binary, &self.config.cwd)?);
        Ok(guard.as_mut().expect("just inserted"))
    }

    fn run_turn(
        &self,
        session: &mut Session,
        request: &AiRequest,
        cancel: &CancelToken,
    ) -> Result<AiResponse, AiError> {
        let deadline = Instant::now() + request.deadline;
        let thread = session.request(
            "thread/start",
            json!({
                "ephemeral": true,
                "sandbox": "read-only",
                "approvalPolicy": "never",
                "cwd": self.config.cwd,
                "model": self.config.model,
                "baseInstructions": request.system,
            }),
            CONTROL_TIMEOUT,
        )?;
        let thread_id = thread
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| AiError::Transient("thread_start_without_id".to_owned()))?
            .to_owned();
        // Once this is written the server may be running the turn. A lost
        // answer no longer says whether it completed, and only that answer
        // would make a retry safe; a rejection still does.
        let turn = session
            .request_raw(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [{"type": "text", "text": request.input}],
                    "outputSchema": request.output_schema,
                    "clientUserMessageId": request.request_id.0,
                }),
                CONTROL_TIMEOUT,
            )
            .map_err(RequestFailure::after_submission)?;
        let turn_id = turn
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .ok_or_else(|| AiError::CompletionUnknown("turn_start_without_id".to_owned()))?
            .to_owned();

        let mut usage = AiUsage::default();
        let mut text: Option<String> = None;
        loop {
            if cancel.is_cancelled() {
                session.interrupt(&thread_id, &turn_id);
                return Err(AiError::Cancelled);
            }
            let now = Instant::now();
            if now >= deadline {
                session.interrupt(&thread_id, &turn_id);
                return Err(AiError::Timeout);
            }
            let message = match session.next_message(POLL.min(deadline - now)) {
                Ok(message) => message,
                Err(Wait::Elapsed) => continue,
                Err(Wait::Gone(error)) => {
                    return Err(AiError::CompletionUnknown(error.to_string()));
                }
            };
            let Some(method) = message.get("method").and_then(Value::as_str) else {
                continue;
            };
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            if params.get("threadId").and_then(Value::as_str) != Some(thread_id.as_str()) {
                continue;
            }
            match method {
                "thread/tokenUsage/updated" => {
                    let last = params.pointer("/tokenUsage/last");
                    usage.input_tokens = last
                        .and_then(|v| v.get("inputTokens"))
                        .and_then(Value::as_u64);
                    usage.output_tokens = last
                        .and_then(|v| v.get("outputTokens"))
                        .and_then(Value::as_u64);
                }
                "item/completed" => {
                    if params.pointer("/item/type").and_then(Value::as_str) == Some("agentMessage")
                    {
                        text = params
                            .pointer("/item/text")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    }
                }
                "turn/completed" => {
                    let status = params.pointer("/turn/status").and_then(Value::as_str);
                    return match status {
                        Some("completed") => {
                            let text = text.ok_or_else(|| {
                                AiError::InvalidOutput("turn_completed_without_message".to_owned())
                            })?;
                            let value = serde_json::from_str::<Value>(&text).map_err(|_| {
                                AiError::InvalidOutput("message_not_json".to_owned())
                            })?;
                            Ok(AiResponse { value, usage })
                        }
                        Some("failed") => Err(self.classify_turn_error(
                            session,
                            params.pointer("/turn/error").unwrap_or(&Value::Null),
                        )),
                        Some("interrupted") => {
                            Err(AiError::Transient("turn_interrupted".to_owned()))
                        }
                        other => Err(AiError::InvalidOutput(format!(
                            "turn_status_{}",
                            other.unwrap_or("unknown")
                        ))),
                    };
                }
                _ => {}
            }
        }
    }

    /// Maps the typed `codexErrorInfo` to the caller's failure classes. No
    /// message text is inspected: the enum is the contract.
    fn classify_turn_error(&self, session: &mut Session, error: &Value) -> AiError {
        let info = error.get("codexErrorInfo").unwrap_or(&Value::Null);
        let http_status = [
            "httpConnectionFailed",
            "responseStreamConnectionFailed",
            "responseStreamDisconnected",
        ]
        .iter()
        .find_map(|key| info.pointer(&format!("/{key}/httpStatusCode")))
        .and_then(Value::as_u64);
        match (info.as_str(), http_status) {
            (Some("usageLimitExceeded" | "rateLimitExceeded"), _) | (_, Some(429)) => {
                AiError::UsageLimited {
                    retry_after: session.retry_after(),
                }
            }
            (Some("unauthorized"), _) | (_, Some(401 | 403)) => AiError::NotAuthenticated,
            (Some("contextWindowExceeded" | "badRequest" | "sessionBudgetExceeded"), _) => {
                AiError::InvalidOutput(format!("codex_{}", info.as_str().unwrap_or("bad_request")))
            }
            (Some(code), _) => AiError::Transient(format!("codex_{code}")),
            (None, Some(status)) => AiError::Transient(format!("codex_http_{status}")),
            (None, None) => AiError::Transient("codex_turn_failed".to_owned()),
        }
    }
}

impl AiBackend for CodexAppServerBackend {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    fn availability(&self) -> Availability {
        if self.resolved_binary().is_none() {
            return Availability::NotInstalled;
        }
        let mut guard = self.lock();
        let session = match self.ensure_session(&mut guard) {
            Ok(session) => session,
            Err(error) => {
                return Availability::Unavailable {
                    reason: error.to_string(),
                };
            }
        };
        let account = match session.request("account/read", json!({}), CONTROL_TIMEOUT) {
            Ok(account) => account,
            Err(error) => {
                return Availability::Unavailable {
                    reason: error.to_string(),
                };
            }
        };
        if account.get("account").is_none_or(Value::is_null) {
            return Availability::NeedsLogin;
        }
        match session.request("model/list", json!({}), CONTROL_TIMEOUT) {
            Ok(models) => {
                let offered = models
                    .get("data")
                    .and_then(Value::as_array)
                    .is_some_and(|models| {
                        models.iter().any(|m| {
                            m.get("id").and_then(Value::as_str) == Some(&self.config.model)
                        })
                    });
                if offered {
                    Availability::Ready
                } else {
                    Availability::Unavailable {
                        reason: format!("model_not_offered:{}", self.config.model),
                    }
                }
            }
            Err(error) => Availability::Unavailable {
                reason: error.to_string(),
            },
        }
    }

    /// `model/list` is the app-server's own answer, so nothing above this
    /// backend keeps a list of Codex models.
    fn models(&self) -> ModelCatalog {
        if self.resolved_binary().is_none() {
            return ModelCatalog::Unknown {
                reason: "codex_not_installed".to_owned(),
            };
        }
        let mut guard = self.lock();
        let session = match self.ensure_session(&mut guard) {
            Ok(session) => session,
            Err(error) => {
                return ModelCatalog::Unknown {
                    reason: error.to_string(),
                };
            }
        };
        match session.request("model/list", json!({}), CONTROL_TIMEOUT) {
            Ok(models) => match models.get("data").and_then(Value::as_array) {
                Some(offered) => ModelCatalog::Offered(
                    offered
                        .iter()
                        .filter_map(|model| model.get("id").and_then(Value::as_str))
                        .map(str::to_owned)
                        .collect(),
                ),
                // The field is the contract; without it the list is unknown
                // rather than empty.
                None => ModelCatalog::Unknown {
                    reason: "model_list_without_data".to_owned(),
                },
            },
            Err(error) => ModelCatalog::Unknown {
                reason: error.to_string(),
            },
        }
    }

    fn execute(&self, request: &AiRequest, cancel: &CancelToken) -> Result<AiResponse, AiError> {
        let mut guard = self.lock();
        let session = self.ensure_session(&mut guard)?;
        let result = self.run_turn(session, request, cancel);
        if matches!(
            result,
            Err(AiError::ProviderUnavailable(_) | AiError::CompletionUnknown(_))
        ) {
            // The child is gone; the next request starts a fresh one.
            *guard = None;
        }
        result
    }
}

enum Wait {
    Elapsed,
    Gone(AiError),
}

/// How a control request ended without a result. Only a rejection proves
/// the server declined the request; the other two leave its fate open,
/// which matters once the request is a turn.
enum RequestFailure {
    /// The server answered with a JSON-RPC error.
    Rejected { method: &'static str, code: i64 },
    /// No answer before the control timeout.
    Unanswered { method: &'static str },
    /// The child or its connection is gone.
    Gone(AiError),
}

impl RequestFailure {
    /// The classification for a request the server has not acted on yet.
    fn before_submission(self) -> AiError {
        match self {
            Self::Rejected { method, code } => {
                AiError::Transient(format!("rpc_error:{method}:{code}"))
            }
            Self::Unanswered { method } => AiError::Transient(format!("control_timeout:{method}")),
            Self::Gone(error) => error,
        }
    }

    /// The classification once the request may already be running.
    fn after_submission(self) -> AiError {
        match self {
            Self::Rejected { .. } => self.before_submission(),
            Self::Unanswered { method } => {
                AiError::CompletionUnknown(format!("control_timeout:{method}"))
            }
            Self::Gone(error) => AiError::CompletionUnknown(error.to_string()),
        }
    }
}

impl Session {
    fn spawn(binary: &Path, cwd: &Path) -> Result<Self, AiError> {
        let mut child = Command::new(binary)
            .args(CodexAppServerBackend::spawn_arguments())
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                AiError::ProviderUnavailable(format!("app_server_spawn_failed:{}", error.kind()))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AiError::ProviderUnavailable("app_server_no_stdin".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AiError::ProviderUnavailable("app_server_no_stdout".to_owned()))?;
        let (sender, incoming) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Ok(value) = serde_json::from_str::<Value>(&line)
                            && sender.send(Line::Message(value)).is_err()
                        {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = sender.send(Line::Eof);
        });
        let mut session = Self {
            child,
            stdin,
            incoming,
            pending: VecDeque::new(),
            next_id: 0,
        };
        session.request(
            "initialize",
            json!({"clientInfo": {"name": "hide-ai", "version": env!("CARGO_PKG_VERSION")}}),
            CONTROL_TIMEOUT,
        )?;
        session.notify("initialized", json!({}))?;
        Ok(session)
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn write(&mut self, message: &Value) -> Result<(), AiError> {
        let mut line = serde_json::to_vec(message)
            .map_err(|_| AiError::Transient("request_not_encodable".to_owned()))?;
        line.push(b'\n');
        self.stdin
            .write_all(&line)
            .and_then(|()| self.stdin.flush())
            .map_err(|error| {
                AiError::ProviderUnavailable(format!("app_server_write_failed:{}", error.kind()))
            })
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), AiError> {
        self.write(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// A control request the server has not acted on: any failure is a
    /// refusal and safe to repeat.
    fn request(
        &mut self,
        method: &'static str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, AiError> {
        self.request_raw(method, params, timeout)
            .map_err(RequestFailure::before_submission)
    }

    fn request_raw(
        &mut self,
        method: &'static str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RequestFailure> {
        self.next_id += 1;
        let id = self.next_id;
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .map_err(RequestFailure::Gone)?;
        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(RequestFailure::Unanswered { method });
            }
            let message = match self.recv(deadline - now) {
                Ok(message) => message,
                Err(Wait::Elapsed) => continue,
                Err(Wait::Gone(error)) => return Err(RequestFailure::Gone(error)),
            };
            if message.get("id").and_then(Value::as_u64) == Some(id)
                && message.get("method").is_none()
            {
                if let Some(error) = message.get("error") {
                    let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
                    return Err(RequestFailure::Rejected { method, code });
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            self.route(message).map_err(RequestFailure::Gone)?;
        }
    }

    /// Next notification, draining what a control request set aside first.
    fn next_message(&mut self, timeout: Duration) -> Result<Value, Wait> {
        if let Some(message) = self.pending.pop_front() {
            return Ok(message);
        }
        let message = self.recv(timeout)?;
        if message.get("id").is_some() && message.get("method").is_some() {
            self.route(message).map_err(Wait::Gone)?;
            return Err(Wait::Elapsed);
        }
        Ok(message)
    }

    fn recv(&mut self, timeout: Duration) -> Result<Value, Wait> {
        match self.incoming.recv_timeout(timeout) {
            Ok(Line::Message(message)) => Ok(message),
            Ok(Line::Eof) | Err(RecvTimeoutError::Disconnected) => {
                let status = match self.child.try_wait() {
                    Ok(Some(status)) => {
                        status.code().map_or("signal".to_owned(), |c| c.to_string())
                    }
                    _ => "unknown".to_owned(),
                };
                Err(Wait::Gone(AiError::ProviderUnavailable(format!(
                    "app_server_exited:{status}"
                ))))
            }
            Err(RecvTimeoutError::Timeout) => Err(Wait::Elapsed),
        }
    }

    /// A server-to-client request (an approval) cannot happen under
    /// `approvalPolicy: never` with a read-only sandbox; refuse it so the
    /// server never waits on us. Notifications are kept for the turn loop.
    fn route(&mut self, message: Value) -> Result<(), AiError> {
        if let (Some(id), Some(_)) = (message.get("id"), message.get("method")) {
            let id = id.clone();
            return self.write(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "hide-ai does not serve requests"},
            }));
        }
        self.pending.push_back(message);
        Ok(())
    }

    /// Asks the server to stop the turn and waits briefly for its completion
    /// so the next request starts on a quiet connection. The child lives on.
    fn interrupt(&mut self, thread_id: &str, turn_id: &str) {
        let _ = self.request(
            "turn/interrupt",
            json!({"threadId": thread_id, "turnId": turn_id}),
            INTERRUPT_GRACE,
        );
        let deadline = Instant::now() + INTERRUPT_GRACE;
        while Instant::now() < deadline {
            match self.next_message(POLL) {
                Ok(message)
                    if message.get("method").and_then(Value::as_str) == Some("turn/completed")
                        && message.pointer("/params/threadId").and_then(Value::as_str)
                            == Some(thread_id) =>
                {
                    return;
                }
                Ok(_) | Err(Wait::Elapsed) => {}
                Err(Wait::Gone(_)) => return,
            }
        }
    }

    /// Seconds until the primary window resets, when the server knows.
    fn retry_after(&mut self) -> Option<Duration> {
        let limits = self
            .request("account/rateLimits/read", json!({}), CONTROL_TIMEOUT)
            .ok()?;
        let resets_at = limits
            .pointer("/rateLimits/primary/resetsAt")
            .and_then(Value::as_u64)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        Some(Duration::from_secs(resets_at.saturating_sub(now)))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Closing stdin is the app-server's shutdown signal.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Shared by every backend that starts a user-installed CLI.
pub(crate) fn resolve_binary(binary: &Path) -> Option<PathBuf> {
    if binary.components().count() > 1 {
        return binary.is_file().then(|| binary.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}
