//! The toolbar's Weekly Usage rows, one per provider.
//!
//! Codex is read from its usage endpoint with the token in `auth.json`, and
//! from the newest session JSONL when that fails. Claude Code is read by
//! running `claude -p /usage` on a worker thread and parsing the text the
//! CLI prints: the CLI authenticates against its own keychain item, so Hide
//! never holds a token and macOS never asks it for one. The earlier direct
//! read of the keychain was denied on every rebuild, because an ad hoc
//! signed dev build has a new code identity each time.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hide_ai::{CancelToken, ClaudeCliBackend, ClaudeConfig, UsageError};
use hide_session::{newest_codex_session_files, parse_rfc3339, read_tail};
use serde_json::{Value, json};

use crate::model::{ProviderUsageBucketSnapshot, ProviderUsageSnapshot};
use crate::reader::BackgroundRead;
use crate::zoneinfo::{Zone, civil_from_days, days_from_civil, days_in_month};

pub const WEEKLY_WINDOW_MINUTES: u64 = 10_080;

const WEEKLY_WINDOW_SECONDS: u64 = 604_800;
const INITIAL_DELAY: Duration = Duration::from_secs(1);
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const POPOVER_REFRESH_AGE: Duration = Duration::from_secs(60);
const STALE_LIMIT_MS: u64 = 15 * 60 * 1_000;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const CODEX_TAIL_BYTES: u64 = 2 * 1024 * 1024;
/// The name the CLI is looked up by on `PATH`, and the name the row shows
/// when it is not there.
const CLAUDE_BINARY: &str = "claude";
/// A reset the CLI prints is at most one weekly window away. A wall time
/// that matched this recently is the reset that just passed (the CLI prints
/// minutes, and the read takes seconds), not the same date a year ahead, so
/// the row reads as expired until the next read.
const RESET_HORIZON_SECONDS: i64 = 8 * 86_400;
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
    /// Where the `claude` child runs: Hide's own state directory, so no
    /// project's `CLAUDE.md` or settings are discovered.
    pub claude_cwd: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct UsageValue {
    label: String,
    used_percent: f64,
    resets_at_unix_seconds: u64,
}

#[derive(Clone, Debug)]
enum UsageBucket {
    Available(UsageValue),
    Unavailable { label: String },
}

#[derive(Clone, Debug)]
struct SuccessfulUsage {
    main: UsageValue,
    buckets: Vec<UsageBucket>,
    checked_at_unix_ms: u64,
}

#[derive(Clone, Debug)]
struct SessionFallback {
    value: UsageValue,
    source_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureDisposition {
    /// A transient failure: a kept success stays on screen for a while.
    Offline,
    Authentication,
    /// The provider answered something the reader cannot use.
    Schema,
    /// The `claude` binary is not on `PATH`; looked up again on the next read.
    NotInstalled,
}

#[derive(Clone, Debug)]
struct ProviderState {
    provider: &'static str,
    label: &'static str,
    last_success: Option<SuccessfulUsage>,
    last_checked_at_unix_ms: Option<u64>,
    last_attempt: Option<Instant>,
    failure: Option<(FailureDisposition, String)>,
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
        log_failure(self.provider, failure.status, &failure.kind);
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
    }
}

/// The freshness key of one `claude -p /usage` read: a changed attempt
/// number is what starts the worker, so the schedule stays in this module
/// and the worker runs exactly once per attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ClaudeUsageRequest {
    attempt: u64,
}

struct ClaudeUsageAnswer {
    checked_at_unix_ms: u64,
    outcome: Result<SuccessfulUsage, FetchFailure>,
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
    /// The Claude read runs on this worker: a `claude` child takes seconds,
    /// and the coordinator thread that drives this reader is the one that
    /// applies every Herdr pane event.
    claude_read: BackgroundRead<ClaudeUsageRequest, ClaudeUsageAnswer>,
    claude_attempt: u64,
    /// Cancelled when the reader is dropped, so a child still running at
    /// shutdown is killed rather than left to finish on its own.
    claude_cancel: CancelToken,
}

