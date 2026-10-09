//! The labels the local coordinator published reach every local projection
//! the runtime ingests (PRD labels-in-hided D-04), not only the
//! coordinator's own: a workspace creation or an agent close hands the
//! runtime a fresh Herdr projection of its own.

use super::*;

use crate::labels::overlay::LabelOverlay;
use crate::labels::store::PaneRecord;

const CHECKOUT: &str = "/private/tmp/hide-labels-overlay";
const TABS: [&str; 1] = ["w-order:t1"];
const PANE: &str = "w-order:t1:p";
const SESSION: &str = "11111111-2222-3333-4444-555555555555";

/// A fresh projection with one idle Claude agent on `session`, carrying no
/// label, as Herdr itself reports it.
fn projection(session: &str) -> SessionSnapshotPayload {
    let mut payload = tab_order_payload(CHECKOUT, &TABS, &TABS, "w-order:t1");
    let agent: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({"agents": [{
        "id": "worker", "pane_id": PANE, "agent": "claude",
        "agent_status": "idle", "state_change_seq": 4, "cwd": CHECKOUT,
        "agent_session": {"kind": "id", "value": session}
    }]}))
    .unwrap();
    payload.agents.extend(agent.agents);
    payload
}

fn row(runtime: &Runtime) -> serde_json::Value {
    serde_json::to_value(&runtime.snapshot().navigator.agents)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["pane_id"] == PANE)
        .cloned()
        .expect("the agent row")
}

#[test]
fn a_projection_from_another_path_keeps_the_published_label_of_its_session() {
    let mut runtime = runtime();
    let record = PaneRecord {
        owner: hide_session::label_reference_token("claude", "id", SESSION),
        goal: Some("파서 버그 수정 작업".to_owned()),
        changed_unix_ms: 1_000,
        ..PaneRecord::default()
    };
    let records = [(PANE.to_owned(), record)];
    runtime.set_label_overlay(LabelOverlay::of_records(
        records.iter().map(|(pane, record)| (pane, record)),
        true,
        true,
    ));

    // A workspace creation's own projection: no coordinator publish between.
    runtime.ingest_session(Ok(projection(SESSION)));
    assert_eq!(row(&runtime)["identity_label"], "파서 버그 수정 작업");

    // The pane now runs another session: the published label is not its own.
    runtime.ingest_session(Ok(projection("99999999-2222-3333-4444-555555555555")));
    assert_eq!(row(&runtime)["identity_label"], "Claude");
}

/// PRD overview-request-view D-19: a typed Enter and a phone reply to an
/// agent pane are kept as the operator's submits; an Enter at an approval
/// prompt, a newline and other keys are not.
#[test]
fn an_operator_submit_is_recorded_for_the_agent_pane_and_an_approval_enter_is_not() {
    let mut runtime = runtime();
    let services = std::sync::Arc::new(
        crate::labels::LabelServices::start(
            None,
            None,
            std::sync::Weak::new(),
            &crate::node::test_node(),
            std::sync::Arc::new(hide_node::Local::of_process()),
        )
        .unwrap(),
    );
    runtime.install_label_services(std::sync::Arc::clone(&services));
    runtime.ingest_session(Ok(projection(SESSION)));
    let submits = || {
        services
            .input
            .submits(crate::labels::store::LOCAL_TARGET, PANE)
    };
    // An escaped return (a newline in Claude Code) is not a submit; the
    // node decides that, and reports only what it decided.
    typed_into(&mut runtime, PANE, false);
    typed_into(&mut runtime, PANE, false);
    assert!(submits().is_empty());
    typed_into(&mut runtime, PANE, true);
    assert_eq!(submits().len(), 1);
    assert!(!submits()[0].while_working);

    assert!(
        !runtime.dispatch_json(
            &serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION, "kind": "pane_input_submitted",
                "payload": {"pane_id": PANE},
            }))
            .unwrap()
        )
    );
    assert_eq!(submits().len(), 2);

    let mut blocked = projection(SESSION);
    let agent = blocked
        .agents
        .iter_mut()
        .find(|agent| agent.pane_id.as_deref() == Some(PANE))
        .unwrap();
    agent.agent_status = Some("blocked".to_owned());
    agent.state_change_seq = Some(5);
    runtime.ingest_session(Ok(blocked));
    typed_into(&mut runtime, PANE, true);
    assert_eq!(submits().len(), 2, "an approval's Enter is not a request");
}

