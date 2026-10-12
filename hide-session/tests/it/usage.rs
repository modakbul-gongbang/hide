//! Source-shaped fixtures with invented identities and counts. The expected
//! disjoint totals follow PRD D-03, D-15 and the native usage field contracts.
use std::fs;
use std::io::Write;

use hide_session::{Agent, usage};

const CLAUDE: &str = concat!(
    r#"{"type":"assistant","sessionId":"session-a","requestId":"request-a","cwd":"/work/app","timestamp":"2026-10-01T23:00:00Z","message":{"id":"message-a","model":"claude-fixture","content":[{"text":"PRIVATE_BODY_SENTINEL"}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20,"cache_creation":{"ephemeral_1h_input_tokens":4,"ephemeral_5m_input_tokens":16}}}}"#,
    "\n"
);

#[test]
fn usage_read_resumes_torn_records_without_persisting_their_body() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(hide_session::CLAUDE_SESSIONS);
    fs::create_dir_all(&root).unwrap();
    let path = root.join("session.jsonl");
    let split = CLAUDE.len() - 20;
    fs::write(&path, &CLAUDE[..split]).unwrap();
    let first = usage::read(home.path(), Agent::Claude, &path, None).unwrap();
    assert!(first.events.is_empty());
    assert!(!first.has_more);
    assert_eq!(first.checkpoint.offset, 0);
    let checkpoint = serde_json::to_string(&first.checkpoint).unwrap();
    assert!(!checkpoint.contains("PRIVATE_BODY_SENTINEL"));
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&CLAUDE.as_bytes()[split..])
        .unwrap();
    let second = usage::read(
        home.path(),
        Agent::Claude,
        &path,
        Some(serde_json::from_str(&checkpoint).unwrap()),
    )
    .unwrap();
    assert_eq!(second.events.len(), 1);
    let event = &second.events[0];
    assert_eq!(
        (
            event.tokens.input,
            event.tokens.output,
            event.tokens.cache_read,
            event.tokens.cache_write,
            event.tokens.cache_write_1h
        ),
        (10, 5, 100, 20, 4)
    );
    assert!(
        !serde_json::to_string(&second)
            .unwrap()
            .contains("PRIVATE_BODY_SENTINEL")
    );
    let third = usage::read(home.path(), Agent::Claude, &path, Some(second.checkpoint)).unwrap();
    assert!(third.events.is_empty());
}

#[test]
fn usage_native_identity_survives_file_copy_and_streaming_updates() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(hide_session::CLAUDE_SESSIONS);
    fs::create_dir_all(&root).unwrap();
    let a = root.join("a.jsonl");
    let b = root.join("b.jsonl");
    fs::write(&a, CLAUDE).unwrap();
    fs::write(
        &b,
        CLAUDE.replace("\"output_tokens\":5", "\"output_tokens\":8"),
    )
    .unwrap();
    let a = usage::read(home.path(), Agent::Claude, &a, None).unwrap();
    let b = usage::read(home.path(), Agent::Claude, &b, None).unwrap();
    assert_eq!(a.events[0].key, b.events[0].key);
    assert_eq!(b.events[0].tokens.output, 8);
}

#[test]
fn usage_oversized_record_advances_in_bounded_steps_and_reports_its_loss() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(hide_session::CLAUDE_SESSIONS);
    fs::create_dir_all(&root).unwrap();
    let path = root.join("large.jsonl");
    let mut bytes = vec![b'x'; 2 * 1024 * 1024 + 3];
    bytes.push(b'\n');
    bytes.extend_from_slice(CLAUDE.as_bytes());
    fs::write(&path, bytes).unwrap();
    let first = usage::read(home.path(), Agent::Claude, &path, None).unwrap();
    assert_eq!(first.read_bytes, 1024 * 1024);
    assert_eq!(first.skipped.get("record_capacity"), Some(&1));
    let second = usage::read(home.path(), Agent::Claude, &path, Some(first.checkpoint)).unwrap();
    assert_eq!(second.read_bytes, 1024 * 1024);
    let third = usage::read(home.path(), Agent::Claude, &path, Some(second.checkpoint)).unwrap();
    assert_eq!(third.events.len(), 1);
    assert!(third.skipped.is_empty());
    assert!(!third.has_more);
}

#[test]
fn usage_discovery_includes_subagents_and_archived_sessions_and_refuses_escape() {
    let home = tempfile::tempdir().unwrap();
    let child = home
        .path()
        .join(hide_session::CLAUDE_SESSIONS)
        .join("project/session/subagents/agent-a.jsonl");
    fs::create_dir_all(child.parent().unwrap()).unwrap();
    fs::write(&child, CLAUDE).unwrap();
    assert_eq!(
        usage::list(home.path(), Agent::Claude, 0)
            .unwrap()
            .files
            .len(),
        1
    );
    let archive = home.path().join(".codex/archived_sessions/archive.jsonl");
    fs::create_dir_all(archive.parent().unwrap()).unwrap();
    fs::write(&archive, "").unwrap();
    assert_eq!(
        usage::list(home.path(), Agent::Codex, 0)
            .unwrap()
            .files
            .len(),
        1
    );
    assert_eq!(
        usage::read(home.path(), Agent::Claude, &archive, None).unwrap_err(),
        usage::Refusal::OutsideRoot
    );
}

#[test]
fn usage_codex_cache_and_reasoning_are_not_added_twice() {
    let home = tempfile::tempdir().unwrap();
    let path = home
        .path()
        .join(hide_session::CODEX_SESSIONS)
        .join("rollout.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-a\",\"cwd\":\"/work/app\"}}\n",
        "{\"type\":\"turn_context\",\"payload\":{\"model\":\"model-a\"}}\n",
        "{\"type\":\"event_msg\",\"timestamp\":\"2026-10-01T12:00:00Z\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":80,\"output_tokens\":10,\"reasoning_output_tokens\":3},\"total_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":80,\"output_tokens\":10,\"reasoning_output_tokens\":3}}}}\n"
    )).unwrap();
    let step = usage::read(home.path(), Agent::Codex, &path, None).unwrap();
    assert_eq!(step.events.len(), 1);
    let t = &step.events[0].tokens;
    assert_eq!(
        (t.input, t.output, t.cache_read, t.reasoning),
        (20, 10, 80, Some(3))
    );
}
