use crate::turns::native::TOOL_MARK_LIMIT;
use crate::turns::{NATIVE_ID_LIMIT_BYTES, ToolTurnMark, TurnMark, TurnMode};
use crate::{
    Agent, AppendedBytes, ParsedSession, Result, SESSION_LINE_LIMIT_BYTES, SessionCursor,
    SessionError, SkipReason, parse_events_into,
};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Search-only durable progress, including a skipped oversized tool record.
/// No transcript bytes or Memory cursor state are retained.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConversationCheckpoint {
    cursor: crate::CursorCheckpoint,
    discarded_bytes: u64,
    has_more: bool,
    /// Boxed: a scan exists only inside an oversized record, and every
    /// label request carries a checkpoint.
    #[serde(default)]
    classifier: Option<Box<LargeRecord>>,
}

impl ConversationCheckpoint {
    /// A position that is a count rather than a file offset: OpenCode's
    /// messages read so far (`opencode`).
    pub(crate) fn at_offset(offset: u64) -> Self {
        Self {
            cursor: crate::CursorCheckpoint {
                offset,
                ..crate::CursorCheckpoint::default()
            },
            ..Self::default()
        }
    }

    pub fn offset(&self) -> u64 {
        self.cursor.offset
    }
    pub fn has_more(&self) -> bool {
        self.has_more
    }
}

/// Incrementally reads conversation events without retaining unrelated records.
#[derive(Debug, Default)]
pub struct ConversationCursor {
    cursor: SessionCursor,
    discarded_bytes: u64,
    has_more: bool,
    classifier: Option<Box<LargeRecord>>,
    read_bytes: u64,
}