/// A runtime whose agent row carries `facts` for its current session.
fn runtime_with_facts(facts: crate::labels::facts::SessionFacts, status: &str) -> Runtime {
    runtime_with_record(
        PaneRecord {
            facts,
            ..PaneRecord::default()
        },
        status,
        true,
    )
}

/// A runtime whose agent row carries `record` for its current session, with
/// agent summaries `summaries`.
fn runtime_with_record(record: PaneRecord, status: &str, summaries: bool) -> Runtime {
    let mut runtime = runtime();
    let record = PaneRecord {
        owner: hide_session::label_reference_token("claude", "id", SESSION),
        ..record
    };
    let records = [(PANE.to_owned(), record)];
    runtime.set_label_overlay(LabelOverlay::of_records(
        records.iter().map(|(pane, record)| (pane, record)),
        true,
        summaries,
    ));
    let mut payload = projection(SESSION);
    for agent in &mut payload.agents {
        if agent.pane_id.as_deref() == Some(PANE) {
            agent.agent_status = Some(status.to_owned());
        }
    }
    runtime.ingest_session(Ok(payload));
    runtime
}

fn operator_asked(text: &str) -> crate::labels::facts::SessionFacts {
    let mut facts = crate::labels::facts::SessionFacts::default();
    facts.title = Some("요청 보기 만들기".to_owned());
    facts.operator_request = Some(crate::labels::facts::Request {
        text: text.to_owned(),
        cut: false,
        images: 0,
        at_unix_ms: 1_000,
        requester: crate::labels::facts::Requester::Operator,
        first: true,
    });
    facts
}

/// PRD overview-request-view B17, B24: with no AI label the row is named by
/// the agent's own title and carries the operator's request as written.
#[test]
fn a_row_without_a_label_shows_the_sessions_own_title_and_the_request() {
    let runtime = runtime_with_facts(operator_asked("요청 보기를 만들어줘"), "idle");
    let row = row(&runtime);
    assert_eq!(row["identity_label"], "요청 보기 만들기");
    assert_eq!(row["request"]["request"]["text"], "요청 보기를 만들어줘");
    assert_eq!(row["request"]["request"]["sender"]["kind"], "operator");
}

/// B14, B18, B21: an unfinished turn stops the row and a wait on something
/// other than a pull request waits; with agent summaries off the same row
/// is named by its own title, carries no line and never stops.
#[test]
fn an_unfinished_turn_stops_the_row_and_summaries_off_take_every_ai_field_away() {
    use crate::labels::analysis::LabelEnd;
    let record = |end| PaneRecord {
        goal: Some("요청 보기와 AI 스위치".to_owned()),
        line: "테스트 환경이 막혀 멈췄어요".to_owned(),
        end: Some(end),
        facts: operator_asked("요청 보기 만들어줘"),
        ..PaneRecord::default()
    };
    let stopped = row(&runtime_with_record(
        record(LabelEnd::Unfinished),
        "idle",
        true,
    ));
    assert_eq!(stopped["identity_label"], "요청 보기와 AI 스위치");
    assert_eq!(stopped["request"]["line"], "테스트 환경이 막혀 멈췄어요");
    assert_eq!(stopped["request"]["verb"], "stopped");
    let waiting = row(&runtime_with_record(
        record(LabelEnd::Waiting),
        "idle",
        true,
    ));
    assert_eq!(waiting["request"]["verb"], "waiting");

    let off = row(&runtime_with_record(
        record(LabelEnd::Unfinished),
        "idle",
        false,
    ));
    assert_eq!(off["identity_label"], "요청 보기 만들기");
    assert!(off["request"].get("line").is_none(), "{off}");
    assert_ne!(off["request"]["verb"], "stopped");
    assert_eq!(off["request"]["request"]["text"], "요청 보기 만들어줘");
}

