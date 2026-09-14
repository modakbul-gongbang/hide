use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::model::{ProviderUsageBucketSnapshot, ProviderUsageSnapshot};

pub const WEEKLY_WINDOW_MINUTES: u64 = 10_080;

const WEEKLY_WINDOW_SECONDS: u64 = 604_800;
const INITIAL_DELAY: Duration = Duration::from_secs(1);
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const POPOVER_REFRESH_AGE: Duration = Duration::from_secs(60);
const STALE_LIMIT_MS: u64 = 15 * 60 * 1_000;
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(3);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const CODEX_TAIL_BYTES: u64 = 2 * 1024 * 1024;
const CODEX_CANDIDATE_LIMIT: usize = 32;
const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const RETRY_BACKOFF: [Duration; 3] = [
    Duration::from_secs(5 * 60),
    Duration::from_secs(10 * 60),
    Duration::from_secs(15 * 60),
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct UsageActivity {
    pub window_visible: bool,
    pub popover_open_generation: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct UsagePaths {
    pub home: Option<PathBuf>,
    pub claude_config_dir: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct UsageValue {
    label: String,
    used_percent: f64,
    resets_at_unix_seconds: u64,
}

#[derive(Clone, Debug)]
struct SuccessfulUsage {
    main: UsageValue,
    buckets: Vec<UsageValue>,
    checked_at_unix_ms: u64,
}

#[derive(Clone, Debug)]
struct SessionFallback {
    value: UsageValue,
    source_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureDisposition {
    Offline,
    Authentication,
    Schema,
}

#[derive(Clone, Debug)]
struct ProviderState {
    provider: &'static str,
    label: &'static str,
    last_success: Option<SuccessfulUsage>,
    last_checked_at_unix_ms: Option<u64>,
    last_attempt: Option<Instant>,
    failure: Option<(FailureDisposition, &'static str)>,
    retry_at: Option<Instant>,
    backoff_index: usize,
}

impl ProviderState {
    fn new(provider: &'static str, label: &'static str) -> Self {
        Self {
            provider,
            label,
            last_success: None,
            last_checked_at_unix_ms: None,
            last_attempt: None,
            failure: None,
            retry_at: None,
            backoff_index: 0,
        }
    }

    fn can_attempt(&self, now: Instant) -> bool {
        self.retry_at.is_none_or(|retry_at| now >= retry_at)
    }

    fn record_success(&mut self, success: SuccessfulUsage, now: Instant) {
        self.last_checked_at_unix_ms = Some(success.checked_at_unix_ms);
        self.last_attempt = Some(now);
        self.last_success = Some(success);
        self.failure = None;
        self.retry_at = None;
        self.backoff_index = 0;
    }

    fn record_failure(&mut self, failure: FetchFailure, now: Instant, checked_at: u64) {
        self.last_checked_at_unix_ms = Some(checked_at);
        self.last_attempt = Some(now);
        self.failure = Some((failure.disposition, failure.kind));
        if failure.status == Some(429) {
            let delay = failure.retry_after.unwrap_or_else(|| {
                let delay = RETRY_BACKOFF[self.backoff_index.min(RETRY_BACKOFF.len() - 1)];
                self.backoff_index = (self.backoff_index + 1).min(RETRY_BACKOFF.len() - 1);
                delay
            });
            self.retry_at = Some(now + delay);
        } else {
            self.retry_at = None;
        }
        log_failure(self.provider, failure.status, failure.kind);
    }
}

pub struct ProviderUsageReader {
    paths: UsagePaths,
    started_at: Instant,
    last_seen_popover_generation: u64,
    claude: ProviderState,
    codex: ProviderState,
    codex_fallback: Option<SessionFallback>,
    published: Vec<ProviderUsageSnapshot>,
    http: UreqUsageClient,
}

impl ProviderUsageReader {
    pub(crate) fn new(paths: UsagePaths) -> Self {
        Self {
            paths,
            started_at: Instant::now(),
            last_seen_popover_generation: 0,
            claude: ProviderState::new("claude", "Claude Code"),
            codex: ProviderState::new("codex", "Codex"),
            codex_fallback: None,
            published: ProviderUsageSnapshot::initial_rows(),
            http: UreqUsageClient::new(),
        }
    }

    /// Performs credential, network and fallback reads on the coordinator
    /// thread, outside `Mutex<Runtime>`, and publishes only a changed answer.
    pub(crate) fn read_if_due(
        &mut self,
        activity: UsageActivity,
    ) -> Option<Vec<ProviderUsageSnapshot>> {
        let now = Instant::now();
        let popover_opened = activity.popover_open_generation > self.last_seen_popover_generation;
        self.last_seen_popover_generation = activity.popover_open_generation;
        let initial_due = self.claude.last_attempt.is_none()
            && self.codex.last_attempt.is_none()
            && now.duration_since(self.started_at) >= INITIAL_DELAY;
        let claude_due = provider_due(&self.claude, now, activity.window_visible, popover_opened);
        let codex_due = provider_due(&self.codex, now, activity.window_visible, popover_opened);

        if initial_due || claude_due {
            self.refresh_claude(now);
        }
        if initial_due || codex_due {
            self.refresh_codex(now);
        }

        let now_unix_ms = unix_milliseconds();
        let next = vec![
            project_provider(&self.claude, None, now_unix_ms),
            project_provider(&self.codex, self.codex_fallback.clone(), now_unix_ms),
        ];
        if next == self.published {
            return None;
        }
        self.published = next.clone();
        Some(next)
    }

    fn refresh_claude(&mut self, now: Instant) {
        if !self.claude.can_attempt(now) {
            return;
        }
        let checked_at = unix_milliseconds();
        let token = match read_claude_token(&self.paths) {
            Ok(token) => token,
            Err(kind) => {
                self.claude
                    .record_failure(FetchFailure::authentication(kind), now, checked_at);
                return;
            }
        };
        let authorization = format!("Bearer {token}");
        let response = self.http.get(
            CLAUDE_USAGE_URL,
            &[
                ("Authorization", authorization.as_str()),
                ("anthropic-beta", "oauth-2025-04-20"),
            ],
        );
        match response.and_then(HttpResponse::success_body) {
            Ok(body) => match parse_claude_usage(&body, checked_at) {
                Ok(success) => self.claude.record_success(success, now),
                Err(failure) => self.claude.record_failure(failure, now, checked_at),
            },
            Err(failure) => self.claude.record_failure(failure, now, checked_at),
        }
    }

    fn refresh_codex(&mut self, now: Instant) {
        if !self.codex.can_attempt(now) {
            return;
        }
        let checked_at = unix_milliseconds();
        let credentials = match read_codex_credentials(&self.paths) {
            Ok(credentials) => credentials,
            Err(kind) => {
                self.codex
                    .record_failure(FetchFailure::authentication(kind), now, checked_at);
                self.refresh_codex_fallback(checked_at);
                return;
            }
        };
        let authorization = format!("Bearer {}", credentials.access_token);
        let response = self.http.get(
            CODEX_USAGE_URL,
            &[
                ("Authorization", authorization.as_str()),
                ("User-Agent", "codex-cli"),
                ("OpenAI-Beta", "codex-1"),
                ("originator", "Codex Desktop"),
                ("ChatGPT-Account-Id", credentials.account_id.as_str()),
            ],
        );
        match response.and_then(HttpResponse::success_body) {
            Ok(body) => match parse_codex_usage(&body, checked_at) {
                Ok(success) => {
                    self.codex.record_success(success, now);
                    self.codex_fallback = None;
                }
                Err(failure) => {
                    self.codex.record_failure(failure, now, checked_at);
                    self.refresh_codex_fallback(checked_at);
                }
            },
            Err(failure) => {
                self.codex.record_failure(failure, now, checked_at);
                self.refresh_codex_fallback(checked_at);
            }
        }
    }

    fn refresh_codex_fallback(&mut self, checked_at: u64) {
        self.codex_fallback =
            read_codex_session_fallback(self.paths.codex_home.as_deref(), checked_at);
    }
}

fn provider_due(
    state: &ProviderState,
    now: Instant,
    window_visible: bool,
    popover_opened: bool,
) -> bool {
    let Some(last_attempt) = state.last_attempt else {
        return false;
    };
    (window_visible && now.duration_since(last_attempt) >= REFRESH_INTERVAL)
        || (popover_opened && now.duration_since(last_attempt) > POPOVER_REFRESH_AGE)
}

fn project_provider(
    state: &ProviderState,
    fallback: Option<SessionFallback>,
    now_unix_ms: u64,
) -> ProviderUsageSnapshot {
    let Some((disposition, kind)) = state.failure else {
        return state.last_success.as_ref().map_or_else(
            || ProviderUsageSnapshot::loading(state.provider, state.label),
            |success| snapshot_from_success(state, success, "available", None, now_unix_ms),
        );
    };

    if disposition == FailureDisposition::Offline
        && let Some(success) = state.last_success.as_ref()
        && now_unix_ms.saturating_sub(success.checked_at_unix_ms) <= STALE_LIMIT_MS
    {
        let minutes = now_unix_ms
            .saturating_sub(success.checked_at_unix_ms)
            .div_ceil(60_000);
        return snapshot_from_success(
            state,
            success,
            "stale",
            Some(format!("Last checked {minutes}m ago · offline")),
            now_unix_ms,
        );
    }

    if state.provider == "codex"
        && disposition != FailureDisposition::Schema
        && let Some(fallback) = fallback
    {
        return snapshot_from_fallback(state, fallback, kind, now_unix_ms);
    }

    let message = if disposition == FailureDisposition::Authentication {
        if state.provider == "claude" {
            "Sign in with claude to see usage".to_owned()
        } else {
            "Sign in with codex to see usage".to_owned()
        }
    } else if disposition == FailureDisposition::Schema {
        format!("{} weekly usage response is unavailable", state.label)
    } else {
        format!("{} weekly usage is unavailable · offline", state.label)
    };
    unavailable_snapshot(state, message, kind)
}

fn snapshot_from_success(
    state: &ProviderState,
    success: &SuccessfulUsage,
    projection_state: &str,
    message: Option<String>,
    now_unix_ms: u64,
) -> ProviderUsageSnapshot {
    if success.main.resets_at_unix_seconds <= now_unix_ms / 1_000 {
        let suffix = if projection_state == "stale" {
            " · offline"
        } else {
            ""
        };
        return unavailable_snapshot(
            state,
            format!(
                "{} weekly usage expired at its last reset{suffix}",
                success.main.label
            ),
            state.failure.map_or("expired", |(_, kind)| kind),
        );
    }
    let buckets = success
        .buckets
        .iter()
        .map(|bucket| {
            if bucket.resets_at_unix_seconds <= now_unix_ms / 1_000 {
                ProviderUsageBucketSnapshot {
                    label: bucket.label.clone(),
                    state: "unavailable".to_owned(),
                    used_percent: None,
                    resets_at_unix_seconds: None,
                    message: Some(format!(
                        "{} weekly usage expired at its last reset{}",
                        bucket.label,
                        if projection_state == "stale" {
                            " · offline"
                        } else {
                            ""
                        }
                    )),
                }
            } else {
                ProviderUsageBucketSnapshot {
                    label: bucket.label.clone(),
                    state: projection_state.to_owned(),
                    used_percent: Some(bucket.used_percent),
                    resets_at_unix_seconds: Some(bucket.resets_at_unix_seconds),
                    message: message.clone(),
                }
            }
        })
        .collect();
    ProviderUsageSnapshot {
        provider: state.provider.to_owned(),
        label: state.label.to_owned(),
        window_minutes: WEEKLY_WINDOW_MINUTES,
        state: projection_state.to_owned(),
        used_percent: Some(success.main.used_percent),
        resets_at_unix_seconds: Some(success.main.resets_at_unix_seconds),
        message,
        last_checked_at_unix_ms: state.last_checked_at_unix_ms,
        last_success_at_unix_ms: Some(success.checked_at_unix_ms),
        last_error_kind: state.failure.map(|(_, kind)| kind.to_owned()),
        buckets,
    }
}

fn snapshot_from_fallback(
    state: &ProviderState,
    fallback: SessionFallback,
    kind: &'static str,
    now_unix_ms: u64,
) -> ProviderUsageSnapshot {
    if fallback.value.resets_at_unix_seconds <= now_unix_ms / 1_000 {
        return unavailable_snapshot(
            state,
            format!(
                "{} weekly usage expired at its last reset · offline",
                fallback.value.label
            ),
            kind,
        );
    }
    ProviderUsageSnapshot {
        provider: state.provider.to_owned(),
        label: state.label.to_owned(),
        window_minutes: WEEKLY_WINDOW_MINUTES,
        state: "fallback".to_owned(),
        used_percent: Some(fallback.value.used_percent),
        resets_at_unix_seconds: Some(fallback.value.resets_at_unix_seconds),
        message: Some("From last Codex session".to_owned()),
        last_checked_at_unix_ms: state.last_checked_at_unix_ms,
        last_success_at_unix_ms: Some(fallback.source_at_unix_ms),
        last_error_kind: Some(kind.to_owned()),
        buckets: Vec::new(),
    }
}

fn unavailable_snapshot(
    state: &ProviderState,
    message: String,
    kind: &'static str,
) -> ProviderUsageSnapshot {
    let mut snapshot = ProviderUsageSnapshot::unavailable(
        state.provider,
        state.label,
        message,
        state.last_checked_at_unix_ms.unwrap_or_default(),
    );
    snapshot.last_error_kind = Some(kind.to_owned());
    snapshot
}

#[derive(Clone, Debug)]
struct FetchFailure {
    disposition: FailureDisposition,
    kind: &'static str,
    status: Option<u16>,
    retry_after: Option<Duration>,
}

impl FetchFailure {
    fn authentication(kind: &'static str) -> Self {
        Self {
            disposition: FailureDisposition::Authentication,
            kind,
            status: None,
            retry_after: None,
        }
    }

    fn schema(kind: &'static str) -> Self {
        Self {
            disposition: FailureDisposition::Schema,
            kind,
            status: None,
            retry_after: None,
        }
    }

    fn http(status: u16, retry_after: Option<Duration>) -> Self {
        Self {
            disposition: if status == 401 {
                FailureDisposition::Authentication
            } else {
                FailureDisposition::Offline
            },
            kind: if status == 429 {
                "rate_limited"
            } else {
                "http"
            },
            status: Some(status),
            retry_after,
        }
    }
}

struct HttpResponse {
    status: u16,
    retry_after: Option<Duration>,
    body: String,
}

impl HttpResponse {
    fn success_body(self) -> Result<String, FetchFailure> {
        if (200..300).contains(&self.status) {
            Ok(self.body)
        } else {
            Err(FetchFailure::http(self.status, self.retry_after))
        }
    }
}

struct UreqUsageClient {
    agent: ureq::Agent,
}

impl UreqUsageClient {
    fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .http_status_as_error(false)
            .build();
        Self {
            agent: config.into(),
        }
    }

    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, FetchFailure> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut response = request.call().map_err(|_| FetchFailure {
            disposition: FailureDisposition::Offline,
            kind: "transport",
            status: None,
            retry_after: None,
        })?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after);
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|_| FetchFailure {
                disposition: FailureDisposition::Offline,
                kind: "body_read",
                status: Some(status),
                retry_after,
            })?;
        Ok(HttpResponse {
            status,
            retry_after,
            body,
        })
    }
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

fn parse_claude_usage(body: &str, checked_at: u64) -> Result<SuccessfulUsage, FetchFailure> {
    let value =
        serde_json::from_str::<Value>(body).map_err(|_| FetchFailure::schema("response_json"))?;
    let main = parse_claude_value(
        value
            .get("seven_day")
            .ok_or_else(|| FetchFailure::schema("weekly_missing"))?,
        "Claude Code",
    )?;
    let mut buckets = Vec::new();
    if let Some(limits) = value.get("limits").and_then(Value::as_array) {
        for limit in limits
            .iter()
            .filter(|limit| limit.get("kind").and_then(Value::as_str) == Some("weekly_scoped"))
        {
            let label = limit
                .pointer("/scope/model/display_name")
                .and_then(Value::as_str)
                .filter(|label| !label.trim().is_empty())
                .ok_or_else(|| FetchFailure::schema("scoped_label"))?;
            buckets.push(parse_claude_value(limit, label)?);
        }
    }
    Ok(SuccessfulUsage {
        main,
        buckets,
        checked_at_unix_ms: checked_at,
    })
}

fn parse_claude_value(value: &Value, label: &str) -> Result<UsageValue, FetchFailure> {
    let used_percent = value
        .get("utilization")
        .and_then(Value::as_f64)
        .filter(|percent| percent.is_finite() && (0.0..=100.0).contains(percent))
        .ok_or_else(|| FetchFailure::schema("utilization"))?;
    let reset = value
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339)
        .ok_or_else(|| FetchFailure::schema("reset_time"))?;
    Ok(UsageValue {
        label: label.to_owned(),
        used_percent,
        resets_at_unix_seconds: reset,
    })
}

fn parse_codex_usage(body: &str, checked_at: u64) -> Result<SuccessfulUsage, FetchFailure> {
    let value =
        serde_json::from_str::<Value>(body).map_err(|_| FetchFailure::schema("response_json"))?;
    let windows = [
        value.pointer("/rate_limit/primary_window"),
        value.pointer("/rate_limit/secondary_window"),
        value.get("primary_window"),
        value.get("secondary_window"),
    ];
    let weekly = windows
        .into_iter()
        .flatten()
        .find(|window| {
            window.get("limit_window_seconds").and_then(Value::as_u64)
                == Some(WEEKLY_WINDOW_SECONDS)
        })
        .ok_or_else(|| FetchFailure::schema("weekly_missing"))?;
    let used_percent = weekly
        .get("used_percent")
        .and_then(Value::as_f64)
        .filter(|percent| percent.is_finite() && (0.0..=100.0).contains(percent))
        .ok_or_else(|| FetchFailure::schema("utilization"))?;
    let reset = weekly
        .get("reset_at")
        .or_else(|| weekly.get("resets_at"))
        .and_then(Value::as_u64)
        .ok_or_else(|| FetchFailure::schema("reset_time"))?;
    Ok(SuccessfulUsage {
        main: UsageValue {
            label: "Codex".to_owned(),
            used_percent,
            resets_at_unix_seconds: reset,
        },
        buckets: Vec::new(),
        checked_at_unix_ms: checked_at,
    })
}

fn read_claude_token(paths: &UsagePaths) -> Result<String, &'static str> {
    let config_dir = paths
        .claude_config_dir
        .clone()
        .or_else(|| paths.home.as_ref().map(|home| home.join(".claude")))
        .ok_or("credential_path")?;
    let service = claude_config_service(&config_dir).map_err(|_| "credential_service")?;
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("hide-claude-keychain".to_owned())
        .spawn(move || {
            let result = read_claude_keychain_service(&service)
                .or_else(|| read_claude_keychain_service("Claude Code-credentials"));
            let _ = sender.send(result);
        })
        .map_err(|_| "keychain_worker")?;
    if let Ok(Some(token)) = receiver.recv_timeout(KEYCHAIN_TIMEOUT) {
        return Ok(token);
    }
    read_claude_credential_file(&config_dir).ok_or("credentials_missing")
}

