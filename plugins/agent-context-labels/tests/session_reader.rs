use agent_context_labels::{AgentKind, AgentSession, LocalSessionReader, Pane, SessionReader};
use hide_session::{EventKind, SESSION_LINE_LIMIT_BYTES, SkipReason};
use std::fs;

#[test]
fn labels_read_the_human_task_after_large_tool_results() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("session.jsonl");
    let tool = format!(
        "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"item_completed\",\"output\":\"{}\"}}}}\n",
        "x".repeat(SESSION_LINE_LIMIT_BYTES + 1)
    );
    let message = "{\"type\":\"response_item\",\"timestamp\":\"2026-09-30T00:00:00Z\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"라벨 플러그인을 복구해줘\"}]}}\n";
    fs::write(&path, format!("{tool}{message}")).unwrap();
    let pane = Pane {
        id: "w-fixture:p1".into(),
        agent: AgentKind::Codex,
        name: None,
        agent_session: Some(AgentSession::new("path", path.to_str().unwrap())),
        agent_status: "working".into(),
        revision: 0,
        state_change_seq: 0,
        cwd: None,
        focused: false,
    };
    let mut reader = LocalSessionReader::new(home.path());
    let first = reader.read(&pane).unwrap();
    assert_eq!(first.events.len(), 1);
    assert_eq!(first.events[0].kind, EventKind::Human);
    assert_eq!(first.events[0].text, "라벨 플러그인을 복구해줘");
    assert_eq!(
        first.skipped_reasons[&SkipReason::NonConversationCapacity],
        1
    );
    let unchanged = reader.read(&pane).unwrap();
    assert_eq!(unchanged.events, first.events);
    assert_eq!(unchanged.skipped_lines, 0);
}
