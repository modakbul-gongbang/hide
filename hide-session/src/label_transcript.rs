//! One bounded, resumable read of a pane's conversation: the session
//! adapter every agent's request, result, title and pull request sightings
//! come from (PRD overview-request-view D-14, D-16).
//!
//! The same function answers for this machine (the core's in-process host)
//! and for a device (`hided node serve`), so the two cannot drift in what
//! they read, prove or refuse. It holds no state: the caller keeps the
//! checkpoint and hands it back on the next read. A read locates the session
//! Herdr's reference names (a Claude Code or Codex file, an OpenCode
//! database row), proves the provider's native owner before and after the
//! read, and returns only conversation events, never injected scaffolding or
//! a path. Failures are stable reason codes.
//!
//! What every adapter answers, and nothing more: the session's own title,
//! each person's message with its time, images and Hide letter sender, each
//! assistant message, and the pull request addresses its tools printed
//! (Claude Code's subagents' tools included). Status, pull requests and
//! lineage do not depend on the agent and are not read here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::turns::TurnTracker;
use crate::{
    Agent, ConfirmedLabelSession, ConversationCheckpoint, ConversationCursor, EventKind,
    SessionError, SessionIdentity, SessionLocator,
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
    /// Where each of a Claude Code session's subagent files was read up to,
    /// by file name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subagents: BTreeMap<String, ConversationCheckpoint>,
    /// The turn the previous read ended in, continued by this one; ignored
    /// when the read starts over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<TurnTracker>,
}

/// A conversation event as the label analysis reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelEvent {
    pub kind: LabelEventKind,
    pub at_unix_ms: u64,
    pub text: String,
    /// Byte offset of the record that produced it (for OpenCode, the
    /// message's index).
    pub offset: u64,
    /// Images attached to a person's message; their bytes are not in `text`.
    #[serde(default)]
    pub images: u32,
    /// The sender a Hide letter header names on a person's message (`envelope`).
    #[serde(default)]
    pub sender: Option<String>,
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
    /// The name the agent gave the session, when this read saw it: Claude
    /// Code's `ai-title`, Codex's `thread_name`, OpenCode's session title.
    #[serde(default)]
    pub title: Option<String>,
    /// The name the operator gave the session (Claude Code's `/rename`),
    /// which wins over `title`.
    #[serde(default)]
    pub custom_title: Option<String>,
    /// Pull request addresses the session's tools printed in what this read
    /// covered (PRD overview-request-view D-31).
    #[serde(default)]
    pub pr_sightings: Vec<crate::PrSighting>,
    /// Where each subagent file was read up to, for the next request.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subagents: BTreeMap<String, ConversationCheckpoint>,
    /// The session's last turn as far as this read reached, for an agent
    /// that reports turns ([`Agent::reports_turns`]); `None` otherwise, and
    /// from a device helper that predates it, which a caller reads as not
    /// known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<TurnTracker>,
}

/// How many of a Claude Code session's subagent files are read.
pub const MAX_SUBAGENT_FILES: usize = 16;
/// The skip reason a read reports with how many subagent files it left out.
pub const SUBAGENT_FILES_CAPPED: &str = "subagent_files_capped";
/// The most subagent bytes one read takes; the rest waits for the next read.
const SUBAGENT_READ_BUDGET: u64 = 2 * 1024 * 1024;
/// How much of the end of Codex's `session_index.jsonl` is read for a
/// session's `thread_name`; the newest entries are last.
const CODEX_INDEX_TAIL_BYTES: u64 = 64 * 1024;

