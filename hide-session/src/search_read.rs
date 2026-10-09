//! Reading session files into the search index, on the node that holds
//! them: [`read_step`] reads at most the 1 MiB cursor budget past what the
//! index saved and answers an [`IndexStep`] the index's owner applies.
use crate::search::{IndexStep, IndexedMessage, SavedFile, SearchIndex};
use crate::{Agent, ConversationCheckpoint, ConversationCursor, EventKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The bytes one read took from the transcript and from its prefix witness.
#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateReads {
    pub cursor_bytes: u64,
    pub witness_bytes: u64,
}

/// Reads the session file at `path` on from `saved`.
pub fn read_step(
    saved: Option<&SavedFile>,
    agent: Agent,
    path: &Path,
) -> Result<(IndexStep, UpdateReads), String> {
    let opened = open_regular(path)?;
    read_opened(saved, agent, path, &opened)
}

pub fn read_step_confirmed(
    home: &Path,
    saved: Option<&SavedFile>,
    agent: Agent,
    path: &Path,
    scope: Option<&crate::SessionReadScope>,
) -> Result<(IndexStep, UpdateReads), String> {
    if agent != Agent::Cursor {
        return read_step(saved, agent, path);
    }
    let scope = scope.ok_or_else(|| "cursor_read_scope_required".to_owned())?;
    let confirmed = crate::cursor::proof(home, path, Some(&scope.id), &scope.cwd)
        .map_err(|_| "cursor_scope_unconfirmed".to_owned())?;
    let observed = stamp_at(path).ok_or_else(|| "cursor_source_unavailable".to_owned())?;
    let checkpoint: Option<ConversationCheckpoint> = saved
        .map(|saved| {
            serde_json::from_str(&saved.cursor).map_err(|_| "cursor_checkpoint_invalid".to_owned())
        })
        .transpose()?;
    if let Some(checkpoint) = &checkpoint {
        if !checkpoint.is_native() {
            return Ok((IndexStep::Reset, UpdateReads::default()));
        }
        if checkpoint
            .native_matches(&confirmed)
            .map_err(|_| "cursor_checkpoint_invalid".to_owned())?
            && saved.is_some_and(|saved| saved.stamp == observed)
            && !checkpoint.has_more()
        {
            return Ok((IndexStep::Done, UpdateReads::default()));
        }
    }
    let read = crate::conversation_cursor::read_conversation(home, agent, path, scope, checkpoint)
        .map_err(|_| "cursor_index_read_failed".to_owned())?;
    if stamp_at(path).as_deref() != Some(&observed) {
        return Err("cursor_source_changed".to_owned());
    }
    let messages = read
        .events
        .into_iter()
        .zip(read.event_offsets)
        .filter(|(event, _)| matches!(event.kind, EventKind::Human | EventKind::Assistant))
        .map(|(event, offset)| IndexedMessage {
            offset,
            role: event.role.to_owned(),
            at_unix_ms: event.at_unix_ms,
            text: event.text,
        })
        .collect();
    Ok((
        IndexStep::Read {
            reset: read.rescanned,
            messages,
            cursor: serde_json::to_string(&read.checkpoint)
                .map_err(|_| "cursor_checkpoint_invalid".to_owned())?,
            stamp: observed,
            witness: "{\"kind\":\"cursor_graph_v1\"}".to_owned(),
            more: read.has_more,
        },
        UpdateReads {
            cursor_bytes: read.read_bytes,
            witness_bytes: 0,
        },
    ))
}

/// One read and its write, where the index and the file share a machine.
/// True means more bytes.
pub fn update(
    index: &mut SearchIndex,
    project: &str,
    session: &str,
    agent: Agent,
    path: &Path,
    cutoff: u64,
) -> Result<bool, String> {
    update_measured(index, project, session, agent, path, cutoff).map(|(more, _)| more)
}

