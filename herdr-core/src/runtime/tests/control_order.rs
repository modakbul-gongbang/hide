//! The order Hide's Herdr controls leave in, and what a late, replaced,
//! refused or lost answer to each one may change.
//!
//! The test decides the order (docs/TESTING.md): the fake Herdr records what
//! actually left over the socket, and each test hands the runtime the answers
//! itself, so the sequence under test never depends on the scheduler.

use super::*;

fn tab_info(tab_id: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "tab_info",
        "tab": {"tab_id": tab_id, "workspace_id": "w-order", "number": 1, "label": "1",
                "focused": true, "pane_count": 1, "agent_status": "idle"}
    })
}

fn tab_created(tab_id: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "tab_created",
        "tab": {"tab_id": tab_id, "workspace_id": "w-order", "number": 9, "label": "9",
                "focused": true, "pane_count": 1, "agent_status": "idle"},
        "root_pane": {"pane_id": format!("{tab_id}:p"), "terminal_id": "fixture-terminal",
                      "workspace_id": "w-order", "tab_id": tab_id, "focused": true,
                      "agent_status": "idle", "revision": 1}
    })
}

/// A Herdr that answers tab focus and tab creation, recording each request.
fn fake_herdr(name: &str) -> FakeHerdr {
    FakeHerdr::start(name, |method, params| match method {
        "tab.focus" => tab_info(params["tab_id"].as_str().expect("a tab id")),
        "tab.create" => tab_created("w-order:t9"),
        other => panic!("unexpected {other}"),
    })
}

/// A runtime on three tabs of one checkout whose Herdr is `herdr`. Its
/// workers cannot report back, so each answer reaches it only when the test
/// calls `complete_lane_tab`.
fn runtime_on(herdr: &FakeHerdr, checkout_path: &str) -> (Runtime, String) {
    let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
    runtime.live = Some(live::LiveContext {
        socket_path: herdr.socket_path().to_path_buf(),
        herdr_bin: None,
        runtime: std::sync::Weak::new(),
        notifier: crate::handle::ChangeNotifier::noop(),
        api_connector: Arc::new(herdr.connector()),
    });
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    (runtime, checkout_id)
}

fn focus_action(tab_id: &str) -> RemoteControlAction {
    RemoteControlAction::FocusTab {
        tab_id: tab_id.to_owned(),
    }
}

fn create_action() -> RemoteControlAction {
    RemoteControlAction::CreateTab {
        workspace_id: "w-order".to_owned(),
        cwd: "/private/tmp/hide-control-order-create".to_owned(),
        label: "new".to_owned(),
        area_id: None,
        admission_id: None,
    }
}

fn acknowledged() -> Result<RemoteControlOutcome, live::ControlFailure> {
    Ok(RemoteControlOutcome::Acknowledged {
        created_tab_id: None,
        created_pane_id: None,
    })
}

/// Herdr's answer to the control on the wire reaches the runtime. The lane's
/// worker would carry the next job on from here; the test does it, because
/// its own workers cannot report back.
fn answer(
    runtime: &mut Runtime,
    action: RemoteControlAction,
    result: Result<RemoteControlOutcome, live::ControlFailure>,
) -> (bool, bool) {
    let (changed, next) = runtime.complete_lane_tab(action, result, 4);
    let started = next.is_some();
    if let Some(next) = next {
        live::spawn_control_lane(next).expect("the next job starts");
    }
    (changed, started)
}