/// The request's session, its proven owner, and the next bounded chunk.
///
/// The owner is proven before and after the read; a file replaced or
/// shrunk in between is refused rather than analyzed across the change.
/// An error is a stable reason code with no path or content in it.
pub fn read(home: &Path, request: &LabelTranscriptRequest) -> Result<LabelTranscript, String> {
    if request.reference_kind == "id" && !is_session_id(&request.reference_value) {
        return Err("label_session_id_invalid".to_owned());
    }
    if request.agent == Agent::OpenCode {
        return crate::opencode::read(home, request);
    }
    let (path, before) = locate_confirmed(
        home,
        request.agent,
        &request.reference_kind,
        &request.reference_value,
        request.cwd.as_deref(),
    )?;
    let reported_id = (request.reference_kind == "id").then_some(request.reference_value.as_str());
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
    for reason in [
        crate::SkipReason::UserTurnCapacity,
        crate::SkipReason::UserTurnInvalid,
    ] {
        if parsed.skipped_reasons.contains_key(&reason) {
            return Err(reason.as_str().to_owned());
        }
    }
    let after = crate::confirm_session_file(
        home,
        request.agent,
        &path,
        reported_id,
        request.cwd.as_deref(),
    )
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
            images: event.images,
            sender: (kind == LabelEventKind::Human)
                .then(|| crate::envelope_sender(&event.text))
                .flatten(),
        });
    }
    // A read from the start, or one that started over, folds the whole file
    // again; otherwise it continues the turn the previous read ended in.
    let turns = request.agent.reports_turns().then(|| {
        let mut turns = match (&request.checkpoint, &parsed.rescan_reason) {
            (Some(_), None) => request.turns.clone().unwrap_or_default(),
            _ => TurnTracker::default(),
        };
        for (offset, mark) in &parsed.turn_marks {
            turns.fold(*offset, mark);
            if turns.capacity_exceeded() {
                break;
            }
        }
        turns
    });
    if turns.as_ref().is_some_and(TurnTracker::capacity_exceeded) {
        return Err("user_turn_capacity".to_owned());
    }
    let mut pr_sightings = parsed.pr_sightings.clone();
    let mut subagents = BTreeMap::new();
    let mut subagents_pending = false;
    let mut subagents_left_out = 0;
    if request.agent == Agent::Claude {
        (subagents_pending, subagents_left_out) = read_subagents(
            home,
            &path,
            &request.subagents,
            &mut subagents,
            &mut pr_sightings,
        );
    }
    let title = match request.agent {
        Agent::Codex => codex_thread_name(home, request, &path),
        Agent::Claude | Agent::Pi | Agent::OpenCode => parsed.title.clone(),
    };
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
        has_more: cursor.has_more() || subagents_pending,
        rescanned: parsed
            .rescan_reason
            .map(|reason| reason.as_str().to_owned()),
        skipped_lines: parsed.skipped_lines,
        skipped_reasons: parsed
            .skipped_reasons
            .iter()
            .map(|(reason, count)| (reason.as_str().to_owned(), *count))
            .chain(
                (subagents_left_out > 0)
                    .then(|| (SUBAGENT_FILES_CAPPED.to_owned(), subagents_left_out)),
            )
            .collect(),
        title,
        custom_title: parsed.custom_title.clone(),
        pr_sightings,
        subagents,
        turns,
    })
}

/// Shared location and native ownership proof for conversation and activity
/// readers. Neither reader may use cwd as ownership evidence or leave the
/// provider's transcript root through a reported path or a link.
pub(crate) fn locate_confirmed(
    home: &Path,
    agent: Agent,
    reference_kind: &str,
    reference_value: &str,
    cwd: Option<&str>,
) -> Result<(PathBuf, ConfirmedLabelSession), String> {
    let identity = match reference_kind {
        "id" => SessionIdentity::id(reference_value),
        "path" => SessionIdentity::path(reference_value),
        _ => return Err("session_kind_unsupported".to_owned()),
    };
    if reference_value.trim().is_empty() {
        return Err("label_session_reference_missing".to_owned());
    }
    if reference_kind == "id" && !is_session_id(reference_value) {
        return Err("label_session_id_invalid".to_owned());
    }
    let located = SessionLocator::new(home)
        .locate("label", agent, Some(&identity), cwd)
        .map_err(|error| match error {
            SessionError::SessionFileMissing => "session_file_missing".to_owned(),
            SessionError::Capacity { .. } => "session_capacity".to_owned(),
            _ => "label_session_location_unavailable".to_owned(),
        })?;
    let path = inside_agent_root(home, agent, &located)?;
    let reported_id = (reference_kind == "id").then_some(reference_value);
    let confirmed = crate::confirm_session_file(home, agent, &path, reported_id, cwd)
        .map_err(|error| error.to_string())?;
    Ok((path, confirmed))
}

