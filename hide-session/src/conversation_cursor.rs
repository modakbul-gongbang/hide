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
    #[serde(default)]
    classifier: Option<LargeRecord>,
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
    classifier: Option<LargeRecord>,
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
        Self {
            cursor: SessionCursor::restore(checkpoint.cursor),
            discarded_bytes: checkpoint.discarded_bytes,
            has_more: false,
            classifier: checkpoint.classifier,
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
        let AppendedBytes {
            contents: appended,
            start_offset,
            identity,
            rescan_reason,
            has_more,
        } = read_appended_file(&mut self.cursor, path, file, budget, &mut self.read_bytes)?;
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
                    let mut scan = LargeRecord::default();
                    scan.feed(&pending)?;
                    scan.feed(&fragment[retained..])?;
                    classifier = Some(scan);
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
                if !classifier.take().is_some_and(|scan| scan.tool_only(agent)) {
                    return Err(SessionError::Capacity {
                        resource: "line_bytes",
                        limit: SESSION_LINE_LIMIT_BYTES as u64,
                    });
                }
                parsed.skipped(SkipReason::NonConversationCapacity);
                discarded_bytes = 0;
                continue;
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
            pending.clear();
        }
        parsed.links = found.finish();
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
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct LargeRecord {
    frames: Vec<JsonFrame>,
    token: Option<JsonString>,
    root_kind: String,
    payload_kind: String,
    content_array: bool,
    conversation: bool,
    invalid: bool,
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
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct JsonString {
    key: bool,
    scope: Scope,
    field: String,
    value: String,
    escaped: bool,
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
        for &byte in bytes {
            if let Some(token) = self.token.as_mut() {
                if token.escaped {
                    // Escaped discriminator spellings stay unknown, never
                    // incorrectly authorizing a discarded conversation.
                    if token.value.len() < 64 {
                        token.value.push('\\');
                    }
                    token.escaped = false;
                } else if byte == b'\\' {
                    self.invalid |= token.key || token.field == "type";
                    token.escaped = true;
                } else if byte == b'"' {
                    let token = self.token.take().unwrap();
                    if token.key {
                        if let Some(frame) = self.frames.last_mut() {
                            frame.key = token.value;
                            frame.expecting_key = false;
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
                                }
                            }
                            _ => (),
                        }
                    }
                } else if token.value.len() < 64 && (token.key || token.field == "type") {
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
                    });
                }
                b'}' | b']' => {
                    if let Some(frame) = self.frames.pop() {
                        self.invalid |= frame.object != (byte == b'}');
                        if frame.scope == Scope::Block {
                            self.conversation |= !frame.block_kind;
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
        self.frames.is_empty() && self.token.is_none() && self.root_kind.is_empty()
    }

    fn tool_only(self, agent: Agent) -> bool {
        if self.invalid || !self.frames.is_empty() || self.token.is_some() {
            return false;
        }
        match agent {
            Agent::OpenCode => false,
            Agent::Codex => {
                !self.root_kind.is_empty()
                    && (self.root_kind != "response_item"
                        || (!self.payload_kind.is_empty() && self.payload_kind != "message"))
            }
            Agent::Claude => match self.root_kind.as_str() {
                "user" | "assistant" => self.content_array && !self.conversation,
                "" | "ai-title" => false,
                _ => true,
            },
        }
    }
}