/// B15, D-29: opening a finished row reads the pane as a focus would, and
/// moves nothing else.
#[test]
fn opening_a_finished_row_reads_it_without_moving_the_focus() {
    let mut runtime = runtime_with_facts(operator_asked("끝내줘"), "done");
    assert_eq!(row(&runtime)["request"]["verb"], "result");
    let focused = runtime.snapshot().focused.pane_id.clone();
    let event = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "overview_open_result",
        "payload": {"pane_id": PANE},
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&event));
    assert_eq!(row(&runtime)["unread"], false);
    assert_eq!(row(&runtime)["request"]["verb"], "idle");
    assert_eq!(runtime.snapshot().focused.pane_id, focused);
    assert!(
        !runtime.dispatch_json(&event),
        "a second open changes nothing"
    );
}

/// B12, D-32: a pull request with running checks is read again once a
/// minute while a window shows the view, and never otherwise.
#[test]
fn running_checks_are_read_again_once_a_minute_only_while_the_view_is_shown() {
    let mut facts = operator_asked("PR 올려줘");
    facts.created_prs = vec![crate::labels::facts::CreatedPr {
        repository: "acme/app".to_owned(),
        number: 7,
        sighted_at_unix_ms: 5,
    }];
    let mut runtime = runtime_with_facts(facts, "idle");
    let mut pending = crate::model::PullRequestSnapshot {
        closing_issues: Vec::new(),
        title: "Request view".to_owned(),
        checks: crate::model::PullRequestChecks::Pending,
        number: 7,
        head_branch: "prd/request-view".to_owned(),
        base_branch: "main".to_owned(),
        url: "https://github.com/acme/app/pull/7".to_owned(),
        badge: crate::model::PullRequestBadge::Open,
        review: None,
        is_draft: false,
        merged_at_unix_ms: None,
        updated_at_unix_ms: Some(10),
        created_at_unix_ms: Some(5),
        closed_at_unix_ms: None,
        head_oid: None,
        cross_repository: false,
    };
    let github = |pull_request: &crate::model::PullRequestSnapshot| crate::model::GithubSnapshot {
        projects: vec![crate::model::GithubProjectSnapshot {
            root_path: "/work/app".to_owned(),
            pull_requests: vec![pull_request.clone()],
            pull_requests_read: true,
            ..Default::default()
        }],
    };
    runtime.ingest_github(github(&pending));
    assert_eq!(row(&runtime)["request"]["verb"], "waiting");
    assert_eq!(
        row(&runtime)["request"]["pull_requests"][0]["created"],
        true
    );

    let start = std::time::Instant::now();
    let generation = |runtime: &Runtime| runtime.github_generations.get("/work/app").copied();
    runtime.reread_pending_checks(start + std::time::Duration::from_secs(120));
    assert_eq!(generation(&runtime), None, "no window shows the view");

    runtime.observe_request_view(true, start);
    runtime.reread_pending_checks(start + std::time::Duration::from_secs(30));
    assert_eq!(generation(&runtime), None, "not a minute yet");
    runtime.reread_pending_checks(start + std::time::Duration::from_secs(61));
    assert_eq!(generation(&runtime), Some(1));

    pending.checks = crate::model::PullRequestChecks::Failed;
    runtime.ingest_github(github(&pending));
    assert_eq!(
        row(&runtime)["request"]["verb"],
        "fix",
        "the run ended with no new turn"
    );
    runtime.reread_pending_checks(start + std::time::Duration::from_secs(130));
    assert_eq!(generation(&runtime), Some(1), "no checks are running");

    runtime.observe_request_view(false, start);
    pending.checks = crate::model::PullRequestChecks::Pending;
    runtime.ingest_github(github(&pending));
    runtime.reread_pending_checks(start + std::time::Duration::from_secs(400));
    assert_eq!(generation(&runtime), Some(1), "the view was left");
}

