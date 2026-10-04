//! The session adapter contract (PRD overview-request-view D-14, D-18):
//! every agent's real record format, at the version a fixture names, reads
//! to the same facts. A new agent, or a new version of one, is supported
//! when its fixture passes this test.
//!
//! Each fixture is one session in that agent's own format holding the same
//! conversation: the operator's two-line request, a hook's injected text,
//! a tool that printed a pull request address, an hcoord request from
//! `ci-lead`, an image sent alone, and a reply that mentions the pull
//! request and an older one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hide_session::label_transcript::{
    LabelEventKind, LabelTranscript, LabelTranscriptRequest, read,
};
use hide_session::{Agent, PrSighting};

/// 2026-10-03T01:00:00Z, when every fixture's conversation starts.
const START: u64 = 1_790_989_200_000;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/adapters")
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A home holding one agent's fixture where that agent keeps its sessions.
fn home(agent: Agent) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    match agent {
        Agent::Claude => copy_tree(
            &fixtures().join("claude-2.1.288"),
            &home.path().join(".claude"),
        ),
        Agent::Codex => copy_tree(
            &fixtures().join("codex-0.160.0"),
            &home.path().join(".codex"),
        ),
        Agent::OpenCode => {
            let folder = home.path().join(".local/share/opencode");
            std::fs::create_dir_all(&folder).unwrap();
            let database = rusqlite::Connection::open(folder.join("opencode.db")).unwrap();
            database
                .execute_batch(
                    &std::fs::read_to_string(fixtures().join("opencode-1.18.30/opencode.sql"))
                        .unwrap(),
                )
                .unwrap();
        }
    }
    home
}

fn request(agent: Agent) -> LabelTranscriptRequest {
    let id = match agent {
        Agent::Claude => "a1b2c3d4-0000-4000-8000-000000000001",
        Agent::Codex => "0199a000-0000-7000-8000-000000000002",
        Agent::OpenCode => "ses_0a1b2c3d4e5f60718293a4b5c6",
    };
    LabelTranscriptRequest {
        agent,
        reference_kind: "id".to_owned(),
        reference_value: id.to_owned(),
        cwd: Some("/work/app".to_owned()),
        checkpoint: None,
        subagents: BTreeMap::new(),
    }
}

/// Reads until nothing is left, as the label worker does.
fn read_whole(home: &Path, agent: Agent) -> LabelTranscript {
    let mut request = request(agent);
    let mut whole = read(home, &request).unwrap();
    while whole.has_more {
        request.checkpoint = Some(whole.checkpoint.clone());
        request.subagents = whole.subagents.clone();
        let next = read(home, &request).unwrap();
        whole.events.extend(next.events);
        whole.pr_sightings.extend(next.pr_sightings);
        whole.has_more = next.has_more;
        whole.checkpoint = next.checkpoint;
        whole.subagents = next.subagents;
        whole.title = next.title.or(whole.title);
        whole.custom_title = next.custom_title.or(whole.custom_title);
    }
    whole
}

fn sighted(transcript: &LabelTranscript, number: u64) -> Option<&PrSighting> {
    transcript
        .pr_sightings
        .iter()
        .find(|sighting| sighting.repository == "acme/app" && sighting.number == number)
}

/// Real provider records with deliberately priced byte lengths, rather than
/// a mocked reader: the adapter contract admits 2MiB of subagents per poll.
fn subagent_poll_fixture() -> (tempfile::TempDir, PathBuf, LabelTranscriptRequest) {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join(".claude/projects/-work-app");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("budget-session.jsonl");
    let record = serde_json::json!({"type":"user","sessionId":"budget-session",
        "origin":{"kind":"human"},"timestamp":"2026-10-03T01:00:00Z",
        "message":{"role":"user","content":"read subagent pull requests"}});
    std::fs::write(&path, format!("{record}\n")).unwrap();
    let folder = project.join("budget-session/subagents");
    std::fs::create_dir_all(&folder).unwrap();
    let request = LabelTranscriptRequest {
        agent: Agent::Claude,
        reference_kind: "path".to_owned(),
        reference_value: path.display().to_string(),
        cwd: None,
        checkpoint: None,
        subagents: BTreeMap::new(),
    };
    (home, folder, request)
}