impl ConversationCursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Search owns a separate durable cursor; no Memory checkpoint is shared.
    pub fn checkpoint(&self) -> ConversationCheckpoint {
        ConversationCheckpoint {
            cursor: self.cursor.checkpoint(),
            discarded_bytes: self.discarded_bytes,
            has_more: self.has_more,
            classifier: self.classifier.clone(),
        }
    }

    /// A checkpoint of this file that resumes at `offset`, an earlier record
    /// boundary this cursor already read past (an event offset). Reading
    /// from it again yields the records from there on; nothing beyond the
    /// file identity is carried, so no transcript bytes are kept.
    pub fn checkpoint_at(&self, offset: u64) -> ConversationCheckpoint {
        let mut cursor = self.cursor.checkpoint();
        cursor.offset = offset.min(cursor.offset);
        ConversationCheckpoint {
            cursor,
            discarded_bytes: 0,
            has_more: false,
            classifier: None,
        }
    }

    pub fn restore(checkpoint: ConversationCheckpoint) -> Self {
        let mut cursor = SessionCursor::restore(checkpoint.cursor);
        let (discarded_bytes, classifier) = match checkpoint.classifier {
            // A scan an older build checkpointed kept no native ids, so it
            // cannot say which call the record answered. Reread that record
            // from its start rather than refuse it on every later read.
            Some(scan) if !scan.native_ids => {
                cursor.offset = cursor.offset.saturating_sub(checkpoint.discarded_bytes);
                (0, None)
            }
            classifier => (checkpoint.discarded_bytes, classifier),
        };
        Self {
            cursor,
            discarded_bytes,
            has_more: false,
            classifier,
            read_bytes: 0,
        }
    }

    pub fn reset(&mut self) {
        self.cursor.reset();
        self.discarded_bytes = 0;
        self.has_more = false;
        self.classifier = None;
    }

    /// More bytes from the last observed file size remain to be read.
    /// A torn final record at EOF waits for an append instead of spinning.
    pub fn has_more(&self) -> bool {
        self.has_more
    }

    pub(crate) fn read_bytes(&self) -> u64 {
        self.read_bytes
    }

    pub fn read(&mut self, agent: Agent, path: &Path) -> Result<ParsedSession> {
        self.read_with_budget(agent, path, crate::SESSION_INCREMENT_READ_LIMIT_BYTES)
    }

    /// A shared poll gives each file only its remaining byte allowance.
    pub(crate) fn read_with_budget(
        &mut self,
        agent: Agent,
        path: &Path,
        budget: u64,
    ) -> Result<ParsedSession> {
        self.read_bytes = 0;
        let file = crate::open_session_file(path)
            .map_err(|error| SessionError::io("open", path, error))?;
        self.read_file_with_budget(agent, path, &file, budget)
    }

    /// Search uses one nonblocking, regular-file descriptor for all reads.
    pub(crate) fn read_file(
        &mut self,
        agent: Agent,
        path: &Path,
        file: &File,
    ) -> Result<ParsedSession> {
        self.read_file_with_budget(agent, path, file, crate::SESSION_INCREMENT_READ_LIMIT_BYTES)
    }

    fn read_file_with_budget(
        &mut self,
        agent: Agent,
        path: &Path,
        file: &File,
        budget: u64,
    ) -> Result<ParsedSession> {
        self.has_more = false;
        self.read_bytes = 0;
        let mut plan_hold = None;
        let title = match agent {
            Agent::Omp => Some(read_current_title(
                path,
                file,
                budget,
                &mut self.read_bytes,
            )?),
            Agent::Grok => {
                let (title, hold) = read_grok_state(path, budget, &mut self.read_bytes)?;
                plan_hold = hold;
                Some(title)
            }
            _ => None,
        };
        let remaining = budget.saturating_sub(self.read_bytes);
        let mut appended_read_bytes = 0;
        let appended_result = read_appended_file(
            &mut self.cursor,
            path,
            file,
            remaining,
            &mut appended_read_bytes,
        );
        self.read_bytes += appended_read_bytes;
        let AppendedBytes {
            contents: appended,
            start_offset,
            identity,
            rescan_reason,
            has_more,
        } = appended_result?;
        if rescan_reason.is_some() {
            self.discarded_bytes = 0;
            self.classifier = None;
        }
        let mut offset = start_offset;
        let mut pending = self.cursor.pending.clone();
        let mut discarded_bytes = self.discarded_bytes;
        let mut classifier = self.classifier.clone();
        let mut parsed = ParsedSession {
            rescan_reason,
            ..ParsedSession::default()
        };
        let mut found = crate::links::LinkAccumulator::default();
        for fragment in appended.split_inclusive(|byte| *byte == b'\n') {
            let line_start = offset.saturating_sub(pending.len() as u64 + discarded_bytes);
            offset += fragment.len() as u64;
            let complete = fragment.last() == Some(&b'\n');
            if discarded_bytes > 0 {
                discarded_bytes += fragment.len() as u64;
                if let Some(classifier) = classifier.as_mut() {
                    classifier.feed(fragment)?;
                }
            } else {
                let retained = fragment.len().min(SESSION_LINE_LIMIT_BYTES - pending.len());
                pending.extend_from_slice(&fragment[..retained]);
                if retained < fragment.len() {
                    // Continue a bounded structural scan across chunks. Only
                    // JSON discriminators survive a checkpoint, never bodies.
                    let mut scan = LargeRecord {
                        native_ids: true,
                        grok: (agent == Agent::Grok).then(Default::default),
                        ..LargeRecord::default()
                    };
                    scan.feed(&pending)?;
                    scan.feed(&fragment[retained..])?;
                    classifier = Some(Box::new(scan));
                    discarded_bytes = pending.len() as u64 + (fragment.len() - retained) as u64;
                    pending.clear();
                }
            }
            // A torn JSON envelope may resume until its provider structure
            // is known. A non-JSON prefix cannot become a discardable tool
            // record by appending bytes and fails at the existing line cap.
            if discarded_bytes > 0
                && classifier
                    .as_ref()
                    .is_some_and(|scan| scan.unclassifiable_prefix())
            {
                return Err(SessionError::Capacity {
                    resource: "line_bytes",
                    limit: SESSION_LINE_LIMIT_BYTES as u64,
                });
            }
            if !complete {
                continue;
            }
            if discarded_bytes > 0 {
                let scan = classifier.take();
                discarded_bytes = 0;
                if agent == Agent::Grok {
                    // The record still counts, without its bodies.
                    let Some(line) = scan
                        .and_then(|scan| scan.grok)
                        .and_then(|grok| grok.reduced())
                    else {
                        return Err(SessionError::Capacity {
                            resource: "line_bytes",
                            limit: SESSION_LINE_LIMIT_BYTES as u64,
                        });
                    };
                    parsed.skipped(SkipReason::BodyCapacity);
                    pending = line.into_bytes();
                } else {
                    let Some(turn) = scan.and_then(|scan| (*scan).discard(agent)) else {
                        return Err(SessionError::Capacity {
                            resource: "line_bytes",
                            limit: SESSION_LINE_LIMIT_BYTES as u64,
                        });
                    };
                    parsed.skipped(SkipReason::NonConversationCapacity);
                    match turn {
                        Ok(Some(mark)) => parsed.turn_marks.push((line_start, mark)),
                        Ok(None) => {}
                        Err(reason) => parsed.skipped(reason),
                    }
                    continue;
                }
            }
            let line = parse_events_into(
                agent,
                &String::from_utf8_lossy(&pending),
                line_start,
                &mut found,
            );
            parsed.events.extend(line.events);
            parsed.event_offsets.extend(line.event_offsets);
            parsed.skipped_lines += line.skipped_lines;
            for (reason, count) in line.skipped_reasons {
                *parsed.skipped_reasons.entry(reason).or_default() += count;
            }
            if line.title.is_some() {
                parsed.title = line.title;
            }
            if line.custom_title.is_some() {
                parsed.custom_title = line.custom_title;
            }
            parsed.pr_sightings.extend(line.pr_sightings);
            parsed.turn_marks.extend(line.turn_marks);
            pending.clear();
        }
        parsed.links = found.finish();
        parsed.coalesce();
        parsed.plan_hold = plan_hold;
        if let Some((title, custom_title)) = title {
            parsed.title = Some(title);
            parsed.custom_title = Some(custom_title);
        }
        // Commit only a successful poll: a relevant capacity failure cannot
        // silently consume a Human turn or publish a partial result.
        self.cursor.offset = offset;
        self.cursor.identity = Some(identity);
        self.cursor.pending = pending;
        self.discarded_bytes = discarded_bytes;
        self.classifier = classifier;
        self.has_more = has_more;
        Ok(parsed)
    }
}

