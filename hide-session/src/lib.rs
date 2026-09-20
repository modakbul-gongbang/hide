//! Locate and read local Claude Code and Codex session JSONL files.
//!
//! The crate has three deliberately separate responsibilities:
//!
//! - [`SessionLocator`] resolves a reported session identity or a working
//!   directory to a local file.
//! - [`SessionCursor`] reads only bytes appended since its previous read and
//!   reports replacement and truncation explicitly.
//! - The conversation parsers turn provider-specific records into a small,
//!   common event stream.
//!
//! No provider response or session body is logged by this crate. Callers can
//! use the public raw-line helpers for records outside the conversation event
//! model, such as Codex usage data.

use anyhow::Error as AnyhowError;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, File, Metadata};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

mod catalog;

pub use catalog::{
    ProjectSession, SESSION_DISCOVERY_LIMIT, SessionAvailability, SessionCatalog,
    SessionCatalogError, SessionFilter,
};

#[cfg(not(unix))]
use std::time::SystemTime;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

/// Number of recent day directories used when Herdr has not reported a Codex
/// session identity yet.
pub const CODEX_FALLBACK_DAYS: usize = 7;
/// Number of newest files considered by the usage fallback.
pub const CODEX_CANDIDATE_LIMIT: usize = 32;

/// The two local agent session formats supported by Hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    Codex,
    Claude,
}

impl Agent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

/// A session identity reported by Herdr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionIdentity {
    Id(String),
    Path(PathBuf),
}

impl SessionIdentity {
    pub fn id(value: impl Into<String>) -> Self {
        Self::Id(value.into())
    }

    pub fn path(value: impl Into<PathBuf>) -> Self {
        Self::Path(value.into())
    }
}

/// The normalized kind of a conversation record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Human,
    Injected,
    Interrupted,
    Assistant,
}

impl EventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Injected => "injected",
            Self::Interrupted => "interrupted",
            Self::Assistant => "assistant",
        }
    }
}

/// A provider-neutral conversation event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationEvent {
    pub role: &'static str,
    pub kind: EventKind,
    pub at_unix_ms: u64,
    pub text: String,
}

impl ConversationEvent {
    pub fn new(
        role: &'static str,
        kind: EventKind,
        at_unix_ms: u64,
        text: impl Into<String>,
    ) -> Self {
        Self {
            role,
            kind,
            at_unix_ms,
            text: text.into(),
        }
    }
}

/// Why one otherwise relevant JSONL line was not turned into an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub enum SkipReason {
    MalformedJson,
    MissingTimestamp,
    InvalidTimestamp,
}

impl SkipReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MalformedJson => "malformed_json",
            Self::MissingTimestamp => "missing_timestamp",
            Self::InvalidTimestamp => "invalid_timestamp",
        }
    }
}

/// Why a cursor had to reread a file from byte zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescanReason {
    Truncated,
    Replaced,
}

impl RescanReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Truncated => "truncated",
            Self::Replaced => "replaced",
        }
    }
}

/// The result of parsing one complete JSONL chunk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedSession {
    pub events: Vec<ConversationEvent>,
    /// Absolute byte offset of the JSONL record that produced each event.
    ///
    /// This stays parallel to `events` so archive callers that only need the
    /// normalized conversation do not have to carry source-location state.
    pub event_offsets: Vec<u64>,
    /// The name the agent gave its own session, when the chunk carried one.
    ///
    /// Claude Code writes an `ai-title` record once it has named the
    /// conversation and repeats it on later turns; the last one in the
    /// chunk wins. It is a property of the session rather than an event, so
    /// it never enters `events`. Codex has no such record and leaves `None`.
    pub title: Option<String>,
    pub skipped_lines: usize,
    pub skipped_reasons: BTreeMap<SkipReason, usize>,
    pub rescan_reason: Option<RescanReason>,
}

impl ParsedSession {
    fn skipped(&mut self, reason: SkipReason) {
        self.skipped_lines += 1;
        *self.skipped_reasons.entry(reason).or_default() += 1;
    }
}

/// Errors that prevent a caller from obtaining or reading a session file.
///
/// A malformed conversation line is intentionally not an error: it is counted
/// in [`ParsedSession`], allowing the rest of a session to remain useful.
#[derive(Debug)]
pub enum SessionError {
    CwdUnavailable,
    SessionFileMissing,
    UnsupportedSessionKind,
    Checkpoint(String),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: AnyhowError,
    },
}