fn sized_tool_record(number: u64, bytes: usize) -> String {
    let mut record = serde_json::json!({"type":"user","sessionId":"budget-session",
        "timestamp":"2026-10-03T01:00:01Z","message":{"role":"user",
        "content":[{"type":"tool_result","tool_use_id":"t",
        "content":format!("https://github.com/acme/app/pull/{number}")}]}});
    let padding = bytes.checked_sub(record.to_string().len() + 1).unwrap();
    let text = record["message"]["content"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    record["message"]["content"][0]["content"] = format!("{text}{}", " ".repeat(padding)).into();
    let line = format!("{record}\n");
    assert_eq!(line.len(), bytes);
    line
}

fn write_poll_subagent(folder: &Path, index: u64, contents: &str) {
    let path = folder.join(format!("agent-{index}.jsonl"));
    std::fs::write(&path, contents).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 - index),
        )
        .unwrap();
}

fn resume_subagent_poll(request: &mut LabelTranscriptRequest, answer: &LabelTranscript) {
    // A relaunch restores only durable checkpoints, including the bounded
    // classifier of an oversized record; no pending transcript is persisted.
    request.checkpoint =
        Some(serde_json::from_slice(&serde_json::to_vec(&answer.checkpoint).unwrap()).unwrap());
    request.subagents =
        serde_json::from_slice(&serde_json::to_vec(&answer.subagents).unwrap()).unwrap();
}

#[test]
fn claude_subagent_poll_caps_aggregate_bytes_and_resumes_a_partial_record_once() {
    let (home, folder, mut request) = subagent_poll_fixture();
    for index in 1..=3 {
        let contents: String = (1..=9)
            .map(|record| sized_tool_record(index * 100 + record, 100 * 1024))
            .collect();
        write_poll_subagent(&folder, index, &contents);
    }
    let first = read(home.path(), &request).unwrap();
    let committed_bytes: u64 = first.subagents.values().map(|cursor| cursor.offset()).sum();
    assert!(
        committed_bytes <= 2 * 1024 * 1024,
        "one subagent poll committed {committed_bytes} bytes against the 2MiB contract"
    );
    assert_eq!(first.subagents["agent-1.jsonl"].offset(), 900 * 1024);
    assert_eq!(first.subagents["agent-2.jsonl"].offset(), 900 * 1024);
    assert_eq!(first.subagents["agent-3.jsonl"].offset(), 200 * 1024);
    assert!(first.has_more);
    let mut sightings: Vec<_> = first.pr_sightings.iter().map(|pr| pr.number).collect();
    assert_eq!(
        sightings,
        (101..=109)
            .chain(201..=209)
            .chain(301..=302)
            .collect::<Vec<_>>()
    );
    resume_subagent_poll(&mut request, &first);
    let next = read(home.path(), &request).unwrap();
    sightings.extend(next.pr_sightings.iter().map(|pr| pr.number));
    assert_eq!(
        sightings,
        (101..=109)
            .chain(201..=209)
            .chain(301..=309)
            .collect::<Vec<_>>()
    );
    assert_eq!(next.subagents["agent-3.jsonl"].offset(), 900 * 1024);
    assert!(!next.has_more);
    resume_subagent_poll(&mut request, &next);
    assert!(read(home.path(), &request).unwrap().pr_sightings.is_empty());
}

#[test]
fn claude_subagent_poll_resumes_an_oversized_tool_discard_across_its_budget() {
    let (home, folder, mut request) = subagent_poll_fixture();
    for index in 1..=2 {
        let contents: String = (1..=7)
            .map(|record| sized_tool_record(index * 100 + record, 100 * 1024))
            .collect();
        write_poll_subagent(&folder, index, &contents);
    }
    write_poll_subagent(
        &folder,
        3,
        &(sized_tool_record(300, 900 * 1024) + &sized_tool_record(301, 1024)),
    );
    let first = read(home.path(), &request).unwrap();
    assert_eq!(first.subagents["agent-3.jsonl"].offset(), 648 * 1024);
    assert!(first.has_more);
    assert!(first.pr_sightings.iter().all(|pr| pr.number < 300));
    resume_subagent_poll(&mut request, &first);
    let next = read(home.path(), &request).unwrap();
    assert_eq!(
        next.pr_sightings
            .iter()
            .map(|pr| pr.number)
            .collect::<Vec<_>>(),
        [301]
    );
    assert!(!next.has_more);
    resume_subagent_poll(&mut request, &next);
    assert!(read(home.path(), &request).unwrap().pr_sightings.is_empty());
}