fn focused_tabs(herdr: &FakeHerdr) -> Vec<String> {
    herdr
        .calls()
        .into_iter()
        .filter(|(method, _)| method == "tab.focus")
        .map(|(_, params)| params["tab_id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// Three clicks in a row send Herdr the first tab and the last one, in that
/// order, and the last is not sent until the first has been answered: a focus
/// names where the operator is now, so the one in the middle is not worth a
/// round trip, and two focuses on the wire at once can land in either order.
#[test]
fn rapid_tab_focus_sends_the_first_and_the_latest_one_after_the_other() {
    let herdr = fake_herdr("order-rapid");
    let (mut runtime, checkout_id) = runtime_on(&herdr, "/private/tmp/hide-control-order-rapid");

    for tab in ["w-order:t2", "w-order:t3", "w-order:t1"] {
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, tab)));
    }
    assert!(runtime.control_lane.is_busy());
    assert_eq!(
        runtime.control_lane.queued_len(),
        1,
        "the middle click was replaced while it waited"
    );
    herdr.wait_for_requests(1, Duration::from_secs(5));
    assert_eq!(focused_tabs(&herdr), ["w-order:t2"]);

    // Only now does Herdr's answer to the first reach the runtime.
    answer(&mut runtime, focus_action("w-order:t2"), acknowledged());
    herdr.wait_for_requests(2, Duration::from_secs(5));
    assert_eq!(focused_tabs(&herdr), ["w-order:t2", "w-order:t1"]);

    answer(&mut runtime, focus_action("w-order:t1"), acknowledged());
    assert!(!runtime.control_lane.is_busy());
}

/// A focus replaced while it still waits never reached Herdr, so no answer to
/// it is on its way and none is remembered; one replaced after it was sent is.
#[test]
fn a_tab_focus_replaced_before_it_was_sent_leaves_no_late_answer_to_wait_for() {
    let herdr = fake_herdr("order-unsent");
    let (mut runtime, checkout_id) = runtime_on(&herdr, "/private/tmp/hide-control-order-unsent");

    for tab in ["w-order:t2", "w-order:t3", "w-order:t1"] {
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, tab)));
    }
    let superseded = runtime
        .superseded_tab_focus
        .iter()
        .map(|held| held.target_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        superseded,
        ["w-order:t2"],
        "t2 was sent and may be answered late; t3 never left"
    );
}

/// An answer that never comes may still have been applied. The focus waiting
/// behind it is not sent, because the lost one could land after it and put
/// Herdr back; Hide keeps the tab the operator chose and says so, and the
/// next click starts a new request.
#[test]
fn a_lost_tab_focus_answer_does_not_release_the_focus_waiting_behind_it() {
    let herdr = fake_herdr("order-lost");
    let (mut runtime, checkout_id) = runtime_on(&herdr, "/private/tmp/hide-control-order-lost");
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t1")));
    herdr.wait_for_requests(1, Duration::from_secs(5));

    let (changed, started) = answer(
        &mut runtime,
        focus_action("w-order:t2"),
        Err(live::ControlFailure::Ambiguous(
            "tab.focus result is unknown: response timed out".into(),
        )),
    );

    assert!(changed);
    assert!(!started, "nothing is sent behind an unknown effect");
    assert!(!runtime.control_lane.is_busy());
    assert_eq!(diagnostic_count(&runtime, "tab.focus.unknown"), 1);
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t1")
    );
    assert!(runtime.pending_tab_focus.is_none());

    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));
    herdr.wait_for_requests(2, Duration::from_secs(5));
    assert_eq!(focused_tabs(&herdr), ["w-order:t2", "w-order:t3"]);
}

/// Herdr refusing one focus neither blocks the lane nor takes the tab away
/// from the operator: the focus waiting behind it still goes out.
#[test]
fn a_refused_tab_focus_lets_the_next_control_through_and_keeps_the_tab() {
    let herdr = fake_herdr("order-refused");
    let (mut runtime, checkout_id) = runtime_on(&herdr, "/private/tmp/hide-control-order-refused");
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));

    answer(
        &mut runtime,
        focus_action("w-order:t2"),
        Err(live::ControlFailure::Definite(
            "tab.focus was refused: tab_not_found".into(),
        )),
    );

    herdr.wait_for_requests(2, Duration::from_secs(5));
    assert_eq!(focused_tabs(&herdr), ["w-order:t2", "w-order:t3"]);
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t3")
    );
    assert_eq!(
        runtime.snapshot().status.last_error.as_ref().unwrap().kind,
        "tab.control.failed"
    );
}

