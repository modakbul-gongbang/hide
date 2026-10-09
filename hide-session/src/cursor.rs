//! Cursor's ordinary local chats: a read-only SQLite content-addressed graph.
//! The native sidecar supplies cwd; neither a display name, derived transcript
//! nor a previous-workspace URI is ownership evidence.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use prost::Message;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{ConversationEvent, EventKind, ParsedSession, RescanReason, Result, SessionError};

pub const SESSIONS: &str = ".cursor/chats";
const META_BYTES: usize = 64 * 1024;
const BLOB_BYTES: usize = crate::SESSION_LINE_LIMIT_BYTES;
const TURN_LIMIT: usize = crate::SESSION_DISCOVERY_LIMIT;
const STEP_LIMIT: usize = 256;
const QUERY_LIMIT: usize = 512;
const EVENT_LIMIT: usize = 200;
const SQL_STEPS: usize = 1_000_000;

fn invalid(reason: &'static str) -> SessionError {
    SessionError::Checkpoint(reason.to_owned())
}

fn database_error(reason: &'static str, error: rusqlite::Error) -> SessionError {
    match error {
        rusqlite::Error::SqliteFailure(code, _) => {
            SessionError::Checkpoint(format!("{reason}:sqlite:{}", code.extended_code))
        }
        _ => invalid(reason),
    }
}

fn capacity(resource: &'static str, limit: usize) -> SessionError {
    SessionError::Capacity {
        resource,
        limit: limit as u64,
    }
}

fn database_uri(path: &Path, wal: bool) -> Result<String> {
    use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
    const URI_PATH: &AsciiSet = &CONTROLS.add(b'%').add(b'?').add(b'#').add(b' ');
    let spelling =
        hide_platform::path::to_wire(path).map_err(|_| invalid("cursor_path_unconfirmed"))?;
    // A local SQLite URI has no remote authority. Network shares are outside
    // this local reader's contract, including on Windows.
    if spelling.starts_with("//") {
        return Err(invalid("cursor_path_unconfirmed"));
    }
    let leading = if spelling.starts_with('/') {
        "file:"
    } else {
        "file:/"
    };
    let read_policy = if wal { "readonly_shm=1" } else { "immutable=1" };
    Ok(format!(
        "{leading}{}?mode=ro&{read_policy}",
        utf8_percent_encode(&spelling, URI_PATH)
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str, limit: usize) -> Result<Vec<u8>> {
    if text.len() > limit.saturating_mul(2) {
        return Err(capacity("cursor_metadata_bytes", limit));
    }
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err(invalid("cursor_hex_invalid"));
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            let high = digit(pair[0]).ok_or_else(|| invalid("cursor_hex_invalid"))?;
            let low = digit(pair[1]).ok_or_else(|| invalid("cursor_hex_invalid"))?;
            Ok(high * 16 + low)
        })
        .collect()
}

fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

pub(crate) fn directory(cwd: &Path) -> Result<String> {
    let spelling = cwd
        .to_str()
        .filter(|value| !value.chars().any(char::is_control))
        .filter(|_| cwd.is_absolute())
        .ok_or_else(|| invalid("cursor_cwd_unconfirmed"))?;
    Ok(format!("{:x}", md5::compute(spelling.as_bytes())))
}