impl SessionError {
    fn io(operation: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.to_path_buf(),
            source: AnyhowError::new(source),
        }
    }
}

impl Display for SessionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CwdUnavailable => formatter.write_str("session_cwd_unavailable"),
            Self::SessionFileMissing => formatter.write_str("session_file_missing"),
            Self::UnsupportedSessionKind => formatter.write_str("session_kind_unsupported"),
            Self::Checkpoint(reason) => write!(formatter, "session_checkpoint_invalid:{reason}"),
            Self::Io {
                operation, source, ..
            } => write!(formatter, "session_{operation}: {source}"),
        }
    }
}

impl Error for SessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, SessionError>;

/// A file identity used to detect an atomic replacement at the same path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct FileIdentity {
    first: u64,
    second: u64,
}

/// Durable state for resuming one append-only session file after relaunch.
///
/// The file identity is intentionally retained. An offset without the inode
/// would silently skip the beginning of an atomic replacement at the same
/// path.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CursorCheckpoint {
    pub offset: u64,
    identity: Option<FileIdentity>,
    pub pending: Vec<u8>,
}

impl FileIdentity {
    fn from_metadata(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        {
            Self {
                first: metadata.dev(),
                second: metadata.ino(),
            }
        }
        #[cfg(not(unix))]
        {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_nanos() as u64);
            Self {
                first: metadata.len(),
                second: modified,
            }
        }
    }
}

/// The complete-line bytes returned by [`SessionCursor::read`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionChunk {
    pub contents: String,
    /// Absolute byte offset where `contents` begins in the session file.
    /// This can precede the prior cursor offset when a torn line was retained.
    pub start_offset: u64,
    pub rescan_reason: Option<RescanReason>,
}

/// Incremental byte reader for one session file.
///
/// The cursor retains an unterminated final line instead of discarding it.
/// This makes a later append complete the same JSON object without rereading
/// the preceding file.
#[derive(Debug, Default)]
pub struct SessionCursor {
    offset: u64,
    identity: Option<FileIdentity>,
    pending: Vec<u8>,
}

impl SessionCursor {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn offset(&self) -> u64 {
        self.offset
    }

    pub fn checkpoint(&self) -> CursorCheckpoint {
        CursorCheckpoint {
            offset: self.offset,
            identity: self.identity,
            pending: self.pending.clone(),
        }
    }

    pub fn restore(checkpoint: CursorCheckpoint) -> Self {
        Self {
            offset: checkpoint.offset,
            identity: checkpoint.identity,
            pending: checkpoint.pending,
        }
    }

    /// Serializes the complete durable cursor, including file identity.
    /// Callers persist this opaque blob rather than reconstructing an offset
    /// and accidentally skipping an atomically replaced session file.
    pub fn encode_checkpoint(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&self.checkpoint())
            .map_err(|error| SessionError::Checkpoint(error.to_string()))
    }

    pub fn restore_checkpoint(bytes: &[u8]) -> Result<Self> {
        let checkpoint = serde_json::from_slice(bytes)
            .map_err(|error| SessionError::Checkpoint(error.to_string()))?;
        Ok(Self::restore(checkpoint))
    }

    pub fn reset(&mut self) {
        self.offset = 0;
        self.identity = None;
        self.pending.clear();
    }

    pub fn read(&mut self, path: &Path) -> Result<SessionChunk> {
        let mut file = File::open(path).map_err(|error| SessionError::io("open", path, error))?;
        let metadata = file
            .metadata()
            .map_err(|error| SessionError::io("stat", path, error))?;
        let identity = FileIdentity::from_metadata(&metadata);
        let rescan_reason = match self.identity {
            Some(previous) if previous != identity => {
                self.offset = 0;
                self.pending.clear();
                Some(RescanReason::Replaced)
            }
            Some(_) if metadata.len() < self.offset => {
                self.offset = 0;
                self.pending.clear();
                Some(RescanReason::Truncated)
            }
            None => {
                self.offset = 0;
                self.pending.clear();
                None
            }
            _ => None,
        };

        file.seek(SeekFrom::Start(self.offset))
            .map_err(|error| SessionError::io("seek", path, error))?;
        let start = self.offset;
        let mut appended = Vec::new();
        file.read_to_end(&mut appended)
            .map_err(|error| SessionError::io("read", path, error))?;
        self.offset = start.saturating_add(appended.len() as u64);
        self.identity = Some(identity);

        let retained_len = self.pending.len() as u64;
        let mut combined = std::mem::take(&mut self.pending);
        combined.extend(appended);
        let chunk_start = start.saturating_sub(retained_len);
        let Some(last_newline) = combined.iter().rposition(|byte| *byte == b'\n') else {
            self.pending = combined;
            return Ok(SessionChunk {
                contents: String::new(),
                start_offset: chunk_start,
                rescan_reason,
            });
        };
        let remainder = combined.split_off(last_newline + 1);
        self.pending = remainder;
        Ok(SessionChunk {
            contents: String::from_utf8_lossy(&combined).into_owned(),
            start_offset: chunk_start,
            rescan_reason,
        })
    }
}

