pub mod context_label;
pub mod provider;

use anyhow::{Context, Result, anyhow};
use fs2::FileExt;
use hide_ai::{AiError, AiResult, AiRouter, CancelToken, ProviderId};
use hide_session::{
    Agent as SessionAgent, SessionCursor, SessionIdentity, SessionLocator, parse_events,
};
pub use hide_session::{ConversationEvent as SessionEvent, EventKind, ParsedSession};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hide_herdr_client::{
    ApiConnector, ApiError, HERDR_PROTOCOL_REVISION, Subscription, UnixSocketConnector,
    request_with_connector, request_with_correlation_id,
};

pub const PLUGIN_ID: &str = "hide.agent-context-labels";
/// The id this plugin shipped under before it moved into the Hide workspace.
/// Its state and config directories are moved once by [`migrate_legacy_state`].
pub const LEGACY_PLUGIN_ID: &str = "herdr-agent-context-labels";
/// Upper bound on the analysis context. The context normally spans the last
/// two human turns; this also guards against one enormous turn.
pub const MAX_ANALYSIS_CONTEXT_CHARS: usize = 4_000;
pub const MAX_SUMMARY_CHARS: usize = 30;
/// How many of the session's user turns feed the summary's "what was asked"
/// arc. Unbounded, a long session would grow the per-event context (and cost)
/// with every new turn; this caps it to the recent history that still shapes
/// the current task title.
pub const MAX_USER_REQUEST_TURNS: usize = 8;
/// How long a pane waits before asking again after the provider layer reported
/// that no provider can answer right now (not logged in, usage limit, no
/// provider connected). Retries of one request are the router's; this is the
/// wait before the pane's next request. Without it the exhausted state was
/// rediscovered on every event, which once wrote 42828 identical lines in a day.
pub const PROVIDER_RECOVERY_INTERVAL: Duration = Duration::from_secs(600);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attention {
    Question,
    Approval,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatusIcon {
    Question,
    Approval,
    Error,
    Working,
    Done,
    Interrupted,
    Idle,
    #[default]
    Stale,
}

impl StatusIcon {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Question => "?",
            Self::Approval => "!",
            Self::Error => "×",
            // Steady, not blinking. A pulse drags the eye toward the one state
            // that needs nothing from the user, and it was also the only thing
            // separating working from done while both were painted green.
            Self::Working => "●",
            Self::Done => "●",
            Self::Interrupted => "‖",
            Self::Idle => "○",
            Self::Stale => "~",
        }
    }

    pub const fn token_name(self) -> &'static str {
        match self {
            Self::Question => "status_question",
            Self::Approval => "status_approval",
            Self::Error => "status_error",
            Self::Working => "status_working",
            Self::Done => "status_done",
            Self::Interrupted => "status_interrupted",
            Self::Idle => "status_idle",
            Self::Stale => "status_stale",
        }
    }
}

/// Ordering key for the sidebar, finer than [`StatusIcon`]: a question proven
/// by a native hook outranks one inferred by the provider, even though both
/// render as `?`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Question,
    Approval,
    SemanticQuestion,
    Error,
    Done,
    Working,
    Interrupted,
    Idle,
    Stale,
}

impl SortKey {
    /// Name used to reference this state in the user's sort-order file.
    pub const fn config_name(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Approval => "approval",
            Self::SemanticQuestion => "semantic_question",
            Self::Error => "error",
            Self::Working => "working",
            Self::Done => "done",
            Self::Interrupted => "interrupted",
            Self::Idle => "idle",
            Self::Stale => "stale",
        }
    }
}

/// Attention-first default: a failed turn is the most expensive thing to leave
/// unread, so it leads, then the hook-confirmed interaction states, then the
/// provider-inferred question, an unseen completion, and the ambient states.
pub const DEFAULT_SORT_ORDER: [SortKey; 9] = [
    SortKey::Error,
    SortKey::Question,
    SortKey::Approval,
    SortKey::SemanticQuestion,
    SortKey::Done,
    SortKey::Working,
    SortKey::Interrupted,
    SortKey::Idle,
    SortKey::Stale,
];

/// Optional user override, read from the plugin's Herdr-assigned config
/// directory: `{"order": ["question", "working", ...]}`. Herdr owns the path
/// contract (`HERDR_PLUGIN_CONFIG_DIR`), the plugin owns the file.
pub const SORT_ORDER_FILE: &str = "sort-order.json";

/// Turn an optional user order into a complete one: listed names first in the
/// given order, unknown names ignored, missing states appended in default
/// order. Invalid JSON keeps the default rather than failing the watcher.
pub fn resolve_sort_order(raw: Option<&str>) -> [SortKey; 9] {
    let Some(raw) = raw else {
        return DEFAULT_SORT_ORDER;
    };
    let Some(names) = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| value.get("order").cloned())
        .and_then(|order| serde_json::from_value::<Vec<String>>(order).ok())
    else {
        return DEFAULT_SORT_ORDER;
    };
    let mut order = Vec::with_capacity(7);
    for name in names {
        if let Some(icon) = DEFAULT_SORT_ORDER
            .iter()
            .find(|icon| icon.config_name() == name)
            && !order.contains(icon)
        {
            order.push(*icon);
        }
    }
    for icon in DEFAULT_SORT_ORDER {
        if !order.contains(&icon) {
            order.push(icon);
        }
    }
    let mut resolved = DEFAULT_SORT_ORDER;
    resolved.copy_from_slice(&order);
    resolved
}

/// One digit per state so the token sort's string comparison matches the
/// numeric order.
pub fn sort_rank(order: &[SortKey; 9], status: SortKey) -> String {
    let position = order
        .iter()
        .position(|icon| *icon == status)
        .unwrap_or(order.len());
    position.to_string()
}

/// Rank digit used for every pane in the seen partition. Seen panes are ordered
/// by recency alone, so they must all compare equal here; the value only has to
/// be a constant, and zero keeps the token's two-digit shape.
const SEEN_RANK: &str = "0";

/// Two characters. The first partitions unseen work above seen work, with a
/// running pane always counted into the unseen partition because active work is
/// not something the user has finished looking at. The second orders by
/// attention within the unseen partition and is flattened in the seen one.
pub fn sort_rank_token(order: &[SortKey; 9], display: &Display) -> String {
    if display.unseen || display.status == StatusIcon::Working {
        format!("0{}", sort_rank(order, display.sort_key))
    } else {
        format!("1{SEEN_RANK}")
    }
}

/// Epoch milliseconds are 13 digits until the year 2286; the fixed width makes
/// a lexicographic comparison agree with a numeric one, so the token sorts
/// correctly whichever way Herdr compares it.
pub fn activity_token(activity_unix_ms: u64) -> String {
    format!("{activity_unix_ms:013}")
}

/// Resolved once per process: the watcher reports every rank, so a mid-run
/// edit takes effect on the next watcher start, like the agent view itself.
static SORT_ORDER: LazyLock<[SortKey; 9]> = LazyLock::new(|| {
    let path = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home)
                    .join(".config/herdr/plugins/config")
                    .join(PLUGIN_ID)
            })
        })
        .map(|dir| dir.join(SORT_ORDER_FILE));
    let raw = path.and_then(|path| fs::read_to_string(path).ok());
    resolve_sort_order(raw.as_deref())
});

/// Every status token this plugin may own. The watcher clears the whole set on
/// each report so exactly one of them is ever live for a pane.
const STATUS_TOKENS: [&str; 11] = [
    "status_question",
    "status_question_new",
    "status_approval",
    "status_approval_new",
    "status_error",
    "status_error_new",
    "status_working",
    "status_done",
    "status_interrupted",
    "status_idle",
    "status_stale",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Codex,
    Claude,
}