/// Grok's current title and plan wait live beside its conversation, each
/// replaced whole by Grok; every byte read is charged to this poll.
fn read_grok_state(
    path: &Path,
    budget: u64,
    read_bytes: &mut u64,
) -> Result<((String, String), Option<crate::turns::UserTurnContent>)> {
    let refused = |error: anyhow::Error| SessionError::Checkpoint(error.to_string());
    let summary = crate::grok::summary(path, read_bytes).map_err(refused)?;
    let hold = crate::grok::plan_hold(path, read_bytes).map_err(refused)?;
    if *read_bytes > budget.min(crate::SESSION_INCREMENT_READ_LIMIT_BYTES) {
        return Err(SessionError::Capacity {
            resource: "title_read_bytes",
            limit: budget,
        });
    }
    Ok((summary.title, hold))
}

/// Refresh the physical first line through the same descriptor without
/// changing event offsets. Every byte read, including a block's tail after
/// the newline, is charged against this poll's existing allowance.
fn read_current_title(
    path: &Path,
    file: &File,
    budget: u64,
    read_bytes: &mut u64,
) -> Result<(String, String)> {
    let mut file = file
        .try_clone()
        .map_err(|e| SessionError::io("clone", path, e))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| SessionError::io("seek", path, e))?;
    let mut prefix = Vec::new();
    loop {
        let allowance = budget
            .min(crate::SESSION_INCREMENT_READ_LIMIT_BYTES)
            .saturating_sub(*read_bytes);
        if allowance == 0 {
            return Err(SessionError::Capacity {
                resource: "title_read_bytes",
                limit: budget,
            });
        }
        let mut block = [0_u8; 256];
        let limit = block
            .len()
            .min(allowance as usize)
            .min(SESSION_LINE_LIMIT_BYTES + 1 - prefix.len());
        let n = file
            .read(&mut block[..limit])
            .map_err(|e| SessionError::io("read", path, e))?;
        *read_bytes += n as u64;
        let newline = block[..n]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| prefix.len() + index);
        prefix.extend_from_slice(&block[..n]);
        if let Some(end) = newline {
            if end + 1 > SESSION_LINE_LIMIT_BYTES {
                return Err(SessionError::Capacity {
                    resource: "line_bytes",
                    limit: SESSION_LINE_LIMIT_BYTES as u64,
                });
            }
            let value = serde_json::from_slice(&prefix[..end]).map_err(|_| {
                SessionError::Checkpoint("label_session_metadata_unconfirmed".to_owned())
            })?;
            return crate::native_file::title_snapshot(&value).ok_or_else(|| {
                SessionError::Checkpoint("label_session_metadata_unconfirmed".to_owned())
            });
        }
        if prefix.len() > SESSION_LINE_LIMIT_BYTES {
            return Err(SessionError::Capacity {
                resource: "line_bytes",
                limit: SESSION_LINE_LIMIT_BYTES as u64,
            });
        }
        if n == 0 {
            return Err(SessionError::Checkpoint(
                "label_session_metadata_unconfirmed".to_owned(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_title_prefix_spends_the_poll_budget_without_advancing_the_checkpoint() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("session.jsonl");
        let header = serde_json::json!({"type":"session","version":3,"title":"현재 제목","titleSource":"user","pad":"x".repeat(400)});
        std::fs::write(&path, format!("{header}\n")).unwrap();
        let mut cursor = ConversationCursor::new();
        let before = cursor.checkpoint();
        assert!(matches!(
            cursor.read_with_budget(Agent::Omp, &path, 256),
            Err(SessionError::Capacity {
                resource: "title_read_bytes",
                limit: 256
            })
        ));
        assert_eq!(cursor.read_bytes(), 256);
        assert_eq!(cursor.checkpoint(), before);
        let current = cursor.read(Agent::Omp, &path).unwrap();
        assert_eq!(current.custom_title.as_deref(), Some("현재 제목"));
        assert!(current.events.is_empty());
        assert!(cursor.read_bytes() <= crate::SESSION_INCREMENT_READ_LIMIT_BYTES);
        assert_eq!(
            cursor.checkpoint().offset(),
            std::fs::metadata(path).unwrap().len()
        );
    }
}

// Keep Memory's cursor implementation untouched; search alone accepts an
// already-open descriptor so path replacement cannot race a blocking open.
fn read_appended_file(
    cursor: &mut SessionCursor,
    path: &Path,
    file: &File,
    budget: u64,
    read_bytes: &mut u64,
) -> Result<AppendedBytes> {
    let mut file = file
        .try_clone()
        .map_err(|e| SessionError::io("clone", path, e))?;
    let metadata = file
        .metadata()
        .map_err(|e| SessionError::io("stat", path, e))?;
    let identity = crate::FileIdentity::from_metadata(&metadata);
    let rescan_reason = match cursor.identity {
        Some(old) if old != identity => Some(crate::RescanReason::Replaced),
        Some(_) if metadata.len() < cursor.offset => Some(crate::RescanReason::Truncated),
        _ => None,
    };
    if cursor.identity.is_none() || rescan_reason.is_some() {
        cursor.offset = 0;
        cursor.pending.clear();
    }
    file.seek(SeekFrom::Start(cursor.offset))
        .map_err(|e| SessionError::io("seek", path, e))?;
    let start = cursor.offset;
    let mut contents = Vec::new();
    let read = file
        .take(budget.min(crate::SESSION_INCREMENT_READ_LIMIT_BYTES))
        .read_to_end(&mut contents);
    // Failed parsing or a partial I/O failure still spent this poll's bytes.
    *read_bytes = contents.len() as u64;
    read.map_err(|e| SessionError::io("read", path, e))?;
    Ok(AppendedBytes {
        has_more: start + (contents.len() as u64) < metadata.len(),
        contents,
        start_offset: start,
        identity,
        rescan_reason,
    })
}

// A bounded lexical/structural scan of an oversized provider envelope.
// Key order and tool-output contents cannot decide whether a record is text.
// The normal serde parser remains responsible for retained conversation JSON.
// Only discriminators and the bounded native ids a turn mark needs are kept,
// so a discarded record still answers or asks its call.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct LargeRecord {
    frames: Vec<JsonFrame>,
    token: Option<JsonString>,
    root_kind: String,
    payload_kind: String,
    content_array: bool,
    conversation: bool,
    invalid: bool,
    /// Scanned by a build that keeps native ids. A checkpoint without it is
    /// reread from the record's start (`ConversationCursor::restore`).
    #[serde(default)]
    native_ids: bool,
    /// Claude native tool blocks, in record order, bounded like a parsed
    /// record's marks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tools: Vec<LargeTool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    tool_capacity: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    payload_question: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload_call: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload_turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload_mode: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    plan_item: bool,
    /// Grok's oversized records are read without their bodies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grok: Option<Box<crate::grok::LargeLine>>,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum LargeTool {
    /// An `AskUserQuestion` call, with the id its block named.
    Question(Option<String>),
    /// A tool result naming the call it answers.
    Result(String),
}
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq)]
enum Scope {
    #[default]
    Root,
    Payload,
    Message,
    Content,
    Block,
    Other,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct JsonFrame {
    scope: Scope,
    object: bool,
    key: String,
    expecting_key: bool,
    block_kind: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    block_use: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    block_question: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    block_result: bool,
    /// A block's `id` or `tool_use_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    call: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    plan_item: bool,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct JsonString {
    key: bool,
    scope: Scope,
    field: String,
    value: String,
    escaped: bool,
}
type Discard = std::result::Result<Option<TurnMark>, SkipReason>;
/// The string values a turn mark needs from a discarded record.
fn native_id(scope: Scope, field: &str) -> bool {
    matches!(
        (scope, field),
        (Scope::Block, "id" | "tool_use_id")
            | (
                Scope::Payload,
                "call_id" | "turn_id" | "collaboration_mode_kind"
            )
    )
}
/// A kept id is cut one byte past the native limit, so an id too long for a
/// parsed record is too long here as well.
fn bounded(id: Option<String>) -> std::result::Result<Option<String>, SkipReason> {
    match id.filter(|id| !id.is_empty()) {
        Some(id) if id.len() > NATIVE_ID_LIMIT_BYTES => Err(SkipReason::UserTurnCapacity),
        id => Ok(id),
    }
}
impl LargeRecord {
    fn value_scope(&self) -> Scope {
        match self.frames.last() {
            None => Scope::Root,
            Some(f) if !f.object && f.scope == Scope::Content => Scope::Block,
            Some(f) => match (f.scope, f.key.as_str()) {
                (Scope::Root, "payload") => Scope::Payload,
                (Scope::Root, "message") => Scope::Message,
                (Scope::Message, "content") => Scope::Content,
                _ => Scope::Other,
            },
        }
    }
    fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        if let Some(grok) = self.grok.as_mut() {
            // Grok's scan alone decides a Grok record.
            return grok.feed(bytes);
        }
        for &byte in bytes {
            if let Some(token) = self.token.as_mut() {
                let id = !token.key && native_id(token.scope, &token.field);
                if token.escaped {
                    // Escaped discriminator spellings stay unknown, never
                    // incorrectly authorizing a discarded conversation.
                    if token.value.len() < 64 {
                        token.value.push('\\');
                    }
                    token.escaped = false;
                } else if byte == b'\\' {
                    self.invalid |= token.key
                        || id
                        || token.field == "type"
                        || (token.field == "name"
                            && matches!(token.scope, Scope::Payload | Scope::Block));
                    token.escaped = true;
                } else if byte == b'"' {
                    let token = self.token.take().unwrap();
                    if token.key {
                        if let Some(frame) = self.frames.last_mut() {
                            frame.key = token.value;
                            frame.expecting_key = false;
                        }
                    } else if id {
                        match token.scope {
                            Scope::Block => {
                                if let Some(frame) = self.frames.last_mut() {
                                    frame.call = Some(token.value);
                                }
                            }
                            _ => match token.field.as_str() {
                                "call_id" => self.payload_call = Some(token.value),
                                "turn_id" => self.payload_turn = Some(token.value),
                                _ => self.payload_mode = Some(token.value),
                            },
                        }
                    } else if token.field == "type" {
                        match token.scope {
                            Scope::Root => self.root_kind = token.value,
                            Scope::Payload => self.payload_kind = token.value,
                            Scope::Block => {
                                let tool = matches!(
                                    token.value.as_str(),
                                    "tool_result" | "tool_use" | "thinking" | "redacted_thinking"
                                );
                                self.conversation |= !tool;
                                if let Some(frame) = self.frames.last_mut() {
                                    frame.block_kind = tool;
                                    frame.block_use = token.value == "tool_use";
                                    frame.block_result = token.value == "tool_result";
                                }
                            }
                            _ => {
                                if self.frames.last().is_some_and(|frame| frame.plan_item) {
                                    self.plan_item = token.value == "Plan";
                                }
                            }
                        }
                    } else if token.field == "name" {
                        match token.scope {
                            Scope::Payload => {
                                self.payload_question = token.value == "request_user_input"
                            }
                            Scope::Block => {
                                if let Some(frame) = self.frames.last_mut() {
                                    frame.block_question = token.value == "AskUserQuestion";
                                }
                            }
                            _ => (),
                        }
                    }
                } else if id {
                    // Native ids are ASCII; any other byte is not certified.
                    self.invalid |= !byte.is_ascii();
                    if token.value.len() <= NATIVE_ID_LIMIT_BYTES {
                        token.value.push(byte as char);
                    }
                } else if token.value.len() < 64
                    && (token.key || matches!(token.field.as_str(), "type" | "name"))
                {
                    token.value.push(byte as char);
                }
                continue;
            }
            match byte {
                b'"' => {
                    let frame = self.frames.last();
                    let key = frame.is_some_and(|f| f.object && f.expecting_key);
                    let scope = frame.map(|f| f.scope).unwrap_or_default();
                    let field = frame.map(|f| f.key.clone()).unwrap_or_default();
                    if !key && self.value_scope() == Scope::Content {
                        self.conversation = true;
                    }
                    self.token = Some(JsonString {
                        key,
                        scope,
                        field,
                        value: String::new(),
                        escaped: false,
                    });
                }
                b'{' | b'[' => {
                    if self.frames.len() >= 64 {
                        return Err(SessionError::Capacity {
                            resource: "json_depth",
                            limit: 64,
                        });
                    }
                    let scope = self.value_scope();
                    let plan_item = self
                        .frames
                        .last()
                        .is_some_and(|frame| frame.scope == Scope::Payload && frame.key == "item");
                    let object = byte == b'{';
                    if scope == Scope::Content {
                        self.content_array = !object;
                        self.conversation |= object;
                    }
                    self.frames.push(JsonFrame {
                        scope,
                        object,
                        key: String::new(),
                        expecting_key: object,
                        block_kind: false,
                        block_use: false,
                        block_question: false,
                        block_result: false,
                        call: None,
                        plan_item,
                    });
                }
                b'}' | b']' => {
                    if let Some(frame) = self.frames.pop() {
                        self.invalid |= frame.object != (byte == b'}');
                        if frame.scope == Scope::Block {
                            self.conversation |= !frame.block_kind;
                            let tool = if frame.block_use && frame.block_question {
                                Some(LargeTool::Question(frame.call))
                            } else if frame.block_result {
                                frame.call.map(LargeTool::Result)
                            } else {
                                None
                            };
                            if let Some(tool) = tool {
                                if self.tools.len() == TOOL_MARK_LIMIT {
                                    self.tool_capacity = true;
                                } else {
                                    self.tools.push(tool);
                                }
                            }
                        }
                    } else {
                        self.invalid = true;
                    }
                }
                b',' => {
                    if let Some(frame) = self.frames.last_mut() {
                        frame.expecting_key = frame.object;
                        frame.key.clear();
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }
    fn unclassifiable_prefix(&self) -> bool {
        match &self.grok {
            Some(grok) => grok.unclassifiable_prefix(),
            None => self.frames.is_empty() && self.token.is_none() && self.root_kind.is_empty(),
        }
    }

    /// Whether the record may be discarded, and the turn mark it carries,
    /// as its parsed form would give it: `None` when only its body could
    /// tell (conversation text, an unknown envelope), so the read fails
    /// rather than lose it.
    fn discard(self, agent: Agent) -> Option<Discard> {
        if !self.native_ids || self.invalid || !self.frames.is_empty() || self.token.is_some() {
            return None;
        }
        match agent {
            // Grok's oversized records are read by its own scan.
            Agent::OpenCode | Agent::Grok | Agent::Pi | Agent::Omp => None,
            Agent::Codex => self.codex(),
            Agent::Claude => self.claude(),
        }
    }

    fn claude(self) -> Option<Discard> {
        match self.root_kind.as_str() {
            "user" | "assistant" if self.content_array && !self.conversation => {}
            "user" | "assistant" | "" | "ai-title" => return None,
            _ => return Some(Ok(None)),
        }
        if self.tool_capacity {
            return Some(Err(SkipReason::UserTurnCapacity));
        }
        let mut marks = Vec::new();
        for tool in self.tools {
            let mark = match (self.root_kind.as_str(), tool) {
                ("assistant", LargeTool::Question(call)) => match bounded(call) {
                    Ok(Some(call)) => ToolTurnMark::Asked {
                        call,
                        content: None,
                    },
                    Ok(None) => return Some(Err(SkipReason::UserTurnInvalid)),
                    Err(reason) => return Some(Err(reason)),
                },
                ("user", LargeTool::Result(call)) => match bounded(Some(call)) {
                    Ok(Some(call)) => ToolTurnMark::Answered { call },
                    Ok(None) => continue,
                    Err(reason) => return Some(Err(reason)),
                },
                _ => continue,
            };
            marks.push(mark);
        }
        Some(Ok((!marks.is_empty()).then_some(TurnMark::Tools(marks))))
    }

    fn codex(self) -> Option<Discard> {
        let turn = || bounded(self.payload_turn.clone());
        let mark = match (self.root_kind.as_str(), self.payload_kind.as_str()) {
            ("", _) | ("response_item", "" | "message") => return None,
            ("response_item", "function_call") if self.payload_question => {
                match bounded(self.payload_call.clone()) {
                    Ok(Some(call)) => Ok(Some(TurnMark::Tools(vec![ToolTurnMark::Asked {
                        call,
                        content: None,
                    }]))),
                    Ok(None) => Err(SkipReason::UserTurnInvalid),
                    Err(reason) => Err(reason),
                }
            }
            ("response_item", "function_call_output") => {
                bounded(self.payload_call.clone()).map(|call| {
                    call.map(|call| TurnMark::Tools(vec![ToolTurnMark::Answered { call }]))
                })
            }
            ("event_msg", "item_completed") if self.plan_item => {
                turn().map(|turn| Some(TurnMark::Plan { turn }))
            }
            ("event_msg", "task_started") => turn().map(|turn| {
                Some(TurnMark::Started {
                    turn,
                    mode: match self.payload_mode.as_deref() {
                        Some("plan") => TurnMode::Plan,
                        Some("default") => TurnMode::Other,
                        _ => TurnMode::Unknown,
                    },
                })
            }),
            ("event_msg", "task_complete") => turn().map(|turn| Some(TurnMark::Completed { turn })),
            ("event_msg", "turn_aborted") => turn().map(|turn| Some(TurnMark::Aborted { turn })),
            _ => Ok(None),
        };
        Some(mark)
    }
}
