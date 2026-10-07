//! Where keys go right after the operator asked for a new tab or a split
//! (PRD instant-pane-topology D-11, B5, B9, B10).
//!
//! The shell sends those keys against the creation request. The fake Herdr
//! records what left over the socket and each test hands the runtime Herdr's
//! answer itself, so the order under test never depends on the scheduler
//! (docs/TESTING.md). Every test asserts what reached a pane's control
//! session, the only place a key can do anything.

use super::*;

const CHECKOUT: &str = "/tmp/hide-key-routing";
/// The pane that held the keyboard when the creation was asked for.
const ORIGIN: &str = "w-order:t1:p";
const SPLIT_PANE: &str = "w-order:p9";

fn event(kind: &str, payload: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "payload": payload
    }))
    .unwrap()
}

fn fake_herdr(name: &str) -> FakeHerdr {
    FakeHerdr::start(name, |method, _| match method {
        "pane.split" => serde_json::json!({"type": "pane_info", "pane": {
            "pane_id": SPLIT_PANE, "terminal_id": "fixture-terminal", "workspace_id": "w-order",
            "tab_id": "w-order:t1", "focused": true, "agent_status": "idle", "revision": 1
        }}),
        "tab.create" => serde_json::json!({
            "type": "tab_created",
            "tab": {"tab_id": "w-order:t9", "workspace_id": "w-order", "number": 9, "label": "9",
                    "focused": true, "pane_count": 1, "agent_status": "idle"},
            "root_pane": {"pane_id": "w-order:t9:p", "terminal_id": "fixture-terminal",
                          "workspace_id": "w-order", "tab_id": "w-order:t9", "focused": true,
                          "agent_status": "idle", "revision": 1}
        }),
        other => panic!("unexpected {other}"),
    })
}

/// One tab with one pane whose control session is open. Workers cannot
/// report back; each Herdr answer reaches the runtime when the test hands it.
fn runtime_on(herdr: &FakeHerdr) -> Runtime {
    let (mut runtime, _) = tab_order_runtime(CHECKOUT);
    runtime.suppress_terminal_session_workers = true;
    runtime.live = Some(live::LiveContext {
        socket_path: herdr.socket_path().to_path_buf(),
        herdr_bin: None,
        runtime: std::sync::Weak::new(),
        notifier: crate::handle::ChangeNotifier::noop(),
        api_connector: Arc::new(herdr.connector()),
        node: Arc::new(hide_node::Local::of_process()),
    });
    runtime.ingest_session(Ok(tab_order_payload(
        CHECKOUT,
        &["w-order:t1"],
        &["w-order:t1"],
        "w-order:t1",
    )));
    // The tab is on screen, so its panes stay attached.
    runtime
        .recent_visible_tabs
        .insert(0, "w-order:t1".to_owned());
    runtime.terminal_sizes.insert(ORIGIN.into(), (24, 80));
    runtime.start_terminal_session(
        ORIGIN,
        TerminalSessionMode::Control,
        1,
        "automatic_initial",
        None,
    );
    runtime.snapshot.terminal.pane_id = Some(ORIGIN.to_owned());
    runtime
}

/// The bytes a pane's control session was asked to write, in order.
fn typed(runtime: &Runtime, pane_id: &str) -> Vec<u8> {
    runtime
        .terminal_sessions
        .get(pane_id)
        .map(|session| session.test_written_lines())
        .unwrap_or_default()
        .iter()
        .filter_map(|line| {
            let line: serde_json::Value = serde_json::from_str(line).unwrap();
            (line["type"] == "terminal.input")
                .then(|| live::decode_base64(line["bytes"].as_str().unwrap()).unwrap())
        })
        .flatten()
        .collect()
}

fn key_for_request(request_id: &str, bytes: &[u8]) -> Vec<u8> {
    event(
        "key",
        serde_json::json!({"pending_request": request_id, "bytes_base64": live::encode_base64(bytes)}),
    )
}