impl AgentKind {
    fn from_herdr(value: &str) -> Option<Self> {
        match value {
            "codex" => Some(Self::Codex),
            "claude" => Some(Self::Claude),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    const fn icon_token(self) -> &'static str {
        match self {
            Self::Codex => "agent_codex",
            Self::Claude => "agent_claude",
        }
    }

    /// Sidebar glyph rather than a word: the row already carries the workspace
    /// name, and the user's config colors the two tokens differently. Both
    /// glyphs are filled so they stay visible at one cell.
    const fn label(self) -> &'static str {
        match self {
            Self::Codex => "⬢",
            Self::Claude => "❋",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub agent: AgentKind,
    pub agent_session: Option<AgentSession>,
    /// Herdr's own lifecycle verdict: idle, working, blocked, done, unknown.
    /// `done` already means "finished while you were not looking", so the
    /// plugin never tracks unseen completion itself.
    pub agent_status: String,
    pub revision: u64,
    pub state_change_seq: u64,
    pub cwd: Option<String>,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentSession {
    kind: String,
    value: String,
}

impl AgentSession {
    pub fn new(kind: &str, value: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            value: value.to_owned(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AgentListResult {
    agents: Vec<AgentListItem>,
}

/// The `agent.list` response carries every field a scan needs. Reading a
/// second endpoint would mean joining two different points in time.
#[derive(Debug, Deserialize)]
struct AgentListItem {
    pane_id: String,
    agent: Option<String>,
    agent_status: String,
    revision: u64,
    state_change_seq: u64,
    cwd: Option<String>,
    #[serde(default)]
    focused: bool,
    agent_session: Option<AgentSession>,
}

impl AgentListItem {
    fn into_pane(self) -> Option<Pane> {
        Some(Pane {
            agent: self.agent.as_deref().and_then(AgentKind::from_herdr)?,
            id: self.pane_id,
            agent_session: self.agent_session,
            agent_status: self.agent_status,
            revision: self.revision,
            state_change_seq: self.state_change_seq,
            cwd: self.cwd,
            focused: self.focused,
        })
    }
}

/// The exact lifecycle subscriptions used by the watcher. Keep this list in
/// one place so the client request and contract-facing tests cannot drift.
pub const WATCHER_SUBSCRIPTIONS: [&str; 7] = [
    "pane.created",
    "pane.updated",
    "pane.closed",
    "pane.exited",
    "pane.focused",
    "pane.agent_detected",
    "pane.agent_status_changed",
];

/// Which side produced an attention verdict; a hook is a fact, the provider is
/// an inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionSource {
    Hook,
    Semantic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub summary: Option<String>,
    pub status: StatusIcon,
    pub sort_key: SortKey,
    pub elapsed: Option<String>,
    pub unseen: bool,
    /// When the pane last changed state, in epoch milliseconds. Published as
    /// its own token because the sidebar's recency tiebreak needs a clock two
    /// panes can be compared on, which `state_change_seq` is not.
    pub activity_unix_ms: u64,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            summary: None,
            status: StatusIcon::Stale,
            sort_key: SortKey::Stale,
            elapsed: None,
            unseen: false,
            activity_unix_ms: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub summary: String,
    pub attention: Option<Attention>,
}

/// Herdr owns the lifecycle and is trusted for it outright. This plugin only
/// refines what Herdr cannot see from the screen: whether a stopped agent is
/// waiting for an answer rather than a keypress, and whether a turn ended in an
/// error. The base state is never replaced by an inference.
pub fn status_icon(agent_status: &str, attention: Option<Attention>) -> StatusIcon {
    let base = match agent_status {
        "working" => StatusIcon::Working,
        // Herdr reports blocked when a dialog is on screen waiting for a key.
        "blocked" => StatusIcon::Approval,
        "done" => StatusIcon::Done,
        "idle" => StatusIcon::Idle,
        _ => StatusIcon::Stale,
    };
    // A running agent is not waiting on anyone and has not stopped to fail.
    if base == StatusIcon::Working {
        return base;
    }
    match attention {
        Some(Attention::Error) => StatusIcon::Error,
        // Either a question tool the hook saw, or plain prose the provider read.
        Some(Attention::Question) => StatusIcon::Question,
        // The hook can see a permission request before Herdr sees the dialog.
        Some(Attention::Approval) => StatusIcon::Approval,
        None => base,
    }
}

/// Cut a summary to the display budget without splitting a word.
pub fn truncate_summary(text: &str) -> String {
    if text.chars().count() <= MAX_SUMMARY_CHARS {
        return text.to_owned();
    }
    let budget: String = text.chars().take(MAX_SUMMARY_CHARS - 1).collect();
    let head = budget
        .rsplit_once(char::is_whitespace)
        .map(|(head, _)| head)
        .filter(|head| head.chars().count() * 2 >= MAX_SUMMARY_CHARS)
        .unwrap_or(&budget);
    format!("{}…", head.trim_end())
}

pub fn normalize_summary(raw: &str) -> Option<String> {
    let candidate = raw
        .rsplit("</think>")
        .next()
        .unwrap_or(raw)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .trim_matches(|character| matches!(character, '`' | '*' | '"' | '#'))
        .trim();
    if candidate.chars().count() < 4 || candidate.chars().any(char::is_control) {
        return None;
    }
    Some(truncate_summary(candidate))
}

static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)sk-[a-z0-9_-]{8,}|(?:api[_-]?key|token|password|secret)\s*[=:]\s*[^\s,;]+")
        .expect("valid secret expression")
});
static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b").expect("valid email expression")
});
static FILE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:(?:/Users|/home|/tmp|/var|/etc)/)[^\s'"`]+"#).expect("valid path expression")
});

/// Build the text handed to the provider.
///
/// The conversation is already restricted to user and assistant prose by the
/// session parser, so the only structural thing worth removing is a fenced code
/// block. Nothing else is dropped: the previous line filter also deleted every
/// Markdown bullet, which is most of what an agent actually says.
pub fn analysis_context(events: &[SessionEvent]) -> String {
    let conversation = events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::Human | EventKind::Assistant))
        .collect::<Vec<_>>();
    if conversation.is_empty() {
        return String::new();
    }
    // Span the last two user turns, not one: whether the final assistant
    // message is a fresh question or a wrap-up of one already answered is
    // often only visible in the preceding exchange.
    let last = conversation
        .iter()
        .rposition(|event| event.kind == EventKind::Human)
        .unwrap_or(0);
    let start = conversation[..last]
        .iter()
        .rposition(|event| event.kind == EventKind::Human)
        .unwrap_or(last);
    let transcript = conversation[start..]
        .iter()
        .map(|event| format!("{}: {}", event.role, event.text))
        .collect::<Vec<_>>()
        .join("\n");

    // The session's recent user turns, not just the latest one: the summary
    // is a task title for the ongoing work, so it needs the arc of what was
    // asked, not just the fragment attached to the most recent exchange.
    // Capped so a long session does not grow the per-event context forever.
    let mut user_requests = conversation
        .iter()
        .filter(|event| event.kind == EventKind::Human)
        .map(|event| event.text.as_str())
        .collect::<Vec<_>>();
    if user_requests.len() > MAX_USER_REQUEST_TURNS {
        user_requests = user_requests.split_off(user_requests.len() - MAX_USER_REQUEST_TURNS);
    }
    let user_requests = user_requests.join("\n---\n");

    let combined = format!(
        "<all-user-requests>\n{user_requests}\n</all-user-requests>\n<latest-exchange>\n{transcript}\n</latest-exchange>"
    );
    redact(&strip_code_fences(&combined))
}

/// Build the same provider context for a complete provider session transcript
/// that the watcher builds after its cursor has accumulated the same events.
/// The legacy line-oriented input accepted by `analyze-stdin` is preserved when
/// the input is not JSONL.
pub fn analysis_context_from_session(agent: SessionAgent, contents: &str) -> String {
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let parsed = parse_events(agent, trimmed);
    if parsed.events.is_empty() && !trimmed.trim_start().starts_with('{') {
        return trimmed.to_owned();
    }
    analysis_context(&parsed.events)
}

/// Which of a turn's two boundaries an analysis is answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisPhase {
    /// The user has just spoken and the agent has not answered yet. Naming the
    /// task here is what lets a working pane say what it is working on.
    TurnStart,
    /// The agent has answered and stopped. Whether it is waiting on the user
    /// can only be judged once its last word is in.
    TurnEnd,
}

impl AnalysisPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::TurnStart => "start",
            Self::TurnEnd => "end",
        }
    }
}

/// Identity of the turn a transcript is currently in, taken from the user's own
/// last message.
///
/// The trigger used to be the hash of the whole context window, which grows
/// with every token the agent emits: a live pane therefore had a permanently
/// changing key and asked the provider again on almost every event. One day of
/// that spent the request budget by midday and then wrote 47141 identical skip
/// lines. The user's message is fixed for the whole turn and changes exactly
/// once, at the boundary, which is the rate the sidebar actually needs.
///
/// `None` means the transcript holds no user message yet, so there is no turn
/// to analyze.
pub fn turn_key(events: &[SessionEvent]) -> Option<u64> {
    let last = events
        .iter()
        .rposition(|event| event.kind == EventKind::Human)?;
    Some(context_fingerprint(&events[last].text))
}

/// The boundary this pane is sitting on, or `None` when the turn's two calls
/// are already spent.
///
/// Both arms are level-triggered rather than edge-triggered: the condition
/// stays true until the call actually lands, so a call deferred by the request
/// spacer, the provider cooldown, or the daily cap is made on a later wake
/// instead of being lost with the edge that produced it.
pub fn analysis_phase(
    newest_user_is_last: bool,
    working: bool,
    start_done: bool,
    end_done: bool,
) -> Option<AnalysisPhase> {
    if newest_user_is_last {
        // The agent has not answered yet, so there is nothing to judge and only
        // the task to name.
        return (!start_done).then_some(AnalysisPhase::TurnStart);
    }
    (!working && !end_done).then_some(AnalysisPhase::TurnEnd)
}