/// A fresh projection with one Codex agent on `SESSION` at `status` and
/// Herdr state `seq`.
fn codex_projection(status: &str, seq: u64) -> SessionSnapshotPayload {
    let mut payload = projection(SESSION);
    for agent in &mut payload.agents {
        if agent.pane_id.as_deref() == Some(PANE) {
            agent.agent = Some("codex".to_owned());
            agent.agent_status = Some(status.to_owned());
            agent.state_change_seq = Some(seq);
        }
    }
    payload
}

/// The overlay a label worker publishes once it has read, under Herdr state
/// `seq`, a Codex session whose plan turn finished with a plan.
fn plan_waiting_overlay(seq: u64) -> LabelOverlay {
    use hide_session::turns::{TurnMark, TurnMode, TurnTracker};
    let mut turns = TurnTracker::default();
    let turn = Some("turn-1".to_owned());
    turns.fold(
        0,
        &TurnMark::Started {
            turn: turn.clone(),
            mode: TurnMode::Plan,
        },
    );
    turns.fold(10, &TurnMark::Plan { turn: turn.clone() });
    turns.fold(20, &TurnMark::Completed { turn });
    let record = PaneRecord {
        owner: hide_session::label_reference_token("codex", "id", SESSION),
        facts: operator_asked("계획을 세워줘"),
        turns: Some(turns),
        turns_seq: Some(seq),
        ..PaneRecord::default()
    };
    LabelOverlay::of_records([(&PANE.to_owned(), &record)], true, false)
}

/// B1: a Codex session file whose plan turn ended with a plan, read as the
/// label worker reads it and laid on the projection with agent summaries off,
/// puts the row in Needs You as an approval.
#[test]
fn a_plan_read_from_the_session_file_puts_the_row_in_needs_you() {
    use hide_session::label_transcript::{LabelTranscriptRequest, read};
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join(".codex/sessions/2026/10/07");
    std::fs::create_dir_all(&folder).unwrap();
    let event = |kind: &str, extra: serde_json::Value| {
        let mut payload = serde_json::json!({"type": kind, "turn_id": "turn-1"});
        payload
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::json!({"timestamp":"2026-10-07T01:00:02.000Z","type":"event_msg","payload":payload})
    };
    let records = [
        serde_json::json!({"timestamp":"2026-10-07T01:00:00.000Z","type":"session_meta",
            "payload":{"id":SESSION,"cwd":CHECKOUT,"cli_version":"0.160.1"}}),
        event(
            "task_started",
            serde_json::json!({"collaboration_mode_kind":"plan"}),
        ),
        serde_json::json!({"timestamp":"2026-10-07T01:00:01.000Z","type":"response_item",
            "payload":{"type":"message","role":"user",
                "content":[{"type":"input_text","text":"계획을 세워줘"}]}}),
        event(
            "item_completed",
            serde_json::json!({"item":{"type":"Plan","id":"i1","text":"1. 고친다"}}),
        ),
        event(
            "task_complete",
            serde_json::json!({"last_agent_message":null}),
        ),
    ];
    std::fs::write(
        folder.join(format!("rollout-2026-10-07T01-00-00-{SESSION}.jsonl")),
        records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let transcript = read(
        home.path(),
        &LabelTranscriptRequest {
            agent: hide_session::Agent::Codex,
            reference_kind: "id".to_owned(),
            reference_value: SESSION.to_owned(),
            cwd: None,
            checkpoint: None,
            subagents: Default::default(),
            turns: None,
        },
    )
    .unwrap();
    let record = PaneRecord {
        owner: hide_session::label_reference_token("codex", "id", SESSION),
        turns: transcript.turns,
        turns_seq: Some(4),
        ..Default::default()
    };
    let mut runtime = runtime();
    // Agent summaries off: the overlay carries no summary, only the facts.
    runtime.set_label_overlay(LabelOverlay::of_records(
        [(&PANE.to_owned(), &record)],
        true,
        false,
    ));
    runtime.ingest_session(Ok(codex_projection("done", 4)));
    let waiting = row(&runtime);
    assert_eq!(waiting["group"], "needs_you", "{waiting}");
    assert_eq!(waiting["demand"], "approval");
}

/// PRD codex-plan-approval-hold B1, B2, B5, D-05: a Codex plan waiting for
/// approval, read for the agent's current state, is an approval in Needs You
/// that stays there after the row is read; once Herdr's state moves past the
/// read, the row is what Herdr says again.
#[test]
fn a_codex_plan_waiting_for_approval_is_an_approval_that_stays_in_needs_you() {
    let mut runtime = runtime();
    runtime.set_label_overlay(plan_waiting_overlay(4));
    runtime.ingest_session(Ok(codex_projection("done", 4)));
    let waiting = row(&runtime);
    assert_eq!(waiting["demand"], "approval");
    assert_eq!(waiting["group"], "needs_you");
    assert_eq!(waiting["request"]["verb"], "answer");

    let open = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "overview_open_result",
        "payload": {"pane_id": PANE},
    }))
    .unwrap();
    runtime.dispatch_json(&open);
    let read = row(&runtime);
    assert_eq!(read["unread"], false);
    assert_eq!(read["group"], "needs_you", "{read}");

    // Herdr says the agent works before its new turn was written: the read
    // for that state still finds the plan, and the row does not wait.
    runtime.set_label_overlay(plan_waiting_overlay(5));
    runtime.ingest_session(Ok(codex_projection("working", 5)));
    assert_eq!(row(&runtime)["demand"], "none");
    assert_eq!(row(&runtime)["group"], "working");

    // The operator approved: Herdr's next state was not read as waiting.
    runtime.set_label_overlay(plan_waiting_overlay(4));
    runtime.ingest_session(Ok(codex_projection("working", 5)));
    assert_eq!(row(&runtime)["demand"], "none");
    assert_eq!(row(&runtime)["group"], "working");
    runtime.ingest_session(Ok(codex_projection("done", 6)));
    assert_eq!(row(&runtime)["demand"], "none");
    assert_eq!(row(&runtime)["group"], "done");
}