#[test]
fn claude_subagent_poll_counts_bytes_even_when_an_admitted_record_fails() {
    let (home, folder, mut request) = subagent_poll_fixture();
    let oversized = serde_json::json!({"type":"assistant","sessionId":"budget-session",
        "message":{"role":"assistant","content":[{"type":"text","text":"x".repeat(900 * 1024)}]}});
    for index in 1..=2 {
        write_poll_subagent(&folder, index, &format!("{oversized}\n"));
    }
    let contents: String = (301..=309)
        .map(|record| sized_tool_record(record, 100 * 1024))
        .collect();
    write_poll_subagent(&folder, 3, &contents);
    let first = read(home.path(), &request).unwrap();
    assert_eq!(
        first
            .pr_sightings
            .iter()
            .map(|pr| pr.number)
            .collect::<Vec<_>>(),
        [301, 302]
    );
    assert!(first.has_more);
    assert!(!first.subagents.contains_key("agent-1.jsonl"));
    assert!(!first.subagents.contains_key("agent-2.jsonl"));
    // The provider removes the failed files. Valid deferred records still
    // resume from their last complete line, without losing or repeating one.
    for index in 1..=2 {
        std::fs::remove_file(folder.join(format!("agent-{index}.jsonl"))).unwrap();
    }
    resume_subagent_poll(&mut request, &first);
    let next = read(home.path(), &request).unwrap();
    assert_eq!(
        next.pr_sightings
            .iter()
            .map(|pr| pr.number)
            .collect::<Vec<_>>(),
        (303..=309).collect::<Vec<_>>()
    );
    assert!(!next.has_more);
}

#[test]
fn every_agent_reads_to_the_same_facts() {
    for agent in [Agent::Claude, Agent::Codex, Agent::OpenCode] {
        let home = home(agent);
        let transcript = read_whole(home.path(), agent);

        assert_eq!(
            transcript.title.as_deref(),
            Some("요청 보기 만들기"),
            "{agent:?} title"
        );

        let people: Vec<_> = transcript
            .events
            .iter()
            .filter(|event| event.kind == LabelEventKind::Human)
            .collect();
        assert_eq!(
            people.len(),
            3,
            "{agent:?}: injected text is no one's request"
        );
        assert_eq!(people[0].text, "요청 보기를 만들어줘\n긴 요청의 둘째 줄");
        assert_eq!(people[0].at_unix_ms, START, "{agent:?}");
        assert_eq!(people[0].sender, None);
        assert_eq!(people[1].sender.as_deref(), Some("ci-lead"), "{agent:?}");
        assert!(people[1].text.ends_with("CI 다시 봐줘"));
        assert_eq!(
            (people[2].text.trim(), people[2].images),
            ("", 1),
            "{agent:?}: an image sent alone, without what the agent wrapped it in"
        );

        let reply = transcript
            .events
            .iter()
            .rev()
            .find(|event| event.kind == LabelEventKind::Assistant)
            .unwrap();
        assert!(
            reply
                .text
                .starts_with("PR을 열었습니다: https://github.com/acme/app/pull/12"),
            "{agent:?}"
        );

        let created = sighted(&transcript, 12).unwrap_or_else(|| panic!("{agent:?} sighting"));
        assert_eq!(
            created.at_unix_ms,
            START + 30_000,
            "{agent:?}: when the tool printed it"
        );
        assert!(
            sighted(&transcript, 99).is_none(),
            "{agent:?}: a reply's mention is not a tool's output"
        );
    }
}

#[test]
fn what_only_one_agent_records_is_read_from_it() {
    // Claude Code: the operator's rename, and a subagent's tools.
    let claude_home = home(Agent::Claude);
    let claude = read_whole(claude_home.path(), Agent::Claude);
    assert_eq!(claude.custom_title.as_deref(), Some("운영자 이름"));
    assert_eq!(sighted(&claude, 13).unwrap().at_unix_ms, START + 90_000);
    assert!(claude.subagents.contains_key("agent-a1.jsonl"));

    // Codex: a custom tool's output.
    let codex_home = home(Agent::Codex);
    let codex = read_whole(codex_home.path(), Agent::Codex);
    assert_eq!(sighted(&codex, 14).unwrap().at_unix_ms, START + 40_000);
}