/// The operator presses New tab and, before Herdr answers, clicks another tab.
/// Creation is sent first and the click after it; the click is the newer
/// intent, so the created tab waits in the strip instead of taking the screen
/// back when its answer arrives.
#[test]
fn a_tab_created_before_a_click_does_not_take_the_screen_back_from_it() {
    let herdr = fake_herdr("order-create");
    let (mut runtime, checkout_id) = runtime_on(&herdr, "/private/tmp/hide-control-order-create");

    runtime
        .submit_local_control(create_action())
        .expect("the lane takes the creation");
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t2")
    );

    answer(
        &mut runtime,
        create_action(),
        Ok(RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("w-order:t9".to_owned()),
            created_pane_id: Some("w-order:t9:p".to_owned()),
        }),
    );

    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t2"),
        "the operator's later choice stands"
    );
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w-order:t2:p")
    );
    assert_eq!(diagnostic_count(&runtime, "tab.create.focus_superseded"), 1);
    herdr.wait_for_requests(2, Duration::from_secs(5));
    assert_eq!(
        herdr.methods(),
        ["tab.create", "tab.focus"],
        "creation left first, the click after its answer"
    );
}

/// With no later click, the created tab is the operator's choice and shows.
#[test]
fn a_tab_created_with_no_later_click_takes_the_screen() {
    let herdr = fake_herdr("order-create-alone");
    let (mut runtime, checkout_id) =
        runtime_on(&herdr, "/private/tmp/hide-control-order-create-alone");
    runtime
        .submit_local_control(create_action())
        .expect("the lane takes the creation");

    answer(
        &mut runtime,
        create_action(),
        Ok(RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("w-order:t9".to_owned()),
            created_pane_id: Some("w-order:t9:p".to_owned()),
        }),
    );

    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w-order:t9:p")
    );
    assert_eq!(diagnostic_count(&runtime, "tab.create.focus_superseded"), 0);
    let _ = checkout_id;
}

/// A refused creation says so, releases the lane, and leaves the screen alone.
#[test]
fn a_refused_tab_creation_releases_the_lane_for_the_focus_behind_it() {
    let herdr = fake_herdr("order-create-refused");
    let (mut runtime, checkout_id) =
        runtime_on(&herdr, "/private/tmp/hide-control-order-create-refused");
    runtime
        .submit_local_control(create_action())
        .expect("the lane takes the creation");
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));

    answer(
        &mut runtime,
        create_action(),
        Err(live::ControlFailure::Definite(
            "tab.create was refused: invalid_cwd".into(),
        )),
    );

    herdr.wait_for_requests(2, Duration::from_secs(5));
    assert_eq!(herdr.methods(), ["tab.create", "tab.focus"]);
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t3")
    );
}

/// The lane has a cap. A control past it is refused where it was asked, never
/// queued without bound and never dropped without a word.
#[test]
fn the_control_lane_refuses_what_passes_its_cap() {
    let herdr = fake_herdr("order-cap");
    let (mut runtime, _) = runtime_on(&herdr, "/private/tmp/hide-control-order-cap");
    runtime
        .submit_local_control(create_action())
        .expect("the first control starts");
    for _ in 0..control_lane::CONTROL_LANE_LIMIT {
        runtime
            .submit_local_control(create_action())
            .expect("a control within the cap waits");
    }

    let refused = runtime.submit_local_control(create_action());

    assert!(refused.is_err());
    assert_eq!(
        runtime.control_lane.queued_len(),
        control_lane::CONTROL_LANE_LIMIT
    );
}

/// A pane focus and a tab focus share the lane, so they leave in the order
/// they were asked for instead of racing each other to Herdr.
#[test]
fn a_tab_focus_waits_for_the_pane_focus_ahead_of_it() {
    let (mut runtime, checkout_id) =
        live_tab_order_runtime("/private/tmp/hide-control-order-pane-then-tab");
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        "/private/tmp/hide-control-order-pane-then-tab",
        &tabs,
        &tabs,
        "w-order:t1",
    )));

    assert!(runtime.dispatch_json(&correlated_pane_focus_event("w-order:t2:p", "pane-1")));
    let running = runtime
        .pane_focus_in_flight
        .clone()
        .expect("the pane focus is on the wire");
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));
    assert_eq!(runtime.control_lane.queued_len(), 1);
    assert!(
        runtime
            .pending_tab_focus
            .as_ref()
            .is_some_and(|pending| !pending.sent),
        "the tab focus has not left"
    );

    let layout = runtime
        .layout_holding_pane(&running.target_id)
        .expect("the focused pane's layout")
        .clone();
    runtime.complete_lane_pane_focus(running, Ok(layout), 5);

    assert!(
        runtime
            .pending_tab_focus
            .as_ref()
            .is_some_and(|pending| pending.sent),
        "the tab focus left once the pane focus was answered"
    );
    assert_eq!(runtime.control_lane.queued_len(), 0);
}

