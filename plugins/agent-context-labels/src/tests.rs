use super::*;
use hide_ai::{
    AiBackend, AiResponse, AiUsage, Availability, NoopLogSink, ProviderId, RouterConfig,
};
use hide_session::{parse_claude_events, parse_codex_events};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::io::{BufRead, Cursor, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

/// Block until the background analysis thread has produced its result, then put
/// it back for the next scan to consume. Waiting on the channel is what makes
/// the provider tests deterministic instead of sleep-timed.
impl<T: HerdrTransport, R: SessionReader> Watcher<T, R> {
    fn await_pending_analysis(&mut self) {
        if self.analysis_in_flight.is_empty() {
            return;
        }
        if let Ok(outcome) = self.analysis_receiver.recv_timeout(Duration::from_secs(5)) {
            let _ = self.analysis_sender.send(outcome);
        }
    }

    /// Stand in for the wall clock passing every per-pane wake time the
    /// watcher scheduled, so a test about the trigger is not also a test about
    /// a sleep.
    fn advance_past_provider_waits(&mut self) {
        // Pull the scheduled wake-up into the past rather than dropping it:
        // that entry is what makes the next scan look at an otherwise
        // unchanged pane at all.
        for retry_at in self.next_analysis_at.values_mut() {
            *retry_at = UNIX_EPOCH;
        }
    }

    /// One compatibility scan cycle: scan, let the provider thread finish, scan again to
    /// consume its result.
    fn settle(&mut self) {
        self.scan().unwrap();
        self.await_pending_analysis();
        self.scan().unwrap();
    }
}

fn pane(id: &str, agent: AgentKind, status: &str) -> Pane {
    Pane {
        id: id.to_owned(),
        agent,
        name: None,
        tab_id: format!("{id}-tab"),
        agent_session: Some(AgentSession::new("id", &format!("{id}-session"))),
        agent_status: status.to_owned(),
        revision: 1,
        state_change_seq: 1,
        cwd: None,
        focused: false,
    }
}

fn human_event(text: impl Into<String>) -> SessionEvent {
    SessionEvent::new("user", EventKind::Human, 0, text)
}

fn assistant_event(text: impl Into<String>) -> SessionEvent {
    SessionEvent::new("assistant", EventKind::Assistant, 0, text)
}

// ---------------------------------------------------------------- AC1

#[test]
fn agent_list_payload_from_a_live_session_parses_without_a_second_endpoint() {
    #[derive(Deserialize)]
    struct AgentListFixture {
        result: AgentListResult,
    }
    let payload = include_str!("../tests/fixtures/agent-list.json");
    let envelope: AgentListFixture = serde_json::from_str(payload).unwrap();
    let panes: Vec<Pane> = envelope
        .result
        .agents
        .into_iter()
        .filter_map(AgentListItem::into_pane)
        .collect();

    assert!(!panes.is_empty(), "live payload has no supported panes");
    // Unsupported kinds (hermes) are dropped rather than erroring the scan.
    assert!(
        panes
            .iter()
            .all(|pane| matches!(pane.agent, AgentKind::Claude | AgentKind::Codex))
    );
    // Everything a scan needs comes from this one response.
    assert!(panes.iter().any(|pane| pane.agent_session.is_some()));
    assert!(panes.iter().any(|pane| pane.cwd.is_some()));
    assert!(panes.iter().all(|pane| pane.state_change_seq > 0));
    assert_eq!(panes.iter().filter(|pane| pane.focused).count(), 1);
}

#[test]
fn watcher_subscribes_to_the_contract_pane_events_only() {
    assert_eq!(
        WATCHER_SUBSCRIPTIONS,
        [
            "pane.created",
            "pane.updated",
            "pane.closed",
            "pane.exited",
            "pane.focused",
            "pane.agent_detected",
            "pane.agent_status_changed",
        ]
    );
    let params = hide_herdr_client::subscription_params_for_panes(
        &WATCHER_SUBSCRIPTIONS,
        &["w1:p1".to_owned()],
    )
    .unwrap();
    assert_eq!(params.get("after_sequence"), None);
    assert_eq!(params["subscriptions"].as_array().unwrap().len(), 7);
    assert!(
        params["subscriptions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|subscription| subscription["type"].as_str().unwrap().starts_with("pane."))
    );
    assert_eq!(
        params["subscriptions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|subscription| subscription["type"] == "pane.agent_status_changed")
            .and_then(|subscription| subscription["pane_id"].as_str()),
        Some("w1:p1")
    );
}

#[test]
fn event_lines_carry_their_kind_and_errors_stay_explicit() {
    let event = parse_watcher_subscription_line(
        &json!({
            "event": "pane_agent_status_changed",
            "data": {"type": "pane_agent_status_changed", "pane_id": "w1:p1"}
        })
        .to_string(),
    )
    .unwrap();
    assert!(matches!(
        event,
        WatcherSubscriptionLine::Event { kind } if kind == "pane_agent_status_changed"
    ));

    let error = parse_watcher_subscription_line(
        r#"{"id":"herdr-core:events.subscribe","error":{"code":"internal","message":"event stream closed"}}"#,
    )
    .unwrap();
    assert!(matches!(
        error,
        WatcherSubscriptionLine::Error { code, message }
            if code == "internal" && message == "event stream closed"
    ));
}

#[test]
fn reconnect_backoff_doubles_and_stops_at_five_seconds() {
    let mut delay = Duration::from_millis(100);
    for expected in [200, 400, 800, 1_600, 3_200, 5_000, 5_000] {
        delay = reconnect_delay(delay);
        assert_eq!(delay, Duration::from_millis(expected));
    }
}

#[test]
fn event_driven_scan_updates_status_and_focus_without_waiting_for_a_tick() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let mut initial = pane("w1:p1", AgentKind::Claude, "working");
    initial.focused = false;
    let transport = FakeTransport::new(vec![initial.clone()]);
    let mut watcher = Watcher::new(transport, no_provider(), FakeSessionReader, paths);

    watcher
        .scan_panes(&[initial.clone()], false, false)
        .unwrap();
    let mut changed = initial.clone();
    changed.agent_status = "blocked".to_owned();
    changed.state_change_seq += 1;
    watcher
        .scan_panes(&[changed.clone()], false, false)
        .unwrap();
    assert_eq!(watcher.last_report().status, StatusIcon::Approval);

    changed.focused = true;
    watcher.scan_panes(&[changed], false, false).unwrap();
    assert!(
        !watcher.last_report().unseen,
        "focus clears unseen immediately"
    );
}

#[test]
fn watcher_clears_the_legacy_summary_token_once_per_pane() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let subject = pane("w1:p1", AgentKind::Claude, "idle");
    let transport = FakeTransport::new(vec![subject.clone()]);
    let mut watcher = Watcher::new(transport, no_provider(), FakeSessionReader, paths);

    watcher
        .scan_panes(std::slice::from_ref(&subject), false, false)
        .unwrap();
    watcher.scan_panes(&[subject], false, false).unwrap();

    assert_eq!(
        watcher.transport.legacy_summary_clears.borrow().as_slice(),
        ["w1:p1".to_owned()]
    );
}

#[cfg(unix)]
#[test]
fn refresh_marker_wakes_the_single_event_loop_without_becoming_state() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let (sender, receiver) = mpsc::channel();
    let wake = WakeSocket::start(&paths, sender).unwrap();

    request_refresh(&paths).unwrap();
    assert!(paths.refresh_request().exists());
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(WatcherMessage::Wake)
    ));

    drop(wake);
    assert!(!paths.wake_socket().exists());
}

// ---------------------------------------------------------------- AC2

#[test]
fn session_path_prefers_the_herdr_reported_identity() {
    let root = tempdir().unwrap();
    let project = root.path().join(".claude/projects/-Users-example");
    let other = root
        .path()
        .join(".claude/projects/-Users-example-projects-other");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&other).unwrap();
    let reported = "54165405-3108-474a-beab-547903c6c23d";
    fs::write(project.join(format!("{reported}.jsonl")), "").unwrap();
    fs::write(other.join("11111111-0000-0000-0000-000000000000.jsonl"), "").unwrap();

    let mut reader = LocalSessionReader::new(root.path());
    let mut target = pane("w1:p1", AgentKind::Claude, "idle");
    target.cwd = Some("/Users/example".to_owned());
    target.agent_session = Some(AgentSession::new("id", reported));

    assert_eq!(
        reader.session_path(&target).unwrap(),
        project.join(format!("{reported}.jsonl"))
    );
}

#[test]
fn session_path_falls_back_to_the_newest_file_for_the_working_directory() {
    let root = tempdir().unwrap();
    let project = root.path().join(".claude/projects/-Users-example");
    fs::create_dir_all(&project).unwrap();
    let older = project.join("aaaaaaaa-0000-0000-0000-000000000000.jsonl");
    let newer = project.join("bbbbbbbb-0000-0000-0000-000000000000.jsonl");
    fs::write(&older, "").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    fs::write(&newer, "").unwrap();

    let mut reader = LocalSessionReader::new(root.path());
    let mut target = pane("w1:p1", AgentKind::Claude, "idle");
    target.cwd = Some("/Users/example".to_owned());

    assert_eq!(reader.session_path(&target).unwrap(), newer);
}

#[test]
fn codex_session_falls_back_through_the_recent_day_directories() {
    let root = tempdir().unwrap();
    let day = root.path().join(".codex/sessions/2026/08/14");
    fs::create_dir_all(&day).unwrap();
    let mine = day.join("rollout-2026-08-14T10-00-00-aaaa.jsonl");
    let theirs = day.join("rollout-2026-08-14T11-00-00-bbbb.jsonl");
    // A real session_meta line embeds the full base instructions and runs to
    // tens of kilobytes, which is what broke a fixed-size head read.
    let meta = |cwd: &str| {
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{cwd}\",\"base_instructions\":\"{}\"}}}}\n",
            "instruction ".repeat(4_000)
        )
    };
    fs::write(&theirs, meta("/Users/example/projects/theirs")).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    fs::write(&mine, meta("/Users/example/projects/mine")).unwrap();
    assert!(fs::metadata(&mine).unwrap().len() > 16 * 1024);

    let mut reader = LocalSessionReader::new(root.path());
    let mut target = pane("w1:p1", AgentKind::Codex, "idle");
    target.cwd = Some("/Users/example/projects/mine".to_owned());

    assert_eq!(reader.session_path(&target).unwrap(), mine);
}

#[test]
fn session_reader_reads_the_full_file_and_keeps_the_first_recent_request() {
    let root = tempdir().unwrap();
    let project = root.path().join(".claude/projects/-Users-example");
    fs::create_dir_all(&project).unwrap();
    let path = project.join("session.jsonl");
    let mut contents = String::new();
    for index in 0..40 {
        contents.push_str(
            &json!({
                "type": "user",
                "timestamp": format!("2026-09-16T00:00:{index:02}Z"),
                "isMeta": true,
                "message": {"content": "injected ".to_owned() + &"x".repeat(8 * 1024)},
            })
            .to_string(),
        );
        contents.push('\n');
    }
    for index in 0..MAX_USER_REQUEST_TURNS {
        contents.push_str(
            &json!({
                "type": "user",
                "timestamp": format!("2026-09-16T01:00:{index:02}Z"),
                "origin": {"kind": "human"},
                "message": {"content": format!("요청 {index}")},
            })
            .to_string(),
        );
        contents.push('\n');
    }
    fs::write(&path, contents).unwrap();
    assert!(fs::metadata(&path).unwrap().len() > 256 * 1024);

    let mut reader = LocalSessionReader::new(root.path());
    let mut target = pane("w1:p1", AgentKind::Claude, "working");
    target.cwd = Some("/Users/example".to_owned());
    target.agent_session = Some(AgentSession::new("id", "session"));

    let parsed = reader.read(&target).unwrap();

    assert_eq!(parsed.events.len(), MAX_USER_REQUEST_TURNS);
    assert_eq!(parsed.events.first().unwrap().text, "요청 0");
    assert_eq!(parsed.events.last().unwrap().text, "요청 7");
}

#[test]
fn session_reader_reads_appends_and_rescans_truncated_or_replaced_files() {
    let root = tempdir().unwrap();
    let project = root.path().join(".claude/projects/-Users-example");
    fs::create_dir_all(&project).unwrap();
    let path = project.join("session.jsonl");
    let line = |text: &str, second: u8| {
        json!({
            "type": "user",
            "timestamp": format!("1970-01-01T00:00:{second:02}Z"),
            "origin": {"kind": "human"},
            "message": {"content": text},
        })
        .to_string()
            + "\n"
    };
    fs::write(&path, line("첫 요청", 1)).unwrap();
    let mut reader = LocalSessionReader::new(root.path());
    let mut target = pane("w1:p1", AgentKind::Claude, "working");
    target.cwd = Some("/Users/example".to_owned());
    target.agent_session = Some(AgentSession::new("id", "session"));

    assert_eq!(reader.read(&target).unwrap().events.len(), 1);
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(line("둘째 요청", 2).as_bytes())
        .unwrap();
    let appended = reader.read(&target).unwrap();
    assert_eq!(appended.rescan_reason, None);
    assert_eq!(
        appended
            .events
            .iter()
            .map(|event| event.text.as_str())
            .collect::<Vec<_>>(),
        ["첫 요청", "둘째 요청"]
    );

    fs::write(&path, line("새 파일 요청", 3)).unwrap();
    let truncated = reader.read(&target).unwrap();
    assert_eq!(
        truncated.rescan_reason,
        Some(hide_session::RescanReason::Truncated)
    );
    assert_eq!(truncated.events.len(), 1);
    assert_eq!(truncated.events[0].text, "새 파일 요청");

    let replacement = project.join("replacement.jsonl");
    fs::write(&replacement, line("교체 파일 요청", 4)).unwrap();
    fs::rename(&replacement, &path).unwrap();
    let replaced = reader.read(&target).unwrap();
    assert_eq!(
        replaced.rescan_reason,
        Some(hide_session::RescanReason::Replaced)
    );
    assert_eq!(replaced.events[0].text, "교체 파일 요청");
}