fn strip_code_fences(text: &str) -> String {
    let mut inside = false;
    text.lines()
        .filter(|line| {
            if line.trim_start().starts_with("```") {
                inside = !inside;
                return false;
            }
            !inside
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact(text: &str) -> String {
    let masked = SECRET.replace_all(text, "[redacted-secret]");
    let masked = EMAIL.replace_all(&masked, "[redacted-personal]");
    let masked = FILE_PATH.replace_all(&masked, "[redacted-path]");
    let masked = masked.trim();
    let start = masked
        .char_indices()
        .rev()
        .nth(MAX_ANALYSIS_CONTEXT_CHARS)
        .map_or(0, |(index, _)| index);
    masked[start..].to_owned()
}

pub fn context_fingerprint(context: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    context.hash(&mut hasher);
    hasher.finish()
}

pub trait SessionReader {
    fn read(&mut self, pane: &Pane) -> Result<ParsedSession>;
}

pub struct LocalSessionReader {
    locator: SessionLocator,
    sessions: HashMap<String, PaneSessionState>,
}

struct PaneSessionState {
    path: PathBuf,
    cursor: SessionCursor,
    events: std::collections::VecDeque<SessionEvent>,
}

const MAX_RETAINED_EVENTS: usize = 512;

impl LocalSessionReader {
    pub fn new(home: &Path) -> Self {
        Self {
            locator: SessionLocator::new(home),
            sessions: HashMap::new(),
        }
    }

    fn session_path(&mut self, pane: &Pane) -> Result<PathBuf> {
        let identity = match pane.agent_session.as_ref() {
            None => None,
            Some(session) => match session.kind.as_str() {
                "id" => Some(SessionIdentity::id(&session.value)),
                "path" => Some(SessionIdentity::path(&session.value)),
                _ if pane.cwd.is_some() => None,
                _ => return Err(anyhow!("session_kind_unsupported")),
            },
        };
        let agent = match pane.agent {
            AgentKind::Claude => SessionAgent::Claude,
            AgentKind::Codex => SessionAgent::Codex,
        };
        self.locator
            .locate(&pane.id, agent, identity.as_ref(), pane.cwd.as_deref())
            .map_err(Into::into)
    }

    fn retain_bounded(events: &mut std::collections::VecDeque<SessionEvent>) {
        let human_positions = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| (event.kind == EventKind::Human).then_some(index))
            .collect::<Vec<_>>();
        let human_count = human_positions.len();
        let first_recent_human = human_count.saturating_sub(MAX_USER_REQUEST_TURNS);
        let second_last_human = human_positions.iter().rev().nth(1).copied();
        let mut human_seen = 0;
        let mut retained = std::collections::VecDeque::new();
        for (index, event) in events.drain(..).enumerate() {
            let keep = if event.kind == EventKind::Human {
                let keep = human_seen >= first_recent_human;
                human_seen += 1;
                keep
            } else {
                match human_positions.as_slice() {
                    [] => false,
                    [first_human] => index >= *first_human,
                    _ => second_last_human.is_some_and(|boundary| index >= boundary),
                }
            };
            if keep {
                retained.push_back(event);
            }
        }
        *events = retained;

        // A streaming response can contain many assistant records after the
        // second-last Human event. Keep the required Human history and bound
        // the rest so a pane cannot accumulate an unbounded transcript.
        while events.len() > MAX_RETAINED_EVENTS {
            let Some(index) = events
                .iter()
                .position(|event| event.kind != EventKind::Human)
            else {
                break;
            };
            events.remove(index);
        }
    }
}

impl SessionReader for LocalSessionReader {
    fn read(&mut self, pane: &Pane) -> Result<ParsedSession> {
        let path = self.session_path(pane)?;
        let state = self
            .sessions
            .entry(pane.id.clone())
            .or_insert_with(|| PaneSessionState {
                path: path.clone(),
                cursor: SessionCursor::new(),
                events: std::collections::VecDeque::new(),
            });
        if state.path != path {
            state.path = path.clone();
            state.cursor.reset();
            state.events.clear();
        }
        let chunk = state.cursor.read(&path).map_err(anyhow::Error::from)?;
        if chunk.rescan_reason.is_some() {
            state.events.clear();
        }
        let parsed = parse_events(
            match pane.agent {
                AgentKind::Claude => SessionAgent::Claude,
                AgentKind::Codex => SessionAgent::Codex,
            },
            &chunk.contents,
        );
        for event in parsed.events {
            if event.kind != EventKind::Injected {
                state.events.push_back(event);
            }
        }
        Self::retain_bounded(&mut state.events);
        Ok(ParsedSession {
            events: state.events.iter().cloned().collect(),
            skipped_lines: parsed.skipped_lines,
            skipped_reasons: parsed.skipped_reasons,
            rescan_reason: chunk.rescan_reason,
        })
    }
}

#[derive(Debug, Serialize)]
struct LogEvent<'a> {
    schema_version: &'static str,
    timestamp_unix_ms: u128,
    event: &'a str,
    pane_id: Option<&'a str>,
    agent: Option<&'a str>,
    detail: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct StatePaths {
    pub root: PathBuf,
}

impl StatePaths {
    pub fn from_home(home: &Path) -> Self {
        Self {
            root: home.join(".local/state").join(PLUGIN_ID),
        }
    }

    pub fn for_tests(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    fn log(&self) -> PathBuf {
        self.root.join("events.jsonl")
    }
    fn lock(&self) -> PathBuf {
        self.root.join("watcher.lock")
    }
    fn settings_lock(&self) -> PathBuf {
        self.root.join("settings.lock")
    }
    fn hook_state(&self) -> PathBuf {
        self.root.join("hook-state.json")
    }
    fn hook_state_lock(&self) -> PathBuf {
        self.root.join("hook-state.lock")
    }
    fn display_state(&self) -> PathBuf {
        self.root.join("display-state.json")
    }
    pub fn refresh_request(&self) -> PathBuf {
        self.root.join("refresh-request")
    }

    pub fn wake_socket(&self) -> PathBuf {
        self.root.join("watcher.sock")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HookStates {
    panes: HashMap<String, HookState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HookState {
    attention: Option<Attention>,
    #[serde(default)]
    pending_tool_id: Option<String>,
    updated_unix_ms: u64,
    observed_blocked: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DisplayStates {
    panes: HashMap<String, PersistedDisplayState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedDisplayState {
    state_change_seq: u64,
    changed_unix_ms: u64,
    summary: Option<String>,
    #[serde(default)]
    semantic_attention: Option<Attention>,
    /// When the semantic verdict was drawn, so a later hook signal can retire it.
    #[serde(default)]
    analysis_unix_ms: u64,
    /// The turn whose start has already been named, so a task summary costs one
    /// request per turn however long the turn runs.
    #[serde(default)]
    analysis_turn_start: Option<u64>,
    /// The turn whose end has already been judged. This is what holds an
    /// attention verdict still: once the turn is recorded here, further output
    /// or a flapping lifecycle cannot draw a second, contradicting verdict.
    #[serde(default)]
    analysis_turn_end: Option<u64>,
    /// The user tore the last turn down mid-run; shown as its own status until
    /// the pane works again.
    #[serde(default)]
    interrupted: bool,
    /// The pane changed state and has not been focused since. Approximates
    /// Herdr's `seen`, which the socket API does not expose to readers.
    #[serde(default)]
    unseen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookUpdate {
    Set(Attention),
    Clear,
    Ignore,
}

fn unix_time_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX))
}

fn locked_state_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    file.lock_exclusive()?;
    Ok(file)
}

/// A corrupt state file must not stop the watcher from starting: the state is a
/// cache of what the panes already say, so it is safe to rebuild from scratch.
fn load_state_json<T: Default + serde::de::DeserializeOwned>(path: &Path) -> (T, Option<String>) {
    match fs::read_to_string(path) {
        Ok(value) => match serde_json::from_str(&value) {
            Ok(parsed) => (parsed, None),
            Err(error) => (T::default(), Some(error.to_string())),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (T::default(), None),
        Err(error) => (T::default(), Some(error.to_string())),
    }
}

fn load_hook_states(paths: &StatePaths) -> HookStates {
    load_state_json(&paths.hook_state()).0
}

pub fn classify_hook_payload(payload: &serde_json::Value) -> HookUpdate {
    let event = payload
        .get("hook_event_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let tool = payload
        .get("tool_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_question_tool = matches!(
        tool.as_str(),
        "askuserquestion" | "request_user_input" | "functions.request_user_input"
    ) || tool.ends_with(".request_user_input");

    match event {
        "PreToolUse" if is_question_tool => HookUpdate::Set(Attention::Question),
        "PermissionRequest" if is_question_tool => HookUpdate::Set(Attention::Question),
        "PermissionRequest" => HookUpdate::Set(Attention::Approval),
        "StopFailure" => HookUpdate::Set(Attention::Error),
        "UserPromptSubmit" | "SessionStart" => HookUpdate::Clear,
        "PostToolUse" | "PostToolUseFailure" => HookUpdate::Clear,
        _ => HookUpdate::Ignore,
    }
}

fn hook_tool_id(payload: &serde_json::Value) -> Option<String> {
    ["tool_use_id", "tool_call_id", "call_id"]
        .into_iter()
        .find_map(|key| payload.get(key).and_then(serde_json::Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// The hook records what it saw and returns. Rendering belongs to the watcher,
/// which is the only writer of this plugin's display tokens.
pub fn apply_hook_payload(
    paths: &StatePaths,
    pane_id: &str,
    payload: &serde_json::Value,
) -> Result<HookUpdate> {
    let update = classify_hook_payload(payload);
    if update == HookUpdate::Ignore {
        return Ok(update);
    }
    fs::create_dir_all(&paths.root)?;
    let _lock = locked_state_file(&paths.hook_state_lock())?;
    let mut states = load_hook_states(paths);
    let event = payload
        .get("hook_event_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let tool_id = hook_tool_id(payload);
    if update == HookUpdate::Clear
        && matches!(event, "PostToolUse" | "PostToolUseFailure")
        && states
            .panes
            .get(pane_id)
            .and_then(|state| state.pending_tool_id.as_ref())
            .is_some_and(|pending_id| tool_id.as_ref() != Some(pending_id))
    {
        return Ok(HookUpdate::Ignore);
    }
    let attention = match update {
        HookUpdate::Set(attention) => Some(attention),
        HookUpdate::Clear => None,
        HookUpdate::Ignore => unreachable!(),
    };
    states.panes.insert(
        pane_id.to_owned(),
        HookState {
            attention,
            pending_tool_id: if matches!(update, HookUpdate::Set(_)) {
                tool_id
            } else {
                None
            },
            updated_unix_ms: unix_time_ms()?,
            observed_blocked: false,
        },
    );
    write_state_json(&paths.hook_state(), &states, "hook-state")?;
    wake_watcher(paths);
    Ok(update)
}

fn write_state_json(path: &Path, value: &impl Serialize, prefix: &str) -> Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| anyhow!("state path has no directory"))?;
    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!("{prefix}.{}.json.tmp", std::process::id()));
    fs::write(&temporary, serde_json::to_vec(value)?)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    pub automatic_summaries: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            automatic_summaries: true,
        }
    }
}

pub fn load_settings(paths: &StatePaths) -> Settings {
    load_state_json::<Option<Settings>>(&paths.settings())
        .0
        .unwrap_or_default()
}

/// Set the target state rather than flipping the current one. Herdr can
/// dispatch the same action several times for one keypress, and a flip run
/// three times lands somewhere nobody asked for.
pub fn set_automatic_summaries(paths: &StatePaths, enabled: bool) -> Result<()> {
    fs::create_dir_all(&paths.root).context("cannot create state directory")?;
    let _lock = settings_lock(paths)?;
    write_state_json(
        &paths.settings(),
        &Settings {
            automatic_summaries: enabled,
        },
        "settings",
    )
    .context("cannot write settings")
}

/// Moves the state and config directories written under the previous plugin
/// id to the current id, once. A directory that already exists under the new
/// id is left alone and the old one is reported rather than merged, because
/// merging two display-state files has no right answer.
pub fn migrate_legacy_state(home: &Path) -> Result<Vec<String>> {
    let mut moved = Vec::new();
    for base in [".local/state", ".config/herdr/plugins/config"] {
        let old = home.join(base).join(LEGACY_PLUGIN_ID);
        let new = home.join(base).join(PLUGIN_ID);
        if !old.is_dir() {
            continue;
        }
        if new.exists() {
            moved.push(format!("{base}=kept_both"));
            continue;
        }
        fs::rename(&old, &new)
            .with_context(|| format!("cannot move {base}/{LEGACY_PLUGIN_ID} to {PLUGIN_ID}"))?;
        moved.push(format!("{base}=moved"));
    }
    Ok(moved)
}

pub fn append_log(
    paths: &StatePaths,
    event: &str,
    pane: Option<&Pane>,
    detail: Option<&str>,
) -> Result<()> {
    fs::create_dir_all(&paths.root).context("cannot create state directory")?;
    let record = LogEvent {
        schema_version: "hide.agent-context-labels.event.v1",
        timestamp_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        event,
        pane_id: pane.map(|item| item.id.as_str()),
        agent: pane.map(|item| item.agent.as_str()),
        detail,
    };
    let data = serde_json::to_string(&record)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    writeln!(file, "{data}")?;
    // Sweeping the state directory on every line would cost a full read_dir per
    // event; only a rotation can change what retention has to remove.
    if file.metadata()?.len() > MAX_LOG_BYTES {
        enforce_retention(paths)?;
    }
    Ok(())
}

const MAX_LOG_BYTES: u64 = 30 * 1024 * 1024;

pub fn enforce_retention(paths: &StatePaths) -> Result<()> {
    fs::create_dir_all(&paths.root)?;
    let log = paths.log();
    const MAX_FILES: usize = 3;
    const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
    if log.exists() && fs::metadata(&log)?.len() > MAX_LOG_BYTES {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        fs::rename(&log, paths.root.join(format!("events.{stamp}.jsonl")))?;
    }
    let now = SystemTime::now();
    let mut logs: Vec<_> = fs::read_dir(&paths.root)?
        .flatten()
        .filter(|item| item.file_name().to_string_lossy().starts_with("events"))
        .filter_map(|item| item.metadata().ok().map(|metadata| (item.path(), metadata)))
        .collect();
    for (path, metadata) in &logs {
        if now
            .duration_since(metadata.modified().unwrap_or(UNIX_EPOCH))
            .unwrap_or_default()
            > MAX_AGE
        {
            let _ = fs::remove_file(path);
        }
    }
    logs.retain(|(path, _)| path.exists());
    logs.sort_by_key(|(_, metadata)| metadata.modified().unwrap_or(UNIX_EPOCH));
    let mut total: u64 = logs.iter().map(|(_, metadata)| metadata.len()).sum();
    while logs.len() > MAX_FILES || total > MAX_LOG_BYTES {
        let (path, metadata) = logs.remove(0);
        total = total.saturating_sub(metadata.len());
        fs::remove_file(path)?;
    }
    Ok(())
}

pub trait HerdrTransport {
    fn panes(&self) -> Result<Vec<Pane>>;
    fn report(&self, pane: &Pane, display: &Display) -> Result<()>;
}

pub trait HerdrEventTransport: HerdrTransport {
    fn subscribe(
        &self,
        after_sequence: u64,
        pane_ids: &[String],
    ) -> std::result::Result<Subscription, ApiError>;
    fn apply_priority_view(&self) -> Result<()>;
}

const HERDR_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct SocketHerdr {
    connector: Arc<dyn ApiConnector>,
    timeout: Duration,
}

impl SocketHerdr {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            connector: Arc::new(UnixSocketConnector::new(socket_path)),
            timeout: HERDR_REQUEST_TIMEOUT,
        }
    }

    pub fn from_environment(home: &Path) -> Self {
        let socket_path = std::env::var_os("HERDR_SOCKET_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config/herdr/herdr.sock"));
        Self::new(socket_path)
    }
}

impl HerdrTransport for SocketHerdr {
    fn panes(&self) -> Result<Vec<Pane>> {
        let value = request_with_connector(
            self.connector.as_ref(),
            "agent.list",
            serde_json::json!({}),
            self.timeout,
        )
        .map_err(|error| anyhow!("agent.list failed: {error}"))?;
        let result: AgentListResult =
            serde_json::from_value(value).context("invalid Herdr agent list")?;
        Ok(result
            .agents
            .into_iter()
            .filter_map(AgentListItem::into_pane)
            .collect())
    }

    fn report(&self, pane: &Pane, display: &Display) -> Result<()> {
        request_with_connector(
            self.connector.as_ref(),
            "pane.report_metadata",
            metadata_params(pane, display),
            self.timeout,
        )
        .map(|_| ())
        .map_err(|error| anyhow!("pane.report_metadata failed: {error}"))
    }
}

impl HerdrEventTransport for SocketHerdr {
    fn subscribe(
        &self,
        after_sequence: u64,
        pane_ids: &[String],
    ) -> std::result::Result<Subscription, ApiError> {
        hide_herdr_client::subscribe_with_connector_for_panes(
            self.connector.as_ref(),
            after_sequence,
            &WATCHER_SUBSCRIPTIONS,
            pane_ids,
            self.timeout,
        )
    }

    fn apply_priority_view(&self) -> Result<()> {
        let result = request_with_correlation_id(
            self.connector.as_ref(),
            "agent-context-labels:view",
            "agent.view.set",
            priority_agent_view_params(),
            self.timeout,
        )
        .map_err(|error| anyhow!("agent.view.set failed: {error}"))?;
        if result.get("active") == Some(&serde_json::Value::Bool(true)) {
            Ok(())
        } else {
            Err(anyhow!("agent_view_rejected"))
        }
    }
}

fn metadata_params(pane: &Pane, display: &Display) -> serde_json::Value {
    let mut tokens = serde_json::Map::new();
    let status_token = match (display.unseen, display.status) {
        (true, StatusIcon::Question) => "status_question_new",
        (true, StatusIcon::Approval) => "status_approval_new",
        (true, StatusIcon::Error) => "status_error_new",
        _ => display.status.token_name(),
    };
    for token in STATUS_TOKENS {
        let value = (token == status_token).then(|| display.status.symbol().to_owned());
        tokens.insert(
            token.to_owned(),
            value.map_or(serde_json::Value::Null, serde_json::Value::String),
        );
    }
    tokens.insert(
        "summary".to_owned(),
        display
            .summary
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    tokens.insert(
        "sort_rank".to_owned(),
        serde_json::Value::String(sort_rank_token(&SORT_ORDER, display)),
    );
    tokens.insert(
        "activity".to_owned(),
        serde_json::Value::String(activity_token(display.activity_unix_ms)),
    );
    tokens.insert(
        "elapsed".to_owned(),
        display
            .elapsed
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    tokens.insert(
        pane.agent.icon_token().to_owned(),
        serde_json::Value::String(pane.agent.label().to_owned()),
    );
    serde_json::json!({
        "pane_id": pane.id,
        "source": PLUGIN_ID,
        "tokens": tokens,
    })
}

fn priority_agent_view_params() -> serde_json::Value {
    serde_json::json!({
        "source": format!("plugin:{PLUGIN_ID}"),
        "label": "attention priority",
        "sort": [
            {"field": {"token": "sort_rank"}, "order": "asc"},
            {"field": {"token": "activity"}, "order": "desc"},
        ],
    })
}

/// Why one analysis produced no verdict, and what the watcher does about it.
enum AnalysisFailure {
    /// The provider layer gave up after its own retries, or no provider is
    /// connected; the class says which.
    Provider(AiError),
    /// A well-formed answer whose content the feature refuses.
    Invalid(String),
    /// The analysis thread itself failed; a bug, reported rather than lost.
    Worker(String),
}

impl AnalysisFailure {
    /// A wait means the same context can succeed once the environment changes
    /// (a login, a usage window). `None` is a failure the input settles, and
    /// the turn is parked with it rather than asked again.
    fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Provider(AiError::UsageLimited { retry_after }) => {
                Some(retry_after.unwrap_or(PROVIDER_RECOVERY_INTERVAL))
            }
            Self::Provider(
                AiError::NotAuthenticated
                | AiError::NoProvider(_)
                | AiError::ProviderUnavailable(_),
            ) => Some(PROVIDER_RECOVERY_INTERVAL),
            Self::Provider(_) | Self::Invalid(_) | Self::Worker(_) => None,
        }
    }

    fn detail(&self) -> String {
        match self {
            Self::Provider(error) => error.to_string(),
            Self::Invalid(reason) | Self::Worker(reason) => reason.clone(),
        }
    }
}

struct AnalysisOutcome {
    pane_id: String,
    turn: u64,
    phase: AnalysisPhase,
    context_chars: usize,
    result: std::result::Result<(ProviderId, Analysis), AnalysisFailure>,
}

/// One label request on the calling thread: the router's answer, then the
/// feature's reading of it, with the answering provider kept alongside.
fn analyze(
    router: &AiRouter,
    request: &hide_ai::AiRequest,
) -> std::result::Result<(ProviderId, Analysis), AnalysisFailure> {
    let AiResult { provider, value } = router
        .execute(request, &CancelToken::new())
        .map_err(AnalysisFailure::Provider)?;
    let analysis = context_label::parse(value)
        .map_err(|error| AnalysisFailure::Invalid(format!("{error:#}")))?;
    Ok((provider, analysis))
}

pub struct Watcher<T: HerdrTransport, R: SessionReader> {
    transport: T,
    router: Arc<AiRouter>,
    session_reader: R,
    paths: StatePaths,
    settings: Settings,
    hook_states: HookStates,
    revisions: HashMap<String, u64>,
    state_change_seqs: HashMap<String, u64>,
    reported_revisions: HashMap<String, u64>,
    next_analysis_at: HashMap<String, SystemTime>,
    display_states: DisplayStates,
    last_displays: HashMap<String, Display>,
    last_animation_at: SystemTime,
    analysis_in_flight: HashSet<String>,
    analysis_sender: mpsc::Sender<AnalysisOutcome>,
    analysis_receiver: mpsc::Receiver<AnalysisOutcome>,
    /// The home whose `hide-ai` settings file this watcher follows, and the
    /// choice it last read from it. `None` for a watcher given its router
    /// directly, which is what a test does.
    ai_settings: Option<(PathBuf, hide_ai::AiSettings)>,
    /// Why that file last failed to read, so a reason is logged when it
    /// changes rather than on every event. The plugin's log is never rotated,
    /// so a broken file logged unconditionally would grow it without end.
    ai_settings_failure: Option<String>,
}

impl<T: HerdrTransport, R: SessionReader> Watcher<T, R> {
    pub fn new(transport: T, router: Arc<AiRouter>, session_reader: R, paths: StatePaths) -> Self {
        let (display_states, display_error) = load_state_json(&paths.display_state());
        if let Some(error) = display_error {
            let _ = append_log(&paths, "display_state_reset", None, Some(&error));
        }
        let (analysis_sender, analysis_receiver) = mpsc::channel();
        Self {
            transport,
            router,
            session_reader,
            settings: load_settings(&paths),
            hook_states: load_hook_states(&paths),
            paths,
            revisions: HashMap::new(),
            state_change_seqs: HashMap::new(),
            reported_revisions: HashMap::new(),
            next_analysis_at: HashMap::new(),
            display_states,
            last_displays: HashMap::new(),
            last_animation_at: SystemTime::now(),
            analysis_in_flight: HashSet::new(),
            analysis_sender,
            analysis_receiver,
            ai_settings: None,
            ai_settings_failure: None,
        }
    }

    /// Follows the operator's saved provider and model choice from this home.
    ///
    /// The choice is re-read on every watcher wake, beside this plugin's own
    /// settings, and a changed one rebuilds the router: a backend is constructed with
    /// its model, and the priority is what the choice reorders. Rebuilding
    /// also drops the sticky failover state, which is right, because the
    /// reason it was sticky was about the provider that is no longer chosen.
    pub fn follow_ai_settings(&mut self, home: &Path) {
        let settings = self.read_ai_settings(home);
        self.router = crate::provider::router(&settings, &self.paths);
        self.ai_settings = Some((home.to_path_buf(), settings));
    }

    /// Reads the choice and records a change in whether it could be read at
    /// all: the reason a file is unreadable is written once, and so is the
    /// recovery, so fixing the file is visible in the same log.
    fn read_ai_settings(&mut self, home: &Path) -> hide_ai::AiSettings {
        let (settings, failure) = crate::provider::settings(home);
        if failure != self.ai_settings_failure {
            match failure.as_deref() {
                Some(reason) => {
                    let _ = append_log(&self.paths, "ai_settings_unreadable", None, Some(reason));
                }
                None => {
                    let _ = append_log(&self.paths, "ai_settings_readable", None, None);
                }
            }
            self.ai_settings_failure = failure;
        }
        settings
    }

    /// Re-reads the choice and rebuilds the router when it moved. Returns
    /// whether it moved, so the caller can record it.
    fn refresh_ai_settings(&mut self) -> bool {
        let Some((home, current)) = self.ai_settings.as_ref() else {
            return false;
        };
        let home = home.clone();
        let current = current.clone();
        let read = self.read_ai_settings(&home);
        if read == current {
            return false;
        }
        self.router = crate::provider::router(&read, &self.paths);
        self.ai_settings = Some((home, read));
        true
    }

    pub fn scan(&mut self) -> Result<usize> {
        let panes = self.transport.panes()?;
        let refresh_requested = self.take_refresh_request();
        let elapsed_due =
            self.last_animation_at.elapsed().unwrap_or_default() >= Duration::from_millis(900);
        self.scan_panes(&panes, refresh_requested, elapsed_due)
    }

    /// Process a known pane snapshot without asking Herdr for another one.
    /// Event-driven production code calls this after a subscription event or a
    /// wake, while `scan` remains a deterministic compatibility seam for the
    /// feature's unit tests.
    pub fn scan_panes(
        &mut self,
        panes: &[Pane],
        refresh_requested: bool,
        elapsed_due: bool,
    ) -> Result<usize> {
        self.scan_panes_at(panes, refresh_requested, elapsed_due, None, false)
    }

    fn scan_panes_at(
        &mut self,
        panes: &[Pane],
        refresh_requested: bool,
        elapsed_due: bool,
        event_received_at: Option<std::time::Instant>,
        replayed_event: bool,
    ) -> Result<usize> {
        // Both files are read once per scan rather than once per pane.
        self.hook_states = load_hook_states(&self.paths);
        self.settings = load_settings(&self.paths);
        // The operator's provider and model choice lives in `hide-ai`'s own
        // file, read on this same boundary rather than on a third schedule.
        if self.refresh_ai_settings()
            && let Some((_, settings)) = self.ai_settings.as_ref()
        {
            let detail = crate::provider::settings_detail(settings);
            let _ = append_log(&self.paths, "ai_settings_changed", None, Some(&detail));
        }
        let mut processed = 0;

        while let Ok(outcome) = self.analysis_receiver.try_recv() {
            self.analysis_in_flight.remove(&outcome.pane_id);
            let Some(pane) = panes.iter().find(|pane| pane.id == outcome.pane_id) else {
                continue;
            };
            match outcome.result {
                Ok((provider, analysis)) => {
                    self.record_analysis(
                        pane,
                        provider,
                        &analysis,
                        outcome.turn,
                        outcome.phase,
                        outcome.context_chars,
                    )?;
                    self.next_analysis_at
                        .insert(pane.id.clone(), SystemTime::now());
                    let display = self.display_for(pane)?;
                    if self.report_if_changed(pane, &display)? {
                        processed += 1;
                    }
                }
                Err(failure) => match failure.retry_after() {
                    Some(wait) => {
                        self.next_analysis_at
                            .insert(pane.id.clone(), SystemTime::now() + wait);
                        append_log(
                            &self.paths,
                            "analysis_provider_unavailable",
                            Some(pane),
                            Some(&failure.detail()),
                        )?;
                    }
                    None => {
                        // Park this turn so it is never asked again until the
                        // user takes the next one.
                        self.next_analysis_at.remove(&pane.id);
                        self.abandon_analysis(pane, outcome.turn)?;
                        append_log(
                            &self.paths,
                            "analysis_abandoned",
                            Some(pane),
                            Some(&failure.detail()),
                        )?;
                    }
                },
            }
        }

        // The status symbol no longer animates, but elapsed time still moves,
        // so the same tick keeps driving a redraw.
        if elapsed_due {
            self.last_animation_at = SystemTime::now();
        }

        let event_unix_ms = match event_received_at {
            Some(received_at) => {
                let age = std::time::Instant::now()
                    .saturating_duration_since(received_at)
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX);
                Some(unix_time_ms()?.saturating_sub(age))
            }
            None => None,
        };
        for pane in panes {
            let lifecycle_changed = self.observe_lifecycle(pane, event_unix_ms, replayed_event)?;
            let revision_changed = self.revisions.get(&pane.id) != Some(&pane.revision);
            let state_changed =
                self.state_change_seqs.get(&pane.id) != Some(&pane.state_change_seq);
            let retry_due = self
                .next_analysis_at
                .get(&pane.id)
                .is_some_and(|retry_at| SystemTime::now() >= *retry_at);
            self.revisions.insert(pane.id.clone(), pane.revision);
            self.state_change_seqs
                .insert(pane.id.clone(), pane.state_change_seq);
            let own_revision =
                revision_changed && self.reported_revisions.remove(&pane.id) == Some(pane.revision);
            let forced = refresh_requested && pane.focused;
            let needs_full_refresh =
                forced || state_changed || retry_due || (revision_changed && !own_revision);
            let changed = if needs_full_refresh {
                self.process(pane, forced)?
            } else if lifecycle_changed || elapsed_due || self.elapsed_changed(pane)? {
                let display = self.display_for(pane)?;
                self.report_if_changed(pane, &display)?
            } else {
                false
            };
            if changed {
                processed += 1;
            }
        }
        Ok(processed)
    }

    /// The refresh action only leaves a marker; the watcher owns every read and
    /// every write, so a second process never races it for the state files.
    fn take_refresh_request(&self) -> bool {
        let path = self.paths.refresh_request();
        path.exists() && fs::remove_file(&path).is_ok()
    }

    fn process(&mut self, pane: &Pane, forced: bool) -> Result<bool> {
        let parsed = match self.session_reader.read(pane) {
            Ok(parsed) => parsed,
            Err(error) => {
                append_log(
                    &self.paths,
                    "raw_session_unavailable",
                    Some(pane),
                    Some(&format!("{error:#}")),
                )?;
                let display = self.display_for(pane)?;
                return self.report_if_changed(pane, &display);
            }
        };
        if let Some(reason) = parsed.rescan_reason {
            append_log(
                &self.paths,
                "session_rescanned",
                Some(pane),
                Some(reason.as_str()),
            )?;
        }
        if parsed.skipped_lines > 0 {
            let reasons = parsed
                .skipped_reasons
                .iter()
                .map(|(reason, count)| format!("{}={count}", reason.as_str()))
                .collect::<Vec<_>>()
                .join(",");
            append_log(
                &self.paths,
                "session_lines_skipped",
                Some(pane),
                Some(&format!("lines={};reasons={reasons}", parsed.skipped_lines)),
            )?;
        }
        let newest_user_is_last = parsed
            .events
            .iter()
            .rev()
            .find(|event| {
                matches!(
                    event.kind,
                    EventKind::Human | EventKind::Interrupted | EventKind::Assistant
                )
            })
            .is_some_and(|event| event.kind == EventKind::Human);
        // An interruption is the user's own act, not a new task: keep the last
        // summary, mark it, and spend no provider request on the torn turn.
        let interrupted = pane.agent_status != "working"
            && parsed
                .events
                .iter()
                .rev()
                .find(|event| matches!(event.kind, EventKind::Human | EventKind::Interrupted))
                .is_some_and(|event| event.kind == EventKind::Interrupted);
        if interrupted && !forced {
            let now = unix_time_ms()?;
            let state = self
                .display_states
                .panes
                .entry(pane.id.clone())
                .or_insert_with(|| PersistedDisplayState {
                    state_change_seq: pane.state_change_seq,
                    changed_unix_ms: now,
                    ..PersistedDisplayState::default()
                });
            // Both phases are settled for this turn: the user's own teardown is
            // not a question to spend a request on, and the turn is over.
            let turn = turn_key(&parsed.events);
            state.analysis_turn_start = turn;
            state.analysis_turn_end = turn;
            state.interrupted = true;
            write_state_json(
                &self.paths.display_state(),
                &self.display_states,
                "display-state",
            )?;
            append_log(
                &self.paths,
                "interruption_marked",
                Some(pane),
                Some(&format!("status={}", pane.agent_status)),
            )?;
            let display = self.display_for(pane)?;
            return self.report_if_changed(pane, &display);
        }
        if !self.settings.automatic_summaries {
            if forced {
                append_log(
                    &self.paths,
                    "summary_refresh_skipped_disabled",
                    Some(pane),
                    None,
                )?;
            }
            let display = self.display_for(pane)?;
            return self.report_if_changed(pane, &display);
        }

        let context = analysis_context(&parsed.events);
        let display = self.display_for(pane)?;
        if context.is_empty() {
            append_log(
                &self.paths,
                "analysis_skipped_empty_context",
                Some(pane),
                None,
            )?;
            return self.report_if_changed(pane, &display);
        }
        // No user message means no turn, so there is nothing to name or judge.
        let Some(turn) = turn_key(&parsed.events) else {
            self.next_analysis_at.remove(&pane.id);
            return self.report_if_changed(pane, &display);
        };
        let persisted = self.display_states.panes.get(&pane.id);
        let phase = if forced {
            // A refresh request is the user asking directly, so it re-answers
            // whichever boundary the pane is currently sitting on.
            Some(if newest_user_is_last {
                AnalysisPhase::TurnStart
            } else {
                AnalysisPhase::TurnEnd
            })
        } else {
            analysis_phase(
                newest_user_is_last,
                pane.agent_status == "working",
                persisted.and_then(|state| state.analysis_turn_start) == Some(turn),
                persisted.and_then(|state| state.analysis_turn_end) == Some(turn),
            )
        };
        let Some(phase) = phase else {
            self.next_analysis_at.remove(&pane.id);
            return self.report_if_changed(pane, &display);
        };
        if self.analysis_in_flight.contains(&pane.id) {
            return self.report_if_changed(pane, &display);
        }
        let router = Arc::clone(&self.router);
        let sender = self.analysis_sender.clone();
        let pane_id = pane.id.clone();
        let context_chars = context.chars().count();
        // The same pane, turn, boundary and context is the same intent: a
        // repeat carries the same key to the provider and the log.
        let request_id = format!(
            "{}:{turn:016x}:{}:{:016x}",
            pane.id,
            phase.label(),
            context_fingerprint(&context)
        );
        let request = context_label::request(&pane.id, request_id, &context);
        self.analysis_in_flight.insert(pane_id.clone());
        std::thread::spawn(move || {
            // The outcome must arrive whatever happens on this thread: a
            // panic that escaped would leave the pane in flight forever.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                analyze(&router, &request)
            }))
            .unwrap_or_else(|_| {
                Err(AnalysisFailure::Worker(
                    "analysis_worker_panicked".to_owned(),
                ))
            });
            let _ = sender.send(AnalysisOutcome {
                pane_id,
                turn,
                phase,
                context_chars,
                result,
            });
        });
        self.report_if_changed(pane, &display)
    }

    fn observe_lifecycle(
        &mut self,
        pane: &Pane,
        event_unix_ms: Option<u64>,
        replayed_event: bool,
    ) -> Result<bool> {
        self.sync_hook_lifecycle(pane)?;
        let now = unix_time_ms()?;
        let is_new = !self.display_states.panes.contains_key(&pane.id);
        let state = self
            .display_states
            .panes
            .entry(pane.id.clone())
            .or_insert_with(|| PersistedDisplayState {
                state_change_seq: pane.state_change_seq,
                changed_unix_ms: now,
                ..PersistedDisplayState::default()
            });
        let mut changed = is_new;
        if state.state_change_seq != pane.state_change_seq {
            state.state_change_seq = pane.state_change_seq;
            if !replayed_event {
                state.changed_unix_ms = event_unix_ms.unwrap_or(now);
            }
            state.unseen = !pane.focused;
            changed = true;
        }
        // Focusing the pane is the act of looking at it.
        if pane.focused && state.unseen {
            state.unseen = false;
            changed = true;
        }
        // A running agent is not waiting on anyone, so an older verdict about
        // attention is stale by definition. The same run also ends any
        // interrupted display: the user has resumed the pane.
        if pane.agent_status == "working" && state.semantic_attention.is_some() {
            state.semantic_attention = None;
            changed = true;
        }
        if pane.agent_status == "working" && state.interrupted {
            state.interrupted = false;
            changed = true;
        }
        if !changed {
            return Ok(false);
        }
        write_state_json(
            &self.paths.display_state(),
            &self.display_states,
            "display-state",
        )?;
        Ok(true)
    }

    /// Retire a hook signal that the lifecycle has since overtaken. A dialog
    /// dismissed without a completion hook, or a failed turn the agent has
    /// already moved on from, would otherwise stay pinned forever.
    fn sync_hook_lifecycle(&mut self, pane: &Pane) -> Result<()> {
        enum Change {
            /// The dialog is on screen; arm the retirement for when it leaves.
            Observe,
            Retire,
        }
        let Some(state) = self.hook_states.panes.get(&pane.id) else {
            return Ok(());
        };
        let blocked = pane.agent_status == "blocked";
        let change = match state.attention {
            Some(Attention::Question | Attention::Approval)
                if blocked != state.observed_blocked =>
            {
                if blocked {
                    Change::Observe
                } else {
                    Change::Retire
                }
            }
            Some(Attention::Error) if pane.agent_status == "working" => Change::Retire,
            _ => return Ok(()),
        };
        fs::create_dir_all(&self.paths.root)?;
        let _lock = locked_state_file(&self.paths.hook_state_lock())?;
        let mut states = load_hook_states(&self.paths);
        let Some(state) = states.panes.get_mut(&pane.id) else {
            return Ok(());
        };
        match change {
            Change::Observe => state.observed_blocked = true,
            Change::Retire => {
                state.attention = None;
                state.pending_tool_id = None;
                state.observed_blocked = false;
                state.updated_unix_ms = unix_time_ms()?;
            }
        }
        write_state_json(&self.paths.hook_state(), &states, "hook-state")?;
        self.hook_states = states;
        Ok(())
    }

    /// Give up on one context without inventing a verdict for it: the summary
    /// and the attention already on screen stay untouched, and only the
    /// fingerprint is stored so the deduplication check stops the re-ask.
    /// Give up on a turn after the retry cap. Both phases are recorded so the
    /// retry loop cannot restart from the other boundary of the same turn.
    fn abandon_analysis(&mut self, pane: &Pane, turn: u64) -> Result<()> {
        let now = unix_time_ms()?;
        let state = self
            .display_states
            .panes
            .entry(pane.id.clone())
            .or_insert_with(|| PersistedDisplayState {
                state_change_seq: pane.state_change_seq,
                changed_unix_ms: now,
                ..PersistedDisplayState::default()
            });
        state.analysis_turn_start = Some(turn);
        state.analysis_turn_end = Some(turn);
        write_state_json(
            &self.paths.display_state(),
            &self.display_states,
            "display-state",
        )
    }

    fn record_analysis(
        &mut self,
        pane: &Pane,
        provider: ProviderId,
        analysis: &Analysis,
        turn: u64,
        phase: AnalysisPhase,
        context_chars: usize,
    ) -> Result<()> {
        let now = unix_time_ms()?;
        let state = self
            .display_states
            .panes
            .entry(pane.id.clone())
            .or_insert_with(|| PersistedDisplayState {
                state_change_seq: pane.state_change_seq,
                changed_unix_ms: now,
                ..PersistedDisplayState::default()
            });
        // A semantic question arrives without any lifecycle change, so the
        // unseen flag must be raised here or it would never light up.
        if analysis.attention.is_some() && state.semantic_attention != analysis.attention {
            state.unseen = !pane.focused;
        }
        state.summary = Some(analysis.summary.clone());
        state.semantic_attention = analysis.attention;
        state.interrupted = false;
        match phase {
            AnalysisPhase::TurnStart => state.analysis_turn_start = Some(turn),
            // Recording the start too keeps a turn first seen at its end from
            // going back and asking for a summary it no longer needs.
            AnalysisPhase::TurnEnd => {
                state.analysis_turn_start = Some(turn);
                state.analysis_turn_end = Some(turn);
            }
        }
        state.analysis_unix_ms = now;
        write_state_json(
            &self.paths.display_state(),
            &self.display_states,
            "display-state",
        )?;
        // Enough to reconstruct a verdict later without recording any content.
        append_log(
            &self.paths,
            "analysis_updated",
            Some(pane),
            Some(&format!(
                "attention={};phase={};context_chars={context_chars};turn={turn:016x};provider={provider}",
                analysis.attention.map_or("none", |_| "question"),
                phase.label(),
            )),
        )
    }

    /// A hook signal is a fact about a tool call; a semantic verdict is an
    /// inference. The fact wins, and once the hook says the interaction ended
    /// only a newer inference may speak. The flag records which side spoke so
    /// the ordering can trust a fact more than an inference.
    fn resolve_attention(&self, pane: &Pane) -> Option<(Attention, AttentionSource)> {
        let persisted = self.display_states.panes.get(&pane.id);
        let semantic = |state: &PersistedDisplayState| {
            state
                .semantic_attention
                .map(|attention| (attention, AttentionSource::Semantic))
        };
        match self.hook_states.panes.get(&pane.id) {
            Some(hook) => match hook.attention {
                Some(attention) => Some((attention, AttentionSource::Hook)),
                None => persisted
                    .filter(|state| state.analysis_unix_ms > hook.updated_unix_ms)
                    .and_then(semantic),
            },
            None => persisted.and_then(semantic),
        }
    }

    fn elapsed_for(&self, pane: &Pane) -> Result<Option<String>> {
        let Some(state) = self.display_states.panes.get(&pane.id) else {
            return Ok(None);
        };
        Ok(Some(format_elapsed(
            unix_time_ms()?.saturating_sub(state.changed_unix_ms),
        )))
    }

    fn elapsed_changed(&self, pane: &Pane) -> Result<bool> {
        let elapsed = self.elapsed_for(pane)?;
        Ok(self
            .last_displays
            .get(&pane.id)
            .is_none_or(|display| display.elapsed != elapsed))
    }

    fn display_for(&self, pane: &Pane) -> Result<Display> {
        let attention = self.resolve_attention(pane);
        let interrupted = attention.is_none()
            && matches!(pane.agent_status.as_str(), "idle" | "done" | "blocked")
            && self
                .display_states
                .panes
                .get(&pane.id)
                .is_some_and(|state| state.interrupted);
        let unseen = self
            .display_states
            .panes
            .get(&pane.id)
            .is_some_and(|state| state.unseen);
        let activity_unix_ms = self
            .display_states
            .panes
            .get(&pane.id)
            .map_or(0, |state| state.changed_unix_ms);
        if interrupted {
            return Ok(Display {
                summary: self
                    .display_states
                    .panes
                    .get(&pane.id)
                    .and_then(|state| state.summary.clone()),
                status: StatusIcon::Interrupted,
                sort_key: SortKey::Interrupted,
                elapsed: self.elapsed_for(pane)?,
                unseen,
                activity_unix_ms,
            });
        }
        let sort_key = match attention {
            Some((Attention::Question, AttentionSource::Hook)) => SortKey::Question,
            Some((Attention::Question, AttentionSource::Semantic)) => SortKey::SemanticQuestion,
            Some((Attention::Approval, _)) => SortKey::Approval,
            Some((Attention::Error, _)) => SortKey::Error,
            // A natively blocked pane is stalled on the user as surely as a
            // hook signal, just without the reason.
            None => match pane.agent_status.as_str() {
                "blocked" => SortKey::Question,
                "working" => SortKey::Working,
                "done" => SortKey::Done,
                "idle" => SortKey::Idle,
                _ => SortKey::Stale,
            },
        };
        Ok(Display {
            summary: self
                .display_states
                .panes
                .get(&pane.id)
                .and_then(|state| state.summary.clone()),
            status: status_icon(&pane.agent_status, attention.map(|(kind, _)| kind)),
            sort_key,
            elapsed: self.elapsed_for(pane)?,
            unseen,
            activity_unix_ms,
        })
    }

    fn report_if_changed(&mut self, pane: &Pane, display: &Display) -> Result<bool> {
        if self.last_displays.get(&pane.id) == Some(display) {
            return Ok(false);
        }
        self.transport.report(pane, display)?;
        self.last_displays.insert(pane.id.clone(), display.clone());
        // Herdr increments pane revision when display metadata changes.
        // Record the expected next revision so that our own report is not an event trigger.
        self.reported_revisions
            .insert(pane.id.clone(), pane.revision.saturating_add(1));
        Ok(true)
    }
}

