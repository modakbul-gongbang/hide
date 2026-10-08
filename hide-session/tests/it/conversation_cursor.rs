use hide_session::{
    Agent, ConversationCursor, EventKind, RescanReason, SESSION_LINE_LIMIT_BYTES, SessionError,
    SkipReason,
};
use serde_json::json;
use std::fs;
use std::io::Write;

fn human(text: &str) -> String {
    format!(
        "{}\n",
        json!({
            "timestamp": "2026-09-30T00:00:00Z", "type": "response_item",
            "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]}
        })
    )
}

#[test]
fn oversized_tool_record_does_not_hide_human_turns_or_their_offsets() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let first = human("작업 라벨을 고쳐줘");
    let tool = format!(
        "{}\n",
        json!({
            "type": "event_msg", "payload": {"type": "item_completed", "output": "x".repeat(SESSION_LINE_LIMIT_BYTES * 5)}
        })
    );
    let last = human("도구 실행 결과 이후에도 계속해줘");
    fs::write(&path, format!("{first}{tool}{last}")).unwrap();
    let mut reader = ConversationCursor::new();
    let mut events = Vec::new();
    let mut offsets = Vec::new();
    let mut oversized = 0;
    for poll in 0..3 {
        let parsed = reader.read(Agent::Codex, &path).unwrap();
        assert_eq!(reader.has_more(), poll == 0);
        events.extend(parsed.events);
        offsets.extend(parsed.event_offsets);
        oversized += parsed
            .skipped_reasons
            .get(&SkipReason::NonConversationCapacity)
            .copied()
            .unwrap_or(0);
    }
    assert_eq!(
        events
            .iter()
            .map(|event| event.text.as_str())
            .collect::<Vec<_>>(),
        ["작업 라벨을 고쳐줘", "도구 실행 결과 이후에도 계속해줘"]
    );
    assert!(events.iter().all(|event| event.kind == EventKind::Human));
    assert_eq!(offsets, [0, (first.len() + tool.len()) as u64]);
    assert_eq!(oversized, 1);
}

#[test]
fn split_oversized_tool_result_resumes_only_at_the_next_record() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let prefix =
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"output\":\"";
    fs::write(
        &path,
        format!("{prefix}{}", "x".repeat(SESSION_LINE_LIMIT_BYTES + 1)),
    )
    .unwrap();
    let mut reader = ConversationCursor::new();
    assert!(reader.read(Agent::Codex, &path).unwrap().events.is_empty());
    assert!(
        !reader.has_more(),
        "a torn record at EOF waits for an append"
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(format!("\"}}}}\n{}", human("완료 결과를 확인해줘")).as_bytes())
        .unwrap();
    let parsed = reader.read(Agent::Codex, &path).unwrap();
    assert_eq!(parsed.events.len(), 1);
    assert_eq!(parsed.events[0].text, "완료 결과를 확인해줘");
}

#[test]
fn oversized_conversation_and_unclassifiable_records_still_fail() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    for contents in [
        human(&"x".repeat(SESSION_LINE_LIMIT_BYTES)),
        "x".repeat(SESSION_LINE_LIMIT_BYTES + 1),
    ] {
        fs::write(&path, contents).unwrap();
        assert!(
            matches!(ConversationCursor::new().read(Agent::Codex, &path),
            Err(SessionError::Capacity { resource: "line_bytes", limit }) if limit == SESSION_LINE_LIMIT_BYTES as u64)
        );
    }
}

#[test]
fn oversized_native_questions_answers_and_plan_records_fail_without_consuming_them() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let huge = "x".repeat(SESSION_LINE_LIMIT_BYTES + 1);
    let records = [
        (
            Agent::Claude,
            json!({"type":"assistant","message":{"content":[{
                "type":"tool_use","id":"question-1","name":"AskUserQuestion",
                "input":{"questions":[{"question":huge}]}
            }]}}),
        ),
        (
            Agent::Claude,
            json!({"type":"user","message":{"content":[{
                "type":"tool_result","tool_use_id":"question-1","content":huge
            }]}}),
        ),
        (
            Agent::Codex,
            json!({"type":"response_item","payload":{
                "type":"function_call","call_id":"question-1","name":"request_user_input",
                "arguments":json!({"questions":[{"question":huge}]}).to_string()
            }}),
        ),
        (
            Agent::Codex,
            json!({"type":"response_item","payload":{
                "type":"function_call_output","call_id":"question-1","output":huge
            }}),
        ),
        (
            Agent::Codex,
            json!({"type":"event_msg","payload":{
                "type":"item_completed","turn_id":"turn-1","item":{"type":"Plan","text":huge}
            }}),
        ),
        (
            Agent::Codex,
            json!({"type":"event_msg","payload":{
                "type":"task_started","turn_id":"turn-1","collaboration_mode_kind":"plan","extra":huge
            }}),
        ),
        (
            Agent::Codex,
            json!({"type":"event_msg","payload":{
                "type":"task_complete","turn_id":"turn-1","extra":huge
            }}),
        ),
        (
            Agent::Codex,
            json!({"type":"event_msg","payload":{
                "type":"turn_aborted","turn_id":"turn-1","extra":huge
            }}),
        ),
    ];
    for (agent, record) in records {
        fs::write(&path, format!("{record}\n{}", human("after native record"))).unwrap();
        let mut reader = ConversationCursor::new();
        for _ in 0..2 {
            assert!(
                matches!(reader.read(agent, &path),
                Err(SessionError::Capacity { resource: "line_bytes", limit })
                    if limit == SESSION_LINE_LIMIT_BYTES as u64),
                "{agent:?}"
            );
            assert_eq!(reader.checkpoint().offset(), 0);
        }
    }
}