// ---------------------------------------------------------------- AC3

#[test]
fn skips_malformed_session_lines() {
    let torn = concat!(
        r#"{"type":"user","timestamp":"1970-01-01T00:00:01Z","origin":{"kind":"human"},"message":{"content":"첫 번째 요청"}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"1970-01-01T00:00:02Z","message":{"content":[{"type":"text","text":"작업했습니다."}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tex"#,
    );

    let parsed = parse_claude_events(torn);

    assert_eq!(parsed.skipped_lines, 1);
    assert_eq!(parsed.events.len(), 2);
    assert_eq!(parsed.events[0].text, "첫 번째 요청");
    assert_eq!(parsed.events[1].text, "작업했습니다.");

    let codex = concat!(
        r#"{"type":"response_item","timestamp":"1970-01-01T00:00:03Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"요약을 고쳐줘"}]}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"messa"#,
    );
    let parsed = parse_codex_events(codex);
    assert_eq!(parsed.skipped_lines, 1);
    assert_eq!(parsed.events.len(), 1);
}

// ---------------------------------------------------------------- AC4

#[test]
fn redaction_preserves_prose_and_bullets() {
    let events = [
        human_event("음료 소비량 문제를 검토해줘"),
        assistant_event(concat!(
            "세 문제 모두 대기 중입니다. 답 주시면 채점해 드릴게요.\n",
            "- 문제 1 물 제외 가장 많이 소비되는 음료\n",
            "- 문제 2 노벨상에 없는 분야\n",
            "+ 추가 문항도 있습니다\n",
            "1-②, 2-④ 이런 식으로 주셔도 됩니다.\n",
            "api_key=supersecret\n",
            "user@example.com\n",
            "/Users/example/secret.txt\n",
            "```rust\n",
            "fn leak() { let secret = 1; }\n",
            "```",
        )),
    ];

    let context = analysis_context(&events);

    // The content the verdict depends on survives.
    assert!(context.contains("- 문제 1 물 제외 가장 많이 소비되는 음료"));
    assert!(context.contains("- 문제 2 노벨상에 없는 분야"));
    assert!(context.contains("+ 추가 문항도 있습니다"));
    assert!(context.contains("답 주시면 채점해 드릴게요"));
    // Secrets, personal data, paths and code do not.
    assert!(!context.contains("supersecret"));
    assert!(!context.contains("user@example.com"));
    assert!(!context.contains("/Users"));
    assert!(!context.contains("leak"));
}

// ---------------------------------------------------------------- AC5

#[test]
fn initial_context_spans_the_first_three_and_last_eight_user_turns() {
    let events = [
        human_event("가장 오래된 요청입니다"),
        assistant_event("가장 오래된 응답입니다"),
        human_event("직전 요청입니다"),
        assistant_event("직전 응답입니다"),
        human_event("최신 요청입니다"),
        assistant_event("긴 응답 ".repeat(200)),
        assistant_event("마지막으로 답을 기다립니다"),
    ];

    let context = analysis_context(&events);

    assert!(context.contains("최신 요청입니다"));
    assert!(context.contains("마지막으로 답을 기다립니다"));
    // The previous exchange stays visible so a wrap-up of an already answered
    // question is not mistaken for a fresh one.
    assert!(context.contains("직전 요청입니다"));
    assert!(context.contains("직전 응답입니다"));
    // The initial view carries the first request as well as the latest request
    // history, with an explicit marker for the middle that was omitted.
    assert!(context.contains("가장 오래된 요청입니다"));
    assert!(context.contains("<omitted-human-turns>0개 사람 턴 생략</omitted-human-turns>"));
    // Older assistant prose is not part of that request history and stays out.
    assert!(!context.contains("가장 오래된 응답입니다"));
}

#[test]
fn stdin_session_context_uses_the_shared_parser() {
    let context = analysis_context_from_session(
        hide_session::Agent::Claude,
        include_str!("../../../hide-session/tests/fixtures/claude.jsonl"),
    );

    assert!(context.contains("첫 번째 요청"), "{context}");
    assert!(context.contains("--quick"), "{context}");
    assert!(context.contains("검토를 시작했습니다."), "{context}");
    assert!(!context.contains("task-notification"), "{context}");
    assert!(!context.contains("system-reminder"), "{context}");
}

#[test]
fn interrupted_turn_keeps_the_previous_task_without_a_provider_call() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let session = ScriptedSessionReader::new();
    session.user("기존 요청");
    session.assistant("작업을 마쳤습니다.");
    let backend = task_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_with(&backend),
        session,
        paths.clone(),
    );

    watcher.settle();
    assert_eq!(backend.calls(), 1);
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("작업 요약 제목")
    );

    watcher
        .session_reader
        .interrupted("[Request interrupted by user]");
    watcher.transport.set_status("idle");
    watcher.scan().unwrap();

    assert_eq!(backend.calls(), 1);
    assert_eq!(watcher.last_report().status, StatusIcon::Interrupted);
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("작업 요약 제목")
    );
}

#[test]
fn a_pane_without_a_human_turn_has_no_task_and_no_provider_call() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_with(&backend),
        ScriptedSessionReader::new(),
        paths,
    );

    watcher.scan().unwrap();

    assert_eq!(backend.calls(), 0);
    assert_eq!(watcher.last_report().task, None);
    assert_eq!(watcher.last_report().status, StatusIcon::Idle);
}

#[test]
fn initial_context_keeps_first_three_and_last_eight_user_turns() {
    let mut events: Vec<SessionEvent> = Vec::new();
    for i in 0..(MAX_USER_REQUEST_TURNS + INITIAL_FIRST_USER_TURNS + 3) {
        events.push(human_event(format!("요청 {i}")));
        events.push(assistant_event(format!("응답 {i}")));
    }

    let context = analysis_context(&events);

    // The first three requests survive even though the session is long.
    for i in 0..INITIAL_FIRST_USER_TURNS {
        assert!(context.contains(&format!("요청 {i}")));
    }
    assert!(context.contains("<omitted-human-turns>3개 사람 턴 생략</omitted-human-turns>"));
    // The most recent MAX_USER_REQUEST_TURNS requests survive too.
    for i in (MAX_USER_REQUEST_TURNS + INITIAL_FIRST_USER_TURNS + 3 - MAX_USER_REQUEST_TURNS)
        ..(MAX_USER_REQUEST_TURNS + INITIAL_FIRST_USER_TURNS + 3)
    {
        assert!(context.contains(&format!("요청 {i}")));
    }
}

#[test]
fn rolling_context_contains_only_the_previous_task_and_new_human_delta() {
    let events = [
        human_event("처음 세션 목표를 정한다"),
        assistant_event("목표를 확인했습니다."),
        human_event("하위 작업으로 테스트도 추가해줘"),
        assistant_event("테스트를 준비했습니다."),
    ];
    let delta = vec![events[2].clone()];

    let context = rolling_analysis_context("세션 목표 구현", &delta, &events);

    assert!(context.contains("<previous-task>세션 목표 구현</previous-task>"));
    assert!(context.contains("하위 작업으로 테스트도 추가해줘"));
    assert!(!context.contains("처음 세션 목표를 정한다"));

    let end_context = rolling_analysis_context("세션 목표 구현", &[], &events);
    assert!(end_context.contains("<new-human-turns>\n<none/>\n</new-human-turns>"));
}

#[test]
fn task_input_cursor_moves_only_when_a_human_turn_changes() {
    let first = [human_event("첫 요청"), assistant_event("첫 응답")];
    let second = [
        human_event("첫 요청"),
        assistant_event("첫 응답"),
        human_event("둘째 요청"),
    ];
    let first_cursor = task_input_cursor(&first).unwrap();
    assert_eq!(task_input_cursor(&first), Some(first_cursor));
    assert_ne!(task_input_cursor(&second), Some(first_cursor));
    assert_eq!(
        new_human_turns(&second, Some(first_cursor))[0].text,
        "둘째 요청"
    );
}

// ---------------------------------------------------------------- AC7

#[test]
fn done_status_comes_from_herdr() {
    assert_eq!(status_icon("done", None), StatusIcon::Done);
    assert_eq!(status_icon("idle", None), StatusIcon::Idle);
    assert_eq!(status_icon("working", None), StatusIcon::Working);
    // Herdr already knows a dialog is waiting for a key; that is `!`, not `?`.
    assert_eq!(status_icon("blocked", None), StatusIcon::Approval);
    assert_eq!(status_icon("unknown", None), StatusIcon::Stale);
    // Working holds one steady symbol: the blink used to be the only thing
    // telling it apart from done, and it pulled the eye to the one state that
    // is not waiting on anyone.
    assert_eq!(StatusIcon::Working.symbol(), "●");
    assert_eq!(StatusIcon::Idle.symbol(), "○");

    // The plugin keeps no unseen-completion state of its own: a pane reported
    // as done renders done, and the same pane reported as idle renders idle,
    // with no local memory in between.
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p5", AgentKind::Claude, "working")]),
        no_provider(),
        FakeSessionReader,
        paths,
    );
    watcher.scan().unwrap();

    watcher.transport.panes.borrow_mut()[0].agent_status = "done".to_owned();
    watcher.transport.panes.borrow_mut()[0].state_change_seq = 2;
    watcher.scan().unwrap();
    assert_eq!(watcher.last_report().status, StatusIcon::Done);

    watcher.transport.panes.borrow_mut()[0].agent_status = "idle".to_owned();
    watcher.transport.panes.borrow_mut()[0].state_change_seq = 3;
    watcher.scan().unwrap();
    assert_eq!(watcher.last_report().status, StatusIcon::Idle);
}

#[test]
fn attention_refines_the_herdr_state_instead_of_replacing_it() {
    // Herdr's own lifecycle is the base and is never overwritten by the two
    // things the plugin adds on top of it.
    for status in ["idle", "done", "blocked", "unknown"] {
        assert_eq!(
            status_icon(status, Some(Attention::Question)),
            StatusIcon::Question,
            "{status} should accept a question refinement"
        );
        assert_eq!(
            status_icon(status, Some(Attention::Error)),
            StatusIcon::Error,
            "{status} should accept an error refinement"
        );
    }

    // A running agent is waiting on nobody, so nothing refines it. Without this
    // guard a verdict drawn one turn ago repaints a pane that has moved on.
    assert_eq!(
        status_icon("working", Some(Attention::Question)),
        StatusIcon::Working
    );
    assert_eq!(
        status_icon("working", Some(Attention::Error)),
        StatusIcon::Working
    );
    assert_eq!(
        status_icon("working", Some(Attention::Approval)),
        StatusIcon::Working
    );

    // The hook sees a permission request before Herdr sees the dialog on screen.
    assert_eq!(
        status_icon("idle", Some(Attention::Approval)),
        StatusIcon::Approval
    );
    // `?` and `!` answer different questions: one needs your words, the other
    // needs a keypress.
    assert_eq!(StatusIcon::Question.symbol(), "?");
    assert_eq!(StatusIcon::Approval.symbol(), "!");
}

#[test]
fn a_failed_turn_is_retired_once_the_agent_runs_again() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let failure = serde_json::json!({ "hook_event_name": "StopFailure" });
    assert_eq!(
        apply_hook_payload(&paths, "w1:p1", &failure).unwrap(),
        HookUpdate::Set(Attention::Error)
    );

    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        no_provider(),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.scan().unwrap();
    assert_eq!(watcher.last_report().status, StatusIcon::Error);

    // The agent picked the work back up, so the old failure is history.
    watcher.transport.panes.borrow_mut()[0].agent_status = "working".to_owned();
    watcher.transport.panes.borrow_mut()[0].state_change_seq = 2;
    watcher.scan().unwrap();
    assert_eq!(watcher.last_report().status, StatusIcon::Working);
    assert_eq!(load_hook_states(&paths).panes["w1:p1"].attention, None);

    watcher.transport.panes.borrow_mut()[0].agent_status = "idle".to_owned();
    watcher.transport.panes.borrow_mut()[0].state_change_seq = 3;
    watcher.scan().unwrap();
    assert_eq!(watcher.last_report().status, StatusIcon::Idle);
}