impl ProviderUsageReader {
    pub(crate) fn new(paths: UsagePaths) -> Self {
        let claude_cancel = CancelToken::new();
        let claude_read = claude_background_read(paths.claude_cwd.clone(), claude_cancel.clone());
        Self {
            paths,
            started_at: Instant::now(),
            last_seen_popover_generation: 0,
            claude: ProviderState::new("claude", "Claude Code"),
            codex: ProviderState::new("codex", "Codex"),
            codex_fallback: None,
            published: ProviderUsageSnapshot::initial_rows(),
            http: UreqUsageClient::new(),
            claude_read,
            claude_attempt: 0,
            claude_cancel,
        }
    }

    /// Runs the Codex credential, network and fallback reads on the
    /// coordinator thread, outside `Mutex<Runtime>`, starts the Claude read
    /// on its worker and collects its answer on a later wake, and publishes
    /// only a changed answer.
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
        self.collect_claude(now);
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

    /// Starts one Claude read. The attempt is recorded now so the schedule
    /// does not ask again while the child runs; the answer lands through
    /// [`Self::collect_claude`].
    fn refresh_claude(&mut self, now: Instant) {
        if !self.claude.can_attempt(now) {
            return;
        }
        self.claude_attempt += 1;
        self.claude.last_attempt = Some(now);
    }

