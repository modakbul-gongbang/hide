//! OpenCode's sessions, read from its SQLite database (PRD
//! overview-request-view D-16, B28).
//!
//! OpenCode keeps every session in `~/.local/share/opencode/opencode.db`: a
//! `session` row, its `message` rows in order, and each message's `part`
//! rows (text, attached files, tool calls with their output). The database
//! is opened read-only with a short busy wait, so a read never holds OpenCode
//! back; a locked database is a read skipped until the next state change.
//!
//! A session's messages are only appended, so the checkpoint is the number
//! of messages already read, and an event's offset is its message's index.
//! An assistant message OpenCode is still writing has no `time.completed`;
//! the read stops before it and takes it whole on a later read.
//!
//! A row over its limit is never loaded: SQLite answers its size from the
//! record header, and the row is counted as skipped (`message_capacity`
//! over [`MESSAGE_LIMIT_BYTES`], `part_capacity` over
//! [`SESSION_LINE_LIMIT_BYTES`], `not_text` for a row that holds no text).
//! One read loads at most [`READ_BUDGET_BYTES`] of rows and leaves the rest
//! for the next; a part the budget cannot take in the first message of a
//! read is skipped (`read_budget`). The title is cut to
//! [`TITLE_LIMIT_CHARS`] before it leaves SQLite, and a title too long to
//! cut that way (over four bytes a character) is no title.
//!
//! Every read proves its owner from OpenCode's own `session` row inside the
//! read's one transaction: the id names a root session (a subagent's child
//! session has a `parent_id` and is never read as anyone's conversation),
//! and the row's `directory` is the checkout the caller names. OpenCode is
//! referenced by id only; a reported path proves nothing here.
//!
//! A `question` tool call keeps its assistant message unfinished while it
//! waits for the operator, so the read folds that unfinished message's
//! question parts without settling it ([`TurnTracker::fold_unsettled`]).

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;

use sha2::{Digest, Sha256};

use crate::label_transcript::{
    LabelEvent, LabelEventKind, LabelTranscript, LabelTranscriptRequest, MemoryReceiptPart,
};
use crate::turns::{ToolTurnMark, TurnMark, TurnTracker};
use crate::{
    ConfirmedLabelSession, ConversationCheckpoint, MAX_SIGHTINGS_PER_OUTPUT, PrSighting,
    SESSION_INCREMENT_READ_LIMIT_BYTES, SESSION_LINE_LIMIT_BYTES, SkipReason,
    label_reference_token, pull_request_addresses,
};

/// How long a read waits for OpenCode's own write to finish.
const BUSY_WAIT: Duration = Duration::from_millis(50);
/// The most messages one read takes; a longer session is read over several.
const MESSAGES_PER_READ: i64 = 200;
/// Bounds header visits and payload queries even for empty or skipped parts.
const PARTS_PER_MESSAGE: i64 = 256;
const PARTS_PER_READ: i64 = 512;
/// Covers each retained String slot and header independently of payload size.
const PART_OVERHEAD_BYTES: u64 = 64;
/// Also bound SQLite's scans/sorts when a provider index is missing or its
/// database is corrupt: LIMIT alone bounds returned rows, not query work.
const SQL_STEPS_PER_READ: usize = 1_000_000;
/// The most row data one read loads, as much as one JSONL increment.
const READ_BUDGET_BYTES: u64 = SESSION_INCREMENT_READ_LIMIT_BYTES;
/// A part row over this is skipped without being loaded.
const ROW_LIMIT_BYTES: i64 = SESSION_LINE_LIMIT_BYTES as i64;
/// A message row holds the message's metadata, never its text; one over
/// this is skipped without being loaded.
const MESSAGE_LIMIT_BYTES: i64 = 64 * 1024;
/// The most Memory receipts one read reports; Hide writes at most one a prompt.
const RECEIPTS_PER_READ: usize = 64;
/// A receipt line names item ids, revisions and a 64-character tag; a longer
/// line is not one Hide wrote.
const RECEIPT_LINE_LIMIT_BYTES: usize = 4 * 1024;
const RECEIPT_MARKER: &str = "<hide-memory-receipt ";
/// The most characters of a session title a read takes.
const TITLE_LIMIT_CHARS: i64 = 512;
/// The longest `directory` a session row may name; a longer one is no checkout.
const DIRECTORY_LIMIT_BYTES: i64 = 4096;