#[derive(Debug)]
enum WatcherMessage {
    Wake,
    SubscriptionLine {
        generation: u64,
        line: String,
        received_at: std::time::Instant,
    },
    SubscriptionEnded {
        generation: u64,
        message: String,
    },
}

struct ActiveWatcherSubscription {
    generation: u64,
    shutdown: Box<dyn hide_herdr_client::ConnectionShutdown>,
    worker: Option<JoinHandle<()>>,
}

impl ActiveWatcherSubscription {
    fn stop(mut self) {
        self.stop_in_place();
    }

    fn stop_in_place(&mut self) {
        self.shutdown.shutdown();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ActiveWatcherSubscription {
    fn drop(&mut self) {
        self.stop_in_place();
    }
}

struct WakeSocket {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl WakeSocket {
    #[cfg(unix)]
    fn start(paths: &StatePaths, sender: mpsc::Sender<WatcherMessage>) -> Result<Self> {
        fs::create_dir_all(&paths.root)?;
        let path = paths.wake_socket();
        if path.exists() {
            fs::remove_file(&path).with_context(|| {
                format!("cannot remove stale watcher wake socket {}", path.display())
            })?;
        }
        let listener = UnixListener::bind(&path)
            .with_context(|| format!("cannot bind watcher wake socket {}", path.display()))?;
        listener
            .set_nonblocking(true)
            .context("cannot configure watcher wake socket")?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("agent-context-labels-wake".to_owned())
            .spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let mut line = String::new();
                            let _ = std::io::BufReader::new(stream).read_line(&mut line);
                            let _ = sender.send(WatcherMessage::Wake);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(_) => break,
                    }
                }
            })
            .context("cannot start watcher wake listener")?;
        Ok(Self {
            path,
            stop,
            worker: Some(worker),
        })
    }

    #[cfg(not(unix))]
    fn start(_paths: &StatePaths, _sender: mpsc::Sender<WatcherMessage>) -> Result<Self> {
        Err(anyhow!(
            "watcher wake socket is unsupported on this platform"
        ))
    }
}