/// Resolve Herdr's session identity and working directory to local files.
pub struct SessionLocator {
    home: PathBuf,
    resolved: HashMap<String, PathBuf>,
}

impl SessionLocator {
    pub fn new(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            resolved: HashMap::new(),
        }
    }

    pub fn locate(
        &mut self,
        pane_id: &str,
        agent: Agent,
        identity: Option<&SessionIdentity>,
        cwd: Option<&str>,
    ) -> Result<PathBuf> {
        // A session Herdr reported for this pane is that pane's session, full
        // stop, and a reported session whose file does not exist yet is a
        // session that has not started, not a reason to read another one.
        // Both fallbacks to the newest file in the cwd were tried and both
        // handed a pane its neighbour's transcript: the first once the
        // reported file went quiet for 30 s, the second in the seconds
        // between Claude's launch and its first written turn, when the id is
        // already reported and the file is not there yet (2026-09-18/19). A
        // pane that starts a new session in place (`/clear`, `--resume`) is
        // reported again by the runtime hook, so a switch reaches here as a
        // new id. The cwd search below serves only a pane with no reported
        // session at all.
        if let Some(identity) = identity {
            let path = self.reported_path(agent, cwd, identity)?;
            self.resolved.insert(pane_id.to_owned(), path.clone());
            return Ok(path);
        }

        if let Some(path) = self.resolved.get(pane_id)
            && path.is_file()
        {
            return Ok(path.clone());
        }

        let cwd = cwd.ok_or(SessionError::CwdUnavailable)?;
        match self.newest_for_cwd(agent, cwd)? {
            Some(path) => {
                self.resolved.insert(pane_id.to_owned(), path.clone());
                Ok(path)
            }
            None => match self.resolved.get(pane_id) {
                Some(path) if path.is_file() => Ok(path.clone()),
                _ => Err(SessionError::SessionFileMissing),
            },
        }
    }

    fn reported_path(
        &self,
        agent: Agent,
        cwd: Option<&str>,
        identity: &SessionIdentity,
    ) -> Result<PathBuf> {
        match identity {
            SessionIdentity::Path(path) => path
                .is_file()
                .then_some(path.clone())
                .ok_or(SessionError::SessionFileMissing),
            SessionIdentity::Id(id) => match agent {
                Agent::Claude => self.claude_path_for_id(cwd, id),
                Agent::Codex => self.codex_path_for_id(id),
            },
        }
    }

    fn claude_root(&self) -> PathBuf {
        self.home.join(".claude/projects")
    }

    fn codex_root(&self) -> PathBuf {
        self.home.join(".codex/sessions")
    }

    fn claude_path_for_id(&self, cwd: Option<&str>, id: &str) -> Result<PathBuf> {
        let name = format!("{id}.jsonl");
        if let Some(cwd) = cwd {
            let direct = self.claude_root().join(project_directory(cwd)).join(&name);
            if direct.is_file() {
                return Ok(direct);
            }
        }
        let root = self.claude_root();
        for entry in read_directory(&root)? {
            let candidate = entry.join(&name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(SessionError::SessionFileMissing)
    }

    fn codex_path_for_id(&self, id: &str) -> Result<PathBuf> {
        for directory in self.recent_codex_days()? {
            for path in jsonl_files(&directory)? {
                if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(id))
                {
                    return Ok(path);
                }
            }
        }
        Err(SessionError::SessionFileMissing)
    }

    fn newest_for_cwd(&self, agent: Agent, cwd: &str) -> Result<Option<PathBuf>> {
        match agent {
            Agent::Claude => Ok(newest_by_modified(jsonl_files(
                &self.claude_root().join(project_directory(cwd)),
            )?)),
            Agent::Codex => self.newest_codex_session(cwd),
        }
    }

    fn newest_codex_session(&self, cwd: &str) -> Result<Option<PathBuf>> {
        let mut candidates = Vec::new();
        for directory in self.recent_codex_days()? {
            for path in jsonl_files(&directory)? {
                let Some(modified) = fs::metadata(&path).and_then(|data| data.modified()).ok()
                else {
                    continue;
                };
                candidates.push((modified, path));
            }
        }
        candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        Ok(candidates
            .into_iter()
            .map(|(_, path)| path)
            .find(|path| codex_session_cwd(path).as_deref() == Some(cwd)))
    }

    fn recent_codex_days(&self) -> Result<Vec<PathBuf>> {
        let mut days = Vec::new();
        for year in newest_directories(&self.codex_root(), 2)? {
            for month in newest_directories(&year, 2)? {
                days.extend(newest_directories(&month, CODEX_FALLBACK_DAYS)?);
            }
        }
        days.sort();
        days.reverse();
        days.truncate(CODEX_FALLBACK_DAYS);
        Ok(days)
    }
}