#[test]
fn a_locked_opencode_database_is_a_skipped_read_that_never_blocks_opencode() {
    let home = home(Agent::OpenCode);
    let path = home.path().join(".local/share/opencode/opencode.db");
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        read(home.path(), &request(Agent::OpenCode)).unwrap_err(),
        "opencode_db_busy"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    // OpenCode's write goes on and the next read takes it.
    writer.execute_batch("COMMIT;").unwrap();
    assert!(read(home.path(), &request(Agent::OpenCode)).is_ok());
}

#[test]
fn an_opencode_message_still_being_written_waits_for_the_next_read() {
    let home = home(Agent::OpenCode);
    let path = home.path().join(".local/share/opencode/opencode.db");
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute(
            "INSERT INTO message VALUES ('msg_07', ?1, ?2, ?2, ?3)",
            rusqlite::params![
                "ses_0a1b2c3d4e5f60718293a4b5c6",
                START + 200_000,
                r#"{"role":"assistant","time":{"created":1790989400000}}"#
            ],
        )
        .unwrap();
    let first = read(home.path(), &request(Agent::OpenCode)).unwrap();
    assert!(!first.has_more);
    let mut again = request(Agent::OpenCode);
    again.checkpoint = Some(first.checkpoint.clone());
    assert!(read(home.path(), &again).unwrap().events.is_empty());
}

/// Adds an operator message with one text part of `bytes` bytes.
fn add_opencode_request(home: &Path, index: u64, bytes: usize) {
    let path = home.join(".local/share/opencode/opencode.db");
    let writer = rusqlite::Connection::open(path).unwrap();
    let at = START + 300_000 + index;
    let message = format!("msg_big_{index:03}");
    writer
        .execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?3, ?4)",
            rusqlite::params![
                message,
                "ses_0a1b2c3d4e5f60718293a4b5c6",
                at,
                format!(r#"{{"role":"user","time":{{"created":{at}}}}}"#)
            ],
        )
        .unwrap();
    writer
        .execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            rusqlite::params![
                format!("prt_big_{index:03}"),
                message,
                "ses_0a1b2c3d4e5f60718293a4b5c6",
                at,
                format!(
                    r#"{{"type":"text","text":"{index} {}"}}"#,
                    "x".repeat(bytes)
                )
            ],
        )
        .unwrap();
}

#[test]
fn an_opencode_part_over_the_row_limit_is_skipped_without_being_read() {
    let home = home(Agent::OpenCode);
    add_opencode_request(home.path(), 0, hide_session::SESSION_LINE_LIMIT_BYTES);
    let transcript = read_whole(home.path(), Agent::OpenCode);
    let first = read(home.path(), &request(Agent::OpenCode)).unwrap();
    assert_eq!(first.skipped_reasons.get("part_capacity"), Some(&1));
    assert!(
        transcript
            .events
            .iter()
            .all(|event| !event.text.starts_with("0 x")),
        "the oversized request is no event"
    );
}

#[test]
fn a_long_opencode_session_is_read_a_budget_at_a_time_and_whole_in_the_end() {
    let home = home(Agent::OpenCode);
    for index in 0..8 {
        add_opencode_request(home.path(), index, 200 * 1024);
    }
    let first = read(home.path(), &request(Agent::OpenCode)).unwrap();
    assert!(
        first.has_more,
        "eight 200 KiB requests are more than one read"
    );
    let loaded: usize = first.events.iter().map(|event| event.text.len()).sum();
    assert!(loaded <= 1024 * 1024 + 4096, "{loaded}");

    let whole = read_whole(home.path(), Agent::OpenCode);
    let big: Vec<_> = whole
        .events
        .iter()
        .filter(|event| event.text.ends_with("xxxx"))
        .map(|event| event.text.split(' ').next().unwrap().to_owned())
        .collect();
    assert_eq!(big, ["0", "1", "2", "3", "4", "5", "6", "7"]);
}