// ---------------------------------------------------------------- AC8

#[test]
fn setting_automatic_summaries_is_idempotent() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    assert!(load_settings(&paths).automatic_summaries);

    // Herdr can dispatch one action several times for a single keypress, so the
    // same command applied repeatedly has to land on the same state.
    for _ in 0..3 {
        set_automatic_summaries(&paths, false).unwrap();
        assert!(!load_settings(&paths).automatic_summaries);
    }
    for _ in 0..3 {
        set_automatic_summaries(&paths, true).unwrap();
        assert!(load_settings(&paths).automatic_summaries);
    }
}

/// The setting reaches the plugin: the router it builds follows the file, and
/// a changed file is picked up on the same scan boundary that already re-reads
/// this plugin's own settings.
#[test]
fn the_saved_choice_decides_the_routers_priority_and_a_changed_file_is_re_read() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    fs::create_dir_all(&paths.root).unwrap();

    // No file at all is the defaults, and the router's own order.
    assert_eq!(
        provider::settings(home.path()).0.router_config().priority,
        RouterConfig::default().priority,
        "nobody has chosen, so nothing is reordered"
    );

    let mut chosen = hide_ai::AiSettings {
        provider: ProviderId::Claude,
        ..hide_ai::AiSettings::default()
    };
    chosen.set_model(ProviderId::Claude, "sonnet");
    hide_ai::settings::save(home.path(), &chosen).unwrap();

    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        no_provider(),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.follow_ai_settings(home.path());
    assert_eq!(
        watcher.router.provider_state().map(|state| state.selected),
        Some(ProviderId::Claude),
        "the router the watcher runs on starts from the saved choice"
    );

    // A choice written while the watcher is running is picked up by the next
    // scan, without restarting it.
    let moved = hide_ai::AiSettings {
        provider: ProviderId::Codex,
        ..chosen.clone()
    };
    hide_ai::settings::save(home.path(), &moved).unwrap();
    watcher.scan().unwrap();
    assert_eq!(
        watcher.router.provider_state().map(|state| state.selected),
        Some(ProviderId::Codex),
        "the changed file moved the router without a restart"
    );
    assert!(
        fs::read_to_string(paths.log())
            .unwrap()
            .contains("ai_settings_changed"),
        "the move is recorded rather than silent"
    );

    // An unchanged file does not rebuild anything.
    let before = Arc::as_ptr(&watcher.router);
    watcher.scan().unwrap();
    assert_eq!(
        Arc::as_ptr(&watcher.router),
        before,
        "an unchanged choice leaves the router, and its sticky state, alone"
    );
}

/// A settings file that cannot be read is not taken as the defaults in
/// silence: the reason lands in the plugin's own log and the defaults are then
/// used.
#[test]
fn an_unreadable_choice_is_logged_before_the_defaults_are_used() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    fs::create_dir_all(&paths.root).unwrap();
    write_broken_ai_settings(home.path());

    assert_eq!(
        provider::settings_once(home.path(), &paths),
        hide_ai::AiSettings::default(),
        "the defaults are used"
    );
    assert!(
        fs::read_to_string(paths.log())
            .unwrap()
            .contains("ai_settings_unreadable"),
        "and the reason is stated first"
    );
}

/// The reason is a state, not a tick. The watcher re-reads the file every
/// the plugin's log is never rotated, so a broken file logged on every scan
/// would grow it without end; the reason is written when
/// it changes, and so is the recovery.
#[test]
fn a_broken_choice_is_logged_once_and_its_repair_is_logged_once() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    fs::create_dir_all(&paths.root).unwrap();
    write_broken_ai_settings(home.path());

    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        no_provider(),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.follow_ai_settings(home.path());
    for _ in 0..5 {
        watcher.scan().unwrap();
    }

    assert_eq!(
        count_log_lines(&paths, "ai_settings_unreadable"),
        1,
        "six reads of the same broken file state the reason once"
    );
    assert_eq!(
        count_log_lines(&paths, "ai_settings_readable"),
        0,
        "nothing has been repaired yet"
    );

    // Repairing the file is visible: it reads again, and it says so once.
    let chosen = hide_ai::AiSettings {
        provider: ProviderId::Claude,
        ..hide_ai::AiSettings::default()
    };
    hide_ai::settings::save(home.path(), &chosen).unwrap();
    for _ in 0..3 {
        watcher.scan().unwrap();
    }

    assert_eq!(
        count_log_lines(&paths, "ai_settings_readable"),
        1,
        "the repair is stated once, not on every scan after it"
    );
    assert_eq!(
        count_log_lines(&paths, "ai_settings_unreadable"),
        1,
        "and the old reason is not repeated"
    );
    assert_eq!(
        watcher.router.provider_state().map(|state| state.selected),
        Some(ProviderId::Claude),
        "the repaired choice is the one in force"
    );
}

fn write_broken_ai_settings(home: &Path) {
    let settings_path = hide_ai::settings::settings_path(home);
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(&settings_path, "{ this is not json").unwrap();
}

fn count_log_lines(paths: &StatePaths, event: &str) -> usize {
    fs::read_to_string(paths.log())
        .unwrap()
        .lines()
        .filter(|line| line.contains(event))
        .count()
}

#[test]
fn corrupt_state_files_do_not_stop_the_watcher() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    fs::create_dir_all(&paths.root).unwrap();
    fs::write(paths.settings(), "{ this is not json").unwrap();
    fs::write(paths.display_state(), "]").unwrap();

    assert!(load_settings(&paths).automatic_summaries);
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        no_provider(),
        FakeSessionReader,
        paths.clone(),
    );
    assert_eq!(watcher.scan().unwrap(), 1);
    assert!(
        fs::read_to_string(paths.log())
            .unwrap()
            .contains("display_state_reset")
    );
}

// ---------------------------------------------------------------- AC9

#[test]
fn task_truncates_on_a_word_boundary() {
    assert_eq!(truncate_task("짧은 요약"), "짧은 요약");
    // "Commit benchmark PRD validation" used to render as "Commit benchmark PRD validatio".
    assert_eq!(
        truncate_task("Commit benchmark PRD validation"),
        "Commit benchmark PRD…"
    );
    assert!(
        truncate_task("Commit benchmark PRD validation")
            .chars()
            .count()
            <= MAX_TASK_CHARS
    );
    // A single unbroken token still has to fit the budget.
    let unbroken = "a".repeat(50);
    assert_eq!(truncate_task(&unbroken).chars().count(), MAX_TASK_CHARS);
    assert!(truncate_task(&unbroken).ends_with('…'));
}

#[test]
fn normalizes_only_safe_one_line_tasks() {
    assert_eq!(
        normalize_task("  간단한 작업 제목  "),
        Some("간단한 작업 제목".into())
    );
    assert_eq!(normalize_task("\n"), None);
    assert_eq!(
        normalize_task("**`Compact task labels`**"),
        Some("Compact task labels".into())
    );
    assert_eq!(normalize_task("bad\u{0000}"), None);
    assert_eq!(normalize_task("짧음"), None);
}

#[test]
fn parses_the_provider_structured_output_contract() {
    assert_eq!(
        context_label::parse_text(
            r#"{"task":"수학 문제 출제 및 채점","task_changed":true,"progress":"착수","expected_reply":"","attention":"none"}"#,
        )
            .unwrap(),
        Analysis {
            task: "수학 문제 출제 및 채점".into(),
            task_changed: true,
            progress: "착수".into(),
            expected_reply: String::new(),
            attention: None,
        }
    );
    // A bare question verdict with no expected_reply field carries no statable
    // user action, so it downgrades like an empty one.
    assert_eq!(
        context_label::parse_text(
            r#"{"task":"음료 소비량 퀴즈 풀이","task_changed":false,"progress":"진행 중","expected_reply":"","attention":"question"}"#,
        )
            .unwrap()
            .attention,
        None
    );
    // A question verdict needs a statable user action to survive.
    assert_eq!(
        context_label::parse_text(
            r#"{"task":"배포 진행 여부 확인","task_changed":true,"progress":"완료","expected_reply":"배포 진행 여부를 답한다","attention":"question"}"#,
        )
        .unwrap()
        .attention,
        Some(Attention::Question)
    );
    // Question without an expected reply is a surface match and is downgraded.
    assert_eq!(
        context_label::parse_text(
            r#"{"task":"새 작업 지시 대기","task_changed":false,"progress":"대기","expected_reply":"","attention":"question"}"#,
        )
        .unwrap()
        .attention,
        None
    );
    assert!(context_label::parse_text(
        r#"{"task":"작업 승인 대기","task_changed":true,"progress":"대기","expected_reply":"","attention":"approval"}"#
    )
    .is_err());
    assert!(context_label::parse_text(
        r#"{"task":"상태 없는 응답","task_changed":true,"expected_reply":"","attention":"none"}"#
    )
    .is_err());
}

#[test]
fn task_parser_enforces_the_minimum_and_display_maximum() {
    let response = |task: &str| {
        json!({
            "task": task,
            "task_changed": true,
            "progress": "착수",
            "expected_reply": "",
            "attention": "none"
        })
    };

    assert!(context_label::parse(response("짧은 제목")).is_err());
    let parsed = context_label::parse(response(&"아주 긴 작업 제목 ".repeat(8))).unwrap();
    assert!(parsed.task.chars().count() <= MAX_TASK_CHARS);
    assert!(parsed.task.ends_with('…'));
    assert!(
        context_label::parse({
            let mut value = response("유효한 작업 제목");
            value["task_changed"] = json!("true");
            value
        })
        .is_err()
    );
}

#[test]
fn an_invalid_later_analysis_keeps_the_previous_task() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let session = ScriptedSessionReader::new();
    session.user("첫 번째 목표를 진행해줘");
    let backend = ScriptedBackend::new(vec![
        Ok(json!({
            "task": "기존 세션 목표 구현",
            "task_changed": true,
            "progress": "착수",
            "expected_reply": "",
            "attention": "none"
        })),
        Ok(json!({"task": "기존 세션 목표 구현"})),
    ]);
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "working")]),
        router_with(&backend),
        session,
        paths.clone(),
    );

    watcher.settle();
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("기존 세션 목표 구현")
    );

    watcher.session_reader.assistant("작업을 마쳤습니다.");
    watcher.transport.set_status("idle");
    watcher.settle();

    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("기존 세션 목표 구현")
    );
    assert!(
        fs::read_to_string(paths.log())
            .unwrap()
            .contains("analysis_abandoned")
    );
}

// ---------------------------------------------------------------- AC10

#[test]
fn failures_are_logged_with_actionable_detail() {
    // A provider failure keeps its class instead of collapsing to one string,
    // and one that can clear on its own parks the pane rather than the turn.
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = usage_limited_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_with(&backend),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.scan().unwrap();
    watcher.await_pending_analysis();
    watcher.scan().unwrap();
    let log = fs::read_to_string(paths.log()).unwrap();
    assert!(
        log.contains("analysis_provider_unavailable") && log.contains("usage_limited"),
        "log was: {log}"
    );
    assert!(!log.contains("analysis_abandoned"), "log was: {log}");
    assert!(watcher.next_analysis_at.contains_key("w1:p1"));

    // A successful verdict records what it decided and what it decided it from.
    let backend = question_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p2", AgentKind::Claude, "idle")]),
        router_with(&backend),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.scan().unwrap();
    watcher.await_pending_analysis();
    watcher.scan().unwrap();
    let log = fs::read_to_string(paths.log()).unwrap();
    assert!(log.contains("attention_chars="), "log was: {log}");
    assert!(log.contains("task_changed="), "log was: {log}");
    assert!(log.contains("context_chars="), "log was: {log}");
    // The turn it judged, and which of the turn's two boundaries it answered.
    assert!(log.contains("turn="), "log was: {log}");
    assert!(log.contains("phase="), "log was: {log}");

    // A transport failure reaches the caller with its reason intact, which is
    // what the watch loop writes into the log.
    let mut broken = Watcher::new(
        BrokenTransport,
        no_provider(),
        FakeSessionReader,
        StatePaths::for_tests(root.path()),
    );
    let error = broken.scan().unwrap_err();
    assert!(format!("{error:#}").contains("herdr socket is unavailable"));
}

#[test]
fn log_is_private_and_has_no_content_fields() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    append_log(&paths, "credential_unavailable", None, None).unwrap();
    let data = fs::read_to_string(paths.log()).unwrap();
    assert!(data.contains("schema_version"));
    assert!(!data.contains("prompt"));
    assert!(!data.contains("response"));
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(paths.log()).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn retention_keeps_at_most_three_log_files() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    fs::create_dir_all(&paths.root).unwrap();
    for index in 0..4 {
        fs::write(paths.root.join(format!("events.{index}.jsonl")), "x").unwrap();
    }
    enforce_retention(&paths).unwrap();
    let count = fs::read_dir(paths.root)
        .unwrap()
        .flatten()
        .filter(|item| item.file_name().to_string_lossy().starts_with("events"))
        .count();
    assert_eq!(count, 3);
}

// ---------------------------------------------------------- attention rules

