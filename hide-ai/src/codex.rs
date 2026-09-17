//! `codex app-server` over stdio, newline-delimited JSON-RPC 2.0.
//!
//! Protocol facts come from `codex app-server generate-json-schema` for the
//! installed CLI, not from guesswork; the request and notification names used
//! here are listed in `agents/runs/hide-ai-provider-layer/CONTRACT-2026-09-10.md`.
//!
//! This backend owns the app-server it starts (`oh-my-principle` resident
//! process practice): it runs the child under a private `CODEX_HOME` that
//! carries only an `auth.json` symlink, so no `config.toml` pulls MCP servers
//! into the tree; it shuts the child down through one graceful path on every
//! exit; it ends the child after ten idle minutes; and it exposes the process
//! measurement the router's cap is enforced against.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::log::AiLogEvent;
use crate::process::{self, ProcessMeasurement};
use crate::{
    AiBackend, AiError, AiLogSink, AiRequest, AiResponse, AiUsage, Availability, CancelToken,
    ModelCatalog, ProviderId,
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

/// Each stage of the graceful shutdown waits this long before escalating:
/// stdin close, then SIGTERM, then SIGKILL. Measured (2026-09-17): stdin close
/// or SIGTERM ends the whole codex tree within three seconds.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// The app-server is shut down after this long with no completed request. The
/// 2026-09-17 incident ran two idle days holding 1,699 processes; a count cap
/// does nothing while requests are zero, so idle time is the release that does.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// How often the idle reaper checks the last-activity clock.
const REAPER_TICK: Duration = Duration::from_secs(15);

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

/// A private `CODEX_HOME` for one app-server: an owner-only directory holding
/// nothing but a symlink to the user's `auth.json`. It carries no
/// `config.toml`, so the app-server starts none of the MCP servers the user's
/// real config declares, and it is removed when the session ends. The
/// credential file itself is only referenced, never read.
struct CodexHome {
    path: PathBuf,
}

impl CodexHome {
    fn create() -> Result<Self, AiError> {
        let dir = std::env::temp_dir().join(format!(
            "hide-ai-codex-home-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        create_private_dir(&dir).map_err(|kind| home_unavailable("mkdir", kind))?;
        let home = Self { path: dir };
        link_auth_json(&home.path).map_err(|kind| home_unavailable("symlink", kind))?;
        Ok(home)
    }
}

impl Drop for CodexHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn home_unavailable(stage: &str, kind: std::io::ErrorKind) -> AiError {
    AiError::ProviderUnavailable(format!("codex_home_unavailable:{stage}:{kind}"))
}

/// The directory the user's real `auth.json` lives in: an explicit
/// `CODEX_HOME`, or `~/.codex`.
fn source_codex_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CODEX_HOME") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    home.unwrap_or_default().join(".codex")
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<(), std::io::ErrorKind> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(false)
        .mode(0o700)
        .create(dir)
        .map_err(|error| error.kind())
}

#[cfg(not(unix))]
fn create_private_dir(_dir: &Path) -> Result<(), std::io::ErrorKind> {
    Err(std::io::ErrorKind::Unsupported)
}

#[cfg(unix)]
fn link_auth_json(home: &Path) -> Result<(), std::io::ErrorKind> {
    let source = source_codex_dir().join("auth.json");
    std::os::unix::fs::symlink(source, home.join("auth.json")).map_err(|error| error.kind())
}

#[cfg(not(unix))]
fn link_auth_json(_home: &Path) -> Result<(), std::io::ErrorKind> {
    Err(std::io::ErrorKind::Unsupported)
}

struct Session {
    child: Child,
    /// Held in an `Option` so shutdown can close the pipe (its EOF is the
    /// app-server's own shutdown signal) while the child is still owned.
    stdin: Option<ChildStdin>,
    incoming: Receiver<Line>,
    /// Notifications that arrived while a response was awaited.
    pending: VecDeque<Value>,
    next_id: u64,
    /// When the last turn finished, for the idle reaper.
    last_activity: Instant,
    /// Removed when the session ends; declared last so it is deleted after the
    /// child has stopped.
    _home: CodexHome,
}

pub struct CodexAppServerBackend {
    config: CodexConfig,
    /// One child, one turn at a time: a background feature never competes
    /// with itself for the account's rate limit. Shared with the idle reaper.
    session: Arc<Mutex<Option<Session>>>,
    /// The measurement taken after the most recent turn.
    measurement: Mutex<ProcessMeasurement>,
    reaper_stop: Arc<AtomicBool>,
    reaper: Mutex<Option<JoinHandle<()>>>,
}

impl CodexAppServerBackend {
    pub fn new(config: CodexConfig, sink: Arc<dyn AiLogSink>) -> Self {
        let session: Arc<Mutex<Option<Session>>> = Arc::new(Mutex::new(None));
        let reaper_stop = Arc::new(AtomicBool::new(false));
        let reaper = spawn_idle_reaper(Arc::clone(&session), sink, Arc::clone(&reaper_stop));
        Self {
            config,
            session,
            measurement: Mutex::new(ProcessMeasurement::Unavailable),
            reaper_stop,
            reaper: Mutex::new(Some(reaper)),
        }
    }

    /// The exact argument vector the child is started with. `mcp_servers` is
    /// not overridden here: the private `CODEX_HOME` carries no `config.toml`,
    /// so the app-server has no MCP servers to start, which `-c mcp_servers={}`
    /// was measured (2026-09-17) not to achieve.
    pub fn spawn_arguments() -> Vec<String> {
        let mut args = vec![
            "app-server".to_owned(),
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
        let pid = session.child.id();
        let result = self.run_turn(session, request, cancel);
        session.last_activity = Instant::now();
        // A refusal or a lost connection means the child is gone; measure the
        // live tree otherwise, while the lock still holds the session still.
        let child_gone = matches!(
            result,
            Err(AiError::ProviderUnavailable(_) | AiError::CompletionUnknown(_))
        );
        *self.measurement.lock().unwrap_or_else(|e| e.into_inner()) = if child_gone {
            ProcessMeasurement::Unavailable
        } else {
            process::measure(pid)
        };
        if child_gone {
            // The next request starts a fresh one.
            *guard = None;
        }
        result
    }

    fn last_measurement(&self) -> ProcessMeasurement {
        *self.measurement.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn restart(&self) {
        let victim = self.lock().take();
        // Drop outside the lock: shutdown may take a few seconds and no other
        // request can start one meanwhile.
        drop(victim);
        *self.measurement.lock().unwrap_or_else(|e| e.into_inner()) =
            ProcessMeasurement::Unavailable;
    }
}

impl Drop for CodexAppServerBackend {
    fn drop(&mut self) {
        self.reaper_stop.store(true, Ordering::SeqCst);
        // End the child now rather than waiting for the reaper to notice.
        let victim = self.lock().take();
        drop(victim);
        if let Some(handle) = self.reaper.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = handle.join();
        }
    }
}

/// Watches the last-activity clock and ends the app-server once it has been
/// idle past [`IDLE_TIMEOUT`], releasing it during the quiet the 2026-09-17
/// incident held it through.
fn spawn_idle_reaper(
    session: Arc<Mutex<Option<Session>>>,
    sink: Arc<dyn AiLogSink>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("hide-ai-codex-idle".to_owned())
        .spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                // Sleep in short steps so the thread joins promptly on drop
                // (a CLI that builds a router and exits must not wait a whole
                // tick), while still only checking idle once per tick.
                let mut waited = Duration::ZERO;
                while waited < REAPER_TICK {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                    waited += Duration::from_millis(200);
                }
                let victim = {
                    let mut guard = session.lock().unwrap_or_else(|e| e.into_inner());
                    match guard.as_ref() {
                        Some(active) if active.last_activity.elapsed() >= IDLE_TIMEOUT => {
                            guard.take()
                        }
                        _ => None,
                    }
                };
                if let Some(active) = victim {
                    let pid = active.child.id();
                    // Drop outside the lock so the graceful shutdown does not
                    // hold requests.
                    drop(active);
                    let mut event = AiLogEvent::new("ai.app_server.idle_exit");
                    event.provider = Some(ProviderId::Codex);
                    event.app_server_pid = Some(pid);
                    event.detail = Some(format!("idle_s={}", IDLE_TIMEOUT.as_secs()));
                    sink.log(event);
                }
            }
        })
        .expect("idle reaper thread")
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
        let home = CodexHome::create()?;
        let mut command = Command::new(binary);
        command
            .args(CodexAppServerBackend::spawn_arguments())
            .current_dir(cwd)
            .env("CODEX_HOME", &home.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = process::spawn(&mut command).map_err(|error| {
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
            stdin: Some(stdin),
            incoming,
            pending: VecDeque::new(),
            next_id: 0,
            last_activity: Instant::now(),
            _home: home,
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
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AiError::ProviderUnavailable("app_server_stdin_closed".to_owned()))?;
        stdin
            .write_all(&line)
            .and_then(|()| stdin.flush())
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

    /// Ends the child through one path: close stdin (its EOF is the
    /// app-server's shutdown signal and ends the whole tree), then escalate to
    /// SIGTERM and finally SIGKILL if it has not exited. Every exit path -
    /// `Drop`, a session swap, an over-budget restart - goes through here.
    fn terminate(&mut self) {
        let pid = self.child.id();
        // 1. stdin EOF: the app-server's own graceful shutdown.
        self.stdin = None;
        if self.wait_for_exit(SHUTDOWN_GRACE) {
            return;
        }
        // 2. SIGTERM.
        signal(pid, term_signal());
        if self.wait_for_exit(SHUTDOWN_GRACE) {
            return;
        }
        // 3. SIGKILL, and reap so no zombie is left.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn wait_for_exit(&mut self, grace: Duration) -> bool {
        let deadline = Instant::now() + grace;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => {}
                Err(_) => return false,
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(POLL.min(deadline - Instant::now()).max(Duration::from_millis(1)));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(unix)]
fn term_signal() -> i32 {
    libc::SIGTERM
}

#[cfg(not(unix))]
fn term_signal() -> i32 {
    15
}

#[cfg(unix)]
fn signal(pid: u32, sig: i32) {
    // SAFETY: `kill` with a pid we started and a plain signal number.
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

#[cfg(not(unix))]
fn signal(_pid: u32, _sig: i32) {}

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