#[test]
fn opencode_metadata_uses_the_read_budget_even_when_it_cannot_be_parsed() {
    let home = home(Agent::OpenCode);
    let path = home.path().join(".local/share/opencode/opencode.db");
    let writer = rusqlite::Connection::open(path).unwrap();
    writer
        .execute_batch("DELETE FROM part; DELETE FROM message;")
        .unwrap();
    // Each record fits the metadata row limit, but together they exceed
    // the read budget by more than ten times. Invalid JSON is still input
    // that the reader loaded, and must consume the same byte budget.
    let metadata = "x".repeat(63 * 1024);
    for index in 0..200 {
        writer
            .execute(
                "INSERT INTO message VALUES (?1, ?2, ?3, ?3, ?4)",
                rusqlite::params![
                    format!("msg_metadata_{index:03}"),
                    request(Agent::OpenCode).reference_value,
                    START + index,
                    metadata
                ],
            )
            .unwrap();
    }
    let first = read(home.path(), &request(Agent::OpenCode)).unwrap();
    assert!(first.events.is_empty());
    assert_eq!(first.checkpoint.offset(), 16);
    assert_eq!(first.skipped_reasons.get("malformed_json"), Some(&16));
    assert!(first.has_more, "remaining metadata belongs to a later read");
    let mut again = request(Agent::OpenCode);
    again.checkpoint = Some(first.checkpoint);
    let next = read(home.path(), &again).unwrap();
    assert_eq!(next.checkpoint.offset(), 32);
    assert_eq!(next.skipped_reasons.get("malformed_json"), Some(&16));
}

#[test]
fn opencode_empty_and_skipped_parts_have_a_work_cap_and_advance() {
    for data in [
        rusqlite::types::Value::Text(String::new()),
        rusqlite::types::Value::Blob(vec![0]),
    ] {
        let home = home(Agent::OpenCode);
        let writer =
            rusqlite::Connection::open(home.path().join(".local/share/opencode/opencode.db"))
                .unwrap();
        writer
            .execute_batch("DELETE FROM part; DELETE FROM message;")
            .unwrap();
        add_opencode_request(home.path(), 0, 10);
        writer
            .execute("DELETE FROM part WHERE message_id = 'msg_big_000'", [])
            .unwrap();
        for index in 0..2_000 {
            writer
                .execute(
                    "INSERT INTO part VALUES (?1, 'msg_big_000', ?2, ?3, ?3, ?4)",
                    rusqlite::params![
                        format!("prt_empty_{index:04}"),
                        request(Agent::OpenCode).reference_value,
                        START + index,
                        data
                    ],
                )
                .unwrap();
        }
        add_opencode_request(home.path(), 1, 10);
        let transcript = read(home.path(), &request(Agent::OpenCode)).unwrap();
        let reason = if matches!(data, rusqlite::types::Value::Text(_)) {
            "malformed_json"
        } else {
            "not_text"
        };
        assert_eq!(transcript.skipped_reasons.get(reason), Some(&256));
        assert_eq!(
            transcript.skipped_reasons.get("part_work_capacity"),
            Some(&1)
        );
        assert_eq!(transcript.checkpoint.offset(), 2);
        assert!(
            transcript
                .events
                .iter()
                .any(|event| event.text.starts_with("1 x"))
        );
        assert!(!transcript.has_more);
    }
}

#[test]
fn opencode_part_work_is_bounded_across_messages_and_resumes() {
    let home = home(Agent::OpenCode);
    let writer =
        rusqlite::Connection::open(home.path().join(".local/share/opencode/opencode.db")).unwrap();
    writer
        .execute_batch("DELETE FROM part; DELETE FROM message;")
        .unwrap();
    for message in 0..3 {
        add_opencode_request(home.path(), message, 10);
        if message == 2 {
            continue;
        }
        writer
            .execute(
                "DELETE FROM part WHERE message_id = ?1",
                [format!("msg_big_{message:03}")],
            )
            .unwrap();
        for index in 0..300 {
            writer
                .execute(
                    "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?4, '')",
                    rusqlite::params![
                        format!("prt_work_{message}_{index:03}"),
                        format!("msg_big_{message:03}"),
                        request(Agent::OpenCode).reference_value,
                        START + index
                    ],
                )
                .unwrap();
        }
    }
    let first = read(home.path(), &request(Agent::OpenCode)).unwrap();
    assert!(first.events.is_empty());
    assert_eq!(first.skipped_reasons.get("malformed_json"), Some(&512));
    assert_eq!(first.skipped_reasons.get("part_work_capacity"), Some(&2));
    assert_eq!(first.checkpoint.offset(), 2);
    assert!(first.has_more);
    let mut again = request(Agent::OpenCode);
    again.checkpoint = Some(first.checkpoint);
    let last = read(home.path(), &again).unwrap();
    assert!(
        last.events
            .iter()
            .any(|event| event.text.starts_with("2 x"))
    );
    assert_eq!(last.checkpoint.offset(), 3);
    assert!(!last.has_more);
}

