//! One bounded, resumable read of a pane's conversation for its label.
//!
//! The same function answers for this machine (the core's in-process host)
//! and for a device (`hide-host-helper`), so the two cannot drift in what
//! they read, prove or refuse. It holds no state: the caller keeps the
//! checkpoint and hands it back on the next read. A read locates the file
//! Herdr's reference names, proves the provider's native owner before and
//! after the bytes are read, and returns only conversation events, never
//! injected scaffolding or a path. Failures are stable reason codes.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    Agent, ConfirmedLabelSession, ConversationCheckpoint, ConversationCursor, EventKind,
    SessionError, SessionIdentity, SessionLocator, confirm_label_session,
};

/// What the caller knows about the pane's conversation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LabelTranscriptRequest {
    pub agent: Agent,
    /// Herdr's `agent_session.kind`: `id` or `path`.
    pub reference_kind: String,
    pub reference_value: String,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Where the previous read stopped; `None` reads from the start.
    #[serde(default)]
    pub checkpoint: Option<ConversationCheckpoint>,
}

/// A conversation event as the label analysis reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelEvent {
    pub kind: LabelEventKind,
    pub at_unix_ms: u64,
    pub text: String,
    /// Byte offset of the record that produced it.
    pub offset: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelEventKind {
    Human,
    Assistant,
    Interrupted,
}

