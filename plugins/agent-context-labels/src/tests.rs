use super::*;
use hide_ai::{
    AiBackend, AiResponse, AiUsage, Availability, NoopLogSink, ProviderId, RouterConfig,
};
use serde_json::{Value, json};
use std::cell::RefCell;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
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

    /// One poll cycle: scan, let the provider thread finish, scan again to
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
        agent_session: None,
        agent_status: status.to_owned(),
        revision: 1,
        state_change_seq: 1,
        cwd: None,
        focused: false,
    }
}

// ---------------------------------------------------------------- AC1

#[test]
fn agent_list_payload_from_a_live_session_parses_without_a_second_endpoint() {
    let payload = include_str!("../tests/fixtures/agent-list.json");
    let envelope: AgentListEnvelope = serde_json::from_str(payload).unwrap();
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

    let reader = LocalSessionReader::new(root.path());
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

    let reader = LocalSessionReader::new(root.path());
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

    let reader = LocalSessionReader::new(root.path());
    let mut target = pane("w1:p1", AgentKind::Codex, "idle");
    target.cwd = Some("/Users/example/projects/mine".to_owned());

    assert_eq!(reader.session_path(&target).unwrap(), mine);
}

#[test]
fn session_reads_only_the_tail_of_a_large_file() {
    let root = tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    let filler = "{\"type\":\"user\",\"message\":{\"content\":\"오래된 내용\"}}\n";
    let mut contents = filler.repeat(20_000);
    contents.push_str("{\"type\":\"user\",\"message\":{\"content\":\"최신 요청\"}}\n");
    fs::write(&path, &contents).unwrap();
    assert!(fs::metadata(&path).unwrap().len() > SESSION_TAIL_BYTES);

    let tail = read_tail(&path, SESSION_TAIL_BYTES).unwrap();

    assert!(tail.len() as u64 <= SESSION_TAIL_BYTES);
    assert!(tail.contains("최신 요청"));
    // The first line of a tail read is cut in half and must be discarded.
    assert!(serde_json::from_str::<serde_json::Value>(tail.lines().next().unwrap()).is_ok());
}

// ---------------------------------------------------------------- AC3

#[test]
fn skips_malformed_session_lines() {
    let torn = concat!(
        r#"{"type":"user","message":{"content":"첫 번째 요청"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"작업했습니다."}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tex"#,
    );

    let parsed = parse_claude_events(torn);

    assert_eq!(parsed.skipped_lines, 1);
    assert_eq!(parsed.events.len(), 2);
    assert_eq!(parsed.events[0].text, "첫 번째 요청");
    assert_eq!(parsed.events[1].text, "작업했습니다.");

    let codex = concat!(
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"요약을 고쳐줘"}]}}"#,
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
    let events = [SessionEvent {
        role: "assistant",
        text: concat!(
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
        )
        .to_owned(),
    }];

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
fn context_spans_the_last_two_user_turns() {
    let events = [
        SessionEvent {
            role: "user",
            text: "가장 오래된 요청입니다".to_owned(),
        },
        SessionEvent {
            role: "assistant",
            text: "가장 오래된 응답입니다".to_owned(),
        },
        SessionEvent {
            role: "user",
            text: "직전 요청입니다".to_owned(),
        },
        SessionEvent {
            role: "assistant",
            text: "직전 응답입니다".to_owned(),
        },
        SessionEvent {
            role: "user",
            text: "최신 요청입니다".to_owned(),
        },
        SessionEvent {
            role: "assistant",
            text: "긴 응답 ".repeat(200),
        },
        SessionEvent {
            role: "assistant",
            text: "마지막으로 답을 기다립니다".to_owned(),
        },
    ];

    let context = analysis_context(&events);

    assert!(context.contains("최신 요청입니다"));
    assert!(context.contains("마지막으로 답을 기다립니다"));
    // The previous exchange stays visible so a wrap-up of an already answered
    // question is not mistaken for a fresh one.
    assert!(context.contains("직전 요청입니다"));
    assert!(context.contains("직전 응답입니다"));
    // Every user turn is carried too, so the summary can see the whole arc of
    // what was asked, not just the latest exchange.
    assert!(context.contains("가장 오래된 요청입니다"));
    // Older assistant prose is not part of that request history and stays out.
    assert!(!context.contains("가장 오래된 응답입니다"));
}