impl Drop for WakeSocket {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        #[cfg(unix)]
        {
            let _ = UnixStream::connect(&self.path);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
enum WatcherSubscriptionLine {
    Event {
        protocol: u64,
        sequence: u64,
        kind: String,
    },
    Error {
        code: String,
        message: String,
    },
}

fn parse_watcher_subscription_line(line: &str) -> Result<WatcherSubscriptionLine> {
    if line.trim().is_empty() {
        return Err(anyhow!("Herdr event stream emitted an empty line"));
    }
    let value: serde_json::Value =
        serde_json::from_str(line).context("Herdr event stream emitted invalid JSON")?;
    if let Some(error) = value.get("error") {
        let id = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("Herdr subscription error is missing id"))?;
        if id != "herdr-core:events.subscribe" {
            return Err(anyhow!("Herdr subscription error id {id:?} is unexpected"));
        }
        let code = error
            .get("code")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("Herdr subscription error is missing code"))?;
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("Herdr subscription error is missing message"))?;
        return Ok(WatcherSubscriptionLine::Error {
            code: code.to_owned(),
            message: message.to_owned(),
        });
    }
    let protocol = value
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow!("Herdr sequenced event is missing protocol"))?;
    let sequence = value
        .get("sequence")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow!("Herdr sequenced event is missing sequence"))?;
    let kind = value
        .get("event")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| data.get("type"))
                .and_then(serde_json::Value::as_str)
        })
        .ok_or_else(|| anyhow!("Herdr sequenced event is missing event type"))?;
    Ok(WatcherSubscriptionLine::Event {
        protocol,
        sequence,
        kind: kind.to_owned(),
    })
}