    fn collect_claude(&mut self, now: Instant) {
        if self.claude_attempt == 0 {
            return;
        }
        let request = ClaudeUsageRequest {
            attempt: self.claude_attempt,
        };
        let Some(answer) = self.claude_read.poll(request) else {
            return;
        };
        match answer.outcome {
            Ok(success) => self.claude.record_success(success, now),
            Err(failure) => self
                .claude
                .record_failure(failure, now, answer.checked_at_unix_ms),
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

impl Drop for ProviderUsageReader {
    fn drop(&mut self) {
        self.claude_cancel.cancel();
    }
}

/// The worker that runs `claude -p /usage` and parses its text. It takes
/// owned inputs, because it outlives any one `read_if_due` call.
fn claude_background_read(
    cwd: Option<PathBuf>,
    cancel: CancelToken,
) -> BackgroundRead<ClaudeUsageRequest, ClaudeUsageAnswer> {
    let backend = cwd.map(|cwd| {
        Arc::new(ClaudeCliBackend::new(ClaudeConfig {
            binary: PathBuf::from(CLAUDE_BINARY),
            cwd,
            ..ClaudeConfig::default()
        }))
    });
    BackgroundRead::on_change(Duration::ZERO, move |_: &ClaudeUsageRequest| {
        let checked_at_unix_ms = unix_milliseconds();
        let outcome = match backend.as_ref() {
            Some(backend) => backend
                .usage_text(&cancel)
                .map_err(FetchFailure::from_usage_error)
                .and_then(|text| {
                    parse_claude_usage_text(&text, unix_seconds(), checked_at_unix_ms)
                }),
            None => Err(FetchFailure::schema("state_dir")),
        };
        ClaudeUsageAnswer {
            checked_at_unix_ms,
            outcome,
        }
    })
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
    let Some((disposition, kind)) = state.failure.as_ref() else {
        return state.last_success.as_ref().map_or_else(
            || ProviderUsageSnapshot::loading(state.provider, state.label),
            |success| snapshot_from_success(state, success, "available", None, now_unix_ms),
        );
    };

    if *disposition == FailureDisposition::Offline
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
        && *disposition != FailureDisposition::Schema
        && let Some(fallback) = fallback
    {
        return snapshot_from_fallback(state, fallback, kind, now_unix_ms);
    }

    let message = match disposition {
        FailureDisposition::Authentication => {
            format!("Sign in with {} to see usage", state.provider)
        }
        FailureDisposition::Schema => {
            format!("{} weekly usage response is unavailable", state.label)
        }
        FailureDisposition::NotInstalled => {
            format!("{CLAUDE_BINARY} is not installed on this Mac")
        }
        // A Claude read that timed out or exited is a local child failing,
        // not the network, so its row does not claim to be offline.
        FailureDisposition::Offline if state.provider == "claude" => {
            format!("{} weekly usage response is unavailable", state.label)
        }
        FailureDisposition::Offline => {
            format!("{} weekly usage is unavailable · offline", state.label)
        }
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
            state
                .failure
                .as_ref()
                .map_or("expired", |(_, kind)| kind.as_str()),
        );
    }
    let buckets = success
        .buckets
        .iter()
        .map(|bucket| match bucket {
            UsageBucket::Unavailable { label } => ProviderUsageBucketSnapshot {
                label: label.clone(),
                state: "unavailable".to_owned(),
                used_percent: None,
                resets_at_unix_seconds: None,
                message: Some(format!("{label} weekly usage response is unavailable")),
            },
            UsageBucket::Available(bucket)
                if bucket.resets_at_unix_seconds <= now_unix_ms / 1_000 =>
            {
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
            }
            UsageBucket::Available(bucket) => ProviderUsageBucketSnapshot {
                label: bucket.label.clone(),
                state: projection_state.to_owned(),
                used_percent: Some(bucket.used_percent),
                resets_at_unix_seconds: Some(bucket.resets_at_unix_seconds),
                message: message.clone(),
            },
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
        last_error_kind: state.failure.as_ref().map(|(_, kind)| kind.clone()),
        buckets,
    }
}

fn snapshot_from_fallback(
    state: &ProviderState,
    fallback: SessionFallback,
    kind: &str,
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
    kind: &str,
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
    /// A diagnostic token; never output, a token, or an account.
    kind: String,
    status: Option<u16>,
    retry_after: Option<Duration>,
}

impl FetchFailure {
    fn of(disposition: FailureDisposition, kind: impl Into<String>) -> Self {
        Self {
            disposition,
            kind: kind.into(),
            status: None,
            retry_after: None,
        }
    }

    fn authentication(kind: &'static str) -> Self {
        Self::of(FailureDisposition::Authentication, kind)
    }

    fn schema(kind: &'static str) -> Self {
        Self::of(FailureDisposition::Schema, kind)
    }

    /// Maps a `claude -p /usage` failure to its row. A timeout or a failed
    /// child is transient: the last success stays for a while. A frame that
    /// is not a result frame is an answer the reader cannot use. A binary
    /// that is not there is its own row, and is looked up again next time.
    fn from_usage_error(error: UsageError) -> Self {
        match error {
            UsageError::NotInstalled => Self::of(FailureDisposition::NotInstalled, "not_installed"),
            UsageError::Timeout => Self::of(FailureDisposition::Offline, "timeout"),
            UsageError::Cancelled => Self::of(FailureDisposition::Offline, "cancelled"),
            UsageError::Failed(kind) => Self::of(FailureDisposition::Offline, kind),
            UsageError::NoResultFrame => Self::schema("no_result_frame"),
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
                "rate_limited".to_owned()
            } else {
                "http".to_owned()
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
        let mut response = request
            .call()
            .map_err(|_| FetchFailure::of(FailureDisposition::Offline, "transport"))?;
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
                kind: "body_read".to_owned(),
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
    parse_retry_after_at(value, unix_seconds())
}

fn parse_retry_after_at(value: &str, now_unix_seconds: u64) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let retry_at = parse_http_date(value)?;
    Some(Duration::from_secs(
        retry_at.saturating_sub(now_unix_seconds),
    ))
}

/// Reads the `/usage` text the CLI prints. The lines that matter look like
///
/// ```text
/// Current session: 4% used · resets Sep 17 at 9pm (Asia/Seoul)
/// Current week (all models): 1% used · resets Sep 24 at 1pm (Asia/Seoul)
/// Current week (Fable): 0% used · resets Sep 24 at 1pm (Asia/Seoul)
/// ```
///
/// `Current week (all models)` is the row; every other `Current week` line
/// is a scoped bucket under it; `Current session` is read and dropped (the
/// popover shows the weekly window only). A CLI that cannot read its login
/// prints `/cost` text instead, still with exit 0 and `is_error: false`, so
/// no `Current` line at all is the logged-out state. A `Current` line that
/// does not parse is a format the reader does not know, never a zero.
fn parse_claude_usage_text(
    text: &str,
    now_unix_seconds: u64,
    checked_at: u64,
) -> Result<SuccessfulUsage, FetchFailure> {
    let mut main = None;
    let mut buckets = Vec::new();
    let mut saw_line = false;
    for line in text.lines().map(str::trim) {
        if !line.starts_with("Current ") {
            continue;
        }
        saw_line = true;
        let parsed = parse_usage_line(line).ok_or_else(|| FetchFailure::schema("line_format"))?;
        if parsed.window != "week" {
            continue;
        }
        let scope = parsed.scope.clone();
        let resets_at = resolve_reset(&parsed, now_unix_seconds);
        if scope == "all models" {
            let resets_at_unix_seconds = resets_at?;
            main = Some(UsageValue {
                label: "Claude Code".to_owned(),
                used_percent: parsed.used_percent,
                resets_at_unix_seconds,
            });
            continue;
        }
        match resets_at {
            Ok(resets_at_unix_seconds) => buckets.push(UsageBucket::Available(UsageValue {
                label: scope,
                used_percent: parsed.used_percent,
                resets_at_unix_seconds,
            })),
            Err(failure) => {
                log_scoped_failure(&failure.kind);
                buckets.push(UsageBucket::Unavailable { label: scope });
            }
        }
    }
    if !saw_line {
        return Err(FetchFailure::authentication("login_unreadable"));
    }
    let main = main.ok_or_else(|| FetchFailure::schema("weekly_missing"))?;
    Ok(SuccessfulUsage {
        main,
        buckets,
        checked_at_unix_ms: checked_at,
    })
}

/// One `Current …` line, as printed.
#[derive(Clone, Debug, PartialEq)]
struct UsageLine {
    window: String,
    /// `all models`, a model name, or empty for the session line.
    scope: String,
    used_percent: f64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    zone: String,
}

fn parse_usage_line(line: &str) -> Option<UsageLine> {
    static LINE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = LINE.get_or_init(|| {
        regex::Regex::new(
            r"^Current (session|week)(?: \(([^)]+)\))?: (\d+(?:\.\d+)?)% (used|left) · resets ([A-Z][a-z]{2}) (\d{1,2}) at (\d{1,2})(?::(\d{2}))?(am|pm) \(([^)]+)\)$",
        )
        .expect("the usage line pattern compiles")
    });
    let captures = pattern.captures(line)?;
    let percent = captures[3].parse::<f64>().ok()?;
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
        return None;
    }
    let used_percent = if &captures[4] == "used" {
        percent
    } else {
        100.0 - percent
    };
    let month = parse_http_month(&captures[5])?;
    let day = captures[6].parse::<i64>().ok()?;
    let clock_hour = captures[7].parse::<i64>().ok()?;
    if !(1..=12).contains(&clock_hour) {
        return None;
    }
    let minute = captures
        .get(8)
        .map_or(Some(0), |minute| minute.as_str().parse::<i64>().ok())?;
    if !(0..=59).contains(&minute) {
        return None;
    }
    let hour = match (&captures[9], clock_hour) {
        ("am", 12) => 0,
        ("am", hour) => hour,
        ("pm", 12) => 12,
        (_, hour) => hour + 12,
    };
    Some(UsageLine {
        window: captures[1].to_owned(),
        scope: captures
            .get(2)
            .map_or_else(String::new, |scope| scope.as_str().to_owned()),
        used_percent,
        month,
        day,
        hour,
        minute,
        zone: captures[10].to_owned(),
    })
}

/// The instant a printed wall time names. The CLI prints no year, so the
/// answer is the first instant at or after `now` whose wall clock in that
/// zone matches - unless the wall time matched within the last window, in
/// which case it has just passed and the reset that passed is meant.
fn resolve_reset(line: &UsageLine, now_unix_seconds: u64) -> Result<u64, FetchFailure> {
    let now = i64::try_from(now_unix_seconds).map_err(|_| FetchFailure::schema("reset_time"))?;
    let zone = Zone::load(&line.zone).map_err(FetchFailure::schema)?;
    let (year_now, _, _) = civil_from_days(
        (now + i64::from(zone.offset_at(now).map_err(FetchFailure::schema)?)).div_euclid(86_400),
    );
    let mut candidates = Vec::new();
    for year in [year_now - 1, year_now, year_now + 1] {
        if !(1..=days_in_month(year, line.month)).contains(&line.day) {
            continue;
        }
        let local = days_from_civil(year, line.month, line.day) * 86_400
            + line.hour * 3_600
            + line.minute * 60;
        candidates.extend(zone.instants_of(local).map_err(FetchFailure::schema)?);
    }
    candidates.sort_unstable();
    let next = candidates.iter().copied().find(|instant| *instant >= now);
    let previous = candidates
        .iter()
        .copied()
        .rev()
        .find(|instant| *instant < now);
    let chosen = match (next, previous) {
        (_, Some(previous)) if now - previous <= RESET_HORIZON_SECONDS => previous,
        (Some(next), _) => next,
        (None, Some(previous)) => previous,
        (None, None) => return Err(FetchFailure::schema("reset_time")),
    };
    u64::try_from(chosen).map_err(|_| FetchFailure::schema("reset_time"))
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

struct CodexCredentials {
    access_token: String,
    account_id: String,
}

fn read_codex_credentials(paths: &UsagePaths) -> Result<CodexCredentials, &'static str> {
    let root = paths.codex_home.clone().ok_or("credential_path")?;
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

fn parse_http_date(value: &str) -> Option<u64> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    let (day, month, year, time) = match parts.as_slice() {
        // IMF-fixdate: Sun, 06 Nov 1994 08:49:37 GMT
        [weekday, day, month, year, time, "GMT"] if weekday.ends_with(',') => (
            day.parse::<i64>().ok()?,
            parse_http_month(month)?,
            year.parse::<i64>().ok()?,
            *time,
        ),
        // Obsolete RFC 850 form: Sunday, 06-Nov-94 08:49:37 GMT
        [weekday, date, time, "GMT"] if weekday.ends_with(',') => {
            let date = date.split('-').collect::<Vec<_>>();
            let [day, month, year] = date.as_slice() else {
                return None;
            };
            let short_year = year.parse::<i64>().ok()?;
            let year = if short_year >= 70 {
                1_900 + short_year
            } else {
                2_000 + short_year
            };
            (
                day.parse::<i64>().ok()?,
                parse_http_month(month)?,
                year,
                *time,
            )
        }
        // ANSI C asctime form: Sun Nov  6 08:49:37 1994
        [_weekday, month, day, time, year] => (
            day.parse::<i64>().ok()?,
            parse_http_month(month)?,
            year.parse::<i64>().ok()?,
            *time,
        ),
        _ => return None,
    };
    let time = time.split(':').collect::<Vec<_>>();
    let [hour, minute, second] = time.as_slice() else {
        return None;
    };
    let hour = hour.parse::<i64>().ok()?;
    let minute = minute.parse::<i64>().ok()?;
    let second = second.parse::<i64>().ok()?;
    if !(1..=days_in_month(year, month)).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    let timestamp = days_from_civil(year, month, day)
        .checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?;
    u64::try_from(timestamp).ok()
}

fn parse_http_month(value: &str) -> Option<i64> {
    Some(match value {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

fn log_failure(provider: &str, status: Option<u16>, kind: &str) {
    crate::diagnostic!(json!({
        "component": "provider_usage",
        "provider": provider,
        "status": status,
        "kind": kind,
    }));
}

fn log_scoped_failure(kind: &str) {
    crate::diagnostic!(json!({
        "component": "provider_usage",
        "provider": "claude",
        "scope": "weekly_scoped",
        "kind": kind,
    }));
}

fn unix_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        state.failure = Some((FailureDisposition::Offline, "transport".to_owned()));
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

    #[test]
    fn retry_after_accepts_delay_seconds_and_all_http_date_forms() {
        let now = 784_111_717;
        assert_eq!(
            parse_retry_after_at("120", now),
            Some(Duration::from_secs(120))
        );
        for value in [
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun Nov  6 08:49:37 1994",
        ] {
            assert_eq!(
                parse_retry_after_at(value, now),
                Some(Duration::from_secs(60)),
                "{value}"
            );
        }
    }

    /// The captured output of claude 2.1.274, run on 2026-09-17 in Asia/Seoul
    /// (`herdr-core/tests/fixtures/claude-usage/`). The expected instants
    /// are the CLI's own `.usage-cache.json` values for the same window
    /// (`1w_resets_at: 1790222400`), not this parser's.
    const USAGE_FRAME: &str = include_str!("../tests/fixtures/claude-usage/usage-2.1.274.json");
    const COST_FRAME: &str = include_str!("../tests/fixtures/claude-usage/cost-2.1.274.json");
    /// 2026-09-17 17:05 Asia/Seoul, when the fixture was captured.
    const CAPTURED_AT: u64 = 1_789_632_300;

    fn result_text(frame: &str) -> String {
        serde_json::from_str::<Value>(frame).unwrap()["result"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn the_captured_usage_text_projects_the_weekly_row_and_one_bucket_per_model() {
        let success =
            parse_claude_usage_text(&result_text(USAGE_FRAME), CAPTURED_AT, 1_000).unwrap();
        assert_eq!(success.main.label, "Claude Code");
        assert_eq!(success.main.used_percent, 1.0);
        assert_eq!(success.main.resets_at_unix_seconds, 1_790_222_400);
        assert_eq!(success.buckets.len(), 1, "the session line is not a bucket");
        let UsageBucket::Available(bucket) = &success.buckets[0] else {
            panic!("Fable should be available");
        };
        assert_eq!(bucket.label, "Fable");
        assert_eq!(bucket.used_percent, 0.0);
        assert_eq!(bucket.resets_at_unix_seconds, 1_790_222_400);

        let state = ProviderState::new("claude", "Claude Code");
        let projected = snapshot_from_success(&state, &success, "available", None, 1_000);
        assert_eq!(projected.used_percent, Some(1.0));
        assert_eq!(projected.buckets.len(), 1);
        assert_eq!(projected.buckets[0].label, "Fable");
    }

    #[test]
    fn cost_text_is_the_logged_out_state_and_never_a_zero() {
        let failure =
            parse_claude_usage_text(&result_text(COST_FRAME), CAPTURED_AT, 1_000).unwrap_err();
        assert_eq!(failure.disposition, FailureDisposition::Authentication);
        let mut state = ProviderState::new("claude", "Claude Code");
        state.record_failure(failure, Instant::now(), 1_000);
        let projected = project_provider(&state, None, 2_000);
        assert_eq!(projected.state, "unavailable");
        assert_eq!(projected.used_percent, None);
        assert_eq!(
            projected.message.as_deref(),
            Some("Sign in with claude to see usage")
        );
    }

    #[test]
    fn a_current_line_the_reader_does_not_know_is_a_format_failure_not_a_login_failure() {
        let text = "Current week (all models): 1% used · resets in 6 days\n";
        let failure = parse_claude_usage_text(text, CAPTURED_AT, 1_000).unwrap_err();
        assert_eq!(failure.disposition, FailureDisposition::Schema);
        assert_eq!(failure.kind, "line_format");

        let session_only = "Current session: 4% used · resets Sep 17 at 9pm (Asia/Seoul)\n";
        let failure = parse_claude_usage_text(session_only, CAPTURED_AT, 1_000).unwrap_err();
        assert_eq!(failure.kind, "weekly_missing");
    }

    #[test]
    fn percent_left_is_read_as_used_and_the_session_line_is_dropped() {
        let text = "Current session: 96% left · resets Sep 17 at 9pm (Asia/Seoul)\n\
                    Current week (all models): 63% left · resets Sep 24 at 1:30pm (Asia/Seoul)\n";
        let success = parse_claude_usage_text(text, CAPTURED_AT, 1_000).unwrap();
        assert_eq!(success.main.used_percent, 37.0);
        assert_eq!(success.main.resets_at_unix_seconds, 1_790_222_400 + 30 * 60);
        assert!(success.buckets.is_empty());
    }

    #[test]
    fn a_scoped_line_in_an_unknown_zone_keeps_the_weekly_row() {
        let text = "Current week (all models): 1% used · resets Sep 24 at 1pm (Asia/Seoul)\n\
                    Current week (Fable): 0% used · resets Sep 24 at 1pm (Mars/Olympus)\n";
        let success = parse_claude_usage_text(text, CAPTURED_AT, 1_000).unwrap();
        assert_eq!(success.main.resets_at_unix_seconds, 1_790_222_400);
        assert!(
            matches!(&success.buckets[0], UsageBucket::Unavailable { label } if label == "Fable")
        );
    }

    /// The CLI prints no year: a reset naming a wall time later this year
    /// is this year's, one naming an earlier date is next year's, and one
    /// naming a wall time that passed minutes ago is the reset that passed,
    /// not the same date a year out.
    #[test]
    fn a_reset_without_a_year_is_the_next_match_unless_it_just_passed() {
        let line = |text: &str| parse_usage_line(text).unwrap();
        let dec_31 =
            line("Current week (all models): 1% used · resets Dec 31 at 11pm (Asia/Seoul)");
        assert_eq!(resolve_reset(&dec_31, CAPTURED_AT).unwrap(), 1_798_725_600);
        let jan_1 = line("Current week (all models): 1% used · resets Jan 1 at 1am (Asia/Seoul)");
        assert_eq!(resolve_reset(&jan_1, CAPTURED_AT).unwrap(), 1_798_732_800);
        // Sep 24 at 1pm Seoul is 1790222400; asked ten minutes later.
        let sep_24 = line("Current week (all models): 1% used · resets Sep 24 at 1pm (Asia/Seoul)");
        assert_eq!(
            resolve_reset(&sep_24, 1_790_222_400 + 600).unwrap(),
            1_790_222_400
        );
        let feb_30 = line("Current week (all models): 1% used · resets Feb 30 at 1pm (Asia/Seoul)");
        assert_eq!(
            resolve_reset(&feb_30, CAPTURED_AT).unwrap_err().kind,
            "reset_time"
        );
    }

    #[test]
    fn each_cli_failure_lands_on_its_row() {
        let now = Instant::now();
        let kept = SuccessfulUsage {
            main: UsageValue {
                label: "Claude Code".to_owned(),
                used_percent: 12.0,
                resets_at_unix_seconds: 4_102_444_800,
            },
            buckets: Vec::new(),
            checked_at_unix_ms: 100_000,
        };

        // A timeout with a recent success keeps it, marked stale.
        let mut state = ProviderState::new("claude", "Claude Code");
        state.record_success(kept.clone(), now);
        state.record_failure(
            FetchFailure::from_usage_error(UsageError::Timeout),
            now,
            200_000,
        );
        let projected = project_provider(&state, None, 200_000);
        assert_eq!(projected.state, "stale");
        assert_eq!(projected.used_percent, Some(12.0));
        assert_eq!(
            projected.message.as_deref(),
            Some("Last checked 2m ago · offline")
        );

        // The same timeout with nothing kept is unavailable, and not "offline".
        let mut state = ProviderState::new("claude", "Claude Code");
        state.record_failure(
            FetchFailure::from_usage_error(UsageError::Timeout),
            now,
            200_000,
        );
        let projected = project_provider(&state, None, 200_000);
        assert_eq!(projected.state, "unavailable");
        assert_eq!(
            projected.message.as_deref(),
            Some("Claude Code weekly usage response is unavailable")
        );

        // Text that is not a result frame drops a kept success at once.
        let mut state = ProviderState::new("claude", "Claude Code");
        state.record_success(kept, now);
        state.record_failure(
            FetchFailure::from_usage_error(UsageError::NoResultFrame),
            now,
            200_000,
        );
        let projected = project_provider(&state, None, 200_000);
        assert_eq!(projected.state, "unavailable");
        assert_eq!(projected.used_percent, None);
        assert_eq!(
            projected.last_error_kind.as_deref(),
            Some("no_result_frame")
        );

        // No binary names the binary.
        let mut state = ProviderState::new("claude", "Claude Code");
        state.record_failure(
            FetchFailure::from_usage_error(UsageError::NotInstalled),
            now,
            200_000,
        );
        let projected = project_provider(&state, None, 200_000);
        assert_eq!(
            projected.message.as_deref(),
            Some("claude is not installed on this Mac")
        );
    }

    /// The worker runs once per attempt: an answer arrives on a later wake,
    /// nothing runs between attempts, and a child in flight is not doubled.
    #[test]
    fn one_claude_read_per_attempt_and_its_answer_lands_on_a_later_wake() {
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counted = Arc::clone(&calls);
        let mut read = BackgroundRead::on_change(Duration::ZERO, move |_: &ClaudeUsageRequest| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(20));
            ClaudeUsageAnswer {
                checked_at_unix_ms: 1,
                outcome: Err(FetchFailure::schema("fixture")),
            }
        });
        let one = ClaudeUsageRequest { attempt: 1 };
        assert!(read.poll(one.clone()).is_none());
        let mut answered = false;
        for _ in 0..500 {
            if read.poll(one.clone()).is_some() {
                answered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(answered);
        for _ in 0..20 {
            assert!(read.poll(one.clone()).is_none());
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(read.poll(ClaudeUsageRequest { attempt: 2 }).is_none());
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
