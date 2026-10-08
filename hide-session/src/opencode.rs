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
/// The most characters of a session title a read takes.
const TITLE_LIMIT_CHARS: i64 = 512;

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
    if !crate::label_owner::valid_native_id(session_id) {
        return Err("label_session_id_invalid".to_owned());
    }
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
    let (title, created) = connection
        .query_row(
            "SELECT CASE WHEN typeof(title) = 'text' AND octet_length(title) <= ?2 * 4 \
             THEN substr(title, 1, ?2) END, time_created FROM session WHERE id = ?1",
            params![session_id, TITLE_LIMIT_CHARS],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)),
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
        skipped: std::collections::BTreeMap::new(),
    };
    let mut next = start;
    let mut stopped_early = false;
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
            next += 1;
            continue;
        }
        let data_bytes: u64 = row.get(2).map_err(|error| refusal(&error))?;
        let id_bytes: u64 = row.get(3).map_err(|error| refusal(&error))?;
        let metadata_bytes = data_bytes.saturating_add(id_bytes);
        if data_bytes > MESSAGE_LIMIT_BYTES as u64 || id_bytes > MESSAGE_LIMIT_BYTES as u64 {
            transcript.skip("message_capacity");
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
        transcript.message(role, at, next, &part_rows);
        next += 1;
    }
    let anchor = transcript
        .events
        .iter()
        .rev()
        .find(|event| event.kind == LabelEventKind::Human)
        .map(|event| ConversationCheckpoint::at_offset(event.offset));
    let has_more = over_budget || (!stopped_early && visited == MESSAGES_PER_READ);
    let skipped_lines = transcript.skipped.values().sum();
    let skipped_reasons = transcript
        .skipped
        .into_iter()
        .map(|(reason, count)| (reason.to_owned(), count))
        .collect();
    Ok(LabelTranscript {
        confirmed: ConfirmedLabelSession {
            owner,
            native_session_id: Some(request.reference_value.clone()),
            source_path: None,
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
        title: title.filter(|title| !title.trim().is_empty()),
        custom_title: None,
        pr_sightings: transcript.sightings,
        subagents: Default::default(),
        turns: None,
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
        Some(rusqlite::ErrorCode::OperationInterrupted) => "opencode_db_work_capacity".to_owned(),
        _ => "opencode_db_unreadable".to_owned(),
    }
}