fn is_watcher_pane_event(kind: &str) -> bool {
    matches!(
        kind,
        "pane.created"
            | "pane.updated"
            | "pane.closed"
            | "pane.exited"
            | "pane.focused"
            | "pane.agent_detected"
            | "pane.agent_status_changed"
            | "pane_created"
            | "pane_updated"
            | "pane_closed"
            | "pane_exited"
            | "pane_focused"
            | "pane_agent_detected"
            | "pane_agent_status_changed"
    )
}

fn watcher_pane_ids(panes: &[Pane]) -> Vec<String> {
    let mut ids = panes.iter().map(|pane| pane.id.clone()).collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn spawn_watcher_subscription(
    subscription: Subscription,
    generation: u64,
    sender: mpsc::Sender<WatcherMessage>,
) -> Result<ActiveWatcherSubscription> {
    let (mut reader, shutdown) = subscription.into_parts();
    let worker = match thread::Builder::new()
        .name("agent-context-labels-events".to_owned())
        .spawn(move || {
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = sender.send(WatcherMessage::SubscriptionEnded {
                            generation,
                            message: "socket reached EOF".to_owned(),
                        });
                        return;
                    }
                    Ok(_) => {
                        if sender
                            .send(WatcherMessage::SubscriptionLine {
                                generation,
                                line,
                                received_at: std::time::Instant::now(),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(WatcherMessage::SubscriptionEnded {
                            generation,
                            message: format!("socket read failed: {error}"),
                        });
                        return;
                    }
                }
            }
        }) {
        Ok(worker) => worker,
        Err(error) => {
            shutdown.shutdown();
            return Err(anyhow!("subscription reader could not be started: {error}"));
        }
    };
    Ok(ActiveWatcherSubscription {
        generation,
        shutdown,
        worker: Some(worker),
    })
}

