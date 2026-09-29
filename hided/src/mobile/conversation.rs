//! An agent's conversation on the phone: the 대화 half of its detail.
//!
//! Claude Code and Codex draw in the terminal's alternate screen, so Herdr
//! keeps no scrollback for their panes and `pane.read` holds only the screen.
//! The whole conversation is in the agent's own transcript, found from the
//! session Herdr reports for the pane and read through `hide-session`, on
//! this Mac only: an SSH device's transcript is on that device, so its detail
//! shows the terminal alone. A pane with no reported session shows the
//! terminal too, because the newest transcript in its folder may be a
//! neighbour's.
//!
//! A page is the newest `PAGE_MESSAGES` messages before a byte offset, read
//! backwards in `WINDOW_BYTES` windows and never more than
//! `PAGE_BUDGET_BYTES` a request; the offset of its oldest message is the
//! cursor the phone sends back for the page before it. What the agent appends
//! is read the same way down to where the last read ended, so a transcript
//! line of any size is passed over rather than stopping the reader. Only the
//! operator's messages, the agent's text and an interruption leave hided:
//! tool calls, tool output and injected context never do.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hide_herdr_client::ApiConnector;
use hide_herdr_client::wire::success_response::{AgentSessionRefKind, ResponseResult};
use hide_session::{
    Agent, EventKind, SessionError, SessionIdentity, SessionLocator, parse_events_at,
    read_page_before,
};
use serde::Serialize;

use super::pane::PaneError;

/// The messages one page carries at most.
pub const PAGE_MESSAGES: usize = 30;
/// The text one page carries at most, past its newest message.
const PAGE_TEXT_BYTES: usize = 256 * 1024;
/// One backwards read.
const WINDOW_BYTES: u64 = 512 * 1024;
/// The most one page or one catch-up reads; a longer catch-up starts over
/// from the newest page.
const PAGE_BUDGET_BYTES: u64 = 8 * 1024 * 1024;
/// The characters one message carries; the rest is cut and marked.
pub const MESSAGE_CHARS: usize = 20_000;

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Message {
    /// The byte offset of its transcript record: unique and in order.
    pub id: u64,
    /// `you`, `agent`, or `stopped` for an interruption.
    pub who: &'static str,
    pub text: String,
    pub truncated: bool,
    pub at_ms: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Page {
    /// Oldest first.
    pub messages: Vec<Message>,
    /// The cursor for the page before this one; `None` at the start.
    pub before: Option<u64>,
}

/// Which transcript a pane's agent writes, as Herdr reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    agent: Agent,
    identity: SessionIdentity,
    cwd: Option<String>,
}

/// The pane's transcript source; `None` when it is not a Claude or Codex
/// agent with a reported session.
pub fn source(
    connector: &Arc<dyn ApiConnector>,
    pane_id: &str,
) -> Result<Option<Source>, PaneError> {
    let value = hide_herdr_client::request_with_connector(
        connector.as_ref(),
        "pane.get",
        serde_json::json!({"pane_id": pane_id}),
        TIMEOUT,
    )
    .map_err(|error| match error.code() {
        Some(code) if code.contains("not_found") => PaneError::Gone,
        _ => PaneError::Unavailable(error.to_string()),
    })?;
    let pane = match serde_json::from_value::<ResponseResult>(value) {
        Ok(ResponseResult::PaneInfo { pane }) => pane,
        Ok(_) => {
            return Err(PaneError::Unavailable(
                "pane.get returned no pane".to_owned(),
            ));
        }
        Err(error) => {
            return Err(PaneError::Unavailable(format!(
                "pane.get response is malformed: {error}"
            )));
        }
    };
    let Some(session) = pane.agent_session else {
        return Ok(None);
    };
    let agent = match session.agent.as_str() {
        "claude" => Agent::Claude,
        "codex" => Agent::Codex,
        _ => return Ok(None),
    };
    let identity = match session.kind {
        AgentSessionRefKind::Id => SessionIdentity::id(session.value),
        AgentSessionRefKind::Path => SessionIdentity::path(session.value),
    };
    Ok(Some(Source {
        agent,
        identity,
        cwd: pane.cwd,
    }))
}

/// One pane's open transcript.
#[derive(Debug)]
pub struct Transcript {
    source: Source,
    path: PathBuf,
    /// Where the messages the phone holds end: a line boundary.
    end: u64,
    /// The file's length at the last read, so an idle poll reads nothing.
    length: u64,
}

/// What the agent appended since the last poll.
#[derive(Debug, Eq, PartialEq)]
pub enum Tail {
    Messages(Vec<Message>),
    /// The file shrank, was replaced, or ran too far ahead: read it anew.
    Reset,
}