#[test]
fn a_split_native_question_cannot_be_discarded_after_checkpoint_restore() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    // Both native discriminators arrive after the discarded body, so the
    // scanner must finish the same bounded block after a durable restore.
    let complete = format!(
        "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"input\":{{\"questions\":[{{\"question\":\"{}\"}}]}},\"name\":\"AskUserQuestion\",\"type\":\"tool_use\",\"id\":\"question-1\"}}]}}}}\n",
        "x".repeat(SESSION_LINE_LIMIT_BYTES + 1)
    );
    let split = complete.find("\"name\"").unwrap();
    fs::write(&path, &complete[..split]).unwrap();
    let mut reader = ConversationCursor::new();
    assert!(
        reader
            .read(Agent::Claude, &path)
            .unwrap()
            .turn_marks
            .is_empty()
    );
    assert!(!reader.has_more(), "a torn record waits for its append");
    let checkpoint = serde_json::to_value(reader.checkpoint()).unwrap();
    fs::write(&path, &complete).unwrap();
    for old_checkpoint in [false, true] {
        let mut checkpoint = checkpoint.clone();
        if old_checkpoint {
            checkpoint["classifier"]
                .as_object_mut()
                .unwrap()
                .remove("user_turn_scanned");
        }
        let mut restored = ConversationCursor::restore(serde_json::from_value(checkpoint).unwrap());
        assert!(matches!(
            restored.read(Agent::Claude, &path),
            Err(SessionError::Capacity {
                resource: "line_bytes",
                ..
            })
        ));
        assert_eq!(restored.checkpoint().offset(), split as u64);
    }
}

#[test]
fn replacement_during_discard_keeps_the_new_session_human_turn() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    fs::write(
        &path,
        format!(
            "{{\"type\":\"event_msg\",\"body\":\"{}",
            "x".repeat(SESSION_LINE_LIMIT_BYTES + 1)
        ),
    )
    .unwrap();
    let mut reader = ConversationCursor::new();
    reader.read(Agent::Codex, &path).unwrap();
    let replacement = directory.path().join("replacement.jsonl");
    fs::write(&replacement, human("새 세션 요청")).unwrap();
    fs::rename(replacement, &path).unwrap();
    let parsed = reader.read(Agent::Codex, &path).unwrap();
    assert_eq!(parsed.rescan_reason, Some(RescanReason::Replaced));
    assert_eq!(parsed.events[0].text, "새 세션 요청");
    assert_eq!(parsed.event_offsets, [0]);
}

#[test]
fn claude_progress_does_not_hide_its_title_or_human_turn() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let ignored = format!(
        "{{\"type\":\"progress\",\"data\":\"{}\"}}\n",
        "x".repeat(SESSION_LINE_LIMIT_BYTES + 1)
    );
    let title = "{\"type\":\"ai-title\",\"aiTitle\":\"설치 문제 확인\"}\n";
    let human = "{\"type\":\"user\",\"origin\":{\"kind\":\"human\"},\"timestamp\":\"2026-09-30T00:00:00Z\",\"message\":{\"content\":\"계속 확인해줘\"}}\n";
    fs::write(&path, format!("{ignored}{title}{human}")).unwrap();
    let parsed = ConversationCursor::new()
        .read(Agent::Claude, &path)
        .unwrap();
    assert_eq!(parsed.title.as_deref(), Some("설치 문제 확인"));
    assert_eq!(parsed.events[0].text, "계속 확인해줘");
    assert_eq!(parsed.event_offsets, [(ignored.len() + title.len()) as u64]);
}