/// [`update`] with the bytes its read took.
pub fn update_measured(
    index: &mut SearchIndex,
    project: &str,
    session: &str,
    agent: Agent,
    path: &Path,
    cutoff: u64,
) -> Result<(bool, UpdateReads), String> {
    let saved = index.saved(project, session)?;
    let (step, reads) = read_step(saved.as_ref(), agent, path)?;
    let more = index.apply(project, session, &path.to_string_lossy(), cutoff, step)?;
    Ok((more, reads))
}

/// [`SearchIndex::search_scoped`] over every session, with the stamps read
/// here.
pub fn search(
    index: &SearchIndex,
    project: &str,
    query: &str,
    cutoff: u64,
) -> Result<crate::search::SearchPage, String> {
    index.search_scoped(project, query, cutoff, None, &mut |paths| Ok(stamps(paths)))
}

/// Each path's current stamp, `None` for one that is gone or unreadable.
pub fn stamps(paths: &[String]) -> Vec<Option<String>> {
    paths.iter().map(|path| stamp_at(Path::new(path))).collect()
}

/// The current stamp of the file at `path`, `None` when it is gone or
/// unreadable.
pub fn stamp_at(path: &Path) -> Option<String> {
    if path.file_name().is_some_and(|name| name == "store.db") {
        crate::cursor::source_stamp(path)
    } else {
        stamp(path).ok()
    }
}

/// The scope of the OpenCode session a catalog locator (`opencode/<id>`)
/// names, held to the id its queued row was proven with.
pub fn opencode_session<'a>(
    locator: &str,
    scope: Option<&'a crate::SessionReadScope>,
) -> Result<&'a crate::SessionReadScope, String> {
    let id = locator
        .strip_prefix(crate::links::OPENCODE_PREFIX)
        .ok_or_else(|| "label_session_outside_roots".to_owned())?;
    let scope = scope.ok_or_else(|| "label_session_scope_required".to_owned())?;
    if scope.id != id {
        return Err("label_session_id_mismatch".to_owned());
    }
    Ok(scope)
}

/// An OpenCode session's stamp: its creation, its message count and its
/// newest write, between the same root and checkout proof as its reads.
pub fn opencode_stamp(home: &Path, scope: &crate::SessionReadScope) -> Result<String, String> {
    crate::opencode::stamp(home, &scope.id, &scope.cwd)
}

/// One bounded read of an OpenCode session past what the index saved. The
/// stamp is taken before the read, so a write that lands during it moves
/// the stamp and the next refresh reads on from the saved position.
pub fn read_opencode_step(
    home: &Path,
    saved: Option<&SavedFile>,
    scope: &crate::SessionReadScope,
) -> Result<IndexStep, String> {
    let stamp = opencode_stamp(home, scope)?;
    let checkpoint = saved
        .map(|saved| serde_json::from_str::<ConversationCheckpoint>(&saved.cursor))
        .transpose()
        .map_err(|e| e.to_string())?;
    if let (Some(saved), Some(checkpoint)) = (saved, &checkpoint)
        && saved.stamp == stamp
        && !checkpoint.has_more()
    {
        return Ok(IndexStep::Done);
    }
    let transcript = crate::opencode::read(
        home,
        &crate::label_transcript::LabelTranscriptRequest {
            agent: Agent::OpenCode,
            reference_kind: "id".into(),
            reference_value: scope.id.clone(),
            cwd: Some(scope.cwd.clone()),
            checkpoint,
            subagents: Default::default(),
            turns: None,
        },
    )?;
    let messages = transcript
        .events
        .into_iter()
        .filter(|event| event.kind != crate::label_transcript::LabelEventKind::Interrupted)
        .map(|event| IndexedMessage {
            offset: event.offset,
            role: event.kind.role().to_owned(),
            at_unix_ms: event.at_unix_ms,
            text: event.text,
        })
        .collect();
    let cursor = transcript.checkpoint.with_more(transcript.has_more);
    Ok(IndexStep::Read {
        reset: transcript.rescanned.is_some(),
        messages,
        cursor: serde_json::to_string(&cursor).map_err(|e| e.to_string())?,
        stamp,
        // OpenCode's database keeps no prefix to witness; the checkpoint's
        // own message witness detects a rewound session.
        witness: String::new(),
        more: transcript.has_more,
    })
}