pub(crate) fn checked_path(home: &Path, path: &Path) -> Result<PathBuf> {
    let checked = crate::native_file::checked_path_under(home, SESSIONS, path)
        .map_err(|_| invalid("cursor_path_unconfirmed"))?;
    let root = hide_platform::fs::identity::canonical(&home.join(SESSIONS))
        .map_err(|_| invalid("cursor_path_unconfirmed"))?;
    let relative = checked
        .strip_prefix(root)
        .map_err(|_| invalid("cursor_path_unconfirmed"))?;
    let parts: Vec<_> = relative.components().collect();
    if parts.len() != 3
        || parts[2].as_os_str() != "store.db"
        || !parts[0].as_os_str().to_str().is_some_and(|name| {
            name.len() == 32
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        || !parts[1].as_os_str().to_str().is_some_and(uuid)
    {
        return Err(invalid("cursor_path_unconfirmed"));
    }
    Ok(checked)
}

// Only fields needed for ownership and the graph enter memory. In particular,
// native title and encryption-key fields are deliberately not deserialized.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    agent_id: String,
    latest_root_blob_id: String,
    created_at: u64,
    #[serde(default)]
    subagent_info: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sidecar {
    schema_version: u32,
    created_at_ms: u64,
    #[serde(default)]
    updated_at_ms: Option<u64>,
    has_conversation: bool,
    #[serde(default)]
    is_subagent: bool,
    cwd: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileMark {
    id: String,
    stamp: String,
}

fn mark(path: &Path) -> Result<FileMark> {
    let file = crate::open_session_file_nofollow(path)
        .map_err(|_| invalid("cursor_source_unavailable"))?;
    if hide_platform::fs::identity::link_count(&file).ok() != Some(1) {
        return Err(invalid("cursor_source_linked"));
    }
    let id = hide_platform::fs::identity::file_id_of(&file)
        .map_err(|_| invalid("cursor_source_unavailable"))?;
    Ok(FileMark {
        id: format!("{}:{}", id.volume(), id.index()),
        stamp: format!(
            "{:?}",
            hide_platform::fs::identity::stamp_of(&file)
                .map_err(|_| invalid("cursor_source_unavailable"))?
        ),
    })
}

fn marks(home: &Path, path: &Path) -> Result<Vec<Option<FileMark>>> {
    checked_path(home, path)?;
    let mut result = Vec::with_capacity(5);
    for candidate in [
        path.to_path_buf(),
        path.with_file_name("meta.json"),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
        PathBuf::from(format!("{}-journal", path.display())),
    ] {
        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => {
                crate::native_file::checked_path_under(home, SESSIONS, &candidate)
                    .map_err(|_| invalid("cursor_source_linked"))?;
                result.push(Some(mark(&candidate)?));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => result.push(None),
            Err(_) => return Err(invalid("cursor_source_unavailable")),
        }
    }
    if result[0].is_none() || result[1].is_none() {
        return Err(invalid("cursor_source_unavailable"));
    }
    // SQLite can create a shared-memory companion for a read-only WAL source.
    // Do not give the reader that work: only an already initialized WAL is read.
    if result[2].is_some() && result[3].is_none() {
        return Err(invalid("cursor_wal_uninitialized"));
    }
    if result[4].is_some() {
        return Err(invalid("cursor_recovery_required"));
    }
    Ok(result)
}

struct Database {
    connection: Connection,
    home: PathBuf,
    path: PathBuf,
    before: Vec<Option<FileMark>>,
    header: crate::native_file::Header,
    root: Vec<u8>,
    created_at: u64,
    updated_at: Option<u64>,
    read_bytes: usize,
    allowance: usize,
    queries: usize,
}

impl Database {
    fn open(home: &Path, path: &Path, scope: Option<(&str, &str)>, budget: usize) -> Result<Self> {
        let path = checked_path(home, path)?;
        let before = marks(home, &path)?;
        if std::fs::metadata(&path)
            .map_err(|_| invalid("cursor_source_unavailable"))?
            .len()
            > crate::SESSION_READ_LIMIT_BYTES
        {
            return Err(capacity(
                "cursor_database_bytes",
                crate::SESSION_READ_LIMIT_BYTES as usize,
            ));
        }
        let sidecar_file = crate::open_session_file_nofollow(&path.with_file_name("meta.json"))
            .map_err(|_| invalid("cursor_sidecar_unavailable"))?;
        let mut raw = Vec::new();
        sidecar_file
            .take(META_BYTES as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| invalid("cursor_sidecar_unavailable"))?;
        if raw.len() > META_BYTES {
            return Err(capacity("cursor_sidecar_bytes", META_BYTES));
        }
        let sidecar: Sidecar =
            serde_json::from_slice(&raw).map_err(|_| invalid("cursor_sidecar_invalid"))?;
        if sidecar.schema_version != 1 || sidecar.is_subagent || sidecar.created_at_ms == 0 {
            return Err(invalid("cursor_root_session_unconfirmed"));
        }
        let bucket = directory(&sidecar.cwd)?;
        if path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            != Some(std::ffi::OsStr::new(&bucket))
        {
            return Err(invalid("cursor_cwd_bucket_mismatch"));
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::from_bits_retain(rusqlite::ffi::SQLITE_OPEN_NOFOLLOW);
        // Bundled SQLite's Unix and Windows VFSes implement readonly_shm.
        // READ_ONLY alone still opens an existing wal-index for writes.
        // With no WAL, an immutable connection reads the checkpointed main
        // without creating WAL/SHM companions. The source marks must remain
        // identical before and after: a concurrent native writer is refused.
        let connection =
            Connection::open_with_flags(database_uri(&path, before[2].is_some())?, flags)
                .map_err(|_| invalid("cursor_database_unavailable"))?;
        connection
            .busy_timeout(Duration::from_millis(50))
            .map_err(|_| invalid("cursor_database_unavailable"))?;
        let work = Arc::new(AtomicUsize::new(0));
        connection.progress_handler(
            1000,
            Some(move || work.fetch_add(1000, Ordering::Relaxed) >= SQL_STEPS),
        );
        connection
            .execute_batch("PRAGMA query_only=ON; BEGIN")
            .map_err(|_| invalid("cursor_database_unavailable"))?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|error| database_error("cursor_schema_unconfirmed", error))?;
        if version != 1 {
            return Err(invalid("cursor_schema_unsupported"));
        }
        let size: usize = connection
            .query_row("SELECT length(value) FROM meta WHERE key='0'", [], |row| {
                row.get(0)
            })
            .map_err(|_| invalid("cursor_metadata_unavailable"))?;
        if size > META_BYTES * 2 {
            return Err(capacity("cursor_metadata_bytes", META_BYTES));
        }
        if size + raw.len() > budget {
            return Err(capacity("cursor_read_bytes", budget));
        }
        let encoded: String = connection
            .query_row("SELECT value FROM meta WHERE key='0'", [], |row| row.get(0))
            .map_err(|_| invalid("cursor_metadata_unavailable"))?;
        let metadata: Metadata = serde_json::from_slice(&unhex(&encoded, META_BYTES)?)
            .map_err(|_| invalid("cursor_metadata_invalid"))?;
        let id = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|id| id.to_str())
            .ok_or_else(|| invalid("cursor_session_id_invalid"))?
            .to_owned();
        if metadata.agent_id != id
            || metadata.subagent_info.is_some()
            || metadata.created_at == 0
            || metadata.created_at != sidecar.created_at_ms
        {
            return Err(invalid("cursor_session_identity_mismatch"));
        }
        if let Some((expected_id, expected_cwd)) = scope {
            let actual = hide_platform::fs::identity::canonical(&sidecar.cwd)
                .map_err(|_| invalid("cursor_cwd_unconfirmed"))?;
            let expected = hide_platform::fs::identity::canonical(Path::new(expected_cwd))
                .map_err(|_| invalid("cursor_cwd_unconfirmed"))?;
            if expected_id != id || actual != expected {
                return Err(invalid("cursor_scope_mismatch"));
            }
        }
        let root = unhex(&metadata.latest_root_blob_id, 32)?;
        if (!root.is_empty() && root.len() != 32) || sidecar.has_conversation == root.is_empty() {
            return Err(invalid("cursor_root_unconfirmed"));
        }
        Ok(Self {
            connection,
            home: home.to_path_buf(),
            path,
            before,
            header: crate::native_file::Header {
                id,
                cwd: sidecar.cwd,
            },
            root,
            created_at: metadata.created_at,
            updated_at: sidecar.updated_at_ms,
            read_bytes: size + raw.len(),
            allowance: budget,
            queries: 4,
        })
    }

    fn blob(&mut self, id: &[u8]) -> Result<Option<Vec<u8>>> {
        if id.len() != 32 {
            return Err(invalid("cursor_blob_reference_invalid"));
        }
        if self.queries + 2 > QUERY_LIMIT {
            return Ok(None);
        }
        self.queries += 2;
        let key = hex(id);
        let size: Option<usize> = self
            .connection
            .query_row(
                "SELECT length(data) FROM blobs WHERE id=?1",
                [&key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| invalid("cursor_blob_unavailable"))?;
        let size = size.ok_or_else(|| invalid("cursor_blob_missing"))?;
        if size > BLOB_BYTES {
            return Err(capacity("cursor_blob_bytes", BLOB_BYTES));
        }
        if size > self.allowance.saturating_sub(self.read_bytes) {
            return Ok(None);
        }
        let bytes: Vec<u8> = self
            .connection
            .query_row("SELECT data FROM blobs WHERE id=?1", [&key], |row| {
                row.get(0)
            })
            .map_err(|_| invalid("cursor_blob_unavailable"))?;
        self.read_bytes += bytes.len();
        if bytes.len() != size || Sha256::digest(&bytes).as_slice() != id {
            return Err(invalid("cursor_blob_hash_mismatch"));
        }
        Ok(Some(bytes))
    }

    fn required<T: Message + Default>(&mut self, id: &[u8]) -> Result<T> {
        let bytes = self
            .blob(id)?
            .ok_or_else(|| capacity("cursor_read_bytes", self.allowance))?;
        T::decode(bytes.as_slice()).map_err(|_| invalid("cursor_protobuf_invalid"))
    }

    fn finish(&self) -> Result<()> {
        self.connection
            .execute_batch("ROLLBACK")
            .map_err(|_| invalid("cursor_read_failed"))?;
        if marks(&self.home, &self.path)? != self.before {
            return Err(invalid("cursor_source_changed"));
        }
        Ok(())
    }
}

/// A native graph position, boxed by the generic conversation checkpoint.
/// Closed turn refs and consumed current steps are hashed, never retained as
/// transcript content. The last turn stays open so its appended steps survive.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    owner: String,
    incarnation: Option<String>,
    turn: usize,
    closed: String,
    user: Vec<u8>,
    user_read: bool,
    steps: usize,
    step_prefix: String,
    offset: u64,
}

impl Checkpoint {
    pub fn offset(&self) -> u64 {
        self.offset
    }
}

pub struct ReadResult {
    pub parsed: ParsedSession,
    pub checkpoint: Checkpoint,
    pub has_more: bool,
    pub read_bytes: u64,
}

fn prefix(ids: &[Vec<u8>]) -> Result<String> {
    let mut hasher = Sha256::new();
    for id in ids {
        if id.len() != 32 {
            return Err(invalid("cursor_blob_reference_invalid"));
        }
        hasher.update(id);
    }
    Ok(hex(&hasher.finalize()))
}

fn minute(value: Option<u64>) -> u64 {
    value.map(|value| value / 60_000 * 60_000).unwrap_or(0)
}

/// Read native conversation units under one transaction and byte allowance.
/// Missing timing is zero (the common unknown-time sentinel); recorded timing
/// is rounded down to a minute. Tools and thoughts never become conversation.
pub fn read(
    home: &Path,
    path: &Path,
    native_id: &str,
    cwd: &str,
    saved: Option<Checkpoint>,
    budget: u64,
) -> Result<ReadResult> {
    let budget =
        usize::try_from(budget.min(crate::SESSION_INCREMENT_READ_LIMIT_BYTES)).map_err(|_| {
            capacity(
                "cursor_read_bytes",
                crate::SESSION_INCREMENT_READ_LIMIT_BYTES as usize,
            )
        })?;
    let mut db = Database::open(home, path, Some((native_id, cwd)), budget)?;
    let incarnation = db.before[0].as_ref().map(|mark| mark.id.clone());
    let owner = hex(&Sha256::digest(native_id.as_bytes()));
    let mut parsed = ParsedSession::default();
    let mut checkpoint = saved.unwrap_or_default();
    let previous_offset = checkpoint.offset;
    if !checkpoint.owner.is_empty()
        && (checkpoint.owner != owner || checkpoint.incarnation != incarnation)
    {
        parsed.rescan_reason = Some(RescanReason::Replaced);
        checkpoint = Checkpoint::default();
    }
    checkpoint.owner = owner;
    checkpoint.incarnation = incarnation.clone();
    let root_id = db.root.clone();
    let root = if root_id.is_empty() {
        Conversation::default()
    } else {
        db.required::<Conversation>(&root_id)?
    };
    if root.turns.len() > TURN_LIMIT {
        return Err(capacity("cursor_turns", TURN_LIMIT));
    }
    let consistent = checkpoint.turn < root.turns.len()
        && (checkpoint.closed.is_empty()
            || checkpoint.closed == prefix(&root.turns[..checkpoint.turn])?);
    if checkpoint.offset != 0 && !consistent {
        parsed.rescan_reason = Some(RescanReason::Truncated);
        checkpoint = Checkpoint {
            owner: checkpoint.owner,
            incarnation: incarnation.clone(),
            ..Checkpoint::default()
        };
    }
    let mut has_more = false;
    while checkpoint.turn < root.turns.len() {
        let turn_id = &root.turns[checkpoint.turn];
        let Some(bytes) = db.blob(turn_id)? else {
            if checkpoint.offset == 0 {
                return Err(capacity("cursor_read_bytes", budget));
            }
            has_more = true;
            break;
        };
        let turn =
            Turn::decode(bytes.as_slice()).map_err(|_| invalid("cursor_protobuf_invalid"))?;
        let agent = match turn.kind {
            Some(turn::Kind::Agent(agent)) => agent,
            Some(turn::Kind::Shell(_)) => AgentTurn::default(),
            None => return Err(invalid("cursor_turn_invalid")),
        };
        if agent.steps.len() > STEP_LIMIT {
            return Err(capacity("cursor_turn_steps", STEP_LIMIT));
        }
        if checkpoint.steps > agent.steps.len()
            || (checkpoint.user_read && checkpoint.user != agent.user_message)
            || (!checkpoint.step_prefix.is_empty()
                && checkpoint.step_prefix != prefix(&agent.steps[..checkpoint.steps])?)
        {
            // A mutable prior prefix cannot be appended to the existing view.
            parsed = ParsedSession {
                rescan_reason: Some(RescanReason::Truncated),
                ..ParsedSession::default()
            };
            checkpoint = Checkpoint {
                owner: checkpoint.owner,
                incarnation: incarnation.clone(),
                ..Checkpoint::default()
            };
            continue;
        }
        if !checkpoint.user_read && !agent.user_message.is_empty() {
            let Some(bytes) = db.blob(&agent.user_message)? else {
                has_more = true;
                break;
            };
            let user =
                User::decode(bytes.as_slice()).map_err(|_| invalid("cursor_protobuf_invalid"))?;
            let text = if user.text_blob_id.is_empty() {
                user.text
            } else {
                let Some(bytes) = db.blob(&user.text_blob_id)? else {
                    has_more = true;
                    break;
                };
                String::from_utf8(bytes).map_err(|_| invalid("cursor_text_invalid"))?
            };
            let injected =
                user.is_simulated_msg.unwrap_or(false) || user.sent_by_agent_id.is_some();
            if !text.is_empty() {
                parsed.events.push(
                    ConversationEvent::new(
                        "user",
                        if injected {
                            EventKind::Injected
                        } else {
                            EventKind::Human
                        },
                        minute(user.started_at_ms),
                        text,
                    )
                    .with_provider_injected(injected),
                );
                parsed.event_offsets.push(checkpoint.offset);
            }
            checkpoint.offset += 1;
            checkpoint.user = agent.user_message;
            checkpoint.user_read = true;
        }
        while checkpoint.steps < agent.steps.len() && parsed.events.len() < EVENT_LIMIT {
            let Some(bytes) = db.blob(&agent.steps[checkpoint.steps])? else {
                has_more = true;
                break;
            };
            let step =
                Step::decode(bytes.as_slice()).map_err(|_| invalid("cursor_protobuf_invalid"))?;
            match step.kind {
                Some(step::Kind::Assistant(message)) if !message.text.is_empty() => {
                    parsed.events.push(ConversationEvent::new(
                        "assistant",
                        EventKind::Assistant,
                        minute(message.started_at_ms),
                        message.text,
                    ));
                    parsed.event_offsets.push(checkpoint.offset);
                }
                Some(_) => {}
                None => return Err(invalid("cursor_step_invalid")),
            }
            checkpoint.offset += 1;
            checkpoint.steps += 1;
        }
        checkpoint.closed = prefix(&root.turns[..checkpoint.turn])?;
        checkpoint.step_prefix = prefix(&agent.steps[..checkpoint.steps])?;
        if checkpoint.steps < agent.steps.len() || parsed.events.len() >= EVENT_LIMIT {
            has_more =
                checkpoint.steps < agent.steps.len() || checkpoint.turn + 1 < root.turns.len();
            break;
        }
        if checkpoint.turn + 1 == root.turns.len() {
            break;
        }
        checkpoint.turn += 1;
        checkpoint.closed = prefix(&root.turns[..checkpoint.turn])?;
        checkpoint.user.clear();
        checkpoint.user_read = false;
        checkpoint.steps = 0;
        checkpoint.step_prefix.clear();
    }
    if has_more && checkpoint.offset == previous_offset && parsed.rescan_reason.is_none() {
        return Err(capacity("cursor_read_bytes", budget));
    }
    db.finish()?;
    Ok(ReadResult {
        parsed,
        checkpoint,
        has_more,
        read_bytes: db.read_bytes as u64,
    })
}

pub struct Header {
    pub id: String,
    pub cwd: PathBuf,
    pub created_at: u64,
    pub updated_at: Option<u64>,
}

pub fn header(home: &Path, path: &Path) -> Result<Header> {
    let db = Database::open(
        home,
        path,
        None,
        crate::SESSION_INCREMENT_READ_LIMIT_BYTES as usize,
    )?;
    db.finish()?;
    Ok(Header {
        id: db.header.id,
        cwd: db.header.cwd,
        created_at: minute(Some(db.created_at)),
        updated_at: db.updated_at.map(|time| minute(Some(time))),
    })
}

// Field numbers come from the installed native agent/v1 generated schema.
// Prost skips unowned fields, including subagent maps, with its recursion cap.
#[derive(Clone, PartialEq, Message)]
struct Conversation {
    #[prost(bytes = "vec", repeated, tag = "8")]
    turns: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
struct Turn {
    #[prost(oneof = "turn::Kind", tags = "1, 2")]
    kind: Option<turn::Kind>,
}
mod turn {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Kind {
        #[prost(message, tag = "1")]
        Agent(super::AgentTurn),
        #[prost(message, tag = "2")]
        Shell(super::Ignored),
    }
}

#[derive(Clone, PartialEq, Message)]
struct AgentTurn {
    #[prost(bytes = "vec", tag = "1")]
    user_message: Vec<u8>,
    #[prost(bytes = "vec", repeated, tag = "2")]
    steps: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
struct User {
    #[prost(string, tag = "1")]
    text: String,
    #[prost(bool, optional, tag = "5")]
    is_simulated_msg: Option<bool>,
    #[prost(bytes = "vec", tag = "18")]
    text_blob_id: Vec<u8>,
    #[prost(uint64, optional, tag = "25")]
    started_at_ms: Option<u64>,
    #[prost(string, optional, tag = "27")]
    sent_by_agent_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct Step {
    #[prost(oneof = "step::Kind", tags = "1, 2, 3")]
    kind: Option<step::Kind>,
}
mod step {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Kind {
        #[prost(message, tag = "1")]
        Assistant(super::Assistant),
        #[prost(message, tag = "2")]
        Tool(super::Ignored),
        #[prost(message, tag = "3")]
        Thinking(super::Ignored),
    }
}

#[derive(Clone, PartialEq, Message)]
struct Assistant {
    #[prost(string, tag = "1")]
    text: String,
    #[prost(uint64, optional, tag = "2")]
    started_at_ms: Option<u64>,
}

#[derive(Clone, PartialEq, Message)]
struct Ignored {}