#[cfg(target_os = "macos")]
fn read_claude_keychain_service(service: &str) -> Option<String> {
    let account = login_account_name()?;
    let bytes = security_framework::passwords::get_generic_password(service, &account).ok()?;
    parse_claude_credential_bytes(&bytes)
}

#[cfg(not(target_os = "macos"))]
fn read_claude_keychain_service(_service: &str) -> Option<String> {
    None
}

fn read_claude_credential_file(config_dir: &Path) -> Option<String> {
    fs::read(config_dir.join(".credentials.json"))
        .ok()
        .and_then(|bytes| parse_claude_credential_bytes(&bytes))
}

fn parse_claude_credential_bytes(bytes: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(bytes)
        .ok()?
        .pointer("/claudeAiOauth/accessToken")?
        .as_str()
        .filter(|token| !token.trim().is_empty())
        .map(str::to_owned)
}

struct CodexCredentials {
    access_token: String,
    account_id: String,
}

fn read_codex_credentials(paths: &UsagePaths) -> Result<CodexCredentials, &'static str> {
    let root = paths
        .codex_home
        .clone()
        .or_else(|| paths.home.as_ref().map(|home| home.join(".codex")))
        .ok_or("credential_path")?;
    let value = fs::read(root.join("auth.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .ok_or("credentials_missing")?;
    let tokens = value.get("tokens").ok_or("credentials_schema")?;
    let access_token = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("credentials_schema")?
        .to_owned();
    let account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("credentials_schema")?
        .to_owned();
    Ok(CodexCredentials {
        access_token,
        account_id,
    })
}

fn read_codex_session_fallback(
    codex_home: Option<&Path>,
    checked_at: u64,
) -> Option<SessionFallback> {
    let sessions_root = codex_home?.join("sessions");
    let candidates = newest_codex_session_files(&sessions_root).ok()?;
    for path in candidates {
        let Ok(tail) = read_tail(&path, CODEX_TAIL_BYTES) else {
            continue;
        };
        if let Some((used_percent, resets_at, source_at)) = parse_latest_codex_weekly_usage(&tail) {
            return Some(SessionFallback {
                value: UsageValue {
                    label: "Codex".to_owned(),
                    used_percent,
                    resets_at_unix_seconds: resets_at,
                },
                source_at_unix_ms: source_at.unwrap_or(checked_at),
            });
        }
    }
    None
}

fn parse_latest_codex_weekly_usage(contents: &str) -> Option<(f64, u64, Option<u64>)> {
    contents.lines().rev().find_map(|line| {
        let value = serde_json::from_str::<Value>(line).ok()?;
        if value.get("type").and_then(Value::as_str) != Some("event_msg")
            || value.pointer("/payload/type").and_then(Value::as_str) != Some("token_count")
        {
            return None;
        }
        let limits = value.pointer("/payload/rate_limits")?;
        let weekly = ["primary", "secondary"].into_iter().find_map(|name| {
            let window = limits.get(name)?;
            (window.get("window_minutes").and_then(Value::as_u64) == Some(WEEKLY_WINDOW_MINUTES))
                .then_some(window)
        })?;
        let source_at = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339)
            .map(|seconds| seconds.saturating_mul(1_000));
        Some((
            weekly.get("used_percent").and_then(Value::as_f64)?,
            weekly.get("resets_at").and_then(Value::as_u64)?,
            source_at,
        ))
    })
}