fn read_directory(path: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(SessionError::io("read_directory", path, error)),
    };
    Ok(entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect())
}

fn jsonl_files(directory: &Path) -> Result<Vec<PathBuf>> {
    Ok(read_directory(directory)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|value| value == "jsonl"))
        .collect())
}

fn newest_directories(root: &Path, limit: usize) -> Result<Vec<PathBuf>> {
    let mut directories = read_directory(root)?
        .into_iter()
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    directories.sort();
    directories.reverse();
    directories.truncate(limit);
    Ok(directories)
}

fn newest_by_modified(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths
        .into_iter()
        .filter_map(|path| {
            let modified = fs::metadata(&path).and_then(|data| data.modified()).ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn project_directory(cwd: &str) -> String {
    cwd.replace('/', "-")
}

/// Return the newest Codex JSONL files in the newest YYYY/MM/DD directory.
///
/// This is intentionally separate from conversation parsing because Codex
/// usage records are not conversation events.
pub fn newest_codex_session_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut level = root.to_path_buf();
    for _ in 0..3 {
        let children = newest_directories(&level, usize::MAX)?
            .into_iter()
            .filter(|path| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .chars()
                        .all(|character| character.is_ascii_digit())
                })
            })
            .collect::<Vec<_>>();
        let Some(next) = children.into_iter().max() else {
            return Err(SessionError::SessionFileMissing);
        };
        level = next;
    }
    let mut candidates = jsonl_files(&level)?
        .into_iter()
        .filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    candidates.truncate(CODEX_CANDIDATE_LIMIT);
    Ok(candidates.into_iter().map(|(_, path)| path).collect())
}

/// Read a bounded tail while dropping a line cut by the byte boundary.
pub fn read_tail(path: &Path, maximum_bytes: u64) -> Result<String> {
    let mut file = File::open(path).map_err(|error| SessionError::io("open", path, error))?;
    let length = file
        .metadata()
        .map_err(|error| SessionError::io("stat", path, error))?
        .len();
    let start = length.saturating_sub(maximum_bytes);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| SessionError::io("seek", path, error))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| SessionError::io("read", path, error))?;
    let contents = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Ok(contents);
    }
    Ok(contents
        .split_once('\n')
        .map_or_else(String::new, |(_, rest)| rest.to_owned()))
}