#[test]
fn hook_events_map_only_runtime_attention_to_status() {
    let payload = |event: &str, tool: Option<&str>| {
        let mut value = serde_json::json!({ "hook_event_name": event });
        if let Some(tool) = tool {
            value["tool_name"] = serde_json::Value::String(tool.to_owned());
        }
        value
    };
    assert_eq!(
        classify_hook_payload(&payload("PreToolUse", Some("AskUserQuestion"))),
        HookUpdate::Set(Attention::Question)
    );
    assert_eq!(
        classify_hook_payload(&payload("PreToolUse", Some("functions.request_user_input"))),
        HookUpdate::Set(Attention::Question)
    );
    assert_eq!(
        classify_hook_payload(&payload("PermissionRequest", Some("Bash"))),
        HookUpdate::Set(Attention::Approval)
    );
    assert_eq!(
        classify_hook_payload(&payload("StopFailure", None)),
        HookUpdate::Set(Attention::Error)
    );
    assert_eq!(
        classify_hook_payload(&payload("PostToolUse", Some("Bash"))),
        HookUpdate::Clear
    );
}

#[test]
fn hook_completion_clears_only_the_matching_pending_tool() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let pending = serde_json::json!({
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_use_id": "tool-2"
    });
    assert_eq!(
        apply_hook_payload(&paths, "w1:p1", &pending).unwrap(),
        HookUpdate::Set(Attention::Approval)
    );

    let unrelated = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Read",
        "tool_use_id": "tool-1"
    });
    assert_eq!(
        apply_hook_payload(&paths, "w1:p1", &unrelated).unwrap(),
        HookUpdate::Ignore
    );
    assert_eq!(
        load_hook_states(&paths).panes["w1:p1"].attention,
        Some(Attention::Approval)
    );

    let matching = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "tool-2"
    });
    assert_eq!(
        apply_hook_payload(&paths, "w1:p1", &matching).unwrap(),
        HookUpdate::Clear
    );
    assert_eq!(load_hook_states(&paths).panes["w1:p1"].attention, None);
}

#[test]
fn a_cleared_hook_retires_an_older_semantic_verdict() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let subject = pane("w1:p1", AgentKind::Claude, "idle");
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![subject.clone()]),
        no_provider(),
        FakeSessionReader,
        paths,
    );
    watcher.display_states.panes.insert(
        subject.id.clone(),
        PersistedDisplayState {
            semantic_attention: Some(Attention::Question),
            analysis_unix_ms: 1_000,
            ..PersistedDisplayState::default()
        },
    );

    // No hook has spoken: the inference stands.
    assert_eq!(
        watcher.resolve_attention(&subject),
        Some((Attention::Question, AttentionSource::Semantic))
    );

    // The hook says the interaction ended after that inference was drawn.
    watcher.hook_states.panes.insert(
        subject.id.clone(),
        HookState {
            attention: None,
            updated_unix_ms: 2_000,
            ..HookState::default()
        },
    );
    assert_eq!(watcher.resolve_attention(&subject), None);

    // A newer inference may speak again.
    watcher
        .display_states
        .panes
        .get_mut(&subject.id)
        .unwrap()
        .analysis_unix_ms = 3_000;
    assert_eq!(
        watcher.resolve_attention(&subject),
        Some((Attention::Question, AttentionSource::Semantic))
    );

    // A live hook signal always wins.
    watcher
        .hook_states
        .panes
        .get_mut(&subject.id)
        .unwrap()
        .attention = Some(Attention::Approval);
    assert_eq!(
        watcher.resolve_attention(&subject),
        Some((Attention::Approval, AttentionSource::Hook))
    );
}

#[test]
fn plain_text_question_survives_a_watcher_restart() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let subject = pane("w1:p5", AgentKind::Claude, "idle");
    let backend = question_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![subject.clone()]),
        router_with(&backend),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.settle();
    assert_eq!(watcher.last_report().status, StatusIcon::Question);
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("음료 소비량 퀴즈 풀이")
    );
    let state = fs::read_to_string(paths.display_state()).unwrap();
    assert!(state.contains("\"progress\":\"대기\""), "{state}");
    assert!(state.contains("\"task_input_cursor\":"), "{state}");
    drop(watcher);

    let mut restarted = Watcher::new(
        FakeTransport::new(vec![subject]),
        no_provider(),
        FakeSessionReader,
        paths,
    );
    restarted.scan().unwrap();
    assert_eq!(restarted.last_report().status, StatusIcon::Question);
    assert_eq!(
        restarted.last_report().task.as_deref(),
        Some("음료 소비량 퀴즈 풀이")
    );
}

#[test]
fn missing_task_state_rebuilds_from_the_initial_session_view() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let session = ScriptedSessionReader::new();
    for index in 0..14 {
        session.user(&format!("세션 전체 요청 {index}"));
    }
    let backend = task_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_with(&backend),
        session,
        paths.clone(),
    );

    watcher.settle();

    assert_eq!(backend.calls(), 1);
    let input = &backend.inputs()[0];
    assert!(input.contains("세션 전체 요청 0"));
    assert!(input.contains("세션 전체 요청 2"));
    assert!(input.contains("세션 전체 요청 13"));
    assert!(input.contains("<omitted-human-turns>3개 사람 턴 생략</omitted-human-turns>"));
    assert!(
        fs::read_to_string(paths.display_state())
            .unwrap()
            .contains("\"task\":\"작업 요약 제목\"")
    );
}

// ------------------------------------------------------------- scheduling

#[test]
fn watcher_deduplicates_reports_and_its_own_revision_bump() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Codex, "blocked")]),
        router_with(&backend),
        FakeSessionReader,
        paths,
    );
    // First report, then the verdict lands and is reported once more.
    assert_eq!(watcher.scan().unwrap(), 1);
    watcher.await_pending_analysis();
    assert_eq!(watcher.scan().unwrap(), 1);
    assert_eq!(backend.calls(), 1);

    // Steady state: an unchanged pane produces no traffic at all.
    assert_eq!(watcher.scan().unwrap(), 0);

    // Herdr bumps the revision because of our own report; that must not look
    // like a new event, and must not spend another provider request.
    let own_bump = watcher.reported_revisions["w1:p1"];
    watcher.transport.panes.borrow_mut()[0].revision = own_bump;
    assert_eq!(watcher.scan().unwrap(), 0);
    assert_eq!(backend.calls(), 1);
}

#[test]
fn a_changed_session_is_analyzed_again() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p5", AgentKind::Claude, "idle")]),
        router_with(&backend),
        StateSequenceReader,
        paths,
    );
    watcher.scan().unwrap();
    watcher.await_pending_analysis();
    watcher.scan().unwrap();
    assert_eq!(backend.calls(), 1);

    watcher.transport.panes.borrow_mut()[0].state_change_seq = 2;
    watcher.scan().unwrap();
    watcher.await_pending_analysis();
    watcher.scan().unwrap();
    assert_eq!(backend.calls(), 2);
}

#[test]
fn a_refresh_request_is_consumed_once_by_the_focused_pane() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    set_automatic_summaries(&paths, false).unwrap();
    let mut focused = pane("w1:p5", AgentKind::Claude, "idle");
    focused.focused = true;
    let backend = task_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![focused]),
        router_with(&backend),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.scan().unwrap();

    request_refresh(&paths).unwrap();
    assert!(paths.refresh_request().exists());
    watcher.scan().unwrap();

    assert!(!paths.refresh_request().exists());
    assert!(
        fs::read_to_string(paths.log())
            .unwrap()
            .contains("task_refresh_skipped_disabled")
    );
    assert_eq!(backend.calls(), 0);
}

#[test]
fn refresh_rederives_the_task_from_the_initial_session_view() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let session = ScriptedSessionReader::new();
    session.user("세션 전체 목표를 구현해줘");
    let backend = ScriptedBackend::new(vec![
        Ok(json!({
            "task": "오래된 누적 작업 제목",
            "task_changed": true,
            "progress": "착수",
            "expected_reply": "",
            "attention": "none"
        })),
        Ok(json!({
            "task": "현재 세션 목표 정리",
            "task_changed": true,
            "progress": "재검토",
            "expected_reply": "",
            "attention": "none"
        })),
    ]);
    let mut focused = pane("w1:p1", AgentKind::Claude, "idle");
    focused.focused = true;
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![focused]),
        router_with(&backend),
        session,
        paths.clone(),
    );

    watcher.settle();
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("오래된 누적 작업 제목")
    );

    watcher.session_reader.assistant("이전 답변입니다.");
    request_refresh(&paths).unwrap();
    watcher.settle();

    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("현재 세션 목표 정리")
    );
    let inputs = backend.inputs();
    assert!(inputs[1].contains("<initial-human-requests>"));
    assert!(!inputs[1].contains("<previous-task>"));
}

#[test]
fn metadata_clears_every_status_token_it_may_own() {
    let subject = pane("w1:p1", AgentKind::Codex, "idle");
    let params = metadata_params(
        &subject,
        &Display {
            task: Some("작업 요약 제목".into()),
            status: StatusIcon::Approval,
            sort_key: SortKey::Approval,
            elapsed: Some("7s".into()),
            ..Display::default()
        },
    );
    let tokens = params["tokens"].as_object().unwrap();

    assert_eq!(tokens["task"], "작업 요약 제목");
    assert!(!tokens.contains_key("summary"));
    assert!(!tokens.contains_key("progress"));
    assert_eq!(tokens.len(), 16);
    assert_eq!(tokens["status_approval"], "!");
    assert_eq!(tokens["agent_codex"], "⬢");
    assert_eq!(tokens["elapsed"], "7s");
    // Approval sits in the user-blocking group at the top of the ordering.
    // The rank digit depends on the machine's optional sort-order file, so
    // only the seen partition prefix is asserted.
    assert!(tokens["sort_rank"].as_str().unwrap().starts_with('1'));
    // Every other status token is explicitly nulled, so two icons can never
    // render at once; the one being set remains a string.
    for token in STATUS_TOKENS {
        if token == "status_approval" {
            continue;
        }
        assert_eq!(tokens[token], Value::Null, "{token} is not cleared");
    }
    // The report never exceeds Herdr's 16-token budget.
    assert!(tokens.len() <= 16, "report touches {} tokens", tokens.len());
    // The pane title belongs to the user, not to this plugin.
    assert!(params.get("title").is_none());
}

#[test]
fn elapsed_time_uses_compact_second_minute_hour_and_day_units() {
    assert_eq!(format_elapsed(42_999), "42s");
    assert_eq!(format_elapsed(60_000), "1m");
    assert_eq!(format_elapsed(3_600_000), "1h");
    assert_eq!(format_elapsed(86_400_000), "1d");
}

#[test]
fn sort_rank_orders_user_blocking_states_before_ambient_states() {
    let order = resolve_sort_order(None);
    let ranks = DEFAULT_SORT_ORDER.map(|icon| sort_rank(&order, icon));
    assert_eq!(ranks, ["0", "1", "2", "3", "4", "5", "6", "7", "8"]);
    // The hook-confirmed question outranks the provider-inferred one.
    assert!(sort_rank(&order, SortKey::Question) < sort_rank(&order, SortKey::SemanticQuestion));
}

#[test]
fn a_failed_turn_leads_the_default_order_ahead_of_every_question() {
    assert_eq!(DEFAULT_SORT_ORDER[0], SortKey::Error);
    let order = resolve_sort_order(None);
    for later in [
        SortKey::Question,
        SortKey::Approval,
        SortKey::SemanticQuestion,
        SortKey::Done,
        SortKey::Working,
    ] {
        assert!(
            sort_rank(&order, SortKey::Error) < sort_rank(&order, later),
            "error must outrank {later:?}"
        );
    }
    // Reordering must not drop or duplicate a state.
    for icon in DEFAULT_SORT_ORDER {
        assert_eq!(
            DEFAULT_SORT_ORDER.iter().filter(|it| **it == icon).count(),
            1,
            "{icon:?} appears more than once"
        );
    }
}

#[test]
fn unread_work_ranks_by_attention_and_seen_work_only_by_recency() {
    let order = resolve_sort_order(None);
    let ranked = |unseen, status, sort_key| {
        sort_rank_token(
            &order,
            &Display {
                status,
                sort_key,
                unseen,
                ..Display::default()
            },
        )
    };

    // Group 1, unread and finished: error, then the questions, then a plain
    // completion. Group 2 is the running pane. Group 3 is everything seen.
    let unread_error = ranked(true, StatusIcon::Error, SortKey::Error);
    let unread_question = ranked(true, StatusIcon::Question, SortKey::Question);
    let unread_done = ranked(true, StatusIcon::Done, SortKey::Done);
    let working = ranked(false, StatusIcon::Working, SortKey::Working);
    let seen_error = ranked(false, StatusIcon::Error, SortKey::Error);
    let seen_idle = ranked(false, StatusIcon::Idle, SortKey::Idle);

    assert!(unread_error < unread_question);
    assert!(unread_question < unread_done);
    assert!(unread_done < working);
    assert!(working < seen_error);
    // Nothing distinguishes two seen panes, so only the recency tiebreak can
    // order them.
    assert_eq!(seen_error, seen_idle);
    // A running pane sorts with the unread group whether or not it was seen.
    assert_eq!(working, ranked(true, StatusIcon::Working, SortKey::Working));
}

