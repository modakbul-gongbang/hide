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
//! [`TITLE_LIMIT_CHARS`] before it leaves SQLite.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;

use crate::label_transcript::{
    LabelEvent, LabelEventKind, LabelTranscript, LabelTranscriptRequest,
};
use crate::{
    ConfirmedLabelSession, ConversationCheckpoint, MAX_SIGHTINGS_PER_OUTPUT, PrSighting,
    SESSION_INCREMENT_READ_LIMIT_BYTES, SESSION_LINE_LIMIT_BYTES, label_reference_token,
    pull_request_addresses,
};

/// How long a read waits for OpenCode's own write to finish.
const BUSY_WAIT: Duration = Duration::from_millis(50);
/// The most messages one read takes; a longer session is read over several.
const MESSAGES_PER_READ: i64 = 200;
/// The most row data one read loads, as much as one JSONL increment.
const READ_BUDGET_BYTES: u64 = SESSION_INCREMENT_READ_LIMIT_BYTES;
/// A part row over this is skipped without being loaded.
const ROW_LIMIT_BYTES: i64 = SESSION_LINE_LIMIT_BYTES as i64;
/// A message row holds the message's metadata, never its text; one over
/// this is skipped without being loaded.
const MESSAGE_LIMIT_BYTES: i64 = 64 * 1024;
/// The most characters of a session title a read takes.
const TITLE_LIMIT_CHARS: i64 = 512;

/// A row's `data` column as one read takes it: the text, or why not.
enum RowData {
    Text(String),
    TooLarge,
    NotText,
}

/// Reads columns `is_text` and `data` of a row selected with
/// `typeof(data) = 'text'` and `CASE WHEN ... fits THEN data END`.
fn row_data(row: &rusqlite::Row<'_>, is_text: usize, data: usize) -> rusqlite::Result<RowData> {
    if !row.get::<_, bool>(is_text)? {
        return Ok(RowData::NotText);
    }
    Ok(match row.get::<_, Option<String>>(data)? {
        Some(text) => RowData::Text(text),
        None => RowData::TooLarge,
    })
}

pub(crate) fn database_path(home: &Path) -> PathBuf {
    home.join(".local/share/opencode/opencode.db")
}