fn newest_codex_session_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut level = root.to_path_buf();
    for _ in 0..3 {
        let children = numeric_child_directories(&level)?;
        let Some(next) = children.into_iter().max() else {
            return Err(format!(
                "no dated session directory under {}",
                level.display()
            ));
        };
        level.push(next);
    }
    let entries = fs::read_dir(&level)
        .map_err(|error| format!("could not read {}: {error}", level.display()))?;
    let mut candidates = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("jsonl"))
                .then(|| Some((entry.metadata().ok()?.modified().ok()?, path)))?
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    candidates.truncate(CODEX_CANDIDATE_LIMIT);
    Ok(candidates.into_iter().map(|(_, path)| path).collect())
}

fn numeric_child_directories(path: &Path) -> Result<Vec<String>, String> {
    let entries = fs::read_dir(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.chars()
                .all(|character| character.is_ascii_digit())
                .then_some(name)
        })
        .collect())
}

fn read_tail(path: &Path, maximum_bytes: u64) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let length = file
        .metadata()
        .map_err(|error| format!("could not stat {}: {error}", path.display()))?
        .len();
    let start = length.saturating_sub(maximum_bytes);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("could not seek {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut contents = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0
        && let Some(first_newline) = contents.find('\n')
    {
        contents.drain(..=first_newline);
    }
    Ok(contents)
}