/// The Enter that approves a waiting plan is the operator's: Codex writes
/// the next turn's message ("Implement the plan.") from it, so it is kept
/// as a submit, unlike an Enter at a prompt Herdr reports.
#[test]
fn the_enter_that_approves_a_waiting_plan_is_the_operators_submit() {
    let mut runtime = runtime();
    let services = std::sync::Arc::new(
        crate::labels::LabelServices::start(
            None,
            None,
            std::sync::Weak::new(),
            &crate::node::test_node(),
            std::sync::Arc::new(hide_node::Local::of_process()),
        )
        .unwrap(),
    );
    runtime.install_label_services(std::sync::Arc::clone(&services));
    let enter = |runtime: &mut Runtime| {
        typed_into(runtime, PANE, true);
        services
            .input
            .submits(crate::labels::store::LOCAL_TARGET, PANE)
            .len()
    };
    // Herdr reads the menu as done under one Codex manifest and as unknown
    // under another; the coordinator observes each state for delivery.
    let mut seq = 4;
    for status in ["done", "unknown"] {
        let overlay = plan_waiting_overlay(seq);
        let projection = codex_projection(status, seq);
        runtime.observe_delivery(crate::node::TEST_NODE, &projection, None, Some(&overlay));
        runtime.set_label_overlay(overlay);
        runtime.ingest_session(Ok(projection));
        assert_eq!(row(&runtime)["blocked"], true, "{status}");
        let before = enter(&mut runtime);
        assert_eq!(enter(&mut runtime), before + 1, "{status}");
        seq += 1;
    }
    // A prompt Herdr reports is answered, not submitted to.
    let projection = codex_projection("blocked", seq);
    runtime.observe_delivery(crate::node::TEST_NODE, &projection, None, None);
    runtime.ingest_session(Ok(projection));
    let before = enter(&mut runtime);
    assert_eq!(enter(&mut runtime), before, "blocked");
}