pub(crate) fn read(
    home: &Path,
    request: &LabelTranscriptRequest,
) -> Result<LabelTranscript, String> {
    if request.reference_kind != "id" {
        return Err("session_kind_unsupported".to_owned());
    }
    let session_id = request.reference_value.as_str();
    let path = database_path(home);
    if !path.is_file() {
        return Err("session_file_missing".to_owned());
    }
    let connection = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| refusal(&error))?;
    connection
        .busy_timeout(BUSY_WAIT)
        .map_err(|error| refusal(&error))?;
    let (title, created) = connection
        .query_row(
            "SELECT substr(title, 1, ?2), time_created FROM session WHERE id = ?1",
            params![session_id, TITLE_LIMIT_CHARS],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|error| refusal(&error))?
        .ok_or_else(|| "session_file_missing".to_owned())?;
    let owner = label_reference_token("opencode", "id", session_id)
        .ok_or_else(|| "label_session_id_invalid".to_owned())?;
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM message WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(|error| refusal(&error))?;
    let count = u64::try_from(count).unwrap_or(0);
    let mut start = request
        .checkpoint
        .as_ref()
        .map_or(0, ConversationCheckpoint::offset);
    // Messages are only appended; fewer than were read means the session
    // was rewound (an OpenCode revert), and it is read from its start again.
    let rescanned = (start > count).then(|| "truncated".to_owned());
    if rescanned.is_some() {
        start = 0;
    }
    let mut statement = connection
        .prepare(
            "SELECT id, typeof(data) = 'text', \
             CASE WHEN typeof(data) = 'text' AND octet_length(data) <= ?4 THEN data END \
             FROM message WHERE session_id = ?1 \
             ORDER BY time_created, id LIMIT ?2 OFFSET ?3",
        )
        .map_err(|error| refusal(&error))?;
    let rows = statement
        .query_map(
            params![
                session_id,
                MESSAGES_PER_READ,
                start as i64,
                MESSAGE_LIMIT_BYTES
            ],
            |row| Ok((row.get::<_, String>(0)?, row_data(row, 1, 2)?)),
        )
        .map_err(|error| refusal(&error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| refusal(&error))?;
    let mut part_bytes = connection
        .prepare(
            "SELECT coalesce(sum(CASE WHEN typeof(data) = 'text' AND octet_length(data) <= ?2 \
             THEN octet_length(data) END), 0) FROM part WHERE message_id = ?1",
        )
        .map_err(|error| refusal(&error))?;
    let mut parts = connection
        .prepare(
            "SELECT typeof(data) = 'text', \
             CASE WHEN typeof(data) = 'text' AND octet_length(data) <= ?2 THEN data END \
             FROM part WHERE message_id = ?1 ORDER BY time_created, id",
        )
        .map_err(|error| refusal(&error))?;
    let mut transcript = Read {
        events: Vec::new(),
        sightings: Vec::new(),
        skipped: std::collections::BTreeMap::new(),
    };
    let mut next = start;
    let mut stopped_early = false;
    let mut over_budget = false;
    let mut spent = 0_u64;
    for (message_id, data) in &rows {
        let data = match data {
            RowData::Text(data) => data,
            RowData::TooLarge => {
                transcript.skip("message_capacity");
                next += 1;
                continue;
            }
            RowData::NotText => {
                transcript.skip("not_text");
                next += 1;
                continue;
            }
        };
        let Ok(message) = serde_json::from_str::<Value>(data) else {
            transcript.skip("malformed_json");
            next += 1;
            continue;
        };
        let role = message.get("role").and_then(Value::as_str);
        let completed = message.pointer("/time/completed").is_some();
        if role == Some("assistant") && !completed {
            stopped_early = true;
            break;
        }
        let at = message
            .pointer("/time/created")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let size: i64 = part_bytes
            .query_row(params![message_id, ROW_LIMIT_BYTES], |row| row.get(0))
            .map_err(|error| refusal(&error))?;
        let size = u64::try_from(size).unwrap_or(0);
        let size = size + data.len() as u64;
        // The first message of a read is always taken, so a read moves on
        // however large it is; its parts past the budget are skipped.
        if spent > 0 && spent + size > READ_BUDGET_BYTES {
            over_budget = true;
            break;
        }
        spent += data.len() as u64;
        let mut part_rows = Vec::new();
        for part in parts
            .query_map(params![message_id, ROW_LIMIT_BYTES], |row| {
                row_data(row, 0, 1)
            })
            .map_err(|error| refusal(&error))?
        {
            match part.map_err(|error| refusal(&error))? {
                RowData::Text(part) if spent + part.len() as u64 <= READ_BUDGET_BYTES => {
                    spent += part.len() as u64;
                    part_rows.push(part);
                }
                RowData::Text(_) => transcript.skip("read_budget"),
                RowData::TooLarge => transcript.skip("part_capacity"),
                RowData::NotText => transcript.skip("not_text"),
            }
        }
        transcript.message(role, at, next, &part_rows);
        next += 1;
    }
    let anchor = transcript
        .events
        .iter()
        .rev()
        .find(|event| event.kind == LabelEventKind::Human)
        .map(|event| ConversationCheckpoint::at_offset(event.offset));
    let has_more = over_budget || (!stopped_early && rows.len() as i64 == MESSAGES_PER_READ);
    let skipped_lines = transcript.skipped.values().sum();
    let skipped_reasons = transcript
        .skipped
        .into_iter()
        .map(|(reason, count)| (reason.to_owned(), count))
        .collect();
    Ok(LabelTranscript {
        confirmed: ConfirmedLabelSession {
            owner,
            incarnation: format!("opencode:{created}"),
            bytes: count,
        },
        events: transcript.events,
        checkpoint: ConversationCheckpoint::at_offset(next),
        anchor,
        has_more,
        rescanned,
        skipped_lines,
        skipped_reasons,
        title: Some(title).filter(|title| !title.trim().is_empty()),
        custom_title: None,
        pr_sightings: transcript.sightings,
        subagents: Default::default(),
    })
}

struct Read {
    events: Vec<LabelEvent>,
    sightings: Vec<PrSighting>,
    skipped: std::collections::BTreeMap<&'static str, usize>,
}

impl Read {
    fn skip(&mut self, reason: &'static str) {
        *self.skipped.entry(reason).or_default() += 1;
    }

    fn message(&mut self, role: Option<&str>, at: u64, offset: u64, parts: &[String]) {
        let mut text = Vec::new();
        let mut images = 0_u32;
        for part in parts {
            let Ok(part) = serde_json::from_str::<Value>(part) else {
                self.skip("malformed_json");
                continue;
            };
            match part.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let Some(body) = part.get("text").and_then(Value::as_str) else {
                        continue;
                    };
                    // OpenCode's own text (a compaction's request) is no
                    // one's message.
                    if part.get("synthetic").and_then(Value::as_bool) == Some(true) {
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
        let text = text.join("\n");
        let kind = match role {
            Some("user") if !text.trim().is_empty() || images > 0 => LabelEventKind::Human,
            Some("assistant") if !text.trim().is_empty() => LabelEventKind::Assistant,
            _ => return,
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
        _ => "opencode_db_unreadable".to_owned(),
    }
}