fn reconnect_delay(delay: Duration) -> Duration {
    (delay * 2).min(Duration::from_secs(5))
}

fn watcher_failure(paths: &StatePaths, streak: &mut u32, error: &str) {
    if *streak == 0 {
        let _ = append_log(paths, "watcher_scan_failed", None, Some(error));
    }
    *streak = streak.saturating_add(1);
}

fn watcher_recovered(paths: &StatePaths, streak: &mut u32) {
    if *streak > 0 {
        let _ = append_log(
            paths,
            "watcher_scan_recovered",
            None,
            Some(&format!("failures={streak}")),
        );
        *streak = 0;
    }
}

impl<T: HerdrEventTransport, R: SessionReader> Watcher<T, R> {
    pub fn apply_priority_view(&self) -> Result<()> {
        self.transport.apply_priority_view()
    }

    /// Run the production watcher loop. Herdr is queried only on startup,
    /// reconnect, and an actual subscribed pane event; idle waits use the
    /// elapsed display boundary or the wake socket.
    pub fn run_event_loop(&mut self) -> Result<()> {
        let (sender, receiver) = mpsc::channel();
        let _wake_socket = WakeSocket::start(&self.paths, sender.clone())?;
        let mut subscription: Option<ActiveWatcherSubscription> = None;
        let mut generation = 0_u64;
        let mut panes = Vec::new();
        let mut cursor = 0_u64;
        let mut replay_until = 0_u64;
        let mut reconnect_at = std::time::Instant::now();
        let mut reconnect_wait = Duration::from_millis(100);
        let mut failure_streak = 0_u32;
        let mut needs_bootstrap = true;

        loop {
            if subscription.is_none() && std::time::Instant::now() >= reconnect_at {
                let bootstrap = self.transport.panes();
                let next_panes = match bootstrap {
                    Ok(next_panes) => next_panes,
                    Err(error) => {
                        watcher_failure(
                            &self.paths,
                            &mut failure_streak,
                            &format!("agent.list failed: {error:#}"),
                        );
                        reconnect_at = std::time::Instant::now() + reconnect_wait;
                        reconnect_wait = reconnect_delay(reconnect_wait);
                        continue;
                    }
                };
                let pane_ids = watcher_pane_ids(&next_panes);
                match self.transport.subscribe(cursor, &pane_ids) {
                    Ok(next) => {
                        let ack = next.ack.clone();
                        let sequence_regressed = ack.sequence < cursor;
                        let rebootstrap = needs_bootstrap
                            || sequence_regressed
                            || (cursor > 0
                                && cursor.saturating_add(1) < ack.oldest_available_sequence);
                        panes = next_panes;
                        let refresh = if panes.is_empty() {
                            false
                        } else {
                            self.take_refresh_request()
                        };
                        if let Err(error) = self.scan_panes(&panes, refresh, false) {
                            watcher_failure(
                                &self.paths,
                                &mut failure_streak,
                                &format!("bootstrap failed: {error:#}"),
                            );
                            next.into_parts().1.shutdown();
                            reconnect_at = std::time::Instant::now() + reconnect_wait;
                            reconnect_wait = reconnect_delay(reconnect_wait);
                            continue;
                        }
                        cursor = if sequence_regressed {
                            ack.sequence
                        } else {
                            cursor.max(ack.sequence)
                        };
                        replay_until = ack.sequence;
                        generation = generation.saturating_add(1);
                        match spawn_watcher_subscription(next, generation, sender.clone()) {
                            Ok(active) => {
                                subscription = Some(active);
                                watcher_recovered(&self.paths, &mut failure_streak);
                                let detail = format!("cursor={cursor};rebootstrap={rebootstrap}");
                                let _ = append_log(
                                    &self.paths,
                                    "herdr_subscription_resumed",
                                    None,
                                    Some(&detail),
                                );
                                reconnect_wait = Duration::from_millis(100);
                                needs_bootstrap = false;
                            }
                            Err(error) => {
                                watcher_failure(
                                    &self.paths,
                                    &mut failure_streak,
                                    &format!("{error:#}"),
                                );
                                reconnect_at = std::time::Instant::now() + reconnect_wait;
                                reconnect_wait = reconnect_delay(reconnect_wait);
                            }
                        }
                    }
                    Err(error) => {
                        let detail = format!("cursor={cursor};error={error}");
                        let _ =
                            append_log(&self.paths, "herdr_subscription_lost", None, Some(&detail));
                        watcher_failure(&self.paths, &mut failure_streak, &error.to_string());
                        if let ApiError::Remote { code, message } = &error
                            && code.contains("protocol")
                        {
                            let mismatch = format!(
                                "Herdr protocol mismatch: expected {HERDR_PROTOCOL_REVISION}, actual {message}"
                            );
                            eprintln!("{mismatch}");
                            let _ = append_log(
                                &self.paths,
                                "herdr_protocol_mismatch",
                                None,
                                Some(&mismatch),
                            );
                            return Err(anyhow!(mismatch));
                        }
                        reconnect_at = std::time::Instant::now() + reconnect_wait;
                        reconnect_wait = reconnect_delay(reconnect_wait);
                    }
                }
            }

            let mut wait = Duration::from_secs(60);
            if subscription.is_none() {
                wait = reconnect_at
                    .saturating_duration_since(std::time::Instant::now())
                    .min(wait);
            } else {
                wait = self.next_elapsed_wait(&panes).min(wait);
            }
            match receiver.recv_timeout(wait) {
                Ok(WatcherMessage::Wake) => {
                    let refresh = if panes.is_empty() {
                        false
                    } else {
                        self.take_refresh_request()
                    };
                    if let Err(error) = self.scan_panes(&panes, refresh, false) {
                        watcher_failure(
                            &self.paths,
                            &mut failure_streak,
                            &format!("wake failed: {error:#}"),
                        );
                    }
                }
                Ok(WatcherMessage::SubscriptionLine {
                    generation: line_generation,
                    line,
                    received_at,
                }) => {
                    if subscription.as_ref().map(|item| item.generation) != Some(line_generation) {
                        continue;
                    }
                    match parse_watcher_subscription_line(&line) {
                        Ok(WatcherSubscriptionLine::Event {
                            protocol,
                            sequence,
                            kind,
                        }) => {
                            if protocol != HERDR_PROTOCOL_REVISION {
                                let mismatch = format!(
                                    "Herdr protocol mismatch: expected {HERDR_PROTOCOL_REVISION}, actual {protocol}"
                                );
                                eprintln!("{mismatch}");
                                let _ = append_log(
                                    &self.paths,
                                    "herdr_protocol_mismatch",
                                    None,
                                    Some(&mismatch),
                                );
                                if let Some(active) = subscription.take() {
                                    active.stop();
                                }
                                return Err(anyhow!(mismatch));
                            }
                            if !is_watcher_pane_event(&kind) {
                                continue;
                            }
                            let replayed_event = sequence <= replay_until;
                            cursor = cursor.max(sequence);
                            match self.transport.panes() {
                                Ok(next_panes) => {
                                    let pane_ids_changed =
                                        watcher_pane_ids(&panes) != watcher_pane_ids(&next_panes);
                                    panes = next_panes;
                                    let refresh = if panes.is_empty() {
                                        false
                                    } else {
                                        self.take_refresh_request()
                                    };
                                    if let Err(error) = self.scan_panes_at(
                                        &panes,
                                        refresh,
                                        false,
                                        Some(received_at),
                                        replayed_event,
                                    ) {
                                        watcher_failure(
                                            &self.paths,
                                            &mut failure_streak,
                                            &format!("event processing failed: {error:#}"),
                                        );
                                    }
                                    if sequence > replay_until {
                                        replay_until = sequence;
                                    }
                                    if pane_ids_changed {
                                        if let Some(active) = subscription.take() {
                                            active.stop();
                                        }
                                        reconnect_at = std::time::Instant::now();
                                    }
                                }
                                Err(error) => {
                                    let detail = format!("cursor={cursor};error={error:#}");
                                    let _ = append_log(
                                        &self.paths,
                                        "herdr_subscription_lost",
                                        None,
                                        Some(&detail),
                                    );
                                    watcher_failure(
                                        &self.paths,
                                        &mut failure_streak,
                                        &format!("event agent.list failed: {error:#}"),
                                    );
                                    if let Some(active) = subscription.take() {
                                        active.stop();
                                    }
                                    reconnect_at = std::time::Instant::now() + reconnect_wait;
                                    reconnect_wait = reconnect_delay(reconnect_wait);
                                }
                            }
                        }
                        Ok(WatcherSubscriptionLine::Error { code, message }) => {
                            let detail = format!("cursor={cursor};code={code};message={message}");
                            let _ = append_log(
                                &self.paths,
                                "herdr_subscription_lost",
                                None,
                                Some(&detail),
                            );
                            needs_bootstrap =
                                code == "event_gap" || code == "event_journal_unavailable";
                            if needs_bootstrap {
                                // The retained journal cannot satisfy this
                                // cursor. A zero cursor asks Herdr to replay
                                // whatever it still has after the fresh list
                                // bootstrap below.
                                cursor = 0;
                            }
                            if let Some(active) = subscription.take() {
                                active.stop();
                            }
                            watcher_failure(&self.paths, &mut failure_streak, &message);
                            reconnect_at = std::time::Instant::now() + reconnect_wait;
                            reconnect_wait = reconnect_delay(reconnect_wait);
                        }
                        Err(error) => {
                            watcher_failure(
                                &self.paths,
                                &mut failure_streak,
                                &format!("subscription malformed: {error:#}"),
                            );
                            if let Some(active) = subscription.take() {
                                active.stop();
                            }
                            needs_bootstrap = true;
                            cursor = 0;
                            reconnect_at = std::time::Instant::now() + reconnect_wait;
                            reconnect_wait = reconnect_delay(reconnect_wait);
                        }
                    }
                }
                Ok(WatcherMessage::SubscriptionEnded {
                    generation: ended_generation,
                    message,
                }) => {
                    if subscription.as_ref().map(|item| item.generation) != Some(ended_generation) {
                        continue;
                    }
                    let detail = format!("cursor={cursor};message={message}");
                    let _ = append_log(&self.paths, "herdr_subscription_lost", None, Some(&detail));
                    if let Some(active) = subscription.take() {
                        active.stop();
                    }
                    watcher_failure(&self.paths, &mut failure_streak, &message);
                    reconnect_at = std::time::Instant::now() + reconnect_wait;
                    reconnect_wait = reconnect_delay(reconnect_wait);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if subscription.is_some()
                        && let Err(error) = self.scan_panes(&panes, false, true)
                    {
                        watcher_failure(
                            &self.paths,
                            &mut failure_streak,
                            &format!("elapsed refresh failed: {error:#}"),
                        );
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    }

    fn next_elapsed_wait(&self, panes: &[Pane]) -> Duration {
        let now = unix_time_ms().unwrap_or(u64::MAX);
        panes
            .iter()
            .filter_map(|pane| {
                self.display_states
                    .panes
                    .get(&pane.id)
                    .map(|state| (now, state.changed_unix_ms))
            })
            .map(|(now, changed)| {
                let elapsed_ms = now.saturating_sub(changed);
                let elapsed_seconds = elapsed_ms / 1_000;
                let target_seconds = if elapsed_seconds < 60 {
                    elapsed_seconds + 1
                } else if elapsed_seconds < 60 * 60 {
                    (elapsed_seconds / 60 + 1) * 60
                } else if elapsed_seconds < 24 * 60 * 60 {
                    (elapsed_seconds / (60 * 60) + 1) * 60 * 60
                } else {
                    (elapsed_seconds / (24 * 60 * 60) + 1) * 24 * 60 * 60
                };
                let target = changed.saturating_add(target_seconds.saturating_mul(1_000));
                Duration::from_millis(target.saturating_sub(now).max(20))
            })
            .min()
            .unwrap_or_else(|| Duration::from_secs(60))
    }
}

pub fn format_elapsed(elapsed_ms: u64) -> String {
    let seconds = elapsed_ms / 1_000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 60 * 60 {
        format!("{}m", seconds / 60)
    } else if seconds < 24 * 60 * 60 {
        format!("{}h", seconds / (60 * 60))
    } else {
        format!("{}d", seconds / (24 * 60 * 60))
    }
}

/// Install the attention-first sidebar ordering over the Herdr socket API.
/// `agent.view.set` is transient by design, so the watcher reapplies it on
/// every start. Panes without a `sort_rank` token sort after ranked panes,
/// which leaves unsupported agents at the bottom rather than interleaved.
pub fn priority_agent_view_request() -> serde_json::Value {
    serde_json::json!({
        "id": "agent-context-labels:view",
        "method": "agent.view.set",
        "params": {
            "source": format!("plugin:{PLUGIN_ID}"),
            "label": "attention priority",
            "sort": [
                {"field": {"token": "sort_rank"}, "order": "asc"},
                // `state_change_seq` counts changes per pane, so it cannot rank
                // two panes against each other; the activity token carries a
                // shared clock and does.
                {"field": {"token": "activity"}, "order": "desc"},
            ],
        },
    })
}

pub fn apply_priority_agent_view(home: &Path) -> Result<()> {
    SocketHerdr::from_environment(home).apply_priority_view()
}

pub fn request_refresh(paths: &StatePaths) -> Result<()> {
    fs::create_dir_all(&paths.root)?;
    fs::write(paths.refresh_request(), b"").context("cannot write refresh request")?;
    wake_watcher(paths);
    Ok(())
}

/// Wake a running watcher without making the marker file the source of truth.
/// A missing watcher is deliberately quiet: the marker remains for its next
/// startup, preserving the existing command semantics.
fn wake_watcher(paths: &StatePaths) {
    #[cfg(unix)]
    {
        if let Ok(mut stream) = UnixStream::connect(paths.wake_socket()) {
            let _ = stream.write_all(b"wake\n");
        }
    }
}

pub fn exclusive_watcher_lock(paths: &StatePaths) -> Result<File> {
    fs::create_dir_all(&paths.root)?;
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(paths.lock())?;
    lock.try_lock_exclusive()
        .map_err(|_| anyhow!("watcher_already_running"))?;
    Ok(lock)
}

pub fn settings_lock(paths: &StatePaths) -> Result<File> {
    fs::create_dir_all(&paths.root)?;
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(paths.settings_lock())?;
    lock.lock_exclusive()?;
    Ok(lock)
}

#[cfg(test)]
mod tests;