/// Reads what was appended to each of a Claude Code session's subagent files
/// (`<session>/subagents/*.jsonl`), for the pull request addresses their
/// tools printed: a subagent's work is its session's (D-46). At most
/// [`MAX_SUBAGENT_FILES`] files, the most recently written first, and
/// [`SUBAGENT_READ_BUDGET`] bytes per read; says whether bytes remain and how
/// many files the cap left out. A file that cannot be read keeps its
/// checkpoint and is tried again on the next read.
fn read_subagents(
    home: &Path,
    session: &Path,
    before: &BTreeMap<String, ConversationCheckpoint>,
    after: &mut BTreeMap<String, ConversationCheckpoint>,
    sightings: &mut Vec<crate::PrSighting>,
) -> (bool, usize) {
    let (Some(folder), Some(stem)) = (session.parent(), session.file_stem()) else {
        return (false, 0);
    };
    let folder = folder.join(stem).join("subagents");
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return (false, 0);
    };
    let mut files = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            let written = entry.metadata().and_then(|meta| meta.modified()).ok();
            name.ends_with(".jsonl").then_some((written, name))
        })
        .collect::<Vec<_>>();
    // A subagent still at work is the one that can print a new address.
    files.sort_by(|(left_at, left), (right_at, right)| {
        right_at.cmp(left_at).then_with(|| left.cmp(right))
    });
    let left_out = files.split_off(files.len().min(MAX_SUBAGENT_FILES));
    // A file the cap leaves out keeps where it was read to.
    for (_, name) in &left_out {
        if let Some(checkpoint) = before.get(name) {
            after.insert(name.clone(), checkpoint.clone());
        }
    }
    let names = files.into_iter().map(|(_, name)| name);
    let mut spent = 0_u64;
    let mut pending = false;
    for name in names {
        let checkpoint = before.get(&name).cloned();
        if spent >= SUBAGENT_READ_BUDGET {
            if let Some(checkpoint) = checkpoint {
                after.insert(name, checkpoint);
            }
            pending = true;
            continue;
        }
        let Ok(file) = inside_agent_root(home, Agent::Claude, &folder.join(&name)) else {
            continue;
        };
        let mut cursor = checkpoint
            .clone()
            .map(ConversationCursor::restore)
            .unwrap_or_default();
        let parsed = cursor.read_with_budget(Agent::Claude, &file, SUBAGENT_READ_BUDGET - spent);
        spent += cursor.read_bytes();
        match parsed {
            Ok(parsed) => {
                sightings.extend(parsed.pr_sightings);
                pending |= cursor.has_more();
                after.insert(name, cursor.checkpoint());
            }
            Err(_) => {
                if let Some(checkpoint) = checkpoint {
                    after.insert(name, checkpoint);
                }
            }
        }
    }
    (pending, left_out.len())
}

/// Codex's name for the session, from the newest `session_index.jsonl`
/// entry for its id; `None` when it has none or the index cannot be read.
fn codex_thread_name(home: &Path, request: &LabelTranscriptRequest, path: &Path) -> Option<String> {
    let id = if request.reference_kind == "id" {
        request.reference_value.clone()
    } else {
        codex_id_from_file_name(path)?
    };
    let index = home.join(".codex/session_index.jsonl");
    let tail = crate::read_tail(&index, CODEX_INDEX_TAIL_BYTES).ok()?;
    tail.lines().rev().find_map(|line| {
        let entry = serde_json::from_str::<serde_json::Value>(line).ok()?;
        (entry.get("id")?.as_str()? == id)
            .then(|| {
                entry
                    .get("thread_name")?
                    .as_str()
                    .map(str::trim)
                    .map(str::to_owned)
            })
            .flatten()
            .filter(|name| !name.is_empty())
    })
}

/// A rollout file is `rollout-<time>-<uuid>.jsonl`, and the uuid is the id.
fn codex_id_from_file_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let id = stem.get(stem.len().checked_sub(36)?..)?;
    (id.len() == 36
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-'))
    .then(|| id.to_owned())
}

/// A native session id as Claude and Codex write them; anything else (a
/// separator, `..`) could steer the file name the locator builds from it.
fn is_session_id(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && value != "."
        && value != ".."
}

