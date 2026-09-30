use crate::{
    Agent, AppendedBytes, ParsedSession, Result, SESSION_LINE_LIMIT_BYTES,
    SESSION_READ_LIMIT_BYTES, SessionCursor, SessionError, SkipReason, parse_events_at,
};
use serde::de::{DeserializeSeed, Error, IgnoredAny, MapAccess, Visitor};
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// Incrementally reads conversation events without retaining unrelated records.
#[derive(Debug, Default)]
pub struct ConversationCursor {
    cursor: SessionCursor,
    discarded_bytes: u64,
    has_more: bool,
}

impl ConversationCursor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.cursor.reset();
        self.discarded_bytes = 0;
        self.has_more = false;
    }

    /// More bytes from the last observed file size remain to be read.
    /// A torn final record at EOF waits for an append instead of spinning.
    pub fn has_more(&self) -> bool {
        self.has_more
    }

    pub fn read(&mut self, agent: Agent, path: &Path) -> Result<ParsedSession> {
        self.has_more = false;
        let AppendedBytes {
            contents: appended,
            start_offset,
            identity,
            rescan_reason,
            has_more,
        } = self.cursor.read_appended(path)?;
        if rescan_reason.is_some() {
            self.discarded_bytes = 0;
        }
        let mut offset = start_offset;
        let mut pending = self.cursor.pending.clone();
        let mut discarded_bytes = self.discarded_bytes;
        let mut parsed = ParsedSession {
            rescan_reason,
            ..ParsedSession::default()
        };
        for fragment in appended.split_inclusive(|byte| *byte == b'\n') {
            let line_start = offset.saturating_sub(pending.len() as u64 + discarded_bytes);
            offset += fragment.len() as u64;
            let complete = fragment.last() == Some(&b'\n');
            if discarded_bytes > 0 {
                discarded_bytes += fragment.len() as u64;
            } else {
                let retained = fragment.len().min(SESSION_LINE_LIMIT_BYTES - pending.len());
                pending.extend_from_slice(&fragment[..retained]);
                if retained < fragment.len() {
                    // Decide by the provider's JSON discriminators, never by
                    // tool output text. Unknown or conversation records still
                    // fail at the existing retention bound.
                    let irrelevant = match nonconversation_prefix(agent, &pending) {
                        Some(verdict) => Some(verdict),
                        None => nonconversation_record(agent, path, line_start)?,
                    };
                    if irrelevant != Some(true) {
                        return Err(SessionError::Capacity {
                            resource: "line_bytes",
                            limit: SESSION_LINE_LIMIT_BYTES as u64,
                        });
                    }
                    discarded_bytes = pending.len() as u64 + (fragment.len() - retained) as u64;
                    pending.clear();
                }
            }
            if !complete {
                continue;
            }
            if discarded_bytes > 0 {
                parsed.skipped(SkipReason::NonConversationCapacity);
                discarded_bytes = 0;
                continue;
            }
            let line = parse_events_at(agent, &String::from_utf8_lossy(&pending), line_start);
            parsed.events.extend(line.events);
            parsed.event_offsets.extend(line.event_offsets);
            parsed.skipped_lines += line.skipped_lines;
            for (reason, count) in line.skipped_reasons {
                *parsed.skipped_reasons.entry(reason).or_default() += count;
            }
            if line.title.is_some() {
                parsed.title = line.title;
            }
            pending.clear();
        }
        // Commit only a successful poll: a relevant capacity failure cannot
        // silently consume a Human turn or publish a partial result.
        self.cursor.offset = offset;
        self.cursor.identity = Some(identity);
        self.cursor.pending = pending;
        self.discarded_bytes = discarded_bytes;
        self.has_more = has_more;
        Ok(parsed)
    }
}

// JSON object keys have no ordering contract. When the discriminator follows
// a large body, serde streams past unknown fields without retaining them.
// This exceptional header scan has the same byte budget as archive reads;
// subsequent polls discard the body without probing it again.
fn nonconversation_record(agent: Agent, path: &Path, offset: u64) -> Result<Option<bool>> {
    #[derive(serde::Deserialize)]
    struct Header {
        #[serde(rename = "type")]
        kind: Option<String>,
        payload: Option<Payload>,
    }
    #[derive(serde::Deserialize)]
    struct Payload {
        #[serde(rename = "type")]
        kind: Option<String>,
    }
    let mut file = File::open(path).map_err(|error| SessionError::io("open", path, error))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| SessionError::io("seek", path, error))?;
    let mut deserializer =
        serde_json::Deserializer::from_reader(BufReader::new(file.take(SESSION_READ_LIMIT_BYTES)));
    let Ok(header) = <Header as serde::Deserialize>::deserialize(&mut deserializer) else {
        return Ok(None);
    };
    Ok(match agent {
        Agent::Claude => header
            .kind
            .map(|kind| !matches!(kind.as_str(), "user" | "assistant" | "ai-title")),
        Agent::Codex => match header.kind.as_deref() {
            Some("response_item") => header
                .payload
                .and_then(|payload| payload.kind)
                .map(|kind| kind != "message"),
            Some(_) => Some(true),
            None => None,
        },
    })
}

fn nonconversation_prefix(agent: Agent, bytes: &[u8]) -> Option<bool> {
    let mut verdict = None;
    // A visitor deliberately stops after a discriminator. The side channel
    // distinguishes that decision from a malformed or incomplete prefix;
    // neither the JSON body nor an error string is retained.
    let _ = RecordKind {
        agent,
        payload: false,
        verdict: &mut verdict,
    }
    .deserialize(&mut serde_json::Deserializer::from_slice(bytes));
    verdict
}

struct RecordKind<'a> {
    agent: Agent,
    payload: bool,
    verdict: &'a mut Option<bool>,
}

impl<'de> DeserializeSeed<'de> for RecordKind<'_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for RecordKind<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a provider record envelope")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<(), M::Error> {
        let mut response_item = false;
        while let Some(key) = map.next_key::<String>()? {
            if key == "type" {
                let kind = map.next_value::<String>()?;
                let irrelevant = match (self.agent, self.payload) {
                    (Agent::Claude, _) => {
                        !matches!(kind.as_str(), "user" | "assistant" | "ai-title")
                    }
                    (Agent::Codex, true) => kind != "message",
                    (Agent::Codex, false) if kind == "response_item" => {
                        response_item = true;
                        continue;
                    }
                    (Agent::Codex, false) => true,
                };
                *self.verdict = Some(irrelevant);
                return Err(M::Error::custom("record discriminator read"));
            }
            if key == "payload" && response_item {
                map.next_value_seed(RecordKind {
                    agent: self.agent,
                    payload: true,
                    verdict: self.verdict,
                })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}