#[test]
fn the_activity_token_is_a_fixed_width_clock_two_panes_can_be_compared_on() {
    // Zero padding keeps a lexicographic comparison agreeing with a numeric
    // one, which is what the view's descending token sort relies on.
    assert_eq!(activity_token(1_755_000_000_000).len(), 13);
    assert!(activity_token(999) < activity_token(1_000));
    assert_eq!(activity_token(0), "0000000000000");

    let params = metadata_params(
        &pane("w1:p1", AgentKind::Codex, "done"),
        &Display {
            status: StatusIcon::Done,
            sort_key: SortKey::Done,
            activity_unix_ms: 1_755_000_000_000,
            ..Display::default()
        },
    );
    assert_eq!(params["tokens"]["activity"], "1755000000000");
    // Always set, never nulled: the sidebar cannot order a pane without it.
    assert_ne!(params["tokens"]["activity"], Value::Null);
}

#[test]
fn the_view_breaks_ties_on_the_activity_clock_not_the_per_pane_counter() {
    let request = priority_agent_view_request();
    let sort = request.pointer("/params/sort").unwrap().as_array().unwrap();

    assert_eq!(sort.len(), 2);
    assert_eq!(
        sort[0],
        serde_json::json!({"field": {"token": "sort_rank"}, "order": "asc"})
    );
    assert_eq!(
        sort[1],
        serde_json::json!({"field": {"token": "activity"}, "order": "desc"})
    );
    // The old key ranked panes by how many times they had changed, not when.
    assert!(!request.to_string().contains("state_change_seq"));
}

#[test]
fn user_sort_order_reorders_listed_states_and_appends_the_rest() {
    let order = resolve_sort_order(Some(r#"{"order":["working","question","nonsense"]}"#));
    assert_eq!(order[0], SortKey::Working);
    assert_eq!(order[1], SortKey::Question);
    // Unlisted states keep their default relative order after the listed ones,
    // so the head of the default order that survives the list comes first.
    assert_eq!(order[2], SortKey::Error);
    assert_eq!(order[3], SortKey::Approval);
    assert_eq!(order[8], SortKey::Stale);
    // Broken JSON must not take the watcher down or scramble the order.
    assert_eq!(resolve_sort_order(Some("not json")), DEFAULT_SORT_ORDER);
}

// ------------------------------------------------------------ documentation

/// The README's configuration example is what users paste, so a token the code
/// publishes but the example omits ships as a state with no color. That already
/// happened: the example carried 7 of 11 tokens, and the three unread variants
/// it dropped are exactly the states a user most needs to see.
#[test]
fn the_documented_sidebar_example_colors_every_token_the_plugin_publishes() {
    // Scope the search to the copy-paste block itself. Checking the whole file
    // would pass on the prose that merely names a token elsewhere, which is
    // the drift this test exists to catch.
    let readme = include_str!("../README.md");
    let example = readme
        .split_once("[ui.sidebar.agents]")
        .and_then(|(_, rest)| rest.split_once("\n```"))
        .map(|(block, _)| block)
        .expect("README has no sidebar configuration example");

    for token in STATUS_TOKENS {
        assert!(
            example.contains(&format!("${token}")),
            "README's sidebar example is missing ${token}"
        );
    }
    // Working and done render the same glyph, so identical colors make them
    // indistinguishable. The README says so; this checks the example obeys it.
    let color_of = |token: &str| {
        example
            .split_once(&format!("{{ token = \"${token}\", fg = \""))
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(color, _)| color.to_owned())
            .unwrap_or_else(|| panic!("no documented color for ${token}"))
    };
    assert_ne!(
        color_of("status_working"),
        color_of("status_done"),
        "working and done share the ● glyph and must not share a color"
    );
}

/// A wrong model name in the docs is a billing claim, not a typo: one guide
/// named a free model and told the reader there would be no charge.
#[test]
fn the_documented_model_is_the_one_the_code_calls() {
    let readme = include_str!("../README.md");
    let model = hide_ai::CodexConfig::default().model;
    assert!(
        readme.contains(&model),
        "README does not name the model the code calls ({model})"
    );
    for (name, text) in [
        ("README.md", readme),
        ("INSTALL.md", include_str!("../INSTALL.md")),
    ] {
        assert!(
            !text.contains(":free"),
            "{name} still advertises a free model"
        );
    }
}

// -------------------------------------------------- turn-keyed analysis

/// The regression this whole design exists for: the old trigger hashed the
/// context window, which grows with every token the agent emits, so a live pane
/// asked the provider again on almost every trigger.
#[test]
fn turn_identity_holds_while_output_grows_and_moves_only_at_a_boundary() {
    let user = |text: &str| human_event(text);
    let assistant = |text: &str| assistant_event(text);

    let mut events = vec![user("정렬 순서를 고쳐줘")];
    let start = turn_key(&events).unwrap();

    // Everything the agent says during its turn leaves the key alone.
    for chunk in ["파일을 읽는 중", "테스트를 실행", "커밋했습니다"] {
        events.push(assistant(chunk));
        assert_eq!(
            turn_key(&events),
            Some(start),
            "assistant output must not start a new turn"
        );
    }
    // The old trigger keyed on this text, which changed at every step above.
    assert_ne!(
        context_fingerprint(&analysis_context(&events)),
        context_fingerprint(&analysis_context(&events[..1]))
    );

    // The user speaking is the boundary, and the only one.
    events.push(user("이제 문서도 고쳐줘"));
    assert_ne!(turn_key(&events), Some(start));

    // No user message at all means no turn to analyze.
    assert_eq!(turn_key(&[]), None);
    assert_eq!(turn_key(&[assistant("혼잣말")]), None);
}

/// Both arms are level-triggered. An edge-triggered version would lose the call
/// whenever the request spacer, the cooldown, or the daily cap deferred it.
#[test]
fn the_phase_stays_offered_until_its_call_actually_lands() {
    use AnalysisPhase::{TurnEnd, TurnStart};

    // Turn start: the user has spoken and the agent has not answered.
    assert_eq!(analysis_phase(true, false, false, false), Some(TurnStart));
    assert_eq!(analysis_phase(true, true, false, false), Some(TurnStart));
    // Offered again and again until it is recorded.
    assert_eq!(analysis_phase(true, true, true, false), None);

    // Turn end: the agent answered and stopped.
    assert_eq!(analysis_phase(false, false, true, false), Some(TurnEnd));
    assert_eq!(analysis_phase(false, false, true, true), None);
    // Still running, so there is nothing to judge yet.
    assert_eq!(analysis_phase(false, true, true, false), None);
}

/// One turn buys one task decision and one verdict, whatever the agent does in
/// between. On 2026-08-17 the old trigger spent 470 calls across 19 panes,
/// 37 of them under ten seconds apart on the same pane.
#[test]
fn one_turn_costs_at_most_two_provider_calls() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let session = ScriptedSessionReader::new();
    let backend = task_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_with(&backend),
        session,
        paths.clone(),
    );

    // Nothing said yet: no turn, so no call however often the watcher wakes.
    for _ in 0..3 {
        watcher.scan().unwrap();
    }
    assert_eq!(backend.calls(), 0);

    // The user speaks and the agent starts. One call names the task.
    watcher.session_reader.user("정렬 순서를 고쳐줘");
    watcher.transport.set_status("working");
    watcher.settle();
    assert_eq!(backend.calls(), 1);

    // The agent works and talks. This is where the old trigger bled requests.
    for chunk in ["파일을 읽는 중", "테스트 실행", "수정 적용", "커밋 완료"] {
        watcher
            .session_reader
            .injected("<task-notification>background result</task-notification>");
        watcher.session_reader.assistant(chunk);
        watcher.advance_past_provider_waits();
        watcher.settle();
    }
    assert_eq!(
        backend.calls(),
        1,
        "output within a turn must not buy another request"
    );

    // The turn ends. One call decides whether the pane is waiting on the user.
    watcher.transport.set_status("idle");
    watcher.advance_past_provider_waits();
    watcher.settle();
    assert_eq!(backend.calls(), 2);

    // Idling afterwards, and a lifecycle that flaps, are both free.
    for status in ["idle", "working", "idle", "done"] {
        watcher.transport.set_status(status);
        watcher.advance_past_provider_waits();
        watcher.settle();
    }
    assert_eq!(
        backend.calls(),
        2,
        "one turn must cost at most two requests"
    );

    // The next turn is a fresh budget of two.
    watcher.session_reader.user("이제 문서도 고쳐줘");
    watcher.transport.set_status("working");
    watcher.advance_past_provider_waits();
    watcher.settle();
    assert_eq!(backend.calls(), 3);
}

#[test]
fn rolling_task_keeps_subtasks_and_replaces_a_new_goal() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let session = ScriptedSessionReader::new();
    let response = |task: &str, task_changed: bool, progress: &str| {
        Ok(json!({
            "task": task,
            "task_changed": task_changed,
            "progress": progress,
            "expected_reply": "",
            "attention": "none"
        }))
    };
    let backend = ScriptedBackend::new(vec![
        response("Task Factory 운영 구축", true, "착수"),
        response("완전히 다른 제목", false, "완료"),
        response("흔들린 작업 제목", false, "착수"),
        response("또 다른 작업 제목", true, "완료"),
        response("Herdr 종료 안정화 PRD", true, "착수"),
    ]);
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "working")]),
        router_with(&backend),
        session,
        paths,
    );

    watcher.session_reader.user("Task Factory를 구축해줘");
    watcher.settle();
    assert_eq!(backend.calls(), 1, "after first start");
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("Task Factory 운영 구축")
    );

    watcher.session_reader.assistant("첫 작업을 마쳤습니다.");
    watcher.transport.set_status("idle");
    watcher.settle();
    assert_eq!(backend.calls(), 2, "after first end");
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("Task Factory 운영 구축")
    );

    watcher
        .session_reader
        .user("하위 작업으로 사용법도 정리해줘");
    watcher.transport.set_status("working");
    watcher.settle();
    assert_eq!(backend.calls(), 3, "after second start");
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("Task Factory 운영 구축")
    );

    watcher.session_reader.assistant("사용법을 정리했습니다.");
    watcher.transport.set_status("idle");
    watcher.settle();
    assert_eq!(backend.calls(), 4, "after second end");
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("Task Factory 운영 구축")
    );

    watcher
        .session_reader
        .user("이제 Herdr 종료 안정화 PRD를 써줘");
    watcher.transport.set_status("working");
    watcher.settle();
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("Herdr 종료 안정화 PRD")
    );
    assert_eq!(backend.calls(), 5);

    let inputs = backend.inputs();
    assert!(inputs[0].contains("<initial-human-requests>"));
    assert!(inputs[2].contains("<previous-task>Task Factory 운영 구축</previous-task>"));
    assert!(inputs[2].contains("하위 작업으로 사용법도 정리해줘"));
    assert!(!inputs[2].contains("Task Factory를 구축해줘"));
    assert!(inputs[1].contains("<new-human-turns>\n<none/>\n</new-human-turns>"));
}

/// The symptom the user reported: a pane's question symbol appearing and
/// vanishing on its own. 158 such flips were recorded on 2026-08-17.
#[test]
fn an_attention_verdict_is_held_for_the_rest_of_its_turn() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let session = ScriptedSessionReader::new();
    session.user("이 방식으로 갈까요?");
    let backend = question_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_with(&backend),
        session,
        paths.clone(),
    );

    // The turn ends on a question, so the pane is marked.
    watcher.session_reader.assistant("이걸로 갈까요?");
    watcher.scan().unwrap();
    watcher.await_pending_analysis();
    watcher.scan().unwrap();
    assert_eq!(watcher.last_report().status, StatusIcon::Question);

    // Late output and a flapping lifecycle used to re-roll this verdict. The
    // turn is already judged, so nothing re-decides it.
    for chunk in ["recap 출력", "백그라운드 셸 종료"] {
        watcher.session_reader.assistant(chunk);
        for status in ["idle", "done"] {
            watcher.transport.set_status(status);
            watcher.scan().unwrap();
            watcher.await_pending_analysis();
            watcher.scan().unwrap();
            assert_eq!(
                watcher.last_report().status,
                StatusIcon::Question,
                "the verdict must not move inside its own turn"
            );
        }
    }
}