fn split(request_id: &str) -> Vec<u8> {
    event(
        "create_pane",
        serde_json::json!({"tab_id": "w-order:t1", "cwd": CHECKOUT, "command": null,
                           "direction": "right", "request_id": request_id}),
    )
}

fn request_state(runtime: &Runtime, request_id: &str) -> Option<crate::model::InputRequestState> {
    runtime
        .snapshot()
        .terminal
        .input_requests
        .iter()
        .find(|row| row.request_id == request_id)
        .map(|row| row.state)
}

fn split_answer(runtime: &mut Runtime, answer: Result<PaneControlOutcome, String>) {
    runtime.ingest_pane_control_result(
        PaneControlAction::Split {
            pane_id: ORIGIN.to_owned(),
            direction: PaneSplitDirection::Right,
            cwd: Some(CHECKOUT.to_owned()),
        },
        answer,
        5,
    );
}

/// The new pane's view reports its size and its control session opens.
fn open_session(runtime: &mut Runtime, pane_id: &str) {
    runtime.dispatch_json(&event(
        "terminal_resize",
        serde_json::json!({"pane_id": pane_id, "cols": 40, "rows": 24}),
    ));
    if !runtime.terminal_sessions.contains_key(pane_id) {
        runtime.start_terminal_session(
            pane_id,
            TerminalSessionMode::Control,
            1,
            "automatic_initial",
            None,
        );
    }
}

/// B9: keys typed between ⌘D and Herdr's answer reach the new pane, in
/// order, once its session opens; the pane that had the keyboard gets none.
#[test]
fn keys_typed_before_a_split_answers_reach_only_the_new_pane() {
    let herdr = fake_herdr("key-routing-split");
    let mut runtime = runtime_on(&herdr);
    runtime.dispatch_json(&split("r-split"));
    assert_eq!(
        request_state(&runtime, "r-split"),
        Some(crate::model::InputRequestState::Pending)
    );
    runtime.dispatch_json(&key_for_request("r-split", b"cla"));
    runtime.dispatch_json(&key_for_request("r-split", b"ude"));
    assert!(typed(&runtime, ORIGIN).is_empty());

    split_answer(
        &mut runtime,
        Ok(PaneControlOutcome::Acknowledged {
            created_pane_id: Some(SPLIT_PANE.to_owned()),
        }),
    );
    assert_eq!(
        request_state(&runtime, "r-split"),
        Some(crate::model::InputRequestState::Ready)
    );
    // Typed after the answer but before the new pane's session opens.
    runtime.dispatch_json(&key_for_request("r-split", b"\r"));
    open_session(&mut runtime, SPLIT_PANE);
    assert_eq!(typed(&runtime, SPLIT_PANE), b"claude\r");
    assert!(typed(&runtime, ORIGIN).is_empty());
}

/// B5: a refused split drops what was typed for it and anything typed for
/// it afterwards; no pane receives a byte.
#[test]
fn a_refused_split_drops_its_keys_everywhere() {
    let herdr = fake_herdr("key-routing-refused");
    let mut runtime = runtime_on(&herdr);
    runtime.dispatch_json(&split("r-refused"));
    runtime.dispatch_json(&key_for_request("r-refused", b"rm -rf build"));
    split_answer(&mut runtime, Err("pane too small".to_owned()));
    assert_eq!(
        request_state(&runtime, "r-refused"),
        Some(crate::model::InputRequestState::Discarded)
    );
    runtime.dispatch_json(&key_for_request("r-refused", b"\r"));
    assert!(typed(&runtime, ORIGIN).is_empty());
    assert!(!runtime.terminal_sessions.contains_key(SPLIT_PANE));
}