#[test]
fn opencode_bounds_sql_work_when_a_part_sort_cannot_use_an_index() {
    let home = home(Agent::OpenCode);
    let writer =
        rusqlite::Connection::open(home.path().join(".local/share/opencode/opencode.db")).unwrap();
    writer
        .execute_batch("DELETE FROM part; DELETE FROM message;")
        .unwrap();
    add_opencode_request(home.path(), 0, 10);
    writer.execute_batch("WITH RECURSIVE rows(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM rows WHERE n<100000) INSERT INTO part SELECT printf('prt_many_%06d', n), 'msg_big_000', 'ses_0a1b2c3d4e5f60718293a4b5c6', n, n, '' FROM rows;").unwrap();
    assert_eq!(
        read(home.path(), &request(Agent::OpenCode)).unwrap_err(),
        "opencode_db_work_capacity"
    );
    let parts: i64 = writer
        .query_row("SELECT count(*) FROM part", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        parts, 100001,
        "the bounded read must not change the provider database"
    );
}

#[test]
fn an_oversized_first_opencode_message_has_bounded_output_and_allows_progress() {
    let home = home(Agent::OpenCode);
    let path = home.path().join(".local/share/opencode/opencode.db");
    let writer = rusqlite::Connection::open(path).unwrap();
    writer
        .execute_batch("DELETE FROM part; DELETE FROM message;")
        .unwrap();
    add_opencode_request(home.path(), 0, 200 * 1024);
    for index in 1..12 {
        writer
            .execute(
                "INSERT INTO part VALUES (?1, 'msg_big_000', ?2, ?3, ?3, ?4)",
                rusqlite::params![
                    format!("prt_extra_{index:03}"),
                    request(Agent::OpenCode).reference_value,
                    START + 300_000 + index,
                    serde_json::json!({ "type": "text", "text": "x".repeat(200 * 1024) })
                        .to_string()
                ],
            )
            .unwrap();
    }
    add_opencode_request(home.path(), 1, 200 * 1024);
    let first = read(home.path(), &request(Agent::OpenCode)).unwrap();
    let output_bytes: usize = first.events.iter().map(|event| event.text.len()).sum();
    assert!(output_bytes <= hide_session::SESSION_INCREMENT_READ_LIMIT_BYTES as usize);
    assert_eq!(first.checkpoint.offset(), 1);
    assert_eq!(first.skipped_reasons.get("read_budget"), Some(&7));
    assert!(first.has_more);
    let mut again = request(Agent::OpenCode);
    again.checkpoint = Some(first.checkpoint);
    let next = read(home.path(), &again).unwrap();
    assert!(
        next.events
            .iter()
            .any(|event| event.text.starts_with("1 x"))
    );
    assert_eq!(next.checkpoint.offset(), 2);
    assert!(!next.has_more);
}

#[test]
fn an_opencode_row_that_holds_no_text_or_too_much_is_skipped_and_the_read_goes_on() {
    let home = home(Agent::OpenCode);
    let path = home.path().join(".local/share/opencode/opencode.db");
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute(
            "UPDATE session SET title = ?1 WHERE id = ?2",
            rusqlite::params!["제".repeat(100_000), "ses_0a1b2c3d4e5f60718293a4b5c6"],
        )
        .unwrap();
    add_opencode_request(home.path(), 0, 10);
    writer
        .execute(
            "UPDATE part SET data = CAST(data AS BLOB) WHERE id = 'prt_big_000'",
            [],
        )
        .unwrap();
    add_opencode_request(home.path(), 1, 10);
    writer
        .execute(
            "UPDATE message SET data = json_set(data, '$.padding', ?1) WHERE id = 'msg_big_001'",
            rusqlite::params!["x".repeat(100 * 1024)],
        )
        .unwrap();
    add_opencode_request(home.path(), 2, 10);

    let first = read(home.path(), &request(Agent::OpenCode)).unwrap();
    assert_eq!(first.title, None, "a title past the cut is no title");
    assert_eq!(first.skipped_reasons.get("not_text"), Some(&1));
    assert_eq!(first.skipped_reasons.get("message_capacity"), Some(&1));
    assert!(
        first
            .events
            .iter()
            .any(|event| event.text.starts_with("2 x")),
        "the request after them is read"
    );
}