/// The state file is a cache, but it is the cache that holds a verdict still,
/// so its shape is part of the contract.
#[test]
fn persisted_state_records_both_turn_phases_and_no_fingerprint() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());

    // A file written by the previous schema must not stop the watcher.
    fs::create_dir_all(&paths.root).unwrap();
    fs::write(
        paths.display_state(),
        r#"{"panes":{"w1:p1":{"state_change_seq":1,"changed_unix_ms":1,"summary":"이전 요약","analysis_fingerprint":42}}}"#,
    )
    .unwrap();

    let session = ScriptedSessionReader::new();
    session.user("정렬 순서를 고쳐줘");
    session.assistant("고쳤습니다");
    let backend = task_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_with(&backend),
        session,
        paths.clone(),
    );

    let migrated = fs::read_to_string(paths.display_state()).unwrap();
    assert!(migrated.contains("\"task\":\"이전 요약\""), "{migrated}");
    assert!(!migrated.contains("\"summary\":"), "{migrated}");

    watcher.scan().unwrap();
    watcher.await_pending_analysis();
    watcher.scan().unwrap();

    let written = fs::read_to_string(paths.display_state()).unwrap();
    assert!(
        !written.contains("analysis_fingerprint"),
        "the content fingerprint is gone: {written}"
    );
    assert!(written.contains("\"task\":"), "{written}");
    assert!(
        !written.contains("\"summary\":"),
        "legacy summary survived: {written}"
    );
    assert!(written.contains("analysis_turn_start"), "{written}");
    assert!(written.contains("analysis_turn_end"), "{written}");
}

// ------------------------------------------------------------------ doubles

struct FakeTransport {
    panes: RefCell<Vec<Pane>>,
    reports: RefCell<Vec<Display>>,
    identity_reports: RefCell<Vec<(String, Identity)>>,
    legacy_summary_clears: RefCell<Vec<String>>,
    /// `agent.rename` calls as (pane id, name); the fake applies Herdr's
    /// own name rule so a refused name reads as the server would refuse it.
    agent_renames: RefCell<Vec<(String, String)>>,
    /// Tab labels the fake server holds, read by `tab_label` and written by
    /// `rename_tab`. A tab missing here fails `tab.get`.
    tab_labels: RefCell<HashMap<String, String>>,
    tab_renames: RefCell<Vec<(String, String)>>,
}

impl FakeTransport {
    fn new(panes: Vec<Pane>) -> Self {
        Self {
            panes: RefCell::new(panes),
            reports: RefCell::new(Vec::new()),
            identity_reports: RefCell::new(Vec::new()),
            legacy_summary_clears: RefCell::new(Vec::new()),
            agent_renames: RefCell::new(Vec::new()),
            tab_labels: RefCell::new(HashMap::new()),
            tab_renames: RefCell::new(Vec::new()),
        }
    }

    /// Move Herdr's lifecycle for every pane, the way the real server does
    /// between event-driven scans.
    fn set_status(&self, status: &str) {
        for pane in self.panes.borrow_mut().iter_mut() {
            pane.agent_status = status.to_owned();
            pane.state_change_seq += 1;
        }
    }
}

impl HerdrTransport for FakeTransport {
    fn panes(&self) -> Result<Vec<Pane>> {
        Ok(self.panes.borrow().clone())
    }
    fn report(&self, _: &Pane, display: &Display) -> Result<()> {
        self.reports.borrow_mut().push(display.clone());
        Ok(())
    }
    fn report_identity(&self, pane: &Pane, identity: &Identity) -> Result<()> {
        self.identity_reports
            .borrow_mut()
            .push((pane.id.clone(), identity.clone()));
        Ok(())
    }
    fn rename_agent(&self, pane: &Pane, name: &str) -> Result<()> {
        if !herdr_agent_name_acceptable(name) {
            return Err(anyhow!("agent.rename failed: invalid_agent_name"));
        }
        self.agent_renames
            .borrow_mut()
            .push((pane.id.clone(), name.to_owned()));
        for held in self.panes.borrow_mut().iter_mut() {
            if held.id == pane.id {
                held.name = Some(name.to_owned());
            }
        }
        Ok(())
    }
    fn tab_label(&self, tab_id: &str) -> Result<String> {
        self.tab_labels
            .borrow()
            .get(tab_id)
            .cloned()
            .ok_or_else(|| anyhow!("tab.get failed: unknown tab {tab_id}"))
    }
    fn rename_tab(&self, tab_id: &str, label: &str) -> Result<()> {
        self.tab_renames
            .borrow_mut()
            .push((tab_id.to_owned(), label.to_owned()));
        self.tab_labels
            .borrow_mut()
            .insert(tab_id.to_owned(), label.to_owned());
        Ok(())
    }

    fn clear_legacy_summary_token(&self, pane: &Pane) -> Result<()> {
        self.legacy_summary_clears
            .borrow_mut()
            .push(pane.id.clone());
        Ok(())
    }
}

struct BrokenTransport;

impl HerdrTransport for BrokenTransport {
    fn panes(&self) -> Result<Vec<Pane>> {
        Err(anyhow!("herdr socket is unavailable"))
    }
    fn report(&self, _: &Pane, _: &Display) -> Result<()> {
        Ok(())
    }
    fn report_identity(&self, _: &Pane, _: &Identity) -> Result<()> {
        Ok(())
    }
    fn rename_agent(&self, _: &Pane, _: &str) -> Result<()> {
        Ok(())
    }
    fn tab_label(&self, _: &str) -> Result<String> {
        Err(anyhow!("herdr socket is unavailable"))
    }
    fn rename_tab(&self, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
}

impl<R: SessionReader> Watcher<FakeTransport, R> {
    fn last_report(&self) -> Display {
        self.transport.reports.borrow().last().cloned().unwrap()
    }
}

struct RecordingShutdown;

impl hide_herdr_client::ConnectionShutdown for RecordingShutdown {
    fn shutdown(&self) {}
}

struct RecordingStream {
    incoming: Cursor<Vec<u8>>,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Read for RecordingStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.incoming.read(buffer)
    }
}

impl Write for RecordingStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.requests.lock().unwrap().push(buffer.to_vec());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl hide_herdr_client::ApiStream for RecordingStream {
    fn set_read_timeout(
        &self,
        _timeout: Option<Duration>,
    ) -> std::result::Result<(), hide_herdr_client::ApiError> {
        Ok(())
    }

    fn set_write_timeout(
        &self,
        _timeout: Option<Duration>,
    ) -> std::result::Result<(), hide_herdr_client::ApiError> {
        Ok(())
    }

    fn read_line_with_timeout(
        &mut self,
        _timeout: Duration,
    ) -> std::result::Result<String, hide_herdr_client::ApiError> {
        let mut line = String::new();
        self.incoming
            .read_line(&mut line)
            .map_err(|error| hide_herdr_client::ApiError::Transport(error.to_string()))?;
        Ok(line)
    }

    fn shutdown_handle(
        &self,
    ) -> std::result::Result<
        Box<dyn hide_herdr_client::ConnectionShutdown>,
        hide_herdr_client::ApiError,
    > {
        Ok(Box::new(RecordingShutdown))
    }
}

struct RecordingConnector {
    responses: Mutex<Vec<Vec<u8>>>,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl hide_herdr_client::ApiConnector for RecordingConnector {
    fn connect(
        &self,
    ) -> std::result::Result<Box<dyn hide_herdr_client::ApiStream>, hide_herdr_client::ApiError>
    {
        let incoming = self.responses.lock().unwrap().remove(0);
        Ok(Box::new(RecordingStream {
            incoming: Cursor::new(incoming),
            requests: Arc::clone(&self.requests),
        }))
    }
}

#[test]
fn socket_transport_uses_one_client_for_list_metadata_and_subscription() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let list_response = json!({
        "id": "herdr-core:agent.list",
        "result": {
            "type": "agent_list",
            "agents": [{
                "pane_id": "w1:p1",
                "agent": "claude",
                "agent_status": "idle",
                "revision": 3,
                "state_change_seq": 9,
                "cwd": "/tmp",
                "focused": true,
                "agent_session": null
            }]
        }
    });
    let report_response = json!({
        "id": "herdr-core:pane.report_metadata",
        "result": {"type": "pane_metadata_reported", "pane_id": "w1:p1"}
    });
    let legacy_clear_response = json!({
        "id": "herdr-core:pane.report_metadata",
        "result": {"type": "pane_metadata_reported", "pane_id": "w1:p1"}
    });
    let subscription_response = format!(
        "{}\n{}\n",
        json!({
            "id": "herdr-core:events.subscribe",
            "result": {
                "type": "subscription_started"
            }
        }),
        json!({
            "event": "pane_focused",
            "data": {"type": "pane_focused", "pane_id": "w1:p1", "workspace_id": "w1"}
        })
    );
    let connector = RecordingConnector {
        responses: Mutex::new(vec![
            list_response.to_string().into_bytes(),
            report_response.to_string().into_bytes(),
            legacy_clear_response.to_string().into_bytes(),
            subscription_response.into_bytes(),
        ]),
        requests: Arc::clone(&requests),
    };
    let socket = SocketHerdr {
        connector: Arc::new(connector),
        timeout: Duration::from_secs(1),
    };

    let panes = socket.panes().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].state_change_seq, 9);
    socket
        .report(
            &panes[0],
            &Display {
                status: StatusIcon::Idle,
                ..Display::default()
            },
        )
        .unwrap();
    socket.clear_legacy_summary_token(&panes[0]).unwrap();
    let subscription = socket.subscribe(&["w1:p1".to_owned()]).unwrap();
    assert_eq!(subscription.ack.kind, "subscription_started");
    let (mut reader, _shutdown) = subscription.into_parts();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("pane_focused"));

    let requests = requests.lock().unwrap();
    let methods = requests
        .iter()
        .map(|request| serde_json::from_slice::<Value>(request).unwrap()["method"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            json!("agent.list"),
            json!("pane.report_metadata"),
            json!("pane.report_metadata"),
            json!("events.subscribe")
        ]
    );
    let legacy_clear_request: Value = serde_json::from_slice(&requests[2]).unwrap();
    assert_eq!(
        legacy_clear_request["params"]["tokens"]["summary"],
        Value::Null
    );
    let subscription_request: Value = serde_json::from_slice(&requests[3]).unwrap();
    assert_eq!(subscription_request["params"].get("after_sequence"), None);
    assert_eq!(
        subscription_request["params"]["subscriptions"]
            .as_array()
            .unwrap()
            .len(),
        WATCHER_SUBSCRIPTIONS.len()
    );
}

/// A provider the test scripts: each request receives the next reply, and the
/// last one repeats. Counting calls is what the request-budget tests observe.
struct ScriptedBackend {
    id: ProviderId,
    calls: AtomicUsize,
    replies: Mutex<Vec<std::result::Result<Value, AiError>>>,
    inputs: Mutex<Vec<String>>,
}

impl ScriptedBackend {
    fn new(replies: Vec<std::result::Result<Value, AiError>>) -> Arc<Self> {
        Self::with_id(ProviderId::Codex, replies)
    }

    fn with_id(id: ProviderId, replies: Vec<std::result::Result<Value, AiError>>) -> Arc<Self> {
        Arc::new(Self {
            id,
            calls: AtomicUsize::new(0),
            replies: Mutex::new(replies),
            inputs: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn inputs(&self) -> Vec<String> {
        self.inputs.lock().unwrap().clone()
    }
}

impl AiBackend for ScriptedBackend {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn availability(&self) -> Availability {
        Availability::Ready
    }

    fn models(&self) -> hide_ai::ModelCatalog {
        hide_ai::ModelCatalog::Offered(vec![format!("{}-model", self.id)])
    }

    fn execute(
        &self,
        request: &hide_ai::AiRequest,
        _: &CancelToken,
    ) -> std::result::Result<AiResponse, AiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inputs.lock().unwrap().push(request.input.clone());
        let mut replies = self.replies.lock().unwrap();
        let reply = if replies.len() > 1 {
            replies.remove(0)
        } else {
            replies[0].clone()
        };
        reply.map(|value| AiResponse {
            value,
            usage: AiUsage::default(),
        })
    }
}

/// The router as the watcher sees it, with backoff sleeps elided so a retry
/// policy is observed by its call count rather than waited out.
fn router_with(backend: &Arc<ScriptedBackend>) -> Arc<AiRouter> {
    router_over(vec![Arc::clone(backend) as Arc<dyn AiBackend>])
}

fn router_over(backends: Vec<Arc<dyn AiBackend>>) -> Arc<AiRouter> {
    Arc::new(AiRouter::with_sleep(
        backends,
        RouterConfig::default(),
        Arc::new(NoopLogSink),
        Box::new(|_| {}),
    ))
}

/// A provider with a bug: the first call panics, later calls answer. The
/// watcher must survive the first and still make the second.
struct PanickingBackend {
    calls: AtomicUsize,
}

impl AiBackend for PanickingBackend {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    fn availability(&self) -> Availability {
        Availability::Ready
    }

    fn models(&self) -> hide_ai::ModelCatalog {
        hide_ai::ModelCatalog::Offered(Vec::new())
    }

    fn execute(
        &self,
        _: &hide_ai::AiRequest,
        _: &CancelToken,
    ) -> std::result::Result<AiResponse, AiError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("provider bug");
        }
        Ok(AiResponse {
            value: json!({
                "task": "두 번째 요청 요약",
                "task_changed": true,
                "progress": "완료",
                "expected_reply": "",
                "attention": "none"
            }),
            usage: AiUsage::default(),
        })
    }
}

/// Nothing connected: every request answers `NoProvider`.
fn no_provider() -> Arc<AiRouter> {
    Arc::new(AiRouter::new(
        Vec::new(),
        RouterConfig::default(),
        Arc::new(NoopLogSink),
    ))
}