pub(crate) fn database_path(home: &Path) -> PathBuf {
    home.join(".local/share/opencode/opencode.db")
}

/// A read-only transaction on OpenCode's database, with the busy wait and
/// the SQL work cap every read shares. A linked database is refused: the
/// file Hide reads is OpenCode's own, never one a link points elsewhere.
pub(crate) fn open(home: &Path) -> Result<Connection, String> {
    let path = database_path(home);
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| "session_file_missing".to_owned())?;
    if metadata.file_type().is_symlink() {
        return Err("label_session_linked".to_owned());
    }
    if !metadata.is_file() {
        return Err("label_session_not_regular".to_owned());
    }
    let connection = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| refusal(&error))?;
    connection
        .busy_timeout(BUSY_WAIT)
        .map_err(|error| refusal(&error))?;
    let mut sql_steps = 0;
    connection.progress_handler(
        1_000,
        Some(move || {
            sql_steps += 1_000;
            sql_steps >= SQL_STEPS_PER_READ
        }),
    );
    connection
        .execute_batch("BEGIN")
        .map_err(|error| refusal(&error))?;
    Ok(connection)
}

/// The root session a proof found, as its own row states it.
pub(crate) struct Proven {
    pub(crate) title: Option<String>,
    pub(crate) created: u64,
    pub(crate) updated: u64,
    pub(crate) messages: u64,
}