/// [`crate::inside_session_root`] for the agent's own folder: a reported
/// path, or a link planted inside the folder, never makes the reader open a
/// file elsewhere.
fn inside_agent_root(home: &Path, agent: Agent, located: &Path) -> Result<PathBuf, String> {
    crate::inside_session_root(home, &[agent], located).map_err(|refusal| {
        match refusal {
            crate::RootRefusal::Unsupported => "session_kind_unsupported",
            crate::RootRefusal::Outside => "label_session_outside_roots",
            crate::RootRefusal::Missing | crate::RootRefusal::Unreadable => "session_file_missing",
        }
        .to_owned()
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

    /// Where Claude keeps a project's transcripts under `home`.
    fn transcript_path(home: &Path, name: &str) -> PathBuf {
        let project = home.join(".claude/projects/-project");
        fs::create_dir_all(&project).unwrap();
        project.join(name)
    }

    fn request(path: &Path, checkpoint: Option<ConversationCheckpoint>) -> LabelTranscriptRequest {
        LabelTranscriptRequest {
            agent: Agent::Claude,
            reference_kind: "path".to_owned(),
            reference_value: path.display().to_string(),
            cwd: None,
            checkpoint,
            subagents: BTreeMap::new(),
            turns: None,
        }
    }

    #[test]
    fn a_resumed_read_returns_only_appended_conversation_and_an_anchor_at_the_last_human() {
        let root = tempfile::tempdir().unwrap();
        let path = transcript_path(root.path(), "session.jsonl");
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

    /// D-46: past the file cap the subagents still at work are read, so a
    /// pull request a late subagent prints is found, and the cap is said.
    #[test]
    fn the_most_recently_written_subagents_are_read_past_the_cap() {
        let root = tempfile::tempdir().unwrap();
        let path = transcript_path(root.path(), "session.jsonl");
        fs::write(
            &path,
            claude_line("user", "s1", "request", "2026-10-01T00:00:00Z"),
        )
        .unwrap();
        let folder = path.parent().unwrap().join("session/subagents");
        fs::create_dir_all(&folder).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        for index in 0..20 {
            let name = folder.join(format!("agent-{index:02}.jsonl"));
            let printed = if index == 18 {
                serde_json::json!({"type":"user","sessionId":"s1","timestamp":"2026-10-01T00:01:00Z",
                    "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t",
                        "content":[{"type":"text","text":"https://github.com/acme/app/pull/18"}]}]}})
                .to_string()
            } else {
                claude_line("assistant", "s1", "working", "2026-10-01T00:00:30Z")
            };
            fs::write(&name, format!("{}\n", printed.trim_end())).unwrap();
            let written = if index == 18 {
                old + std::time::Duration::from_secs(1_000)
            } else {
                old + std::time::Duration::from_secs(index)
            };
            fs::File::options()
                .write(true)
                .open(&name)
                .unwrap()
                .set_modified(written)
                .unwrap();
        }
        let answer = read(root.path(), &request(&path, None)).unwrap();
        assert_eq!(
            answer
                .pr_sightings
                .iter()
                .map(|sighting| sighting.number)
                .collect::<Vec<_>>(),
            [18]
        );
        assert_eq!(answer.skipped_reasons.get(SUBAGENT_FILES_CAPPED), Some(&4));
        assert_eq!(answer.subagents.len(), MAX_SUBAGENT_FILES);
    }

    #[test]
    fn a_reported_id_the_file_does_not_carry_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let path = transcript_path(root.path(), "session.jsonl");
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
        let path = transcript_path(root.path(), "session.jsonl");
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

        let sentence = transcript_path(root.path(), "sentence.jsonl");
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
        let mut request = request(&transcript_path(root.path(), "missing.jsonl"), None);
        request.reference_kind = "pid".to_owned();
        assert_eq!(
            read(root.path(), &request).unwrap_err(),
            "session_kind_unsupported"
        );
    }

    #[test]
    fn an_id_that_is_not_a_bare_session_id_is_refused_before_any_file_is_looked_up() {
        let root = tempfile::tempdir().unwrap();
        let mut request = request(&transcript_path(root.path(), "unused.jsonl"), None);
        request.reference_kind = "id".to_owned();
        for id in ["../../secret", "a/b", "..", "id with space", "id\\x"] {
            request.reference_value = id.to_owned();
            assert_eq!(
                read(root.path(), &request).unwrap_err(),
                "label_session_id_invalid",
                "{id}"
            );
        }
    }

    #[test]
    fn a_path_outside_the_agents_transcript_root_is_refused_without_naming_it() {
        let root = tempfile::tempdir().unwrap();
        transcript_path(root.path(), "unused.jsonl");
        let elsewhere = root.path().join("elsewhere.jsonl");
        fs::write(
            &elsewhere,
            claude_line("user", "s1", "private", "2026-10-01T00:00:00Z"),
        )
        .unwrap();
        let error = read(root.path(), &request(&elsewhere, None)).unwrap_err();
        assert_eq!(error, "label_session_outside_roots");

        // A Codex root is no root for a Claude session either.
        let codex = root.path().join(".codex/sessions");
        fs::create_dir_all(&codex).unwrap();
        let in_codex = codex.join("session.jsonl");
        fs::copy(&elsewhere, &in_codex).unwrap();
        assert_eq!(
            read(root.path(), &request(&in_codex, None)).unwrap_err(),
            "label_session_outside_roots"
        );
    }

    #[test]
    fn a_link_inside_the_root_that_leads_outside_it_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = root.path().join("elsewhere.jsonl");
        fs::write(
            &elsewhere,
            claude_line("user", "s1", "private", "2026-10-01T00:00:00Z"),
        )
        .unwrap();
        let link = transcript_path(root.path(), "s1.jsonl");
        match hide_platform::fs::link::create_link(&elsewhere, &link) {
            Ok(()) => {}
            // A Windows account with neither the privilege nor Developer
            // Mode cannot make the link this test needs.
            Err(error) if hide_platform::fs::link::needs_privilege(&error) => return,
            Err(error) => panic!("{error}"),
        }

        assert_eq!(
            read(root.path(), &request(&link, None)).unwrap_err(),
            "label_session_outside_roots"
        );
        let mut by_id = request(&link, None);
        by_id.reference_kind = "id".to_owned();
        by_id.reference_value = "s1".to_owned();
        assert_eq!(
            read(root.path(), &by_id).unwrap_err(),
            "label_session_outside_roots"
        );
    }
}