/// Read the cwd from Codex's first session_meta line.
pub fn codex_session_cwd(path: &Path) -> Option<String> {
    let mut first = String::new();
    BufReader::new(File::open(path).ok()?)
        .read_line(&mut first)
        .ok()?;
    let value: Value = serde_json::from_str(first.trim()).ok()?;
    value
        .pointer("/payload/cwd")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

enum LineResult {
    Ignore,
    Skip(SkipReason),
    Event(ConversationEvent),
    /// A session-level record rather than a turn: the title the agent gave
    /// the conversation.
    Title(String),
}

/// Parse Claude Code JSONL records into normalized events.
pub fn parse_claude_events(contents: &str) -> ParsedSession {
    parse_lines(contents, parse_claude_line)
}

/// Parse Codex response_item JSONL records into normalized events.
pub fn parse_codex_events(contents: &str) -> ParsedSession {
    parse_lines(contents, parse_codex_line)
}

/// Parse a complete-line chunk for the selected provider.
pub fn parse_events(agent: Agent, contents: &str) -> ParsedSession {
    parse_events_at(agent, contents, 0)
}

/// Parse a complete-line chunk while preserving each record's absolute byte
/// offset. The caller supplies the byte offset where `contents` begins.
pub fn parse_events_at(agent: Agent, contents: &str, base_offset: u64) -> ParsedSession {
    match agent {
        Agent::Claude => parse_lines_at(contents, base_offset, parse_claude_line),
        Agent::Codex => parse_lines_at(contents, base_offset, parse_codex_line),
    }
}

fn parse_lines(contents: &str, mut extract: impl FnMut(&Value) -> LineResult) -> ParsedSession {
    parse_lines_at(contents, 0, &mut extract)
}

fn parse_lines_at(
    contents: &str,
    base_offset: u64,
    mut extract: impl FnMut(&Value) -> LineResult,
) -> ParsedSession {
    let mut parsed = ParsedSession::default();
    let mut relative_offset = 0_u64;
    for raw_line in contents.split_inclusive('\n') {
        let line_offset = base_offset.saturating_add(relative_offset);
        relative_offset = relative_offset.saturating_add(raw_line.len() as u64);
        let line = raw_line.trim_end_matches(['\n', '\r']);
        if line.trim().is_empty() {
            continue;
        }
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(_) => {
                parsed.skipped(SkipReason::MalformedJson);
                continue;
            }
        };
        match extract(&value) {
            LineResult::Ignore => {}
            LineResult::Skip(reason) => parsed.skipped(reason),
            LineResult::Event(event) => {
                parsed.events.push(event);
                parsed.event_offsets.push(line_offset);
            }
            LineResult::Title(title) => parsed.title = Some(title),
        }
    }
    parsed
}

fn parse_claude_line(item: &Value) -> LineResult {
    let role = match item.get("type").and_then(Value::as_str) {
        Some("user") => "user",
        Some("assistant") => "assistant",
        Some("ai-title") => {
            return match item
                .get("aiTitle")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
            {
                Some(title) => LineResult::Title(title.to_owned()),
                None => LineResult::Ignore,
            };
        }
        _ => return LineResult::Ignore,
    };
    let Some(text) = session_text(item.pointer("/message/content")) else {
        return LineResult::Ignore;
    };
    let timestamp = match timestamp_ms(item.get("timestamp")) {
        Ok(timestamp) => timestamp,
        Err(reason) => return LineResult::Skip(reason),
    };
    if role == "assistant" {
        return LineResult::Event(ConversationEvent::new(
            role,
            EventKind::Assistant,
            timestamp,
            text,
        ));
    }
    let origin_is_human = item.pointer("/origin/kind").and_then(Value::as_str) == Some("human");
    let is_meta = item.get("isMeta").and_then(Value::as_bool).unwrap_or(false);
    let is_system_prompt = item.get("promptSource").and_then(Value::as_str) == Some("system");
    let interrupted = is_interruption(&text);
    let command = slash_command_text(&text);
    let kind = if interrupted {
        EventKind::Interrupted
    } else if origin_is_human && !is_meta && !is_system_prompt && !has_injected_prefix(&text) {
        EventKind::Human
    } else {
        EventKind::Injected
    };
    let text = if kind == EventKind::Human {
        command.unwrap_or(text)
    } else {
        text
    };
    LineResult::Event(ConversationEvent::new(role, kind, timestamp, text))
}

fn parse_codex_line(item: &Value) -> LineResult {
    if item.get("type").and_then(Value::as_str) != Some("response_item") {
        return LineResult::Ignore;
    }
    let Some(payload) = item.get("payload") else {
        return LineResult::Ignore;
    };
    if payload.get("type").and_then(Value::as_str) != Some("message") {
        return LineResult::Ignore;
    }
    let role = match payload.get("role").and_then(Value::as_str) {
        Some("user") => "user",
        Some("assistant") => "assistant",
        _ => return LineResult::Ignore,
    };
    let Some(text) = session_text(payload.get("content")) else {
        return LineResult::Ignore;
    };
    let timestamp = match timestamp_ms(item.get("timestamp")) {
        Ok(timestamp) => timestamp,
        Err(reason) => return LineResult::Skip(reason),
    };
    if role == "assistant" {
        return LineResult::Event(ConversationEvent::new(
            role,
            EventKind::Assistant,
            timestamp,
            text,
        ));
    }
    let kind = if is_interruption(&text) {
        EventKind::Interrupted
    } else if has_injected_prefix(&text) {
        EventKind::Injected
    } else {
        EventKind::Human
    };
    let text = if kind == EventKind::Human {
        slash_command_text(&text).unwrap_or(text)
    } else {
        text
    };
    LineResult::Event(ConversationEvent::new(role, kind, timestamp, text))
}