fn task_backend() -> Arc<ScriptedBackend> {
    ScriptedBackend::new(vec![Ok(json!({
        "task": "작업 요약 제목",
        "task_changed": true,
        "progress": "착수",
        "expected_reply": "",
        "attention": "none"
    }))])
}

fn question_backend() -> Arc<ScriptedBackend> {
    ScriptedBackend::new(vec![Ok(json!({
        "task": "음료 소비량 퀴즈 풀이",
        "task_changed": false,
        "progress": "대기",
        "expected_reply": "퀴즈 답을 말한다",
        "attention": "question"
    }))])
}

fn usage_limited_backend() -> Arc<ScriptedBackend> {
    ScriptedBackend::new(vec![Err(AiError::UsageLimited { retry_after: None })])
}

/// Well-formed JSON that is not the answer: the router refuses it against
/// the feature schema before the feature ever sees it.
fn malformed_backend() -> Arc<ScriptedBackend> {
    ScriptedBackend::new(vec![Ok(json!({"task": "작업 요약 제목"}))])
}

/// A transcript the test drives turn by turn, so a scan sees exactly the
/// conversation state the scenario is about.
struct ScriptedSessionReader {
    title: RefCell<Option<String>>,
    events: RefCell<Vec<SessionEvent>>,
}

impl ScriptedSessionReader {
    fn new() -> Self {
        Self {
            title: RefCell::new(None),
            events: RefCell::new(Vec::new()),
        }
    }

    /// Claude's `ai-title` record landing in the transcript.
    fn titled(&self, title: &str) {
        *self.title.borrow_mut() = Some(title.to_owned());
    }

    fn user(&self, text: &str) {
        self.events.borrow_mut().push(human_event(text));
    }

    /// Assistant output landing mid-turn. This is the churn that used to make
    /// every event look like a new question to ask the provider.
    fn assistant(&self, text: &str) {
        self.events.borrow_mut().push(assistant_event(text));
    }

    fn injected(&self, text: &str) {
        self.events
            .borrow_mut()
            .push(SessionEvent::new("user", EventKind::Injected, 0, text));
    }

    fn interrupted(&self, text: &str) {
        self.events
            .borrow_mut()
            .push(SessionEvent::new("user", EventKind::Interrupted, 0, text));
    }
}

impl SessionReader for ScriptedSessionReader {
    fn read(&mut self, _: &Pane) -> Result<ParsedSession> {
        Ok(ParsedSession {
            title: self.title.borrow().clone(),
            events: self.events.borrow().clone(),
            skipped_lines: 0,
            skipped_reasons: Default::default(),
            rescan_reason: None,
        })
    }
}

struct FakeSessionReader;

impl SessionReader for FakeSessionReader {
    fn read(&mut self, _: &Pane) -> Result<ParsedSession> {
        Ok(ParsedSession {
            title: None,
            events: vec![human_event("작업 요약을 생성하고 표시를 검증해줘")],
            skipped_lines: 0,
            skipped_reasons: Default::default(),
            rescan_reason: None,
        })
    }
}

/// A failure that repeats for the same input must stop being re-asked. Two idle
/// panes once spent 1520 provider calls overnight on a context that could never
/// succeed, which exhausted the daily budget before anyone was awake.
#[test]
fn a_repeating_failure_is_abandoned_instead_of_retried_forever() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let mut focused = pane("w1:p9", AgentKind::Claude, "idle");
    focused.focused = true;
    let backend = malformed_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![focused]),
        router_with(&backend),
        FakeSessionReader,
        paths.clone(),
    );

    // One request. The router's own bounded retry for an answer the input
    // settles is the only repetition that happens.
    watcher.settle();
    let settled_attempts = usize::from(RouterConfig::default().max_invalid_output_attempts);
    assert_eq!(backend.calls(), settled_attempts);

    // From here nobody asks for anything, and the watcher must stay quiet
    // rather than rediscovering the same failure on every event.
    for _ in 0..8 {
        watcher.advance_past_provider_waits();
        watcher.settle();
    }
    assert_eq!(
        backend.calls(),
        settled_attempts,
        "a settled failure was asked again"
    );

    let log = fs::read_to_string(paths.log()).unwrap();
    assert!(
        log.contains("analysis_abandoned") && log.contains("invalid_output"),
        "log was: {log}"
    );
    // Abandoning is not a verdict: nothing is claimed about the pane.
    assert!(!log.contains("attention="), "log was: {log}");
}

/// The answering provider is part of the verdict's record, so a fallback can
/// be seen in the log rather than inferred.
#[test]
fn the_recorded_verdict_names_the_provider_that_answered() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let codex = ScriptedBackend::new(vec![Err(AiError::ProviderUnavailable("gone".to_owned()))]);
    let claude = ScriptedBackend::with_id(
        ProviderId::Claude,
        vec![Ok(json!({
            "task": "대체 provider 작업",
            "task_changed": true,
            "progress": "완료",
            "expected_reply": "",
            "attention": "none"
        }))],
    );
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_over(vec![
            Arc::clone(&codex) as Arc<dyn AiBackend>,
            Arc::clone(&claude) as Arc<dyn AiBackend>,
        ]),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.settle();
    assert_eq!(codex.calls(), 1);
    assert_eq!(claude.calls(), 1);
    let log = fs::read_to_string(paths.log()).unwrap();
    assert!(
        log.contains("analysis_recorded") && log.contains("provider=claude"),
        "log was: {log}"
    );
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("대체 provider 작업")
    );
}

/// A provider that panics inside the analysis thread must not leave the pane
/// in flight: the outcome arrives, the turn is abandoned with the reason, and
/// the next turn is analyzed.
#[test]
fn a_panicking_analysis_still_reports_and_frees_the_pane() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = Arc::new(PanickingBackend {
        calls: AtomicUsize::new(0),
    });
    let session = ScriptedSessionReader::new();
    session.user("첫 요청");
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "working")]),
        router_over(vec![Arc::clone(&backend) as Arc<dyn AiBackend>]),
        session,
        paths.clone(),
    );
    // Silence the default panic report for this test's expected panic.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    watcher.settle();
    std::panic::set_hook(previous);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert!(watcher.analysis_in_flight.is_empty());
    let log = fs::read_to_string(paths.log()).unwrap();
    assert!(
        log.contains("analysis_abandoned") && log.contains("analysis_worker_panicked"),
        "log was: {log}"
    );

    // The next turn is a fresh intent and goes through.
    watcher.session_reader.user("두 번째 요청");
    watcher.transport.panes.borrow_mut()[0].state_change_seq = 2;
    watcher.settle();
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        watcher.last_report().task.as_deref(),
        Some("두 번째 요청 요약")
    );
}

/// Moving into the Hide workspace changed the plugin id, and the automatic
/// summaries toggle lives in the state directory named by it.
#[test]
fn legacy_state_moves_to_the_new_id_once() {
    let home = tempdir().unwrap();
    let old_state = home.path().join(".local/state").join(LEGACY_PLUGIN_ID);
    let new_state = home.path().join(".local/state").join(PLUGIN_ID);
    fs::create_dir_all(&old_state).unwrap();
    fs::write(
        old_state.join("settings.json"),
        r#"{"automatic_summaries":false}"#,
    )
    .unwrap();

    assert_eq!(
        migrate_legacy_state(home.path()).unwrap(),
        vec![".local/state=moved"]
    );
    assert!(!old_state.exists());
    assert!(!load_settings(&StatePaths::from_home(home.path())).automatic_summaries);

    // Nothing left to move.
    assert!(migrate_legacy_state(home.path()).unwrap().is_empty());

    // Both present: neither is merged or deleted, and the caller is told.
    fs::create_dir_all(&old_state).unwrap();
    assert_eq!(
        migrate_legacy_state(home.path()).unwrap(),
        vec![".local/state=kept_both"]
    );
    assert!(old_state.exists() && new_state.exists());
}

/// The feature, not the transport, decides what is asked and what shape the
/// answer must take.
#[test]
fn a_label_request_carries_the_feature_prompt_and_schema() {
    let request = context_label::request("w1:p1", "w1:p1:turn".to_owned(), "<latest-exchange/>");
    assert_eq!(request.feature_id, context_label::FEATURE_ID);
    assert_eq!(request.subject_id, "w1:p1");
    assert_eq!(request.system, context_label::SYSTEM_PROMPT);
    assert!(request.input.contains("<latest-exchange/>"));
    assert_eq!(
        request.output_schema["required"],
        json!([
            "task",
            "task_changed",
            "progress",
            "expected_reply",
            "attention"
        ])
    );
}

struct StateSequenceReader;

impl SessionReader for StateSequenceReader {
    fn read(&mut self, pane: &Pane) -> Result<ParsedSession> {
        Ok(ParsedSession {
            title: None,
            events: vec![human_event(format!("새 요청 {}", pane.state_change_seq))],
            skipped_lines: 0,
            skipped_reasons: Default::default(),
            rescan_reason: None,
        })
    }
}

// ------------------------------------------------- session identity (PRD D-02..D-05)

fn parsed(title: Option<&str>, events: Vec<SessionEvent>) -> ParsedSession {
    ParsedSession {
        title: title.map(str::to_owned),
        events,
        skipped_lines: 0,
        skipped_reasons: Default::default(),
        rescan_reason: None,
    }
}

/// PRD D-02: Claude's own title names the session; without one the first
/// human turn stands in and says so; Codex always takes the first turn.
#[test]
fn session_name_is_the_claude_title_or_the_first_human_turn() {
    let turns = vec![
        human_event("결제 멱등키 PR을 리뷰하고 머지 준비해줘"),
        human_event("두 번째 질문"),
    ];
    assert_eq!(
        session_name(
            AgentKind::Claude,
            &parsed(Some("  결제 멱등키\n PR  "), turns.clone())
        ),
        (Some("결제 멱등키 PR".to_owned()), false)
    );
    let long: String = "가".repeat(120);
    let (cut, _) = session_name(AgentKind::Claude, &parsed(Some(&long), Vec::new()));
    assert_eq!(cut.unwrap().chars().count(), MAX_SESSION_NAME_CHARS);

    let (fallback, missing) = session_name(AgentKind::Claude, &parsed(None, turns.clone()));
    assert!(fallback.unwrap().starts_with("결제 멱등키 PR을"));
    assert!(missing, "a Claude session without ai-title is logged once");

    let (codex, missing) = session_name(AgentKind::Codex, &parsed(Some("ignored"), turns));
    assert!(codex.unwrap().starts_with("결제 멱등키 PR을"));
    assert!(!missing);

    assert_eq!(
        session_name(AgentKind::Codex, &parsed(None, Vec::new())),
        (None, false)
    );
    assert_eq!(
        session_name(AgentKind::Claude, &parsed(None, vec![human_event("짧음")])),
        (None, false),
        "a turn too short to be a name is no name, not a logged gap"
    );
}

#[test]
fn herdr_takes_only_a_lowercase_identifier_as_an_agent_name() {
    assert!(herdr_agent_name_acceptable("hook-bug-check"));
    assert!(herdr_agent_name_acceptable("a_1"));
    assert!(!herdr_agent_name_acceptable("Hook"));
    assert!(!herdr_agent_name_acceptable("1st"));
    assert!(!herdr_agent_name_acceptable("has space"));
    assert!(!herdr_agent_name_acceptable("결제"));
    assert!(!herdr_agent_name_acceptable(&"a".repeat(33)));
    assert!(herdr_agent_name_acceptable(&"a".repeat(32)));
}

#[test]
fn a_tab_label_is_owned_when_herdr_or_this_plugin_wrote_it() {
    assert!(tab_label_owned("3", None));
    assert!(tab_label_owned("Tab 12", None));
    assert!(tab_label_owned("결제 멱등키 PR", Some("결제 멱등키 PR")));
    assert!(!tab_label_owned("Tab", None));
    assert!(!tab_label_owned("Release", None));
    assert!(!tab_label_owned("결제 멱등키 PR", Some("다른 이름")));
}