impl LabelEventKind {
    /// The speaker as the analysis context names it.
    pub const fn role(self) -> &'static str {
        match self {
            Self::Human | Self::Interrupted => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LabelTranscript {
    /// The proven owner, file incarnation and size after the read.
    pub confirmed: ConfirmedLabelSession,
    pub events: Vec<LabelEvent>,
    /// Resumes after the last complete record read.
    pub checkpoint: ConversationCheckpoint,
    /// Resumes at the last human record this read returned, so a reader that
    /// lost its events (a restart) can recover the current turn; `None` when
    /// this read returned no human record.
    pub anchor: Option<ConversationCheckpoint>,
    /// More bytes than one read's budget remain.
    pub has_more: bool,
    /// The read started over because the file was replaced or truncated.
    pub rescanned: Option<String>,
    pub skipped_lines: usize,
    pub skipped_reasons: BTreeMap<String, usize>,
}

/// The request's file, its proven owner, and the next bounded chunk.
///
/// The owner is proven before and after the read; a file replaced or
/// shrunk in between is refused rather than analyzed across the change.
/// An error is a stable reason code with no path or content in it.
pub fn read(home: &Path, request: &LabelTranscriptRequest) -> Result<LabelTranscript, String> {
    let identity = match request.reference_kind.as_str() {
        "id" => SessionIdentity::id(&request.reference_value),
        "path" => SessionIdentity::path(&request.reference_value),
        _ => return Err("session_kind_unsupported".to_owned()),
    };
    if request.reference_value.trim().is_empty() {
        return Err("label_session_reference_missing".to_owned());
    }
    let path = SessionLocator::new(home)
        .locate(
            "label",
            request.agent,
            Some(&identity),
            request.cwd.as_deref(),
        )
        .map_err(|error| match error {
            SessionError::SessionFileMissing => "session_file_missing".to_owned(),
            _ => "label_session_location_unavailable".to_owned(),
        })?;
    let reported_id = (request.reference_kind == "id").then_some(request.reference_value.as_str());
    let before = confirm_label_session(request.agent, &path, reported_id)
        .map_err(|error| error.to_string())?;
    let mut cursor = request
        .checkpoint
        .clone()
        .map(ConversationCursor::restore)
        .unwrap_or_default();
    let parsed = cursor
        .read(request.agent, &path)
        .map_err(|error| match error {
            SessionError::Capacity { resource, limit } => {
                format!("session_capacity:{resource}:{limit}")
            }
            SessionError::SessionFileMissing => "session_file_missing".to_owned(),
            _ => "label_session_read_failed".to_owned(),
        })?;
    let after = confirm_label_session(request.agent, &path, reported_id)
        .map_err(|error| error.to_string())?;
    if after.owner != before.owner
        || after.incarnation != before.incarnation
        || after.bytes < before.bytes
    {
        return Err("label_session_read_changed".to_owned());
    }
    let mut events = Vec::with_capacity(parsed.events.len());
    for (event, offset) in parsed.events.iter().zip(&parsed.event_offsets) {
        let kind = match event.kind {
            EventKind::Human => LabelEventKind::Human,
            EventKind::Assistant => LabelEventKind::Assistant,
            EventKind::Interrupted => LabelEventKind::Interrupted,
            EventKind::Injected => continue,
        };
        events.push(LabelEvent {
            kind,
            at_unix_ms: event.at_unix_ms,
            text: event.text.clone(),
            offset: *offset,
        });
    }
    let anchor = events
        .iter()
        .rev()
        .find(|event| event.kind == LabelEventKind::Human)
        .map(|event| cursor.checkpoint_at(event.offset));
    Ok(LabelTranscript {
        confirmed: after,
        events,
        checkpoint: cursor.checkpoint(),
        anchor,
        has_more: cursor.has_more(),
        rescanned: parsed
            .rescan_reason
            .map(|reason| reason.as_str().to_owned()),
        skipped_lines: parsed.skipped_lines,
        skipped_reasons: parsed
            .skipped_reasons
            .iter()
            .map(|(reason, count)| (reason.as_str().to_owned(), *count))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn claude_line(kind: &str, session: &str, text: &str, at: &str) -> String {
        let record = if kind == "user" {
            serde_json::json!({"type":"user","sessionId":session,"timestamp":at,
                "origin":{"kind":"human"},"message":{"role":"user","content":text}})
        } else {
            serde_json::json!({"type":"assistant","sessionId":session,"timestamp":at,
                "message":{"role":"assistant","content":[{"type":"text","text":text}]}})
        };
        format!("{record}\n")
    }

    fn request(path: &Path, checkpoint: Option<ConversationCheckpoint>) -> LabelTranscriptRequest {
        LabelTranscriptRequest {
            agent: Agent::Claude,
            reference_kind: "path".to_owned(),
            reference_value: path.display().to_string(),
            cwd: None,
            checkpoint,
        }
    }

    #[test]
    fn a_resumed_read_returns_only_appended_conversation_and_an_anchor_at_the_last_human() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("session.jsonl");
        fs::write(
            &path,
            claude_line("user", "s1", "first request", "2026-10-01T00:00:00Z")
                + &claude_line("assistant", "s1", "on it", "2026-10-01T00:00:01Z"),
        )
        .unwrap();
        let first = read(root.path(), &request(&path, None)).unwrap();
        assert_eq!(
            first.events.iter().map(|e| e.kind).collect::<Vec<_>>(),
            [LabelEventKind::Human, LabelEventKind::Assistant]
        );
        assert!(first.anchor.is_some());

        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(claude_line("assistant", "s1", "done", "2026-10-01T00:00:02Z").as_bytes())
            .unwrap();
        let second = read(root.path(), &request(&path, Some(first.checkpoint.clone()))).unwrap();
        assert_eq!(second.events.len(), 1);
        assert_eq!(second.events[0].text, "done");
        assert!(second.anchor.is_none());

        // Resuming at the anchor recovers the turn from its human record.
        let resumed = read(root.path(), &request(&path, first.anchor.clone())).unwrap();
        assert_eq!(resumed.events[0].text, "first request");
        assert_eq!(resumed.events.len(), 3);
    }

    #[test]
    fn a_reported_id_the_file_does_not_carry_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("session.jsonl");
        fs::write(
            &path,
            claude_line("user", "native-a", "request", "2026-10-01T00:00:00Z"),
        )
        .unwrap();
        let mut request = request(&path, None);
        let proven = read(root.path(), &request).unwrap();
        assert_eq!(
            Some(proven.confirmed.owner),
            crate::label_reference_token("claude", "id", "native-a")
        );
        request.reference_kind = "id".to_owned();
        request.reference_value = "native-b".to_owned();
        assert!(read(root.path(), &request).is_err());
    }

    #[test]
    fn an_oversized_tool_result_is_skipped_and_an_oversized_sentence_is_a_capacity_failure() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("session.jsonl");
        let huge = "x".repeat(crate::SESSION_LINE_LIMIT_BYTES + 10);
        let tool = serde_json::json!({"type":"user","sessionId":"s1","timestamp":"2026-10-01T00:00:01Z",
            "message":{"role":"user","content":[{"type":"tool_result","content":huge}]}});
        fs::write(
            &path,
            claude_line("user", "s1", "request", "2026-10-01T00:00:00Z")
                + &format!("{tool}\n")
                + &claude_line("assistant", "s1", "after the tool", "2026-10-01T00:00:02Z"),
        )
        .unwrap();
        let answer = read(root.path(), &request(&path, None)).unwrap();
        assert_eq!(
            answer.skipped_reasons.get("non_conversation_capacity"),
            Some(&1)
        );
        assert_eq!(answer.events.last().unwrap().text, "after the tool");

        let sentence = root.path().join("sentence.jsonl");
        fs::write(
            &sentence,
            claude_line("user", "s1", "request", "2026-10-01T00:00:00Z")
                + &claude_line("assistant", "s1", &huge, "2026-10-01T00:00:01Z"),
        )
        .unwrap();
        let error = read(root.path(), &request(&sentence, None)).unwrap_err();
        assert!(error.starts_with("session_capacity:line_bytes"), "{error}");
    }

    #[test]
    fn an_unsupported_reference_kind_reads_nothing() {
        let root = tempfile::tempdir().unwrap();
        let mut request = request(&root.path().join("missing.jsonl"), None);
        request.reference_kind = "pid".to_owned();
        assert_eq!(
            read(root.path(), &request).unwrap_err(),
            "session_kind_unsupported"
        );
    }
}