fn read_opened(
    saved: Option<&SavedFile>,
    agent: Agent,
    path: &Path,
    opened: &File,
) -> Result<(IndexStep, UpdateReads), String> {
    let mut reads = UpdateReads::default();
    if opened.metadata().map_err(|e| e.to_string())?.len() > crate::SESSION_READ_LIMIT_BYTES {
        return Err("Sessions larger than 64 MiB cannot be indexed or opened here.".into());
    }
    let observed_stamp = metadata_stamp(&opened.metadata().map_err(|e| e.to_string())?)?;
    if stamp(path)? != observed_stamp {
        return Err("Session source changed before indexing.".into());
    }
    let mut cursor = ConversationCursor::new();
    let mut reset = false;
    let mut prefix = PrefixWitness::default();
    if let Some(saved) = saved {
        let checkpoint: ConversationCheckpoint =
            serde_json::from_str(&saved.cursor).map_err(|e| e.to_string())?;
        prefix = serde_json::from_str(&saved.witness).map_err(|e| e.to_string())?;
        if saved.stamp == observed_stamp {
            if prefix.hashed_offset < checkpoint.offset() {
                let end = checkpoint.offset().min(prefix.hashed_offset + HASH_BLOCK);
                prefix
                    .hashes
                    .push(hash_block(opened, prefix.hashed_offset, end)?);
                reads.witness_bytes = end - prefix.hashed_offset;
                prefix.hashed_offset = end;
                prefix.verified_chunks = prefix.hashes.len();
                if stamp(path)? != observed_stamp {
                    return Err("Session changed while validating its prefix.".into());
                }
                let more = checkpoint.has_more() || prefix.hashed_offset < checkpoint.offset();
                return Ok((witness_step(&prefix, more)?, reads));
            }
            if !checkpoint.has_more() {
                return Ok((IndexStep::Done, reads));
            }
        }
        if saved.stamp != observed_stamp {
            let saved_size = saved
                .stamp
                .split(':')
                .nth(2)
                .and_then(|v| v.parse::<u64>().ok());
            reset = prefix.hashed_offset < checkpoint.offset()
                || saved_size
                    .is_none_or(|size| opened.metadata().map(|m| m.len() <= size).unwrap_or(true));
            if !reset {
                if prefix.verified_for != observed_stamp {
                    prefix.verified_for = observed_stamp.clone();
                    prefix.verified_chunks = 0;
                }
                if prefix.verified_chunks < prefix.hashes.len() {
                    let n = prefix.verified_chunks;
                    let end = checkpoint.offset().min((n as u64 + 1) * HASH_BLOCK);
                    reads.witness_bytes = end - n as u64 * HASH_BLOCK;
                    if hash_block(opened, n as u64 * HASH_BLOCK, end)? != prefix.hashes[n] {
                        return Ok((IndexStep::Reset, reads));
                    }
                    if stamp(path)? != observed_stamp {
                        return Err("Session changed while validating its prefix.".into());
                    }
                    prefix.verified_chunks += 1;
                    return Ok((witness_step(&prefix, true)?, reads));
                }
            }
        }
        if !reset {
            cursor = ConversationCursor::restore(checkpoint);
        }
    }
    if reset {
        prefix = PrefixWitness::default();
    }
    let old_offset = cursor.checkpoint().offset();
    let parsed = cursor
        .read_file(agent, path, opened)
        .map_err(|e| e.to_string())?;
    reset |= parsed.rescan_reason.is_some();
    if parsed.rescan_reason.is_some() {
        prefix = PrefixWitness::default();
    }
    let checkpoint = cursor.checkpoint();
    // Only the partial previous block and newly consumed blocks are hashed.
    let first = if reset { 0 } else { old_offset / HASH_BLOCK };
    prefix.hashes.truncate(first as usize);
    prefix.hashed_offset = first * HASH_BLOCK;
    reads.cursor_bytes = cursor.read_bytes();
    // Small files complete in one call while the combined transcript and
    // hash reads still fit the same 1 MiB budget. Large files stage hashes.
    let hash_bytes = checkpoint.offset().saturating_sub(prefix.hashed_offset);
    if hash_bytes <= crate::SESSION_INCREMENT_READ_LIMIT_BYTES.saturating_sub(reads.cursor_bytes) {
        while prefix.hashed_offset < checkpoint.offset() {
            let end = checkpoint.offset().min(prefix.hashed_offset + HASH_BLOCK);
            prefix
                .hashes
                .push(hash_block(opened, prefix.hashed_offset, end)?);
            reads.witness_bytes += end - prefix.hashed_offset;
            prefix.hashed_offset = end;
        }
    }
    prefix.verified_for = observed_stamp.clone();
    prefix.verified_chunks = prefix.hashes.len();
    // A replacement during reading cannot commit a mixed file/cursor.
    if stamp(path)? != observed_stamp {
        return Err("Session changed while indexing; retrying on the next refresh.".into());
    }
    let messages = parsed
        .events
        .into_iter()
        .zip(parsed.event_offsets)
        .filter(|(event, _)| matches!(event.kind, EventKind::Human | EventKind::Assistant))
        .map(|(event, offset)| IndexedMessage {
            offset,
            role: event.role.to_owned(),
            at_unix_ms: event.at_unix_ms,
            text: event.text,
        })
        .collect();
    Ok((
        IndexStep::Read {
            reset,
            messages,
            cursor: serde_json::to_string(&checkpoint).map_err(|e| e.to_string())?,
            stamp: observed_stamp,
            witness: serde_json::to_string(&prefix).map_err(|e| e.to_string())?,
            more: cursor.has_more() || prefix.hashed_offset < checkpoint.offset(),
        },
        reads,
    ))
}
fn witness_step(prefix: &PrefixWitness, more: bool) -> Result<IndexStep, String> {
    Ok(IndexStep::Witness {
        witness: serde_json::to_string(prefix).map_err(|e| e.to_string())?,
        more,
    })
}
fn stamp(path: &Path) -> Result<String, String> {
    metadata_stamp(&fs::metadata(path).map_err(|e| e.to_string())?)
}
fn metadata_stamp(m: &fs::Metadata) -> Result<String, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(format!(
            "{}:{}:{}:{}:{}",
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec()
        ))
    }
    #[cfg(not(unix))]
    {
        Ok(format!(
            "{}:{:?}",
            m.len(),
            m.modified().map_err(|e| e.to_string())?
        ))
    }
}
const HASH_BLOCK: u64 = 1024 * 1024;
#[derive(Default, Serialize, Deserialize)]
struct PrefixWitness {
    hashes: Vec<String>,
    #[serde(default)]
    hashed_offset: u64,
    verified_for: String,
    verified_chunks: usize,
}
fn hash_block(opened: &File, start: u64, end: u64) -> Result<String, String> {
    let mut file = opened.try_clone().map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut bytes = [0; 64 * 1024];
    let mut reader = file.take(end.saturating_sub(start));
    loop {
        let n = reader.read(&mut bytes).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&bytes[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn open_regular(path: &Path) -> Result<File, String> {
    crate::open_session_file(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptor_replaced_before_path_stamp_cannot_publish_removed_text() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("source.jsonl");
        fs::write(&path, r#"{"type":"response_item","timestamp":"2026-10-01T00:00:00Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"removed old body"}]}}"#).unwrap();
        let opened = open_regular(&path).unwrap();
        let replacement = temp.path().join("replacement");
        fs::write(&replacement, "new source").unwrap();
        fs::rename(&replacement, &path).unwrap();
        assert!(
            read_opened(None, Agent::Codex, &path, &opened)
                .unwrap_err()
                .contains("changed before")
        );
    }
}