impl Transcript {
    /// Finds the transcript and reads its newest page. `SessionFileMissing`
    /// means the session is reported but has written nothing yet.
    pub fn open(home: &Path, pane_id: &str, source: Source) -> Result<(Self, Page), SessionError> {
        let path = SessionLocator::new(home).locate(
            pane_id,
            source.agent,
            Some(&source.identity),
            source.cwd.as_deref(),
        )?;
        let length = file_length(&path)?;
        let (page, end) = page_before(&path, source.agent, None)?;
        Ok((
            Self {
                source,
                path,
                end,
                length,
            },
            page,
        ))
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    /// What reads the page before a cursor, apart from the transcript, so it
    /// can run while the transcript keeps polling.
    pub fn pager(&self) -> Pager {
        Pager {
            path: self.path.clone(),
            agent: self.source.agent,
        }
    }

    /// The messages appended since the last read.
    pub fn poll(&mut self) -> Result<Tail, SessionError> {
        let length = file_length(&self.path)?;
        if length == self.length {
            return Ok(Tail::Messages(Vec::new()));
        }
        if length < self.end {
            return Ok(Tail::Reset);
        }
        self.length = length;
        let mut messages = Vec::new();
        let mut end = None;
        let mut newest_end = None;
        let mut read = 0;
        loop {
            let chunk = read_page_before(&self.path, end, WINDOW_BYTES)?;
            newest_end.get_or_insert(chunk.end_offset);
            read += chunk.end_offset - chunk.start_offset;
            let mut found = messages_in(self.source.agent, &chunk.contents, chunk.start_offset);
            found.retain(|message| message.id >= self.end);
            found.append(&mut messages);
            messages = found;
            if chunk.start_offset <= self.end {
                break;
            }
            if read > PAGE_BUDGET_BYTES {
                return Ok(Tail::Reset);
            }
            end = Some(chunk.start_offset);
        }
        self.end = newest_end.unwrap_or(self.end);
        Ok(Tail::Messages(messages))
    }
}

/// Reads older pages of one transcript.
#[derive(Clone, Debug)]
pub struct Pager {
    path: PathBuf,
    agent: Agent,
}

impl Pager {
    pub fn before(&self, cursor: u64) -> Result<Page, SessionError> {
        page_before(&self.path, self.agent, Some(cursor)).map(|(page, _)| page)
    }
}

fn file_length(path: &Path) -> Result<u64, SessionError> {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|_| SessionError::SessionFileMissing)
}

/// The newest page before `end` (the file's end when `None`), and where it ends.
fn page_before(path: &Path, agent: Agent, end: Option<u64>) -> Result<(Page, u64), SessionError> {
    let mut messages: Vec<Message> = Vec::new();
    let mut end = end;
    let mut newest_end = None;
    let mut read = 0;
    let before = loop {
        let chunk = read_page_before(path, end, WINDOW_BYTES)?;
        newest_end.get_or_insert(chunk.end_offset);
        read += chunk.end_offset - chunk.start_offset;
        let mut found = messages_in(agent, &chunk.contents, chunk.start_offset);
        found.append(&mut messages);
        messages = found;
        if let Some(keep) = page_start(&messages) {
            messages.drain(..keep);
            break messages.first().map(|message| message.id);
        }
        if chunk.start_offset == 0 {
            break None;
        }
        if read >= PAGE_BUDGET_BYTES {
            break Some(chunk.start_offset);
        }
        end = Some(chunk.start_offset);
    };
    let end = newest_end.unwrap_or(0);
    Ok((Page { messages, before }, end))
}

/// Where a full page starts in `messages` (oldest first), or `None` while
/// they do not fill one: `PAGE_MESSAGES` of them, or `PAGE_TEXT_BYTES` of
/// text past the newest.
fn page_start(messages: &[Message]) -> Option<usize> {
    let mut text = 0;
    for (taken, message) in messages.iter().rev().enumerate() {
        if taken == PAGE_MESSAGES || (taken > 0 && text + message.text.len() > PAGE_TEXT_BYTES) {
            return Some(messages.len() - taken);
        }
        text += message.text.len();
    }
    None
}