fn parse_rfc3339(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !matches!(bytes.get(10), Some(b'T' | b't'))
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let number = |start: usize, end: usize| value.get(start..end)?.parse::<i64>().ok();
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    let mut cursor = 19;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == start {
            return None;
        }
    }
    let offset = match bytes.get(cursor)? {
        b'Z' | b'z' if cursor + 1 == bytes.len() => 0,
        sign @ (b'+' | b'-')
            if cursor + 6 == bytes.len() && bytes.get(cursor + 3) == Some(&b':') =>
        {
            let offset_hour = number(cursor + 1, cursor + 3)?;
            let offset_minute = number(cursor + 4, cursor + 6)?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            let seconds = offset_hour * 3_600 + offset_minute * 60;
            if *sign == b'+' { seconds } else { -seconds }
        }
        _ => return None,
    };
    let timestamp = days_from_civil(year, month, day)
        .checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?
        .checked_sub(offset)?;
    u64::try_from(timestamp).ok()
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(target_os = "macos")]
fn login_account_name() -> Option<String> {
    let uid = unsafe { libc::getuid() };
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0_u8; 16 * 1_024];
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            entry.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return None;
    }
    let name = unsafe { std::ffi::CStr::from_ptr((*result).pw_name) };
    name.to_str()
        .ok()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

