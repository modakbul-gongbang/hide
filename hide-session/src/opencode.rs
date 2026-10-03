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

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;

use crate::label_transcript::{
    LabelEvent, LabelEventKind, LabelTranscript, LabelTranscriptRequest,
};
use crate::{
    ConfirmedLabelSession, ConversationCheckpoint, MAX_SIGHTINGS_PER_OUTPUT, PrSighting,
    SESSION_LINE_LIMIT_BYTES, label_reference_token, pull_request_addresses,
};

/// How long a read waits for OpenCode's own write to finish.
const BUSY_WAIT: Duration = Duration::from_millis(50);
/// The most messages one read takes; a longer session is read over several.
const MESSAGES_PER_READ: i64 = 200;

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
            "SELECT title, time_created FROM session WHERE id = ?1",
            params![session_id],
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
            "SELECT id, data FROM message WHERE session_id = ?1 \
             ORDER BY time_created, id LIMIT ?2 OFFSET ?3",
        )
        .map_err(|error| refusal(&error))?;
    let rows = statement
        .query_map(
            params![session_id, MESSAGES_PER_READ, start as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|error| refusal(&error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| refusal(&error))?;
    let mut parts = connection
        .prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY time_created, id")
        .map_err(|error| refusal(&error))?;
    let mut transcript = Read {
        events: Vec::new(),
        sightings: Vec::new(),
        skipped: 0,
    };
    let mut next = start;
    let mut stopped_early = false;
    for (message_id, data) in &rows {
        let Ok(message) = serde_json::from_str::<Value>(data) else {
            transcript.skipped += 1;
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
        let part_rows = parts
            .query_map(params![message_id], |row| row.get::<_, String>(0))
            .map_err(|error| refusal(&error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| refusal(&error))?;
        transcript.message(role, at, next, &part_rows);
        next += 1;
    }
    let anchor = transcript
        .events
        .iter()
        .rev()
        .find(|event| event.kind == LabelEventKind::Human)
        .map(|event| ConversationCheckpoint::at_offset(event.offset));
    let has_more = !stopped_early && rows.len() as i64 == MESSAGES_PER_READ;
    let mut skipped_reasons = std::collections::BTreeMap::new();
    if transcript.skipped > 0 {
        skipped_reasons.insert("malformed_json".to_owned(), transcript.skipped);
    }
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
        skipped_lines: transcript.skipped,
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
    skipped: usize,
}

impl Read {
    fn message(&mut self, role: Option<&str>, at: u64, offset: u64, parts: &[String]) {
        let mut text = Vec::new();
        let mut images = 0_u32;
        for part in parts {
            let Ok(part) = serde_json::from_str::<Value>(part) else {
                self.skipped += 1;
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
                    if body.len() > SESSION_LINE_LIMIT_BYTES {
                        self.skipped += 1;
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
