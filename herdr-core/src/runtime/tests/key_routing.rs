//! Where keys go right after the operator asked for a new tab or a split
//! (PRD instant-pane-topology D-11, B5, B9, B10).
//!
//! The shell sends those keys against the creation request, and they reach
//! the node beside the screen, never the core (PRD core-host-node-terminal
//! D-05): the node holds them until the core names the request's pane.
//! These tests assert what the core tells the node about each request and
//! what the snapshot says of it; the node's own tests
//! (`hide-node/src/terminal/{input,tests}.rs`) assert where the keys go.
//! The fake Herdr records what left over the socket and each test hands the
//! runtime Herdr's answer itself, so the order under test never depends on
//! the scheduler (docs/TESTING.md).

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

/// One tab with one attached pane, on a fake Herdr, with recorded terminal
/// routes. Workers cannot report back; each Herdr answer reaches the
/// runtime when the test hands it.
fn runtime_on(herdr: &FakeHerdr) -> (Runtime, Arc<RecordedTerminals>) {
    let (mut runtime, _) = tab_order_runtime(CHECKOUT);
    let terminals = record_terminals(&mut runtime);
    runtime.live = Some(live::LiveContext {
        socket_path: herdr.socket_path().to_path_buf(),
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
    runtime
        .recent_visible_tabs
        .insert(0, "w-order:t1".to_owned());
    report_terminal(&mut runtime, ORIGIN, terminal_state("controlling", 1));
    runtime.snapshot.terminal.pane_id = Some(ORIGIN.to_owned());
    terminals.take();
    (runtime, terminals)
}

/// What the core told the nodes about creation requests, in order.
fn request_controls(terminals: &RecordedTerminals) -> Vec<TerminalControl> {
    terminals
        .take()
        .into_iter()
        .filter(|control| {
            matches!(
                control,
                TerminalControl::RequestOpen { .. }
                    | TerminalControl::RequestResolve { .. }
                    | TerminalControl::RequestDiscard { .. }
            )
        })
        .collect()
}

fn open(request: &str) -> TerminalControl {
    TerminalControl::RequestOpen {
        request: request.to_owned(),
    }
}

fn resolve(request: &str, pane: &str) -> TerminalControl {
    TerminalControl::RequestResolve {
        request: request.to_owned(),
        pane: pane.to_owned(),
    }
}

fn is_discard(control: &TerminalControl, request: &str) -> bool {
    matches!(control, TerminalControl::RequestDiscard { request: r, .. } if r == request)
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

/// B9: a split opens its request at once, and Herdr's answer names the new
/// pane to the node holding the keys typed meanwhile; the request reads
/// ready.
#[test]
fn keys_typed_before_a_split_answers_reach_only_the_new_pane() {
    let herdr = fake_herdr("key-routing-split");
    let (mut runtime, terminals) = runtime_on(&herdr);
    runtime.dispatch_json(&split("r-split"));
    assert_eq!(
        request_state(&runtime, "r-split"),
        Some(crate::model::InputRequestState::Pending)
    );
    assert_eq!(request_controls(&terminals), [open("r-split")]);

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
    assert_eq!(
        request_controls(&terminals),
        [resolve("r-split", SPLIT_PANE)]
    );
}

/// B5: a refused split discards its request at the node, which drops what
/// was typed for it and anything typed for it afterwards.
#[test]
fn a_refused_split_drops_its_keys_everywhere() {
    let herdr = fake_herdr("key-routing-refused");
    let (mut runtime, terminals) = runtime_on(&herdr);
    runtime.dispatch_json(&split("r-refused"));
    split_answer(&mut runtime, Err("pane too small".to_owned()));
    assert_eq!(
        request_state(&runtime, "r-refused"),
        Some(crate::model::InputRequestState::Discarded)
    );
    let controls = request_controls(&terminals);
    assert_eq!(controls.len(), 2, "{controls:?}");
    assert_eq!(controls[0], open("r-refused"));
    assert!(is_discard(&controls[1], "r-refused"), "{controls:?}");
}

/// A split the core refuses before Herdr (no live connection) still answers
/// its request, so the shell stops sending keys against it and the node
/// drops any it holds.
#[test]
fn a_split_refused_before_herdr_answers_its_request_as_discarded() {
    let herdr = fake_herdr("key-routing-offline");
    let (mut runtime, terminals) = runtime_on(&herdr);
    runtime.live = None;
    runtime.dispatch_json(&split("r-offline"));
    assert_eq!(
        request_state(&runtime, "r-offline"),
        Some(crate::model::InputRequestState::Discarded)
    );
    assert!(
        request_controls(&terminals)
            .iter()
            .any(|control| is_discard(control, "r-offline"))
    );
}

/// B1, B9 for a new tab: the answer names the created tab's pane to the
/// node holding the keys typed for ⌘T.
#[test]
fn keys_typed_before_a_new_tab_answers_reach_its_pane() {
    let herdr = fake_herdr("key-routing-tab");
    let (mut runtime, terminals) = runtime_on(&herdr);
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
    assert_eq!(
        request_controls(&terminals),
        [open("r-tab"), resolve("r-tab", "w-order:t9:p")]
    );
}

/// B9: a new tab's request and its pane survive a session update that does
/// not carry the pane yet, as when Herdr's layout comes after Hide's
/// drawing of the tab has expired: the node keeps the keys for it.
#[test]
fn keys_for_a_new_tab_survive_an_update_before_herdr_lays_out_its_pane() {
    let herdr = fake_herdr("key-routing-tab-late");
    let (mut runtime, terminals) = runtime_on(&herdr);
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
    let controls = terminals.take();
    assert!(
        !controls.iter().any(|control| is_discard(control, "r-late")
            || matches!(control, TerminalControl::Forget { pane } if pane == "w-order:t9:p")),
        "the request or its pane was given up before Herdr laid it out: {controls:?}"
    );
    assert_eq!(
        request_state(&runtime, "r-late"),
        Some(crate::model::InputRequestState::Ready)
    );
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

/// D-11: once the split's pane has been laid out and then left, the request
/// is discarded, so keys sent against it reach no pane, even one Herdr later
/// gives that id.
#[test]
fn keys_for_a_split_whose_pane_left_reach_no_pane_that_reuses_its_id() {
    let herdr = fake_herdr("key-routing-pane-gone");
    let (mut runtime, terminals) = runtime_on(&herdr);
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
    assert!(
        !request_controls(&terminals)
            .iter()
            .any(|control| is_discard(control, "r-gone"))
    );

    runtime.ingest_session(Ok(session_with_split_pane(false)));
    assert_eq!(
        request_state(&runtime, "r-gone"),
        Some(crate::model::InputRequestState::Discarded)
    );
    assert!(
        request_controls(&terminals)
            .iter()
            .any(|control| is_discard(control, "r-gone"))
    );
}