#[test]
fn all_user_requests_is_capped_to_the_recent_turns() {
    let mut events: Vec<SessionEvent> = Vec::new();
    for i in 0..(MAX_USER_REQUEST_TURNS + 3) {
        events.push(SessionEvent {
            role: "user",
            text: format!("요청 {i}"),
        });
        events.push(SessionEvent {
            role: "assistant",
            text: format!("응답 {i}"),
        });
    }

    let context = analysis_context(&events);

    // The oldest requests, beyond the cap, are dropped.
    assert!(!context.contains("요청 0"));
    assert!(!context.contains("요청 2"));
    // The most recent MAX_USER_REQUEST_TURNS requests survive.
    for i in (events.len() / 2 - MAX_USER_REQUEST_TURNS)..(events.len() / 2) {
        assert!(context.contains(&format!("요청 {i}")));
    }
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
fn summary_truncates_on_a_word_boundary() {
    assert_eq!(truncate_summary("짧은 요약"), "짧은 요약");
    // "Commit benchmark PRD validation" used to render as "Commit benchmark PRD validatio".
    assert_eq!(
        truncate_summary("Commit benchmark PRD validation"),
        "Commit benchmark PRD…"
    );
    assert!(
        truncate_summary("Commit benchmark PRD validation")
            .chars()
            .count()
            <= MAX_SUMMARY_CHARS
    );
    // A single unbroken token still has to fit the budget.
    let unbroken = "a".repeat(50);
    assert_eq!(
        truncate_summary(&unbroken).chars().count(),
        MAX_SUMMARY_CHARS
    );
    assert!(truncate_summary(&unbroken).ends_with('…'));
}

#[test]
fn normalizes_only_safe_one_line_summaries() {
    assert_eq!(
        normalize_summary("  간단한 작업 요약  "),
        Some("간단한 작업 요약".into())
    );
    assert_eq!(normalize_summary("\n"), None);
    assert_eq!(
        normalize_summary("**`Compact task labels`**"),
        Some("Compact task labels".into())
    );
    assert_eq!(normalize_summary("bad\u{0000}"), None);
    assert_eq!(normalize_summary("ab"), None);
}

#[test]
fn parses_the_provider_structured_output_contract() {
    assert_eq!(
        context_label::parse_text(r#"{"summary":"수학 문제 출제 및 채점","attention":"none"}"#)
            .unwrap(),
        Analysis {
            summary: "수학 문제 출제 및 채점".into(),
            attention: None,
        }
    );
    // A bare question verdict with no expected_reply field carries no statable
    // user action, so it downgrades like an empty one.
    assert_eq!(
        context_label::parse_text(r#"{"summary":"음료 소비량 퀴즈 풀이","attention":"question"}"#)
            .unwrap()
            .attention,
        None
    );
    // A question verdict needs a statable user action to survive.
    assert_eq!(
        context_label::parse_text(
            r#"{"expected_reply":"배포 진행 여부를 답한다","summary":"배포 진행 여부 확인","attention":"question"}"#
        )
        .unwrap()
        .attention,
        Some(Attention::Question)
    );
    // Question without an expected reply is a surface match and is downgraded.
    assert_eq!(
        context_label::parse_text(
            r#"{"expected_reply":"","summary":"새 작업 지시 대기","attention":"question"}"#
        )
        .unwrap()
        .attention,
        None
    );
    assert!(
        context_label::parse_text(r#"{"summary":"작업 승인","attention":"approval"}"#).is_err()
    );
    assert!(context_label::parse_text(r#"{"summary":"상태 없는 응답"}"#).is_err());
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
    assert!(log.contains("attention=question"), "log was: {log}");
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
    drop(watcher);

    let mut restarted = Watcher::new(
        FakeTransport::new(vec![subject]),
        no_provider(),
        FakeSessionReader,
        paths,
    );
    restarted.scan().unwrap();
    assert_eq!(restarted.last_report().status, StatusIcon::Question);
}

// ------------------------------------------------------------- scheduling

#[test]
fn watcher_deduplicates_reports_and_its_own_revision_bump() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let backend = summary_backend();
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
    let backend = summary_backend();
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
    let backend = summary_backend();
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
            .contains("summary_refresh_skipped_disabled")
    );
    assert_eq!(backend.calls(), 0);
}

#[test]
fn metadata_clears_every_status_token_it_may_own() {
    let subject = pane("w1:p1", AgentKind::Codex, "idle");
    let args = metadata_arguments(
        &subject,
        &Display {
            summary: Some("작업 요약".into()),
            status: StatusIcon::Approval,
            sort_key: SortKey::Approval,
            elapsed: Some("7s".into()),
            ..Display::default()
        },
    );

    assert!(args.iter().any(|item| item == "summary=작업 요약"));
    assert!(args.iter().any(|item| item == "status_approval=!"));
    assert!(args.iter().any(|item| item == "agent_codex=⬢"));
    assert!(args.iter().any(|item| item == "elapsed=7s"));
    // Approval sits in the user-blocking group at the top of the ordering.
    // The rank digit depends on the machine's optional sort-order file, so
    // only the seen partition prefix is asserted.
    assert!(
        args.iter()
            .any(|item| item.starts_with("sort_rank=1") && item.len() == "sort_rank=1x".len())
    );
    // Every other status token is cleared, so two icons can never render at
    // once; the one being set is not cleared to stay under the 16-token cap.
    for token in STATUS_TOKENS {
        if token == "status_approval" {
            continue;
        }
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--clear-token" && pair[1] == token),
            "{token} is not cleared"
        );
    }
    // The report never exceeds Herdr's 16-token budget.
    let touched = args
        .iter()
        .filter(|a| *a == "--token" || *a == "--clear-token")
        .count();
    assert!(touched <= 16, "report touches {touched} tokens");
    // The pane title belongs to the user, not to this plugin.
    assert!(!args.iter().any(|item| item == "--title"));
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

    let args = metadata_arguments(
        &pane("w1:p1", AgentKind::Codex, "done"),
        &Display {
            status: StatusIcon::Done,
            sort_key: SortKey::Done,
            activity_unix_ms: 1_755_000_000_000,
            ..Display::default()
        },
    );
    assert!(args.iter().any(|item| item == "activity=1755000000000"));
    // Always set, never cleared: the sidebar cannot order a pane without it.
    assert!(
        !args
            .windows(2)
            .any(|pair| pair[0] == "--clear-token" && pair[1] == "activity")
    );
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
/// asked the provider again on almost every poll.
#[test]
fn turn_identity_holds_while_output_grows_and_moves_only_at_a_boundary() {
    let user = |text: &str| SessionEvent {
        role: "user",
        text: text.to_owned(),
    };
    let assistant = |text: &str| SessionEvent {
        role: "assistant",
        text: text.to_owned(),
    };

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

/// One turn buys one summary and one verdict, whatever the agent does in
/// between. On 2026-08-17 the old trigger spent 470 calls across 19 panes,
/// 37 of them under ten seconds apart on the same pane.
#[test]
fn one_turn_costs_at_most_two_provider_calls() {
    let root = tempdir().unwrap();
    let paths = StatePaths::for_tests(root.path());
    let session = ScriptedSessionReader::new();
    let backend = summary_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_with(&backend),
        session,
        paths.clone(),
    );

    // Nothing said yet: no turn, so no call however often the watcher polls.
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
    let backend = summary_backend();
    let mut watcher = Watcher::new(
        FakeTransport::new(vec![pane("w1:p1", AgentKind::Claude, "idle")]),
        router_with(&backend),
        session,
        paths.clone(),
    );
    watcher.scan().unwrap();
    watcher.await_pending_analysis();
    watcher.scan().unwrap();

    let written = fs::read_to_string(paths.display_state()).unwrap();
    assert!(
        !written.contains("analysis_fingerprint"),
        "the content fingerprint is gone: {written}"
    );
    assert!(written.contains("analysis_turn_start"), "{written}");
    assert!(written.contains("analysis_turn_end"), "{written}");
}

// ------------------------------------------------------------------ doubles

struct FakeTransport {
    panes: RefCell<Vec<Pane>>,
    reports: RefCell<Vec<Display>>,
}

impl FakeTransport {
    fn new(panes: Vec<Pane>) -> Self {
        Self {
            panes: RefCell::new(panes),
            reports: RefCell::new(Vec::new()),
        }
    }

    /// Move Herdr's lifecycle for every pane, the way the real server does
    /// between polls.
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
}

struct BrokenTransport;

impl HerdrTransport for BrokenTransport {
    fn panes(&self) -> Result<Vec<Pane>> {
        Err(anyhow!("herdr socket is unavailable"))
    }
    fn report(&self, _: &Pane, _: &Display) -> Result<()> {
        Ok(())
    }
}

impl<R: SessionReader> Watcher<FakeTransport, R> {
    fn last_report(&self) -> Display {
        self.transport.reports.borrow().last().cloned().unwrap()
    }
}

/// A provider the test scripts: each request receives the next reply, and the
/// last one repeats. Counting calls is what the request-budget tests observe.
struct ScriptedBackend {
    calls: AtomicUsize,
    replies: Mutex<Vec<std::result::Result<Value, AiError>>>,
}

impl ScriptedBackend {
    fn new(replies: Vec<std::result::Result<Value, AiError>>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            replies: Mutex::new(replies),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl AiBackend for ScriptedBackend {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    fn availability(&self) -> Availability {
        Availability::Ready
    }

    fn execute(
        &self,
        _: &hide_ai::AiRequest,
        _: &CancelToken,
    ) -> std::result::Result<AiResponse, AiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
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
    let backend: Arc<dyn AiBackend> = Arc::clone(backend) as Arc<dyn AiBackend>;
    Arc::new(AiRouter::with_sleep(
        vec![backend],
        RouterConfig::default(),
        Arc::new(NoopLogSink),
        Box::new(|_| {}),
    ))
}

/// Nothing connected: every request answers `NoProvider`.
fn no_provider() -> Arc<AiRouter> {
    Arc::new(AiRouter::new(
        Vec::new(),
        RouterConfig::default(),
        Arc::new(NoopLogSink),
    ))
}

fn summary_backend() -> Arc<ScriptedBackend> {
    ScriptedBackend::new(vec![Ok(json!({
        "expected_reply": "",
        "summary": "작업 요약",
        "attention": "none"
    }))])
}

fn question_backend() -> Arc<ScriptedBackend> {
    ScriptedBackend::new(vec![Ok(json!({
        "expected_reply": "퀴즈 답을 말한다",
        "summary": "음료 소비량 퀴즈 풀이",
        "attention": "question"
    }))])
}

fn usage_limited_backend() -> Arc<ScriptedBackend> {
    ScriptedBackend::new(vec![Err(AiError::UsageLimited { retry_after: None })])
}

/// Well-formed JSON that is not the answer: the router refuses it against
/// the feature schema before the feature ever sees it.
fn malformed_backend() -> Arc<ScriptedBackend> {
    ScriptedBackend::new(vec![Ok(json!({"summary": "작업 요약"}))])
}

/// A transcript the test drives turn by turn, so a scan sees exactly the
/// conversation state the scenario is about.
struct ScriptedSessionReader {
    events: RefCell<Vec<SessionEvent>>,
}

impl ScriptedSessionReader {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
        }
    }

    fn user(&self, text: &str) {
        self.events.borrow_mut().push(SessionEvent {
            role: "user",
            text: text.to_owned(),
        });
    }

    /// Assistant output landing mid-turn. This is the churn that used to make
    /// every poll look like a new question to ask the provider.
    fn assistant(&self, text: &str) {
        self.events.borrow_mut().push(SessionEvent {
            role: "assistant",
            text: text.to_owned(),
        });
    }
}

impl SessionReader for ScriptedSessionReader {
    fn read(&self, _: &Pane) -> Result<ParsedSession> {
        Ok(ParsedSession {
            events: self.events.borrow().clone(),
            skipped_lines: 0,
        })
    }
}

struct FakeSessionReader;

impl SessionReader for FakeSessionReader {
    fn read(&self, _: &Pane) -> Result<ParsedSession> {
        Ok(ParsedSession {
            events: vec![SessionEvent {
                role: "user",
                text: "작업 요약을 생성하고 표시를 검증해줘".to_owned(),
            }],
            skipped_lines: 0,
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
    // rather than rediscovering the same failure on every poll.
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
        json!(["expected_reply", "summary", "attention"])
    );
}

struct StateSequenceReader;

impl SessionReader for StateSequenceReader {
    fn read(&self, pane: &Pane) -> Result<ParsedSession> {
        Ok(ParsedSession {
            events: vec![SessionEvent {
                role: "user",
                text: format!("새 요청 {}", pane.state_change_seq),
            }],
            skipped_lines: 0,
        })
    }
}