#[cfg(target_os = "macos")]
fn claude_config_service(config_dir: &Path) -> Result<String, String> {
    let normalized = normalize_nfc(&config_dir.to_string_lossy())?;
    let digest = sha256(normalized.as_bytes())?;
    let suffix = digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("Claude Code-credentials-{suffix}"))
}

#[cfg(not(target_os = "macos"))]
fn claude_config_service(_config_dir: &Path) -> Result<String, String> {
    Err("Claude keychain service names require macOS".to_owned())
}

#[cfg(target_os = "macos")]
fn sha256(bytes: &[u8]) -> Result<[u8; 32], String> {
    if bytes.len() > u32::MAX as usize {
        return Err("configuration path is too long to hash".to_owned());
    }
    let mut digest = [0_u8; 32];
    let result = unsafe {
        CC_SHA256(
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            digest.as_mut_ptr(),
        )
    };
    (!result.is_null())
        .then_some(digest)
        .ok_or_else(|| "SHA-256 failed".to_owned())
}

#[cfg(target_os = "macos")]
fn normalize_nfc(value: &str) -> Result<String, String> {
    const UTF8: u32 = 0x0800_0100;
    let source = unsafe {
        CFStringCreateWithBytes(
            std::ptr::null(),
            value.as_ptr(),
            value.len() as isize,
            UTF8,
            false,
        )
    };
    if source.is_null() {
        return Err("configuration path could not be normalized".to_owned());
    }
    let mutable = unsafe { CFStringCreateMutableCopy(std::ptr::null(), 0, source) };
    unsafe { CFRelease(source) };
    if mutable.is_null() {
        return Err("configuration path could not be normalized".to_owned());
    }
    unsafe { CFStringNormalize(mutable, 2) };
    let length = unsafe { CFStringGetLength(mutable) };
    let capacity = unsafe { CFStringGetMaximumSizeForEncoding(length, UTF8) } + 1;
    let mut output = vec![0_i8; capacity.max(1) as usize];
    let copied = unsafe { CFStringGetCString(mutable, output.as_mut_ptr(), capacity, UTF8) };
    unsafe { CFRelease(mutable) };
    if !copied {
        return Err("configuration path could not be normalized".to_owned());
    }
    unsafe { std::ffi::CStr::from_ptr(output.as_ptr()) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "normalized configuration path is not UTF-8".to_owned())
}