/// A split the core refuses before Herdr (no live connection) still answers
/// its request, so the shell stops sending keys against it.
#[test]
fn a_split_refused_before_herdr_answers_its_request_as_discarded() {
    let herdr = fake_herdr("key-routing-offline");
    let mut runtime = runtime_on(&herdr);
    runtime.live = None;
    runtime.dispatch_json(&split("r-offline"));
    assert_eq!(
        request_state(&runtime, "r-offline"),
        Some(crate::model::InputRequestState::Discarded)
    );
    runtime.dispatch_json(&key_for_request("r-offline", b"ls\r"));
    assert!(typed(&runtime, ORIGIN).is_empty());
}

/// A request the core never saw is never delivered anywhere.
#[test]
fn keys_for_an_unknown_request_reach_no_pane() {
    let herdr = fake_herdr("key-routing-unknown");
    let mut runtime = runtime_on(&herdr);
    runtime.dispatch_json(&key_for_request("never-sent", b"ls\r"));
    assert!(typed(&runtime, ORIGIN).is_empty());
}

/// B10: input for a pane whose session is still opening waits and is
/// written first, in order, when the session opens.
#[test]
fn input_to_an_attaching_pane_is_kept_and_written_in_order() {
    let herdr = fake_herdr("key-routing-attaching");
    let mut runtime = runtime_on(&herdr);
    runtime.ingest_session(Ok(tab_order_payload(
        CHECKOUT,
        &["w-order:t1", "w-order:t2"],
        &["w-order:t1", "w-order:t2"],
        "w-order:t1",
    )));
    let pane = "w-order:t2:p";
    assert!(!runtime.terminal_sessions.contains_key(pane));
    for chunk in [b"echo ".as_slice(), b"one\r"] {
        runtime.dispatch_json(&event(
            "key",
            serde_json::json!({"pane_id": pane, "bytes_base64": live::encode_base64(chunk)}),
        ));
    }
    assert!(runtime.snapshot().status.last_error.is_none());
    open_session(&mut runtime, pane);
    assert_eq!(typed(&runtime, pane), b"echo one\r");
}

/// B1, B9 for a new tab: keys typed for ⌘T reach the created tab's pane.
#[test]
fn keys_typed_before_a_new_tab_answers_reach_its_pane() {
    let herdr = fake_herdr("key-routing-tab");
    let mut runtime = runtime_on(&herdr);
    let workspace_id = runtime.snapshot().navigator.workspaces[0].id.clone();
    let checkout_id = runtime.snapshot().navigator.focused_checkout_id.clone();
    // The checkout's tabs live in Herdr workspace w-order, so the tab is made
    // there rather than through opening an owner.
    for checkout in runtime
        .snapshot
        .navigator
        .workspaces
        .iter_mut()
        .flat_map(|workspace| workspace.checkouts.iter_mut())
    {
        checkout.owner_workspace_id = Some("w-order".to_owned());
    }
    runtime.dispatch_json(&event(
        "create_tab",
        serde_json::json!({"workspace_id": workspace_id, "checkout_id": checkout_id,
                           "label": "2", "request_id": "r-tab"}),
    ));
    runtime.dispatch_json(&key_for_request("r-tab", b"git status"));
    let Some(super::terminal_input::InputOrigin::TabCreate(admission_id)) =
        runtime.input_requests.origin_of("r-tab")
    else {
        panic!("the creation opened its key request");
    };
    herdr.wait_for_requests(1, Duration::from_secs(5));
    runtime.complete_lane_tab(
        RemoteControlAction::CreateTab {
            workspace_id: "w-order".to_owned(),
            cwd: CHECKOUT.to_owned(),
            label: "2".to_owned(),
            area_id: None,
            admission_id: Some(admission_id),
        },
        Ok(live::RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("w-order:t9".to_owned()),
            created_pane_id: Some("w-order:t9:p".to_owned()),
        }),
        7,
    );
    runtime.ingest_session(Ok(tab_order_payload(
        CHECKOUT,
        &["w-order:t1", "w-order:t9"],
        &["w-order:t1", "w-order:t9"],
        "w-order:t9",
    )));
    open_session(&mut runtime, "w-order:t9:p");
    assert_eq!(typed(&runtime, "w-order:t9:p"), b"git status");
    assert!(typed(&runtime, ORIGIN).is_empty());
}