/// The messages a phone shows from one chunk of complete lines.
fn messages_in(agent: Agent, contents: &str, start_offset: u64) -> Vec<Message> {
    let parsed = parse_events_at(agent, contents, start_offset);
    parsed
        .events
        .into_iter()
        .zip(parsed.event_offsets)
        .filter_map(|(event, id)| {
            let who = match event.kind {
                EventKind::Human => "you",
                EventKind::Assistant => "agent",
                EventKind::Interrupted => "stopped",
                EventKind::Injected => return None,
            };
            let text = event.text.trim();
            let truncated = text.chars().nth(MESSAGE_CHARS).is_some();
            let text = if truncated {
                text.chars().take(MESSAGE_CHARS).collect()
            } else {
                text.to_owned()
            };
            Some(Message {
                id,
                who,
                text,
                truncated,
                at_ms: event.at_unix_ms,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn human(text: &str, second: u32) -> String {
        serde_json::json!({
            "type": "user", "userType": "external", "promptId": "p",
            "timestamp": format!("2026-09-29T10:00:{second:02}Z"),
            "message": {"role": "user", "content": text},
        })
        .to_string()
    }

    fn assistant(text: &str, second: u32) -> String {
        serde_json::json!({
            "type": "assistant", "timestamp": format!("2026-09-29T10:00:{second:02}Z"),
            "message": {"role": "assistant", "content": [{"type": "text", "text": text}]},
        })
        .to_string()
    }

    fn tool_output(size: usize) -> String {
        serde_json::json!({
            "type": "user", "timestamp": "2026-09-29T10:00:00Z",
            "message": {"role": "user", "content": [{"type": "tool_result", "content": "x".repeat(size)}]},
        })
        .to_string()
    }

    fn transcript(lines: &[String]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.jsonl");
        std::fs::write(
            &path,
            lines
                .iter()
                .map(|line| format!("{line}\n"))
                .collect::<String>(),
        )
        .unwrap();
        (directory, path)
    }

    fn texts(page: &[Message]) -> Vec<&str> {
        page.iter().map(|message| message.text.as_str()).collect()
    }

    #[test]
    fn a_page_holds_the_newest_messages_and_the_cursor_walks_back_to_the_start() {
        let lines: Vec<String> = (0..70)
            .map(|index| assistant(&format!("m{index}"), index % 60))
            .collect();
        let (_directory, path) = transcript(&lines);
        let (newest, _) = page_before(&path, Agent::Claude, None).unwrap();
        assert_eq!(newest.messages.len(), PAGE_MESSAGES);
        assert_eq!(newest.messages.last().unwrap().text, "m69");
        let older = Pager {
            path: path.clone(),
            agent: Agent::Claude,
        }
        .before(newest.before.unwrap())
        .unwrap();
        assert_eq!(texts(&older.messages).first(), Some(&"m10"));
        assert_eq!(texts(&older.messages).last(), Some(&"m39"));
        let oldest = Pager {
            path,
            agent: Agent::Claude,
        }
        .before(older.before.unwrap())
        .unwrap();
        assert_eq!(
            texts(&oldest.messages),
            (0..10).map(|index| format!("m{index}")).collect::<Vec<_>>()
        );
        assert_eq!(oldest.before, None);
    }

    #[test]
    fn tool_output_and_injected_context_never_reach_the_phone_and_huge_lines_are_passed_over() {
        let lines = [
            human("테스트 다시 돌려줘", 1),
            tool_output(3 * 1024 * 1024),
            human("<system-reminder>internal</system-reminder>", 2),
            assistant("다 통과했어요", 3),
        ];
        let (_directory, path) = transcript(&lines);
        let (page, _) = page_before(&path, Agent::Claude, None).unwrap();
        assert_eq!(
            texts(&page.messages),
            ["테스트 다시 돌려줘", "다 통과했어요"]
        );
        assert_eq!(
            page.messages
                .iter()
                .map(|message| message.who)
                .collect::<Vec<_>>(),
            ["you", "agent"]
        );
        assert_eq!(page.before, None);
    }

    #[test]
    fn a_poll_returns_only_appended_messages_even_past_a_huge_line() {
        let (_directory, path) = transcript(&[human("첫 질문", 1), assistant("첫 답", 2)]);
        let length = file_length(&path).unwrap();
        let (_, end) = page_before(&path, Agent::Claude, None).unwrap();
        let mut open = Transcript {
            source: Source {
                agent: Agent::Claude,
                identity: SessionIdentity::id("s"),
                cwd: None,
            },
            path: path.clone(),
            end,
            length,
        };
        assert_eq!(open.poll().unwrap(), Tail::Messages(Vec::new()));
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        // A line still being written ends the append.
        write!(
            file,
            "{}\n{}\n{{\"type\":\"assi",
            tool_output(700 * 1024),
            assistant("두 번째 답", 3)
        )
        .unwrap();
        let Tail::Messages(appended) = open.poll().unwrap() else {
            panic!("reset")
        };
        assert_eq!(texts(&appended), ["두 번째 답"]);
        writeln!(file, "stant\"}}").unwrap();
        writeln!(file, "{}", human("다음", 4)).unwrap();
        let Tail::Messages(appended) = open.poll().unwrap() else {
            panic!("reset")
        };
        assert_eq!(texts(&appended), ["다음"]);
    }

    #[test]
    fn a_long_message_is_cut_and_marked() {
        let (_directory, path) = transcript(&[assistant(&"가".repeat(MESSAGE_CHARS + 5), 1)]);
        let (page, _) = page_before(&path, Agent::Claude, None).unwrap();
        assert!(page.messages[0].truncated);
        assert_eq!(page.messages[0].text.chars().count(), MESSAGE_CHARS);
    }
}