#[cfg(target_os = "macos")]
#[link(name = "System")]
unsafe extern "C" {
    fn CC_SHA256(data: *const std::ffi::c_void, len: u32, digest: *mut u8) -> *mut u8;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithBytes(
        allocator: *const std::ffi::c_void,
        bytes: *const u8,
        count: isize,
        encoding: u32,
        external_representation: bool,
    ) -> *const std::ffi::c_void;
    fn CFStringCreateMutableCopy(
        allocator: *const std::ffi::c_void,
        capacity: isize,
        source: *const std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn CFStringNormalize(value: *mut std::ffi::c_void, form: isize);
    fn CFStringGetLength(value: *const std::ffi::c_void) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(
        value: *const std::ffi::c_void,
        buffer: *mut i8,
        capacity: isize,
        encoding: u32,
    ) -> bool;
    fn CFRelease(value: *const std::ffi::c_void);
}

fn log_failure(provider: &str, status: Option<u16>, kind: &str) {
    crate::diagnostic!(json!({
        "component": "provider_usage",
        "provider": provider,
        "status": status,
        "kind": kind,
    }));
}

fn unix_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_fixture_projects_weekly_and_scoped_buckets_only() {
        let success = parse_claude_usage(
            r#"{
              "five_hour":{"utilization":91,"resets_at":"2026-09-15T01:00:00Z"},
              "seven_day":{"utilization":43.4,"resets_at":"2030-01-01T00:00:00Z"},
              "limits":[
                {"kind":"weekly_scoped","utilization":61,"resets_at":"2030-01-02T03:04:05+09:00","scope":{"model":{"display_name":"Fable"}}},
                {"kind":"monthly","utilization":2,"resets_at":"2030-02-01T00:00:00Z"}
              ]
            }"#,
            1_000,
        ).unwrap();
        assert_eq!(success.main.used_percent, 43.4);
        assert_eq!(success.main.resets_at_unix_seconds, 1_893_456_000);
        assert_eq!(success.buckets.len(), 1);
        assert_eq!(success.buckets[0].label, "Fable");
        assert_eq!(success.buckets[0].resets_at_unix_seconds, 1_893_521_045);
    }

    #[test]
    fn malformed_scoped_bucket_is_an_observable_schema_failure() {
        let failure = parse_claude_usage(
            r#"{"seven_day":{"utilization":40,"resets_at":"2030-01-01T00:00:00Z"},"limits":[{"kind":"weekly_scoped","utilization":1}]}"#,
            1_000,
        ).unwrap_err();
        assert_eq!(failure.disposition, FailureDisposition::Schema);
        assert_eq!(failure.kind, "scoped_label");
    }

    #[test]
    fn codex_fixture_selects_window_by_duration_and_ignores_additional_limits() {
        let success = parse_codex_usage(
            r#"{
              "rate_limit":{"primary_window":{"used_percent":76,"limit_window_seconds":604800,"reset_at":1893456000},"secondary_window":null},
              "additional_rate_limits":[{"primary_window":{"used_percent":99,"limit_window_seconds":604800,"reset_at":1}}]
            }"#,
            1_000,
        ).unwrap();
        assert_eq!(success.main.used_percent, 76.0);
        assert_eq!(success.main.resets_at_unix_seconds, 1_893_456_000);
        assert!(success.buckets.is_empty());
    }

    #[test]
    fn rfc3339_parser_handles_utc_fraction_and_offsets() {
        assert_eq!(parse_rfc3339("2030-01-01T00:00:00Z"), Some(1_893_456_000));
        assert_eq!(
            parse_rfc3339("2030-01-01T09:00:00.123+09:00"),
            Some(1_893_456_000)
        );
        assert_eq!(parse_rfc3339("2026-02-29T00:00:00Z"), None);
    }

    #[test]
    fn codex_parser_chooses_the_exact_weekly_window_and_latest_event() {
        let contents = concat!(
            "{\"timestamp\":\"2026-09-14T12:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":12.0,\"window_minutes\":10080,\"resets_at\":4102444800}}}}\n",
            "{\"timestamp\":\"2026-09-14T12:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":90.0,\"window_minutes\":300,\"resets_at\":4102444800},\"secondary\":{\"used_percent\":58.0,\"window_minutes\":10080,\"resets_at\":4102444801}}}}\n",
        );
        assert_eq!(
            parse_latest_codex_weekly_usage(contents),
            Some((58.0, 4_102_444_801, Some(1_789_387_260_000)))
        );
    }

    #[test]
    fn stale_projection_expires_before_the_fifteen_minute_window() {
        let mut state = ProviderState::new("claude", "Claude Code");
        state.last_success = Some(SuccessfulUsage {
            main: UsageValue {
                label: "Claude Code".to_owned(),
                used_percent: 50.0,
                resets_at_unix_seconds: 100,
            },
            buckets: Vec::new(),
            checked_at_unix_ms: 90_000,
        });
        state.failure = Some((FailureDisposition::Offline, "transport"));
        let projected = project_provider(&state, None, 101_000);
        assert_eq!(projected.state, "unavailable");
        assert!(projected.message.unwrap().contains("expired"));
    }

    #[test]
    fn refresh_schedule_stops_hidden_ticks_and_observes_popover_age() {
        let now = Instant::now();
        let mut state = ProviderState::new("codex", "Codex");
        state.last_attempt = Some(now - REFRESH_INTERVAL);

        assert!(!provider_due(&state, now, false, false));
        assert!(provider_due(&state, now, true, false));

        state.last_attempt = Some(now - POPOVER_REFRESH_AGE - Duration::from_millis(1));
        assert!(provider_due(&state, now, false, true));
        assert!(!provider_due(&state, now, false, false));
    }

    #[test]
    fn rate_limit_retry_after_prevents_an_early_repeat() {
        let now = Instant::now();
        let mut state = ProviderState::new("claude", "Claude Code");
        state.record_failure(
            FetchFailure::http(429, Some(Duration::from_secs(90))),
            now,
            1_000,
        );

        assert!(!state.can_attempt(now + Duration::from_secs(89)));
        assert!(state.can_attempt(now + Duration::from_secs(90)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn config_service_hashes_nfc_path() {
        assert_eq!(
            claude_config_service(Path::new("/tmp/Cafe\u{301}")).unwrap(),
            claude_config_service(Path::new("/tmp/Café")).unwrap()
        );
    }
}