/// Proves that `id` names a root session OpenCode started in `cwd`.
pub(crate) fn prove(connection: &Connection, id: &str, cwd: Option<&str>) -> Result<Proven, String> {
    if !crate::label_owner::valid_native_id(id) {
        return Err("label_session_id_invalid".to_owned());
    }
    let (title, created, updated, directory, child) = connection
        .query_row(
            "SELECT CASE WHEN typeof(title) = 'text' AND octet_length(title) <= ?2 * 4 \
             THEN substr(title, 1, ?2) END, time_created, time_updated, \
             CASE WHEN typeof(directory) = 'text' AND octet_length(directory) <= ?3 \
             THEN directory END, parent_id IS NOT NULL FROM session WHERE id = ?1",
            params![id, TITLE_LIMIT_CHARS, DIRECTORY_LIMIT_BYTES],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| refusal(&error))?
        .ok_or_else(|| "session_file_missing".to_owned())?;
    if child {
        return Err("label_session_not_root".to_owned());
    }
    let cwd = cwd.ok_or_else(|| "label_session_cwd_unconfirmed".to_owned())?;
    let directory = directory.ok_or_else(|| "label_session_cwd_unconfirmed".to_owned())?;
    if !same_directory(&directory, cwd) {
        return Err("label_session_cwd_mismatch".to_owned());
    }
    let messages: i64 = connection
        .query_row(
            "SELECT count(*) FROM message WHERE session_id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|error| refusal(&error))?;
    Ok(Proven {
        title: title.filter(|title| !title.trim().is_empty()),
        created: u64::try_from(created).map_err(|_| "label_session_metadata_unconfirmed")?,
        updated: u64::try_from(updated).map_err(|_| "label_session_metadata_unconfirmed")?,
        messages: u64::try_from(messages).unwrap_or(0),
    })
}

/// The checkout OpenCode recorded is the one named: the same absolute
/// spelling, or the same folder once every link is resolved.
fn same_directory(native: &str, cwd: &str) -> bool {
    let native_path = Path::new(native);
    if !native_path.is_absolute() || native.chars().any(char::is_control) {
        return false;
    }
    if native.trim_end_matches('/') == cwd.trim_end_matches('/') {
        return true;
    }
    match (
        hide_platform::fs::identity::canonical(native_path),
        hide_platform::fs::identity::canonical(Path::new(cwd)),
    ) {
        (Ok(native), Ok(cwd)) => native == cwd,
        _ => false,
    }
}

impl Proven {
    pub(crate) fn confirmed(&self, id: &str) -> Result<ConfirmedLabelSession, String> {
        Ok(ConfirmedLabelSession {
            owner: label_reference_token("opencode", "id", id)
                .ok_or_else(|| "label_session_id_invalid".to_owned())?,
            native_session_id: Some(id.to_owned()),
            source_path: None,
            incarnation: format!("opencode:{}", self.created),
            bytes: self.messages,
        })
    }
}

/// The proven owner of `id` in `cwd`, read in one transaction.
pub(crate) fn confirm(
    home: &Path,
    id: &str,
    cwd: Option<&str>,
) -> Result<ConfirmedLabelSession, String> {
    let connection = open(home)?;
    prove(&connection, id, cwd)?.confirmed(id)
}

/// A session's activity: its newest write (the session row or any of its
/// messages) and its message count, between the same proof as every read.
/// The id is the exact route `opencode -s <id>` takes, so a proven id is
/// also the proven resume route.
pub(crate) fn activity(
    home: &Path,
    request: &crate::session_activity::SessionActivityRequest,
) -> Result<crate::session_activity::SessionActivity, String> {
    if request.reference_kind != "id" {
        return Err("session_kind_unsupported".to_owned());
    }
    let id = request.reference_value.as_str();
    if request.expected_id.as_deref().is_some_and(|expected| expected != id) {
        return Err("session_route_owner_changed".to_owned());
    }
    let connection = open(home)?;
    let proven = prove(&connection, id, request.cwd.as_deref())?;
    let newest: Option<i64> = connection
        .query_row(
            "SELECT max(time_updated) FROM message WHERE session_id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|error| refusal(&error))?;
    let newest = newest.and_then(|newest| u64::try_from(newest).ok()).unwrap_or(0);
    Ok(crate::session_activity::SessionActivity {
        modified_at_unix_ms: proven.updated.max(newest),
        bytes: proven.messages,
    })
}

/// A message id's digest, the witness a checkpoint keeps of where it ended.
fn witness(message_id: &str) -> u64 {
    let digest = Sha256::digest(message_id.as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("a digest has eight bytes"))
}

/// The id of the message at `index` in reading order.
fn message_at(connection: &Connection, id: &str, index: u64) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT CASE WHEN octet_length(id) <= ?3 THEN id END FROM message \
             WHERE session_id = ?1 ORDER BY time_created, id LIMIT 1 OFFSET ?2",
            params![id, index as i64, MESSAGE_LIMIT_BYTES],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(|error| refusal(&error))
}

pub(crate) fn read(
    home: &Path,
    request: &LabelTranscriptRequest,
) -> Result<LabelTranscript, String> {
    if request.reference_kind != "id" {
        return Err("session_kind_unsupported".to_owned());
    }
    let session_id = request.reference_value.as_str();
    let connection = open(home)?;
    let proven = prove(&connection, session_id, request.cwd.as_deref())?;
    let count = proven.messages;
    let mut start = request
        .checkpoint
        .as_ref()
        .map_or(0, ConversationCheckpoint::offset);
    // Messages are only appended; fewer than were read means the session
    // was rewound (an OpenCode revert), and it is read from its start again.
    // A rewound session that grew back past the count keeps no message the
    // checkpoint's witness names, so it is read again too.
    let mut rescanned = (start > count).then(|| "truncated".to_owned());
    if rescanned.is_none()
        && start > 0
        && let Some((created, last)) = request
            .checkpoint
            .as_ref()
            .and_then(ConversationCheckpoint::message_witness)
        && (created != proven.created
            || message_at(&connection, session_id, start - 1)?.as_deref().map(witness)
                != Some(last))
    {
        rescanned = Some("replaced".to_owned());
    }
    if rescanned.is_some() {
        start = 0;
    }
    let mut turns = match (&request.checkpoint, &rescanned) {
        (Some(_), None) => request.turns.clone().unwrap_or_default(),
        _ => TurnTracker::default(),
    };
    let mut statement = connection
        .prepare(
            "SELECT rowid, typeof(data) = 'text', octet_length(data), octet_length(id) \
             FROM message WHERE session_id = ?1 \
             ORDER BY time_created, id LIMIT ?2 OFFSET ?3",
        )
        .map_err(|error| refusal(&error))?;
    let rows = statement
        .query(params![session_id, MESSAGES_PER_READ, start as i64])
        .map_err(|error| refusal(&error))?;
    let mut rows = rows;
    // Sorting visits only row headers, not 200 retained metadata payloads.
    // The payload query runs only after the cumulative budget admits it.
    let mut metadata = connection
        .prepare("SELECT id, data FROM message WHERE rowid = ?1")
        .map_err(|error| refusal(&error))?;
    let mut part_bytes = connection
        .prepare(
            "SELECT coalesce(sum(CASE WHEN is_text AND bytes <= ?2 \
             THEN bytes ELSE 0 END + ?4), 0) FROM \
             (SELECT typeof(data) = 'text' AS is_text, octet_length(data) AS bytes \
             FROM part WHERE message_id = ?1 ORDER BY time_created, id LIMIT ?3)",
        )
        .map_err(|error| refusal(&error))?;
    let mut parts = connection
        .prepare(
            "SELECT rowid, typeof(data) = 'text', octet_length(data) \
             FROM part WHERE message_id = ?1 ORDER BY time_created, id LIMIT ?2",
        )
        .map_err(|error| refusal(&error))?;
    let mut part_data = connection
        .prepare("SELECT data FROM part WHERE rowid = ?1")
        .map_err(|error| refusal(&error))?;
    let mut transcript = Read {
        events: Vec::new(),
        sightings: Vec::new(),
        receipts: Vec::new(),
        skipped: std::collections::BTreeMap::new(),
        turn_error: None,
    };
    // The witness of the message before the next one read: the checkpoint's
    // own (verified above) until this read passes a message.
    let mut previous = request
        .checkpoint
        .as_ref()
        .filter(|_| rescanned.is_none())
        .and_then(ConversationCheckpoint::message_witness)
        .map(|(_, last)| last);
    let mut anchor = None;
    let mut next = start;
    let mut unfinished = None;
    let mut over_budget = false;
    let mut spent = 0_u64;
    let mut visited = 0_i64;
    let mut part_work = 0_i64;
    while let Some(row) = rows.next().map_err(|error| refusal(&error))? {
        if part_work >= PARTS_PER_READ {
            over_budget = true;
            break;
        }
        visited += 1;
        if !row.get::<_, bool>(1).map_err(|error| refusal(&error))? {
            transcript.skip("not_text");
            // A message Hide cannot read leaves no witness of its own.
            previous = None;
            next += 1;
            continue;
        }
        let data_bytes: u64 = row.get(2).map_err(|error| refusal(&error))?;
        let id_bytes: u64 = row.get(3).map_err(|error| refusal(&error))?;
        let metadata_bytes = data_bytes.saturating_add(id_bytes);
        if data_bytes > MESSAGE_LIMIT_BYTES as u64 || id_bytes > MESSAGE_LIMIT_BYTES as u64 {
            transcript.skip("message_capacity");
            previous = None;
            next += 1;
            continue;
        }
        if metadata_bytes > READ_BUDGET_BYTES.saturating_sub(spent) {
            over_budget = true;
            break;
        }
        spent += metadata_bytes;
        let rowid: i64 = row.get(0).map_err(|error| refusal(&error))?;
        let (message_id, data): (String, String) = metadata
            .query_row(params![rowid], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| refusal(&error))?;
        let Ok(message) = serde_json::from_str::<Value>(&data) else {
            transcript.skip("malformed_json");
            previous = Some(witness(&message_id));
            next += 1;
            continue;
        };
        let role = message.get("role").and_then(Value::as_str);
        let completed = message.pointer("/time/completed").is_some();
        if role == Some("assistant") && !completed {
            unfinished = Some(message_id);
            break;
        }
        let at = message
            .pointer("/time/created")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let part_limit = PARTS_PER_MESSAGE.min(PARTS_PER_READ - part_work);
        let size: i64 = part_bytes
            .query_row(
                params![message_id, ROW_LIMIT_BYTES, part_limit, PART_OVERHEAD_BYTES],
                |row| row.get(0),
            )
            .map_err(|error| refusal(&error))?;
        let size = u64::try_from(size).unwrap_or(0);
        // The first message of a read is always taken, so a read moves on
        // however large its parts are; admission still precedes allocation.
        if next > start && size > READ_BUDGET_BYTES.saturating_sub(spent) {
            over_budget = true;
            break;
        }
        let mut part_rows = Vec::new();
        let mut part_headers = parts
            // One extra header proves the per-message cap was crossed.
            // It is never loaded or retained: at most 200 cap probes/read.
            .query(params![message_id, part_limit + 1])
            .map_err(|error| refusal(&error))?;
        let mut message_parts = 0;
        while let Some(part) = part_headers.next().map_err(|error| refusal(&error))? {
            if message_parts == part_limit {
                transcript.skip("part_work_capacity");
                break;
            }
            message_parts += 1;
            part_work += 1;
            if PART_OVERHEAD_BYTES > READ_BUDGET_BYTES.saturating_sub(spent) {
                transcript.skip("read_budget");
                continue;
            }
            spent += PART_OVERHEAD_BYTES;
            if !part.get::<_, bool>(1).map_err(|error| refusal(&error))? {
                transcript.skip("not_text");
                continue;
            }
            let bytes: u64 = part.get(2).map_err(|error| refusal(&error))?;
            if bytes > ROW_LIMIT_BYTES as u64 {
                transcript.skip("part_capacity");
                continue;
            }
            if bytes > READ_BUDGET_BYTES.saturating_sub(spent) {
                transcript.skip("read_budget");
                continue;
            }
            spent += bytes;
            let rowid: i64 = part.get(0).map_err(|error| refusal(&error))?;
            let data = part_data
                .query_row(params![rowid], |row| row.get::<_, String>(0))
                .map_err(|error| refusal(&error))?;
            part_rows.push(data);
        }
        let (human, marks) = transcript.message(role, at, next, &part_rows);
        if human {
            anchor = Some(ConversationCheckpoint::at_message(
                next,
                proven.created,
                previous,
            ));
            turns.fold(next, &TurnMark::HumanTurn);
        }
        if !marks.is_empty() {
            turns.fold(next, &TurnMark::Tools(marks));
        }
        previous = Some(witness(&message_id));
        next += 1;
    }
    drop(rows);
    if let Some(reason) = transcript.turn_error {
        return Err(reason.as_str().to_owned());
    }
    // The unfinished message is the session's newest: its question parts are
    // what OpenCode waits on now. They are folded unsettled, so the same
    // message folds again once OpenCode completes it.
    if let Some(message_id) = unfinished.as_deref() {
        let asking = unfinished_questions(&connection, message_id)?;
        if asking.bytes > READ_BUDGET_BYTES.saturating_sub(spent) {
            if next == start {
                return Err(SkipReason::UserTurnCapacity.as_str().to_owned());
            }
            over_budget = true;
        } else if !asking.marks.is_empty() {
            turns.fold_unsettled(next, &TurnMark::Tools(asking.marks));
        }
    }
    if turns.capacity_exceeded() {
        return Err(SkipReason::UserTurnCapacity.as_str().to_owned());
    }
    let has_more = over_budget || (unfinished.is_none() && visited == MESSAGES_PER_READ);
    let skipped_lines = transcript.skipped.values().sum();
    let skipped_reasons = transcript
        .skipped
        .into_iter()
        .map(|(reason, count)| (reason.to_owned(), count))
        .collect();
    Ok(LabelTranscript {
        confirmed: proven.confirmed(session_id)?,
        events: transcript.events,
        checkpoint: ConversationCheckpoint::at_message(next, proven.created, previous),
        anchor,
        has_more,
        rescanned,
        skipped_lines,
        skipped_reasons,
        title: proven.title,
        custom_title: None,
        pr_sightings: transcript.sightings,
        memory_receipts: transcript.receipts,
        subagents: Default::default(),
        turns: Some(turns),
    })
}

/// The question marks of the message OpenCode is still writing, and the
/// bytes they took. Only its `question` tool parts are loaded, each within
/// the row limit and at most one past the question cap.
struct Asking {
    marks: Vec<ToolTurnMark>,
    bytes: u64,
}

fn unfinished_questions(connection: &Connection, message_id: &str) -> Result<Asking, String> {
    let mut statement = connection
        .prepare(
            "SELECT data FROM part WHERE message_id = ?1 AND typeof(data) = 'text' \
             AND octet_length(data) <= ?2 AND json_valid(data) \
             AND json_extract(data, '$.type') = 'tool' \
             AND json_extract(data, '$.tool') = 'question' \
             ORDER BY time_created, id LIMIT ?3",
        )
        .map_err(|error| refusal(&error))?;
    let mut rows = statement
        .query(params![
            message_id,
            ROW_LIMIT_BYTES,
            crate::turns::QUESTION_CALL_LIMIT as i64 + 1
        ])
        .map_err(|error| refusal(&error))?;
    let mut parts = Vec::new();
    let mut bytes = 0_u64;
    while let Some(row) = rows.next().map_err(|error| refusal(&error))? {
        let data: String = row.get(0).map_err(|error| refusal(&error))?;
        bytes += data.len() as u64 + PART_OVERHEAD_BYTES;
        parts.push(
            serde_json::from_str::<Value>(&data)
                .map_err(|_| SkipReason::UserTurnInvalid.as_str().to_owned())?,
        );
    }
    let marks = crate::turns::native::opencode_marks(&parts)
        .map_err(|reason| reason.as_str().to_owned())?;
    Ok(Asking { marks, bytes })
}

struct Read {
    events: Vec<LabelEvent>,
    sightings: Vec<PrSighting>,
    receipts: Vec<MemoryReceiptPart>,
    skipped: std::collections::BTreeMap<&'static str, usize>,
    /// A question record Hide cannot fold; the read is refused, as a native
    /// file's is, rather than reporting a wait it does not know.
    turn_error: Option<SkipReason>,
}

impl Read {
    fn skip(&mut self, reason: &'static str) {
        *self.skipped.entry(reason).or_default() += 1;
    }

    /// Reads one finished message: whether it was a person's, and the
    /// question marks its parts hold.
    fn message(
        &mut self,
        role: Option<&str>,
        at: u64,
        offset: u64,
        parts: &[String],
    ) -> (bool, Vec<ToolTurnMark>) {
        let mut text = Vec::new();
        let mut images = 0_u32;
        let mut questions = Vec::new();
        for part in parts {
            let Ok(part) = serde_json::from_str::<Value>(part) else {
                self.skip("malformed_json");
                continue;
            };
            if role == Some("assistant") {
                match crate::turns::native::opencode_part(&part) {
                    Ok(Some(mark)) => questions.push(mark),
                    Ok(None) => {}
                    Err(reason) => {
                        self.turn_error.get_or_insert(reason);
                    }
                }
            }
            match part.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let Some(body) = part.get("text").and_then(Value::as_str) else {
                        continue;
                    };
                    // OpenCode's own text (a compaction's request) and Hide's
                    // plugin's are no one's message; a receipt in Hide's is
                    // reported for Memory to check.
                    if part.get("synthetic").and_then(Value::as_bool) == Some(true) {
                        if role == Some("user") {
                            self.receipts(body, offset);
                        }
                        continue;
                    }
                    text.push(body.to_owned());
                }
                Some("file")
                    if part
                        .get("mime")
                        .and_then(Value::as_str)
                        .is_some_and(|mime| mime.starts_with("image/")) =>
                {
                    images += 1;
                }
                Some("tool") => self.tool(&part, at),
                _ => {}
            }
        }
        if questions.len() > crate::turns::native::TOOL_MARK_LIMIT {
            self.turn_error.get_or_insert(SkipReason::UserTurnCapacity);
        }
        let text = text.join("\n");
        let kind = match role {
            Some("user") if !text.trim().is_empty() || images > 0 => LabelEventKind::Human,
            Some("assistant") if !text.trim().is_empty() => LabelEventKind::Assistant,
            _ => return (false, questions),
        };
        let human = kind == LabelEventKind::Human;
        let sender = human.then(|| crate::envelope_sender(&text)).flatten();
        self.events.push(LabelEvent {
            kind,
            at_unix_ms: at,
            text,
            offset,
            images: if human { images } else { 0 },
            sender,
        });
        (human, questions)
    }

    fn receipts(&mut self, body: &str, offset: u64) {
        for line in body.lines().map(str::trim) {
            if !line.starts_with(RECEIPT_MARKER) {
                continue;
            }
            if line.len() > RECEIPT_LINE_LIMIT_BYTES || self.receipts.len() >= RECEIPTS_PER_READ {
                self.skip("memory_receipt_capacity");
                continue;
            }
            self.receipts.push(MemoryReceiptPart {
                offset,
                text: line.to_owned(),
            });
        }
    }

    fn tool(&mut self, part: &Value, message_at: u64) {
        let Some(output) = part.pointer("/state/output").and_then(Value::as_str) else {
            return;
        };
        let at = part
            .pointer("/state/time/end")
            .and_then(Value::as_u64)
            .unwrap_or(message_at);
        for (repository, number) in pull_request_addresses(output) {
            if self.sightings.len() >= MAX_SIGHTINGS_PER_OUTPUT * 4 {
                return;
            }
            self.sightings.push(PrSighting {
                repository,
                number,
                at_unix_ms: at,
            });
        }
    }
}

/// A database refusal as a stable reason code: a lock OpenCode holds is a
/// read to skip, anything else a database Hide cannot read.
fn refusal(error: &rusqlite::Error) -> String {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            "opencode_db_busy".to_owned()
        }
        Some(rusqlite::ErrorCode::OperationInterrupted) => "opencode_db_work_capacity".to_owned(),
        _ => "opencode_db_unreadable".to_owned(),
    }
}