/// All prefixes that identify prompt scaffolding live here so both providers
/// use the same fixed classification table.
pub const INJECTED_PREFIXES: &[&str] = &[
    "<task-notification>",
    "<system-reminder>",
    "<hide-memory-context>",
    "Another Claude session",
    "# AGENTS.md instructions",
    "<environment_context>",
    "<user_instructions>",
];

fn has_injected_prefix(text: &str) -> bool {
    let text = text.trim_start();
    INJECTED_PREFIXES
        .iter()
        .any(|prefix| text.starts_with(prefix))
}

fn is_interruption(text: &str) -> bool {
    text.trim_start().starts_with("[Request interrupted")
}

fn slash_command_text(text: &str) -> Option<String> {
    let name = text
        .split_once("<command-name>")?
        .1
        .split_once("</command-name>")?
        .0
        .trim();
    if name.is_empty() {
        return None;
    }
    let args = text
        .split_once("<command-args>")
        .and_then(|(_, rest)| {
            rest.split_once("</command-args>")
                .map(|(args, _)| args.trim())
        })
        .unwrap_or("");
    Some(if args.is_empty() {
        format!("/{name}")
    } else {
        args.to_owned()
    })
}

fn session_text(content: Option<&Value>) -> Option<String> {
    let text = match content? {
        Value::String(text) => text.to_owned(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("text" | "input_text" | "output_text")
                )
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    (!text.trim().is_empty()).then_some(text)
}

fn timestamp_ms(value: Option<&Value>) -> std::result::Result<u64, SkipReason> {
    let Some(value) = value else {
        return Err(SkipReason::MissingTimestamp);
    };
    if let Some(value) = value.as_u64() {
        return if value >= 10_000_000_000 {
            Ok(value)
        } else {
            value.checked_mul(1_000).ok_or(SkipReason::InvalidTimestamp)
        };
    }
    let Some(value) = value.as_str() else {
        return Err(SkipReason::InvalidTimestamp);
    };
    parse_rfc3339_unix_ms(value).ok_or(SkipReason::InvalidTimestamp)
}

/// Parse an RFC3339 timestamp into Unix seconds.
pub fn parse_rfc3339(value: &str) -> Option<u64> {
    parse_rfc3339_parts(value).map(|(seconds, _)| seconds)
}

fn parse_rfc3339_unix_ms(value: &str) -> Option<u64> {
    let (seconds, millis) = parse_rfc3339_parts(value)?;
    seconds.checked_mul(1_000)?.checked_add(millis)
}

fn parse_rfc3339_parts(value: &str) -> Option<(u64, u64)> {
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
    let millis = if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == start {
            return None;
        }
        let fraction = value.get(start..cursor)?;
        let mut millis = 0_u64;
        for digit in fraction.bytes().take(3) {
            millis = millis * 10 + u64::from(digit - b'0');
        }
        if fraction.len() == 1 {
            millis *= 100;
        } else if fraction.len() == 2 {
            millis *= 10;
        }
        millis
    } else {
        0
    };
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
    Some((u64::try_from(timestamp).ok()?, millis))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parses_rfc3339_with_fraction_and_offset() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:01Z"), Some(1));
        assert_eq!(
            parse_rfc3339_unix_ms("1970-01-01T09:00:01.234+09:00"),
            Some(1_234)
        );
        assert_eq!(parse_rfc3339("not-a-date"), None);
    }

    #[test]
    fn cursor_reads_the_first_file_then_only_appended_complete_lines() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("session.jsonl");
        fs::write(&path, b"one\ntwo\n").unwrap();
        let mut cursor = SessionCursor::new();

        assert_eq!(
            cursor.read(&path).unwrap(),
            SessionChunk {
                contents: "one\ntwo\n".to_owned(),
                start_offset: 0,
                rescan_reason: None,
            }
        );
        let offset = cursor.offset();
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"three\n")
            .unwrap();
        let appended = cursor.read(&path).unwrap();
        assert_eq!(appended.contents, "three\n");
        assert_eq!(appended.start_offset, offset);
        assert!(cursor.offset() > offset);
    }

    #[test]
    fn cursor_holds_a_torn_final_line_until_the_next_append() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("session.jsonl");
        fs::write(&path, b"partial").unwrap();
        let mut cursor = SessionCursor::new();
        assert_eq!(cursor.read(&path).unwrap().contents, "");

        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"-line\n")
            .unwrap();
        let completed = cursor.read(&path).unwrap();
        assert_eq!(completed.contents, "partial-line\n");
        assert_eq!(completed.start_offset, 0);
    }

    #[test]
    fn cursor_reports_truncation_and_replacement() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("session.jsonl");
        fs::write(&path, b"first\nsecond\n").unwrap();
        let mut cursor = SessionCursor::new();
        cursor.read(&path).unwrap();
        fs::write(&path, b"new\n").unwrap();
        let truncated = cursor.read(&path).unwrap();
        assert_eq!(truncated.rescan_reason, Some(RescanReason::Truncated));
        assert_eq!(truncated.contents, "new\n");

        let replacement = directory.path().join("replacement.jsonl");
        fs::write(&replacement, b"replacement\n").unwrap();
        fs::rename(&replacement, &path).unwrap();
        let replaced = cursor.read(&path).unwrap();
        assert_eq!(replaced.rescan_reason, Some(RescanReason::Replaced));
        assert_eq!(replaced.contents, "replacement\n");
    }

    #[test]
    fn parser_counts_only_relevant_lines_and_preserves_reason_categories() {
        let input = concat!(
            "{\"type\":\"user\",\"timestamp\":\"1970-01-01T00:00:01Z\",\"origin\":{\"kind\":\"human\"},\"message\":{\"content\":\"요청\"}}\n",
            "{\"type\":\"user\",\"timestamp\":\"bad\",\"origin\":{\"kind\":\"human\"},\"message\":{\"content\":\"건너뜀\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":\"시간 없음\"}}\n",
            "not-json\n",
            "{\"type\":\"session_meta\",\"payload\":{}}\n",
        );
        let parsed = parse_claude_events(input);
        assert_eq!(parsed.events.len(), 1);
        assert_eq!(parsed.events[0].kind, EventKind::Human);
        assert_eq!(parsed.events[0].at_unix_ms, 1_000);
        assert_eq!(parsed.skipped_lines, 3);
        assert_eq!(parsed.skipped_reasons[&SkipReason::InvalidTimestamp], 1);
        assert_eq!(parsed.skipped_reasons[&SkipReason::MissingTimestamp], 1);
        assert_eq!(parsed.skipped_reasons[&SkipReason::MalformedJson], 1);
    }

    #[test]
    fn parser_preserves_absolute_jsonl_offsets_for_emitted_events() {
        let ignored = "{\"type\":\"session_meta\",\"payload\":{}}\n";
        let first = "{\"type\":\"user\",\"timestamp\":\"1970-01-01T00:00:01Z\",\"origin\":{\"kind\":\"human\"},\"message\":{\"content\":\"one\"}}\n";
        let second = "{\"type\":\"assistant\",\"timestamp\":\"1970-01-01T00:00:02Z\",\"message\":{\"content\":\"two\"}}\n";
        let parsed = parse_events_at(Agent::Claude, &format!("{ignored}{first}{second}"), 700);
        assert_eq!(
            parsed.event_offsets,
            vec![
                700 + ignored.len() as u64,
                700 + ignored.len() as u64 + first.len() as u64
            ]
        );
    }

    #[test]
    fn parses_claude_human_injected_interrupted_and_command_events() {
        let fixture = include_str!("../tests/fixtures/claude.jsonl");
        let parsed = parse_claude_events(fixture);
        assert_eq!(
            parsed
                .events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            vec![
                EventKind::Human,
                EventKind::Injected,
                EventKind::Injected,
                EventKind::Injected,
                EventKind::Human,
                EventKind::Interrupted,
                EventKind::Assistant,
            ]
        );
        assert_eq!(parsed.events[0].text, "첫 번째 요청");
        assert_eq!(parsed.events[4].text, "--quick");
        assert_eq!(parsed.events[5].text, "[Request interrupted by user]");
        assert_eq!(parsed.skipped_lines, 0);
        assert_eq!(parsed.title, None, "the fixture carries no ai-title record");
    }

    /// Claude's `ai-title` record is the session's own name: the last one
    /// wins, it is not an event, and an empty one is nothing.
    #[test]
    fn claude_ai_title_is_the_session_title_and_not_an_event() {
        let chunk = concat!(
            r#"{"type":"ai-title","aiTitle":"  Hook 버그 확인 ","sessionId":"s1"}"#,
            "\n",
            r#"{"type":"user","timestamp":"2026-09-18T00:00:00.000Z","origin":{"kind":"human"},"message":{"role":"user","content":"hook 고쳐줘"}}"#,
            "\n",
            r#"{"type":"ai-title","aiTitle":"","sessionId":"s1"}"#,
            "\n",
            r#"{"type":"ai-title","aiTitle":"Hook 보고 경로 교체","sessionId":"s1"}"#,
            "\n",
        );
        let parsed = parse_claude_events(chunk);
        assert_eq!(parsed.title.as_deref(), Some("Hook 보고 경로 교체"));
        assert_eq!(parsed.events.len(), 1);
        assert_eq!(parsed.skipped_lines, 0);
        let codex = parse_codex_events(chunk);
        assert_eq!(codex.title, None);
    }

    #[test]
    fn parses_codex_human_and_injected_prefixes() {
        let fixture = include_str!("../tests/fixtures/codex.jsonl");
        let parsed = parse_codex_events(fixture);
        assert_eq!(
            parsed
                .events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            vec![
                EventKind::Injected,
                EventKind::Injected,
                EventKind::Injected,
                EventKind::Human,
                EventKind::Assistant,
            ]
        );
        assert_eq!(parsed.events[3].text, "실제 요청");
        assert_eq!(parsed.events[4].at_unix_ms, 1_789_516_805_000);
    }

    #[test]
    fn locator_finds_claude_and_codex_files() {
        let home = tempdir().unwrap();
        let claude_dir = home.path().join(".claude/projects/-Users-example");
        let codex_dir = home.path().join(".codex/sessions/2026/09/16");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::create_dir_all(&codex_dir).unwrap();
        let claude = claude_dir.join("claude-id.jsonl");
        let codex = codex_dir.join("rollout-codex-id.jsonl");
        fs::write(&claude, b"").unwrap();
        fs::write(
            &codex,
            b"{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/Users/example\"}}\n",
        )
        .unwrap();
        let mut locator = SessionLocator::new(home.path());
        assert_eq!(
            locator
                .locate(
                    "pane-claude",
                    Agent::Claude,
                    Some(&SessionIdentity::id("claude-id")),
                    Some("/Users/example"),
                )
                .unwrap(),
            claude
        );
        assert_eq!(
            locator
                .locate(
                    "pane-codex",
                    Agent::Codex,
                    Some(&SessionIdentity::id("codex-id")),
                    Some("/Users/example"),
                )
                .unwrap(),
            codex
        );
    }

    #[test]
    fn a_reported_session_is_kept_when_a_neighbour_in_the_same_cwd_is_newer() {
        let home = tempdir().unwrap();
        let claude_dir = home.path().join(".claude/projects/-Users-example");
        fs::create_dir_all(&claude_dir).unwrap();
        let quiet = claude_dir.join("quiet-id.jsonl");
        let busy = claude_dir.join("busy-id.jsonl");
        fs::write(&quiet, b"").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        fs::File::open(&quiet).unwrap().set_modified(old).unwrap();
        fs::write(&busy, b"").unwrap();
        let mut locator = SessionLocator::new(home.path());
        assert_eq!(
            locator
                .locate(
                    "pane-quiet",
                    Agent::Claude,
                    Some(&SessionIdentity::id("quiet-id")),
                    Some("/Users/example"),
                )
                .unwrap(),
            quiet
        );
        // Without a reported id the newest file in the cwd is still the answer.
        assert_eq!(
            locator
                .locate("pane-unknown", Agent::Claude, None, Some("/Users/example"))
                .unwrap(),
            busy
        );
    }

    /// A reported session whose file is not written yet (Claude before its
    /// first turn) is missing, never the neighbour's newest transcript.
    #[test]
    fn a_reported_session_without_a_file_yet_is_missing_not_the_neighbour() {
        let home = tempdir().unwrap();
        let claude_dir = home.path().join(".claude/projects/-Users-example");
        fs::create_dir_all(&claude_dir).unwrap();
        let busy = claude_dir.join("busy-id.jsonl");
        fs::write(&busy, b"").unwrap();
        let mut locator = SessionLocator::new(home.path());
        let unborn = SessionIdentity::id("unborn-id");
        assert!(matches!(
            locator.locate(
                "pane-new",
                Agent::Claude,
                Some(&unborn),
                Some("/Users/example")
            ),
            Err(SessionError::SessionFileMissing)
        ));
        // Once the file exists the same pane resolves to it, not to what
        // the cwd search would have cached.
        let own = claude_dir.join("unborn-id.jsonl");
        fs::write(&own, b"").unwrap();
        assert_eq!(
            locator
                .locate(
                    "pane-new",
                    Agent::Claude,
                    Some(&unborn),
                    Some("/Users/example")
                )
                .unwrap(),
            own
        );
    }
}