/// PRD D-03, D-04: a name Herdr can hold becomes the agent name, the tab it
/// alone occupies takes the same label, and the `name` token stays empty
/// because the sidebar already reads the agent name.
#[test]
fn an_acceptable_title_renames_the_agent_and_its_tab_once() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let reader = ScriptedSessionReader::new();
    reader.titled("hook-bug-check");
    reader.user("훅 버그를 확인하고 고쳐줘");
    let transport = FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "working")]);
    transport
        .tab_labels
        .borrow_mut()
        .insert("w1:p1-tab".to_owned(), "Tab 1".to_owned());
    let mut watcher = Watcher::new(transport, router_with(&backend), reader, paths.clone());
    watcher.settle();

    let renames = watcher.transport.agent_renames.borrow().clone();
    assert_eq!(
        renames,
        vec![("w1:p1".to_owned(), "hook-bug-check".to_owned())]
    );
    let tabs = watcher.transport.tab_renames.borrow().clone();
    assert_eq!(
        tabs,
        vec![("w1:p1-tab".to_owned(), "hook-bug-check".to_owned())]
    );
    let identity = watcher
        .transport
        .identity_reports
        .borrow()
        .last()
        .cloned()
        .unwrap()
        .1;
    assert_eq!(identity.name, None, "the agent name carries the identity");
    assert_eq!(identity.progress.as_deref(), Some("착수"));

    // The same title is not asked about again, and Herdr echoing our name
    // back as the pane's name still reads as ours.
    watcher.scan().unwrap();
    watcher.scan().unwrap();
    assert_eq!(watcher.transport.agent_renames.borrow().len(), 1);
    assert_eq!(watcher.transport.tab_renames.borrow().len(), 1);
    assert_eq!(
        watcher.transport.panes.borrow()[0].name.as_deref(),
        Some("hook-bug-check")
    );

    // A new title from the session moves both names again.
    watcher.session_reader.titled("hook-bug-fixed");
    watcher.transport.panes.borrow_mut()[0].revision += 1;
    watcher.scan().unwrap();
    assert_eq!(
        watcher.transport.agent_renames.borrow().last().unwrap().1,
        "hook-bug-fixed"
    );
    assert_eq!(
        watcher.transport.tab_renames.borrow().last().unwrap().1,
        "hook-bug-fixed"
    );
    let log = fs::read_to_string(paths.log()).unwrap();
    assert!(!log.contains("agent_rename_failed"), "log was: {log}");
    assert!(!log.contains("session_title_missing"), "log was: {log}");
}

/// PRD D-03 (amended): a title Herdr refuses as an agent name is not
/// retried; it travels as the `name` token in the second report, while the
/// tab, which takes any label, is still renamed.
#[test]
fn a_korean_title_becomes_the_name_token_and_the_tab_label() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let reader = ScriptedSessionReader::new();
    reader.titled("결제 멱등키 PR");
    reader.user("결제 멱등키 PR을 리뷰해줘");
    let transport = FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "working")]);
    transport
        .tab_labels
        .borrow_mut()
        .insert("w1:p1-tab".to_owned(), "1".to_owned());
    let mut watcher = Watcher::new(transport, router_with(&backend), reader, paths.clone());
    watcher.settle();

    assert!(watcher.transport.agent_renames.borrow().is_empty());
    let identity = watcher
        .transport
        .identity_reports
        .borrow()
        .first()
        .cloned()
        .unwrap()
        .1;
    assert_eq!(identity.name.as_deref(), Some("결제 멱등키 PR"));
    assert_eq!(
        watcher.transport.tab_renames.borrow().clone(),
        vec![("w1:p1-tab".to_owned(), "결제 멱등키 PR".to_owned())]
    );
    let state = &watcher.display_states.panes["w1:p1"];
    assert_eq!(state.plugin_name.as_deref(), Some("결제 멱등키 PR"));
    assert!(!state.plugin_name_as_agent);
}

/// PRD B10, D-04: a name the operator gave the agent and a label the
/// operator typed on the tab are never overwritten, and the `name` token is
/// left empty so the operator's name is what the sidebar shows.
#[test]
fn an_operator_named_agent_and_a_typed_tab_label_are_left_alone() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let reader = ScriptedSessionReader::new();
    reader.titled("결제 멱등키 PR");
    reader.user("결제 멱등키 PR을 리뷰해줘");
    let mut named = pane("w1:p1", AgentKind::Claude, "working");
    named.name = Some("observer".to_owned());
    let transport = FakeTransport::new(vec![named]);
    transport
        .tab_labels
        .borrow_mut()
        .insert("w1:p1-tab".to_owned(), "Release".to_owned());
    let mut watcher = Watcher::new(transport, router_with(&backend), reader, paths.clone());
    watcher.settle();

    assert!(watcher.transport.agent_renames.borrow().is_empty());
    assert!(watcher.transport.tab_renames.borrow().is_empty());
    let identity = watcher
        .transport
        .identity_reports
        .borrow()
        .last()
        .cloned()
        .unwrap()
        .1;
    assert_eq!(
        identity.name, None,
        "the operator's name wins over the session title"
    );
    assert_eq!(identity.progress.as_deref(), Some("착수"));
    assert_eq!(watcher.display_states.panes["w1:p1"].plugin_name, None);
    let log = fs::read_to_string(paths.log()).unwrap();
    assert!(!log.contains("tab_rename_failed"), "log was: {log}");
}

/// PRD D-04: a tab that holds two agents keeps its own label, since neither
/// agent's name describes it.
#[test]
fn a_tab_shared_by_two_agents_keeps_its_label() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let reader = ScriptedSessionReader::new();
    reader.titled("hook-bug-check");
    reader.user("훅 버그를 확인하고 고쳐줘");
    let mut first = pane("w1:p1", AgentKind::Claude, "working");
    let mut second = pane("w1:p2", AgentKind::Codex, "working");
    first.tab_id = "w1:t1".to_owned();
    second.tab_id = "w1:t1".to_owned();
    let transport = FakeTransport::new(vec![first, second]);
    transport
        .tab_labels
        .borrow_mut()
        .insert("w1:t1".to_owned(), "1".to_owned());
    let mut watcher = Watcher::new(transport, router_with(&backend), reader, paths);
    watcher.settle();

    assert!(watcher.transport.tab_renames.borrow().is_empty());
    assert_eq!(watcher.transport.tab_labels.borrow()["w1:t1"], "1");
    // Each agent is still named its own way: the Claude title as the agent
    // name, the Codex first turn (Korean) as its token.
    assert_eq!(watcher.transport.agent_renames.borrow().len(), 1);
    let codex_token = watcher
        .transport
        .identity_reports
        .borrow()
        .iter()
        .find(|(id, _)| id == "w1:p2")
        .map(|(_, identity)| identity.name.clone())
        .unwrap();
    assert!(codex_token.is_some());
}

/// PRD D-02: a pane whose session Herdr has not recorded yet reads the
/// newest transcript in its cwd, which may be another pane's; no name is
/// written from that guess, so the tab and agent wait for the pane's own
/// session.
#[test]
fn a_pane_without_its_own_session_is_not_named_from_a_borrowed_transcript() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let reader = ScriptedSessionReader::new();
    reader.titled("hook-bug-check");
    reader.user("훅 버그를 확인하고 고쳐줘");
    let mut unrecorded = pane("w1:p1", AgentKind::Claude, "working");
    unrecorded.agent_session = None;
    let transport = FakeTransport::new(vec![unrecorded]);
    transport
        .tab_labels
        .borrow_mut()
        .insert("w1:p1-tab".to_owned(), "1".to_owned());
    let mut watcher = Watcher::new(transport, router_with(&backend), reader, paths);
    watcher.settle();

    assert!(watcher.transport.agent_renames.borrow().is_empty());
    assert!(watcher.transport.tab_renames.borrow().is_empty());
    let identity = watcher
        .transport
        .identity_reports
        .borrow()
        .last()
        .cloned()
        .unwrap()
        .1;
    assert_eq!(identity.name, None);
    assert_eq!(
        identity.progress.as_deref(),
        Some("착수"),
        "the sentence tokens still flow"
    );
}

/// PRD D-11: a rename Herdr refuses or a tab it cannot read is one log line
/// and no retry; the name that did not land is the visible failure.
#[test]
fn a_refused_rename_and_an_unknown_tab_are_logged_once_and_not_retried() {
    struct RefusingTransport(FakeTransport);
    impl HerdrTransport for RefusingTransport {
        fn panes(&self) -> Result<Vec<Pane>> {
            self.0.panes()
        }
        fn report(&self, pane: &Pane, display: &Display) -> Result<()> {
            self.0.report(pane, display)
        }
        fn report_identity(&self, pane: &Pane, identity: &Identity) -> Result<()> {
            self.0.report_identity(pane, identity)
        }
        fn rename_agent(&self, _: &Pane, name: &str) -> Result<()> {
            Err(anyhow!("agent.rename failed: name {name} is taken"))
        }
        fn tab_label(&self, tab_id: &str) -> Result<String> {
            self.0.tab_label(tab_id)
        }
        fn rename_tab(&self, tab_id: &str, label: &str) -> Result<()> {
            self.0.rename_tab(tab_id, label)
        }
    }
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let reader = ScriptedSessionReader::new();
    reader.titled("hook-bug-check");
    reader.user("훅 버그를 확인하고 고쳐줘");
    let transport = RefusingTransport(FakeTransport::new(vec![pane(
        "w1:p1",
        AgentKind::Claude,
        "working",
    )]));
    let mut watcher = Watcher::new(transport, router_with(&backend), reader, paths.clone());
    watcher.settle();
    watcher.scan().unwrap();

    let log = fs::read_to_string(paths.log()).unwrap();
    assert_eq!(
        log.matches("agent_rename_failed").count(),
        1,
        "log was: {log}"
    );
    assert_eq!(
        log.matches("tab_rename_failed").count(),
        1,
        "log was: {log}"
    );
    // The refused name still reaches the sidebar, as the token.
    let identity = watcher
        .transport
        .0
        .identity_reports
        .borrow()
        .first()
        .cloned()
        .unwrap()
        .1;
    assert_eq!(identity.name.as_deref(), Some("hook-bug-check"));
    assert!(watcher.transport.0.tab_renames.borrow().is_empty());
}

/// PRD D-02: a Claude session that has not written `ai-title` yet is named
/// by its first turn and logged once, not once per scan.
#[test]
fn a_missing_claude_title_is_logged_once() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = task_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "working")]),
        router_with(&backend),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.settle();
    watcher.transport.panes.borrow_mut()[0].revision += 5;
    watcher.scan().unwrap();
    let log = fs::read_to_string(paths.log()).unwrap();
    assert_eq!(
        log.matches("session_title_missing").count(),
        1,
        "log was: {log}"
    );
    let identity = watcher
        .transport
        .identity_reports
        .borrow()
        .first()
        .cloned()
        .unwrap()
        .1;
    assert!(
        identity.name.is_some(),
        "the first turn stands in until the title lands"
    );

    // Codex has no title record, so its first turn is the name, not a gap.
    let paths = StatePaths::for_tests(root.path().join("codex").as_path());
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p2", AgentKind::Codex, "working")]),
        router_with(&backend),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.settle();
    let log = fs::read_to_string(paths.log()).unwrap();
    assert!(!log.contains("session_title_missing"), "log was: {log}");
}

/// PRD D-05, B4: the second report carries the sentence tokens, an
/// `expected_reply` is cut to the forty characters the row can show, a
/// restart republishes both from the persisted state without a provider
/// call, and the agent going back to work clears the reply it asked for.
#[test]
fn the_second_report_carries_the_sentence_survives_a_restart_and_clears_on_work() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let long_reply = "가".repeat(55);
    let backend = ScriptedBackend::new(vec![Ok(json!({
        "task": "음료 소비량 퀴즈 풀이",
        "task_changed": true,
        "progress": "선택 대기",
        "expected_reply": long_reply,
        "attention": "question"
    }))]);
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Codex, "blocked")]),
        router_with(&backend),
        FakeSessionReader,
        paths.clone(),
    );
    watcher.settle();
    let identity = watcher
        .transport
        .identity_reports
        .borrow()
        .last()
        .cloned()
        .unwrap()
        .1;
    assert_eq!(identity.progress.as_deref(), Some("선택 대기"));
    assert_eq!(
        identity
            .expected_reply
            .as_deref()
            .map(|reply| reply.chars().count()),
        Some(MAX_EXPECTED_REPLY_CHARS)
    );
    assert_eq!(backend.calls(), 1);

    // A restart: the same tokens go out again from disk, and nothing is asked.
    let mut restarted = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Codex, "blocked")]),
        router_with(&backend),
        FakeSessionReader,
        paths.clone(),
    );
    restarted.scan().unwrap();
    let republished = restarted
        .transport
        .identity_reports
        .borrow()
        .first()
        .cloned()
        .unwrap()
        .1;
    assert_eq!(republished, identity);
    assert_eq!(
        backend.calls(),
        1,
        "the persisted sentence costs no request"
    );

    // The agent works again: the reply it waited for is no longer asked of anyone.
    restarted.transport.set_status("working");
    restarted.scan().unwrap();
    let working = restarted
        .transport
        .identity_reports
        .borrow()
        .last()
        .cloned()
        .unwrap()
        .1;
    assert_eq!(working.expected_reply, None);
    assert_eq!(
        working.progress.as_deref(),
        Some("선택 대기"),
        "progress stays until the next verdict"
    );
}

/// The second report clears a token it no longer holds with an explicit
/// null, so a stale value cannot survive on the pane.
#[test]
fn identity_params_clear_absent_tokens_explicitly() {
    let identity = Identity {
        name: Some("결제 멱등키 PR".to_owned()),
        progress: None,
        expected_reply: None,
    };
    let params = identity_params(&pane("w1:p1", AgentKind::Claude, "idle"), &identity);
    assert_eq!(params["pane_id"], json!("w1:p1"));
    assert_eq!(params["source"], json!(PLUGIN_ID));
    assert_eq!(params["tokens"]["name"], json!("결제 멱등키 PR"));
    assert_eq!(params["tokens"]["progress"], Value::Null);
    assert_eq!(params["tokens"]["expected_reply"], Value::Null);
}