/// B9: keys held for a new tab's pane survive a session update that does
/// not carry the pane yet, as when Herdr's layout comes after Hide's
/// drawing of the tab has expired, and reach it once it is laid out.
#[test]
fn keys_for_a_new_tab_survive_an_update_before_herdr_lays_out_its_pane() {
    let herdr = fake_herdr("key-routing-tab-late");
    let mut runtime = runtime_on(&herdr);
    let workspace_id = runtime.snapshot().navigator.workspaces[0].id.clone();
    let checkout_id = runtime.snapshot().navigator.focused_checkout_id.clone();
    for checkout in runtime
        .snapshot
        .navigator
        .workspaces
        .iter_mut()
        .flat_map(|workspace| workspace.checkouts.iter_mut())
    {
        checkout.owner_workspace_id = Some("w-order".to_owned());
    }
    runtime.dispatch_json(&event(
        "create_tab",
        serde_json::json!({"workspace_id": workspace_id, "checkout_id": checkout_id,
                           "label": "2", "request_id": "r-late"}),
    ));
    runtime.dispatch_json(&key_for_request("r-late", b"git "));
    let Some(super::terminal_input::InputOrigin::TabCreate(admission_id)) =
        runtime.input_requests.origin_of("r-late")
    else {
        panic!("the creation opened its key request");
    };
    herdr.wait_for_requests(1, Duration::from_secs(5));
    runtime.complete_lane_tab(
        RemoteControlAction::CreateTab {
            workspace_id: "w-order".to_owned(),
            cwd: CHECKOUT.to_owned(),
            label: "2".to_owned(),
            area_id: None,
            admission_id: Some(admission_id),
        },
        Ok(live::RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("w-order:t9".to_owned()),
            created_pane_id: Some("w-order:t9:p".to_owned()),
        }),
        7,
    );
    runtime.dispatch_json(&key_for_request("r-late", b"log"));
    // The drawing has expired and Herdr's next update does not list the tab.
    runtime.provisional_tabs.clear();
    runtime.ingest_session(Ok(tab_order_payload(
        CHECKOUT,
        &["w-order:t1"],
        &["w-order:t1"],
        "w-order:t1",
    )));
    runtime.ingest_session(Ok(tab_order_payload(
        CHECKOUT,
        &["w-order:t1", "w-order:t9"],
        &["w-order:t1", "w-order:t9"],
        "w-order:t9",
    )));
    open_session(&mut runtime, "w-order:t9:p");
    assert_eq!(typed(&runtime, "w-order:t9:p"), b"git log");
    assert!(typed(&runtime, ORIGIN).is_empty());
}

