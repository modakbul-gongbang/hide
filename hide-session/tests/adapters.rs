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