/// A newer focus replaces the waiting one but queues behind whatever was
/// accepted in between, so a tab created between two clicks still reaches
/// Herdr before the focus that came after it.
#[test]
fn a_replacing_focus_queues_behind_the_control_accepted_between_the_two() {
    let herdr = fake_herdr("order-replace-position");
    let (mut runtime, checkout_id) =
        runtime_on(&herdr, "/private/tmp/hide-control-order-replace-position");

    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));
    runtime
        .submit_local_control(create_action())
        .expect("the create is accepted");
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t1")));
    assert_eq!(runtime.control_lane.queued_len(), 2);

    herdr.wait_for_requests(1, Duration::from_secs(5));
    answer(&mut runtime, focus_action("w-order:t2"), acknowledged());
    herdr.wait_for_requests(2, Duration::from_secs(5));
    let methods = || {
        herdr
            .calls()
            .into_iter()
            .map(|(method, _)| method)
            .collect::<Vec<_>>()
    };
    assert_eq!(methods(), ["tab.focus", "tab.create"]);

    answer(&mut runtime, create_action(), acknowledged());
    herdr.wait_for_requests(3, Duration::from_secs(5));
    assert_eq!(focused_tabs(&herdr), ["w-order:t2", "w-order:t1"]);
}

/// The unknown-result rule is the same whichever control was lost: a pane
/// focus whose answer never came holds back the tab focus waiting behind it.
#[test]
fn a_lost_pane_focus_answer_does_not_release_the_tab_focus_behind_it() {
    let (mut runtime, checkout_id) =
        live_tab_order_runtime("/private/tmp/hide-control-order-pane-lost");
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        "/private/tmp/hide-control-order-pane-lost",
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert!(runtime.dispatch_json(&correlated_pane_focus_event("w-order:t2:p", "pane-1")));
    let running = runtime.pane_focus_in_flight.clone().expect("on the wire");
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));

    let (changed, next) = runtime.complete_lane_pane_focus(
        running,
        Err(live::ControlFailure::Ambiguous(
            "pane.focus result is unknown: response timed out".into(),
        )),
        5,
    );

    assert!(changed);
    assert!(next.is_none(), "nothing is sent behind an unknown effect");
    assert!(!runtime.control_lane.is_busy());
    assert_eq!(diagnostic_count(&runtime, "tab.focus.unknown"), 1);
    assert!(runtime.pending_tab_focus.is_none());
}

/// A move that waited behind other controls is stale when the operator has
/// replaced it, and is not sent; one that is current starts its five seconds
/// when it leaves, not when it was accepted.
#[test]
fn a_tab_move_replaced_while_it_waited_is_not_sent() {
    let herdr = fake_herdr("order-stale-move");
    let (mut runtime, checkout_id) =
        runtime_on(&herdr, "/private/tmp/hide-control-order-stale-move");
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    let stale = RemoteControlAction::MoveTab {
        checkout_id: checkout_id.clone(),
        tab_id: "w-order:t1".to_owned(),
        insert_index: 2,
        expected_order: vec![
            "w-order:t2".into(),
            "w-order:t3".into(),
            "w-order:t1".into(),
        ],
        generation: 999,
        connection_generation: runtime.live_generation,
    };
    runtime.submit_local_control(stale).expect("accepted");
    assert_eq!(runtime.control_lane.queued_len(), 1);

    herdr.wait_for_requests(1, Duration::from_secs(5));
    let (_, started) = answer(&mut runtime, focus_action("w-order:t2"), acknowledged());

    assert!(!started, "no pending move carries this generation");
    assert!(!runtime.control_lane.is_busy());
    assert_eq!(herdr.calls().len(), 1);
}