/// The session with the split's new pane laid out beside the origin pane,
/// or without it once it is gone.
fn session_with_split_pane(laid_out: bool) -> SessionSnapshotPayload {
    if !laid_out {
        return tab_order_payload(CHECKOUT, &["w-order:t1"], &["w-order:t1"], "w-order:t1");
    }
    crate::sidebar::owned_label_fixture(serde_json::json!({
        "agents": [],
        "focused_workspace_id": "w-order",
        "focused_pane_id": ORIGIN,
        "workspaces": [{"workspace_id": "w-order", "label": "order", "active_tab_id": "w-order:t1"}],
        "tabs": [{"workspace_id": "w-order", "tab_id": "w-order:t1", "label": "", "number": 1}],
        "panes": [
            {"pane_id": ORIGIN, "cwd": CHECKOUT},
            {"pane_id": SPLIT_PANE, "cwd": CHECKOUT}
        ],
        "layouts": [{
            "workspace_id": "w-order", "tab_id": "w-order:t1", "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": ORIGIN,
            "panes": [
                {"pane_id": ORIGIN, "rect": {"x": 0, "y": 0, "width": 40, "height": 24}},
                {"pane_id": SPLIT_PANE, "rect": {"x": 40, "y": 0, "width": 40, "height": 24}}
            ],
            "splits": [{"direction": "right", "ratio": 0.5,
                        "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}]
        }]
    }))
    .expect("session with the split pane")
}

/// D-11: once the split's pane has been laid out and then left, keys sent
/// against its request reach no pane, even one Herdr later gives that id.
#[test]
fn keys_for_a_split_whose_pane_left_reach_no_pane_that_reuses_its_id() {
    let herdr = fake_herdr("key-routing-pane-gone");
    let mut runtime = runtime_on(&herdr);
    runtime.dispatch_json(&split("r-gone"));
    split_answer(
        &mut runtime,
        Ok(PaneControlOutcome::Acknowledged {
            created_pane_id: Some(SPLIT_PANE.to_owned()),
        }),
    );
    runtime.ingest_session(Ok(session_with_split_pane(true)));
    assert_eq!(
        request_state(&runtime, "r-gone"),
        Some(crate::model::InputRequestState::Ready)
    );

    runtime.ingest_session(Ok(session_with_split_pane(false)));
    assert_eq!(
        request_state(&runtime, "r-gone"),
        Some(crate::model::InputRequestState::Discarded)
    );

    // Herdr reuses the id for another pane, whose session is open.
    runtime.ingest_session(Ok(session_with_split_pane(true)));
    open_session(&mut runtime, SPLIT_PANE);
    runtime.dispatch_json(&key_for_request("r-gone", b"rm -rf build\r"));
    assert!(typed(&runtime, SPLIT_PANE).is_empty());
    assert!(typed(&runtime, ORIGIN).is_empty());
}

/// B10: keys held for a pane whose retries run out are dropped, and keys
/// typed after that are not kept for a later Reconnect.
#[test]
fn input_held_for_a_pane_whose_retries_ran_out_is_not_written_on_reconnect() {
    let herdr = fake_herdr("key-routing-retries-out");
    let mut runtime = runtime_on(&herdr);
    // A second pane on the shown tab, with no session yet.
    runtime.ingest_session(Ok(session_with_split_pane(true)));
    let pane = SPLIT_PANE;
    runtime.dispatch_json(&event(
        "key",
        serde_json::json!({"pane_id": pane, "bytes_base64": live::encode_base64(b"make deploy")}),
    ));

    // The tab is the one on screen, so its panes are kept connected.
    let checkout_id = runtime
        .snapshot
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .find(|checkout| {
            checkout
                .tabs
                .iter()
                .any(|tab| tab.id.as_deref() == Some("w-order:t1"))
        })
        .map(|checkout| checkout.id.clone())
        .expect("the tab's checkout");
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    runtime
        .visible_tab_ids
        .insert(checkout_id, "w-order:t1".to_owned());

    // Its starts failed and the last retry is due with none left.
    let now = Instant::now();
    let lifecycle = TerminalSessionLifecycle {
        state: "unavailable",
        ..TerminalSessionLifecycle::default()
    };
    runtime
        .terminal_session_lifecycles
        .insert(pane.into(), lifecycle);
    let mut recovery = crate::terminal_recovery::Recovery::new(now, "failed".into());
    recovery.retries = 4;
    recovery.due = Some(now);
    runtime.terminal_recovery.insert(pane.into(), recovery);
    runtime.maintain_terminals(now);
    assert_eq!(
        runtime.terminal_session_lifecycles[pane].retry_decision,
        "manual"
    );

    runtime.dispatch_json(&event(
        "key",
        serde_json::json!({"pane_id": pane, "bytes_base64": live::encode_base64(b"\r")}),
    ));
    // The operator's Reconnect opens a session.
    runtime.terminal_session_lifecycles.remove(pane);
    runtime.terminal_recovery.remove(pane);
    open_session(&mut runtime, pane);
    assert!(typed(&runtime, pane).is_empty());
    assert!(typed(&runtime, ORIGIN).is_empty());
}
