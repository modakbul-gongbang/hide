//! PRD agent-sleep through the runtime's own events and session updates.
//! The Herdr workers are not run here (their boundary has its own tests in
//! `agent_sleep_herdr.rs`); what they hand back is fed in through the same
//! `ingest_agent_sleep_*` calls a worker makes.

use super::*;

const CHECKOUT: &str = "/private/tmp/hide-agent-sleep";
const TABS: [&str; 2] = ["w-order:t1", "w-order:t2"];
const SLEEPER: &str = "w-order:t2:p";

fn event(kind: &str, payload: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "payload": payload
    }))
    .unwrap()
}

/// The two-tab session with t1 in front, and, when `seq` is given, an
/// idle Claude agent in t2 that Herdr lists with that state sequence.
fn session(seq: Option<u64>) -> SessionSnapshotPayload {
    let mut payload = tab_order_payload(CHECKOUT, &TABS, &TABS, "w-order:t1");
    if let Some(seq) = seq {
        let owned: SessionSnapshotPayload =
            crate::sidebar::owned_label_fixture(serde_json::json!({"agents": [{
                "id": "reviewer", "pane_id": SLEEPER, "agent": "claude",
                "agent_status": "idle", "state_change_seq": seq, "cwd": CHECKOUT,
                "agent_session": {"kind": "id", "value": "11111111-2222-3333-4444-555555555555"},
                "tokens": {"task": "Review the parser"}
            }]}))
            .unwrap();
        payload.agents.extend(owned.agents);
    }
    payload
}

fn row(runtime: &Runtime) -> serde_json::Value {
    serde_json::to_value(&runtime.snapshot().navigator.agents)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["pane_id"] == SLEEPER)
        .cloned()
        .expect("the agent row")
}

fn pane(runtime: &Runtime) -> serde_json::Value {
    let pane = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .flat_map(|checkout| checkout.tabs.iter())
        .flat_map(|tab| tab.panes.iter())
        .find(|pane| pane.id == SLEEPER)
        .expect("the pane");
    serde_json::to_value(pane).unwrap()
}

/// A live runtime whose agent in t2 was put to sleep from the pane menu
/// and whose end worker reported success.
fn asleep() -> (Runtime, String) {
    let (mut runtime, checkout_id) = live_tab_order_runtime(CHECKOUT);
    runtime.ingest_session(Ok(session(Some(4))));
    assert_eq!(row(&runtime)["group"], "seen");
    assert!(pane(&runtime)["sleep_action"]["available"] == true);
    runtime.dispatch_json(&event(
        "agent_sleep",
        serde_json::json!({"pane_id": SLEEPER}),
    ));
    assert!(runtime.snapshot().status.last_error.is_none());
    assert!(
        row(&runtime).get("sleep").is_none(),
        "the agent is awake until its end lands (B8)"
    );
    assert!(runtime.ingest_agent_sleep_end(SLEEPER, Ok(4)));
    (runtime, checkout_id)
}

/// The generic dormant owner is exercised with a proven fixture session;
/// production Claude still reaches only the unchanged keep-pane branch.
fn dormant_intent(runtime: &mut Runtime) -> crate::agent_sleep::SleepId {
    use crate::agent_sleep::{DormantPhase, DormantRecord};
    runtime
        .snapshot
        .navigator
        .agents
        .iter_mut()
        .find(|agent| agent.pane_id == SLEEPER)
        .unwrap()
        .row_facts
        .get_or_insert_with(Default::default);
    let agent = runtime
        .snapshot
        .navigator
        .agents
        .iter()
        .find(|agent| agent.pane_id == SLEEPER)
        .unwrap()
        .clone();
    let tab = runtime
        .snapshot
        .navigator
        .workspaces
        .iter()
        .flat_map(|project| &project.checkouts)
        .flat_map(|checkout| &checkout.tabs)
        .find(|tab| tab.panes.iter().any(|pane| pane.id == SLEEPER))
        .unwrap();
    let context = runtime.close_context(tab).unwrap();
    let native_session_id = agent.session_id.unwrap();
    runtime
        .snapshot
        .ui_state
        .agent_sleep
        .admit_dormant(DormantRecord {
            source_reference: agent
                .row_facts
                .as_ref()
                .and_then(|facts| facts.native_reference.clone()),
            phase: DormantPhase::SavingClose,
            revision: 1,
            node_id: runtime.node.as_str().into(),
            connection_generation: runtime.live_generation,
            old_pane_id: SLEEPER.into(),
            old_state_change_seq: agent.state_change_seq,
            label_owner: hide_session::label_reference_token("claude", "id", &native_session_id)
                .unwrap(),
            kind: "claude".into(),
            native_session_id,
            identity_label: agent.identity_label,
            cwd: context.checkout_path.clone(),
            context,
            close_key: None,
            closed: false,
            wake_pane_id: None,
            wake_tab_id: None,
            since_unix_ms: 1,
            transition_started_unix_ms: 1,
            reason: None,
        })
        .unwrap()
}

/// External Herdr replies use its pinned wire contract, not an owned worker mock.
#[cfg(unix)]
fn dormant_wire_session(cwd: &str, tabs: &[&str]) -> serde_json::Value {
    serde_json::json!({"version":"fixture", "protocol":hide_herdr_client::HERDR_PROTOCOL_REVISION,
        "workspaces":[{"workspace_id":"w-order","number":1,"label":"order","active_tab_id":"w-order:t1","focused":true,"agent_status":"idle","pane_count":tabs.len(),"tab_count":tabs.len(),"tokens":{"hide_owner":crate::checkout_owner::owner_mark(crate::node::TEST_NODE, cwd)}}],
        "tabs":tabs.iter().enumerate().map(|(index, tab)| serde_json::json!({
            "tab_id":tab,"workspace_id":"w-order","number":index+1,"label":"fixture",
            "focused":index==0,"pane_count":1,"agent_status":"idle"
        })).collect::<Vec<_>>(),
        "panes":tabs.iter().map(|tab| serde_json::json!({
            "pane_id":format!("{tab}:p"),"terminal_id":format!("term-{tab}"),"workspace_id":"w-order",
            "tab_id":tab,"cwd":cwd,"focused":false,"agent_status":"idle","revision":0
        })).collect::<Vec<_>>(),
        "layouts":tabs.iter().map(|tab| serde_json::json!({
            "workspace_id":"w-order","tab_id":tab,"zoomed":false,
            "area":{"x":0,"y":0,"width":80,"height":24},"focused_pane_id":format!("{tab}:p"),
            "panes":[{"pane_id":format!("{tab}:p"),"focused":true,"rect":{"x":0,"y":0,"width":80,"height":24}}],"splits":[]
        })).collect::<Vec<_>>(),"agents":[]})
}

#[cfg(unix)]
fn persisted_dormant(path: &Path, id: &crate::agent_sleep::SleepId) -> serde_json::Value {
    let state: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    state["agent_sleep"]["dormant"][id.as_str()].clone()
}

/// B5: real state writes authorize the ordinary close and wake workers.
/// The external server observes each saved intent before accepting its effect.
#[test]
#[cfg(unix)]
fn a_durable_dormant_journey_has_one_resume_authority_and_exact_native_confirmation() {
    durable_dormant_journey("claude", "none");
}

#[test]
#[cfg(unix)]
fn a_pi_dormant_journey_refuses_a_changed_route_before_close_wake_or_start() {
    for phase in [
        "none",
        "before-close",
        "before-wake",
        "before-start",
        "missing-source-before-close",
        "missing-source-before-wake",
        "missing-source-before-start",
    ] {
        durable_dormant_journey("pi", phase);
    }
}

#[cfg(unix)]
fn durable_dormant_journey(kind: &'static str, interference: &'static str) {
    use crate::agent_sleep::DormantPhase;
    let folder = tempfile::tempdir().unwrap();
    let cwd = folder
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let (mut runtime, _) = live_tab_order_runtime(&cwd);
    let home = tempfile::tempdir().unwrap();
    let native_folder = home.path().join(".pi/agent/sessions").join(format!(
        "--{}--",
        cwd.trim_start_matches(['/', '\\'])
            .replace(['/', '\\', ':'], "-")
    ));
    std::fs::create_dir_all(&native_folder).unwrap();
    let native_path = native_folder.join("native.jsonl");
    let native_id = "11111111-2222-3333-4444-555555555555";
    std::fs::write(
        &native_path,
        format!(
            "{}\n",
            serde_json::json!({"type":"session", "version":3, "id":native_id, "cwd":cwd})
        ),
    )
    .unwrap();
    let native_before = std::fs::read(&native_path).unwrap();
    let other_before = [native_before.as_slice(), b"{\"type\":\"message\",\"id\":\"other-message\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"different history\"}]}}\n"].concat();
    if kind == "pi" {
        runtime.own_node = Arc::new(hide_node::Local::new(Some(home.path().to_path_buf())));
        runtime.live.as_mut().unwrap().node = runtime.own_node();
    }
    let mut initial = session(Some(4));
    for pane in &mut initial.panes {
        pane.cwd = Some(cwd.clone());
    }
    initial.agents[0].cwd = Some(cwd.clone());
    initial.agents[0].agent = Some(kind.into());
    initial.agents[0].facts = Some(crate::request_view::RowFacts {
        native_session_id: Some(native_id.into()),
        native_reference: Some(crate::sidebar::SessionAgentSessionPayload {
            kind: "path".into(),
            value: native_path.display().to_string(),
        }),
        ..Default::default()
    });
    runtime.ingest_session(Ok(initial));
    let id = dormant_intent(&mut runtime);
    {
        let record = runtime
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .get_mut(&id)
            .unwrap();
        record.kind = kind.into();
        record.label_owner = hide_session::label_reference_token(kind, "id", native_id).unwrap();
    }
    if interference == "before-close" {
        std::fs::copy(&native_path, native_folder.join("duplicate.jsonl")).unwrap();
    }
    if interference == "missing-source-before-close" {
        std::fs::write(native_folder.join("other.jsonl"), &other_before).unwrap();
        std::fs::remove_file(&native_path).unwrap();
    }
    assert_eq!(runtime.snapshot.ui_state.agent_sleep.dormant[&id].cwd, cwd);
    let path = runtime.state_path.clone();
    let saved_path = path.clone();
    let saved_id = id.clone();
    let saved_cwd = cwd.clone();
    let create_source = native_path.clone();
    let create_other = other_before.clone();
    let (effects, observed) = std::sync::mpsc::channel();
    let mut created = false;
    let herdr = FakeHerdr::start("dormant-success", move |method, params| match method {
        "layout.export" => {
            assert_eq!(
                persisted_dormant(&saved_path, &saved_id)["phase"],
                "saving_close"
            );
            serde_json::json!({"type":"layout_export","layout":{
                "workspace_id":"w-order","tab_id":"w-order:t2","zoomed":false,
                "focused_pane_id":SLEEPER,"root":{"type":"pane","pane_id":SLEEPER,"cwd":saved_cwd,"env":{}}
            }})
        }
        "pane.close" => {
            let saved = persisted_dormant(&saved_path, &saved_id);
            assert_eq!(saved["phase"], "saving_close_ready");
            assert!(saved["close_key"].is_string());
            assert_eq!(params["pane_id"], SLEEPER);
            effects.send("close").unwrap();
            serde_json::json!({"type":"ok"})
        }
        "session.snapshot" => {
            // The layout worker reads once before and once after creating its own tab.
            let tabs = if created {
                vec!["w-order:t1", "w-order:t3"]
            } else {
                vec!["w-order:t1"]
            };
            serde_json::json!({"type":"session_snapshot","snapshot":dormant_wire_session(&saved_cwd, &tabs)})
        }
        "layout.apply" => {
            if interference == "missing-source-before-start" {
                std::fs::write(create_source.with_file_name("other.jsonl"), &create_other).unwrap();
                std::fs::remove_file(&create_source).unwrap();
            }
            if interference == "before-start" {
                std::fs::copy(
                    &create_source,
                    create_source.with_file_name("duplicate.jsonl"),
                )
                .unwrap();
            }
            assert_eq!(
                persisted_dormant(&saved_path, &saved_id)["phase"],
                "saving_wake"
            );
            assert_eq!(params["root"]["cwd"], saved_cwd);
            assert!(
                params["root"]["env"]["HIDE_REOPEN_INTENT"]
                    .as_str()
                    .unwrap()
                    .contains(saved_id.as_str())
            );
            created = true;
            effects.send("create").unwrap();
            serde_json::json!({"type":"layout_apply","layout":{
                "workspace_id":"w-order","tab_id":"w-order:t3","zoomed":false,
                "focused_pane_id":"w-order:t3:p","root":{"type":"pane","pane_id":"w-order:t3:p","cwd":saved_cwd,"env":params["root"]["env"]}
            }})
        }
        "tab.move" => tab_list(&["w-order:t1", "w-order:t3"]),
        "pane.process_info" => serde_json::json!({"type":"pane_process_info","process_info":{
            "pane_id":"w-order:t3:p","shell_pid":42,"foreground_process_group_id":42,
            "foreground_processes":[{"pid":42,"name":"zsh"}]
        }}),
        "agent.start" => {
            let saved = persisted_dormant(&saved_path, &saved_id);
            assert_eq!(saved["phase"], "saving_start");
            assert_eq!(saved["wake_pane_id"], "w-order:t3:p");
            assert_eq!(params["pane_id"], "w-order:t3:p");
            assert_eq!(params["kind"], kind);
            assert_eq!(
                params["args"],
                serde_json::json!([
                    if kind == "pi" {
                        "--session"
                    } else {
                        "--resume"
                    },
                    "11111111-2222-3333-4444-555555555555"
                ])
            );
            effects.send("start").unwrap();
            serde_json::json!({"type":"agent_started","argv":[],"agent":{
                "pane_id":"w-order:t3:p","terminal_id":"term-wake","workspace_id":"w-order",
                "tab_id":"w-order:t3","focused":false,"agent_status":"idle","revision":1
            }})
        }
        other => panic!("unexpected dormant effect: {other}"),
    });
    runtime.live.as_mut().unwrap().api_connector = Arc::new(herdr.connector());
    let shared = Arc::new(std::sync::Mutex::new(runtime));
    shared.lock().unwrap().live.as_mut().unwrap().runtime = Arc::downgrade(&shared);
    shared.lock().unwrap().write_ui_state().unwrap();
    if matches!(interference, "before-close" | "missing-source-before-close") {
        wait_for("refused Pi close", || {
            !shared
                .lock()
                .unwrap()
                .snapshot
                .ui_state
                .agent_sleep
                .dormant
                .contains_key(&id)
        });
        // Capturing the old layout is read-only. B2/B8 forbid close or launch
        // effects after the exact native route becomes unconfirmed.
        assert!(observed.try_recv().is_err());
        assert!(
            !herdr.methods().iter().any(|method| matches!(
                method.as_str(),
                "pane.close" | "tab.close" | "layout.apply" | "agent.start"
            )),
            "unexpected effect: {:?}",
            herdr.methods()
        );
        assert_eq!(row(&shared.lock().unwrap())["pane_id"], SLEEPER);
        assert_eq!(
            std::fs::read(if native_path.exists() {
                native_path.clone()
            } else {
                native_folder.join("other.jsonl")
            })
            .unwrap(),
            if interference.starts_with("missing-source") {
                other_before
            } else {
                native_before
            }
        );
        return;
    }
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(5)).unwrap(),
        "close"
    );
    let retired_capture;
    {
        let mut runtime = shared.lock().unwrap();
        retired_capture = runtime
            .close_operations
            .values()
            .next()
            .unwrap()
            .request
            .clone();
        assert!(!runtime.snapshot.recent_closed.can_reopen);
        assert!(runtime.snapshot.recent_closed.pending.is_empty());
        runtime.dispatch_json(&event("reopen_closed", serde_json::json!({})));
        assert!(runtime.reopen_after_close.is_none());
        runtime.ingest_session(Ok(tab_order_payload(
            &cwd,
            &["w-order:t1"],
            &["w-order:t1"],
            "w-order:t1",
        )));
        assert_eq!(runtime.snapshot.navigator.sleeping_sessions.len(), 1);
        assert_eq!(
            runtime.snapshot.navigator.sleeping_sessions[0].phase,
            DormantPhase::Sleeping
        );
        assert_eq!(runtime.snapshot.recent_closed.count, 0);
        if interference == "before-wake" {
            std::fs::copy(&native_path, native_folder.join("duplicate.jsonl")).unwrap();
        }
        if interference == "missing-source-before-wake" {
            std::fs::write(native_folder.join("other.jsonl"), &other_before).unwrap();
            std::fs::remove_file(&native_path).unwrap();
        }
        assert!(runtime.request_dormant_wake(&id));
        assert!(!runtime.request_dormant_wake(&id));
    }
    if matches!(interference, "before-wake" | "missing-source-before-wake") {
        wait_for("refused Pi wake tab", || {
            shared.lock().unwrap().snapshot.ui_state.agent_sleep.dormant[&id].phase
                == DormantPhase::Failed
        });
        assert!(
            !herdr
                .methods()
                .iter()
                .any(|method| matches!(method.as_str(), "layout.apply" | "agent.start"))
        );
        assert!(
            shared.lock().unwrap().snapshot.ui_state.agent_sleep.dormant[&id]
                .wake_pane_id
                .is_none()
        );
        assert_eq!(
            std::fs::read(if native_path.exists() {
                native_path.clone()
            } else {
                native_folder.join("other.jsonl")
            })
            .unwrap(),
            if interference.starts_with("missing-source") {
                other_before
            } else {
                native_before
            }
        );
        return;
    }
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(5)).unwrap(),
        "create"
    );
    if matches!(interference, "before-start" | "missing-source-before-start") {
        wait_for("refused Pi wake start", || {
            shared.lock().unwrap().snapshot.ui_state.agent_sleep.dormant[&id].phase
                == DormantPhase::Failed
        });
        assert!(!herdr.methods().iter().any(|method| method == "agent.start"));
        assert_eq!(
            shared.lock().unwrap().snapshot.ui_state.agent_sleep.dormant[&id]
                .wake_pane_id
                .as_deref(),
            Some("w-order:t3:p")
        );
        assert_eq!(
            std::fs::read(if native_path.exists() {
                native_path.clone()
            } else {
                native_folder.join("other.jsonl")
            })
            .unwrap(),
            if interference.starts_with("missing-source") {
                other_before
            } else {
                native_before
            }
        );
        return;
    }
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(5)).unwrap(),
        "start"
    );
    {
        let mut runtime = shared.lock().unwrap();
        let work = crate::agent_sleep_herdr::DormantWork {
            id: id.clone(),
            record: runtime.snapshot.ui_state.agent_sleep.dormant[&id].clone(),
        };
        let mut stale = work.clone();
        stale.record.connection_generation += 1;
        assert!(
            !runtime
                .ingest_dormant_tab(&stale, crate::agent_sleep_herdr::DormantTabOutcome::Unknown)
        );
        assert_eq!(
            runtime.snapshot.ui_state.agent_sleep.dormant[&id],
            work.record
        );
        assert!(!runtime.request_dormant_wake(&id));
        assert_eq!(runtime.snapshot.navigator.sleeping_sessions.len(), 1);
        let mut confirmed = tab_order_payload(
            &cwd,
            &["w-order:t1", "w-order:t3"],
            &["w-order:t1", "w-order:t3"],
            "w-order:t1",
        );
        let mut agent = session(Some(5)).agents.remove(0);
        agent.agent = Some(kind.into());
        agent.pane_id = Some("w-order:t3:p".into());
        agent.cwd = Some(cwd.clone());
        agent.facts = Some(crate::request_view::RowFacts {
            native_session_id: Some(native_id.into()),
            ..Default::default()
        });
        confirmed.agents.push(agent);
        runtime.ingest_session(Ok(confirmed));
        assert!(runtime.snapshot.navigator.sleeping_sessions.is_empty());
        assert!(runtime.snapshot.ui_state.agent_sleep.dormant.is_empty());
        assert_eq!(runtime.snapshot.recent_closed.count, 0);
        assert!(persisted_dormant(&path, &id).is_null());
        let (changed, effects) = runtime.ingest_close_capture_result(
            &retired_capture,
            Ok(live::CloseCaptureOutcome { item: None }),
        );
        assert!(!changed);
        assert!(effects.is_empty());
        assert!(!runtime.ingest_close_effect_result(
            &live::CloseEffectRequest {
                retain_for_reopen: false,
                allow_replacement_create: false,
                replacement: None,
                key: retired_capture.key.clone(),
                connection_generation: retired_capture.connection_generation,
                target: retired_capture.target.clone(),
            },
            Ok(())
        ));
        assert!(runtime.close_operations.is_empty());
    }
    let calls = herdr.methods();
    for method in ["pane.close", "layout.apply", "agent.start"] {
        assert_eq!(
            calls
                .iter()
                .filter(|called| called.as_str() == method)
                .count(),
            1,
            "{calls:?}"
        );
    }
}

/// An uncertain close is inspected through the real worker, never repaired.
#[test]
#[cfg(unix)]
fn checking_an_uncertain_sent_sleep_keeps_live_topology_and_sends_no_mutation() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    runtime.ingest_session(Ok(session(Some(4))));
    let id = dormant_intent(&mut runtime);
    let record = runtime
        .snapshot
        .ui_state
        .agent_sleep
        .dormant
        .get_mut(&id)
        .unwrap();
    record.phase = crate::agent_sleep::DormantPhase::CloseUnknown;
    record.close_key = Some("saved-sent-close".into());
    runtime.refresh_dormant_rows();
    let herdr = FakeHerdr::start("dormant-status-read-only", |method, _| match method {
        "session.snapshot" => {
            serde_json::json!({"type":"session_snapshot","snapshot":dormant_wire_session(CHECKOUT, &TABS)})
        }
        other => panic!("status inspection must not mutate Herdr: {other}"),
    });
    runtime.live.as_mut().unwrap().api_connector = Arc::new(herdr.connector());
    let shared = Arc::new(std::sync::Mutex::new(runtime));
    shared.lock().unwrap().live.as_mut().unwrap().runtime = Arc::downgrade(&shared);
    assert!(shared.lock().unwrap().request_dormant_status(&id));
    wait(&shared, "read-only dormant status", |runtime| {
        runtime.dormant_status_check.is_none()
    });
    let runtime = shared.lock().unwrap();
    assert_eq!(herdr.methods(), ["session.snapshot"]);
    assert_eq!(pane(&runtime)["id"], SLEEPER);
    let row = &runtime.snapshot.navigator.sleeping_sessions[0];
    assert_eq!(row.sleep_id, id);
    assert_eq!(row.phase, crate::agent_sleep::DormantPhase::CloseUnknown);
    assert!(!row.wake_available);
    assert!(!row.checking);
    assert_eq!(runtime.snapshot.recent_closed.count, 0);
}

/// A retired sleep record cannot change the original close's purpose.
#[test]
fn a_dormant_close_never_becomes_reopenable_after_archive_retirement() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    runtime.ingest_session(Ok(session(Some(4))));
    let id = dormant_intent(&mut runtime);
    runtime
        .snapshot
        .ui_state
        .agent_sleep
        .dormant
        .get_mut(&id)
        .unwrap()
        .phase = crate::agent_sleep::DormantPhase::Closing;
    let herdr = FakeHerdr::start("dormant-retired-capture", |method, _| match method {
        "layout.export" => serde_json::json!({"type":"layout_export","layout":{
            "workspace_id":"w-order","tab_id":"w-order:t2","zoomed":false,"focused_pane_id":SLEEPER,
            "root":{"type":"pane","pane_id":SLEEPER,"cwd":CHECKOUT,"env":{}}
        }}),
        other => panic!("unexpected retired sleep effect: {other}"),
    });
    runtime.live.as_mut().unwrap().api_connector = Arc::new(herdr.connector());
    runtime.close_local_pane(SLEEPER.into(), false, false);
    let key = runtime.snapshot.ui_state.agent_sleep.dormant[&id]
        .close_key
        .clone()
        .unwrap();
    let operation = runtime.close_operations.get_mut(&key).unwrap();
    operation.phase = "awaiting_topology".into();
    operation.item = Some(ClosedItem::Pane {
        key: key.clone(),
        context: operation.request.context.clone(),
        pane: operation
            .request
            .panes
            .iter()
            .find(|pane| pane.pane_id == SLEEPER)
            .unwrap()
            .clone(),
        placement: crate::recent_closed::PanePlacement {
            neighbor_pane_id: None,
            parent_path: vec![],
            direction: crate::recent_closed::ClosedSplitDirection::Right,
            ratio: 0.5,
            target_was_first: false,
        },
    });
    assert!(!operation.request.retain_for_reopen);
    runtime.snapshot.ui_state.agent_sleep.dormant.remove(&id);
    assert!(runtime.mark_close_topology_confirmed(&key));
    runtime.promote_close_reservations();
    assert_eq!(runtime.snapshot.recent_closed.count, 0);
    assert!(!runtime.snapshot.recent_closed.can_reopen);
    assert!(runtime.snapshot.recent_closed.pending.is_empty());
    herdr.wait_for_requests(1, Duration::from_secs(5));
}

#[test]
fn a_failed_actual_state_write_never_sends_a_dormant_close() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    runtime.ingest_session(Ok(session(Some(4))));
    let server = FakeHerdr::start("sleep-save-failure", |method, _| {
        panic!("unexpected external effect: {method}")
    });
    runtime.live.as_mut().unwrap().api_connector = Arc::new(server.connector());
    let id = dormant_intent(&mut runtime);
    // A directory cannot be atomically replaced by the private state file.
    let blocked = tempfile::tempdir().unwrap();
    runtime.state_path = blocked.path().to_path_buf();
    assert!(runtime.write_ui_state().is_err());
    assert!(server.methods().is_empty());
    assert!(runtime.close_operations.is_empty());
    assert!(
        !runtime
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .contains_key(&id)
    );
    assert_eq!(row(&runtime)["pane_id"], SLEEPER);
    assert!(runtime.snapshot.navigator.sleeping_sessions.is_empty());
}

#[test]
fn an_old_successful_save_cannot_close_a_replaced_execution() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    runtime.ingest_session(Ok(session(Some(4))));
    let server = FakeHerdr::start("sleep-stale-save", |method, _| {
        panic!("unexpected stale effect: {method}")
    });
    runtime.live.as_mut().unwrap().api_connector = Arc::new(server.connector());
    let id = dormant_intent(&mut runtime);
    let saved = runtime.snapshot.ui_state.agent_sleep.clone();
    runtime.ingest_session(Ok(session(Some(5))));
    runtime
        .snapshot
        .navigator
        .agents
        .iter_mut()
        .find(|agent| agent.pane_id == SLEEPER)
        .unwrap()
        .row_facts = Some(Default::default());
    assert!(runtime.ingest_dormant_saved(&saved, true));
    assert!(server.methods().is_empty());
    assert!(runtime.close_operations.is_empty());
    assert!(
        !runtime
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .contains_key(&id)
    );
    assert_eq!(row(&runtime)["pane_id"], SLEEPER);
}

/// An interrupted save before close admission sent no close. Checking that
/// recovered intent must release it without touching the original execution.
#[test]
fn checking_an_unsent_recovered_sleep_leaves_the_pane_awake() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    runtime.ingest_session(Ok(session(Some(4))));
    let server = FakeHerdr::start("sleep-unsent-check", |method, _| {
        panic!("an unsent sleep needs no external effect: {method}")
    });
    runtime.live.as_mut().unwrap().api_connector = Arc::new(server.connector());
    let id = dormant_intent(&mut runtime);
    runtime
        .snapshot
        .ui_state
        .agent_sleep
        .dormant
        .get_mut(&id)
        .unwrap()
        .after_load();
    runtime.refresh_dormant_rows();
    assert_eq!(runtime.snapshot.navigator.sleeping_sessions.len(), 1);
    assert!(runtime.dispatch_json(&event(
        "check_sleeping_session",
        serde_json::json!({"sleep_id": id})
    )));
    assert!(server.methods().is_empty());
    assert!(runtime.snapshot.ui_state.agent_sleep.dormant.is_empty());
    assert!(runtime.snapshot.navigator.sleeping_sessions.is_empty());
    assert_eq!(row(&runtime)["pane_id"], SLEEPER);
    assert!(row(&runtime).get("sleep").is_none());
}

/// A status answer captured for an intent cannot recreate it after a fresh
/// native session confirmation has already removed its archived row.
#[test]
fn a_late_sleep_status_answer_never_resurrects_a_cleared_intent() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    runtime.ingest_session(Ok(session(Some(4))));
    let id = dormant_intent(&mut runtime);
    let record = runtime
        .snapshot
        .ui_state
        .agent_sleep
        .dormant
        .get_mut(&id)
        .unwrap();
    record.closed = true;
    record.phase = crate::agent_sleep::DormantPhase::WakeUnknown;
    let work = crate::agent_sleep_herdr::DormantWork {
        id: id.clone(),
        record: record.clone(),
    };
    let generation = runtime.live_generation;
    runtime.dormant_status_check = Some((id.clone(), generation));
    runtime.snapshot.ui_state.agent_sleep.dormant.remove(&id);
    runtime.refresh_dormant_rows();
    runtime.ingest_dormant_status(&work, generation, Err("late answer".into()));
    assert!(runtime.snapshot.navigator.sleeping_sessions.is_empty());
    assert!(runtime.snapshot.ui_state.agent_sleep.dormant.is_empty());
    assert!(runtime.dormant_status_check.is_none());
    assert!(runtime.close_operations.is_empty());
}

/// The minute pass must publish the transition to Unknown even with Never
/// selected and no automatic sleep due. The outcome never resends a start.
#[test]
fn an_expired_dormant_operation_publishes_its_status_action_once() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    runtime.ingest_session(Ok(session(Some(4))));
    let id = dormant_intent(&mut runtime);
    let record = runtime
        .snapshot
        .ui_state
        .agent_sleep
        .dormant
        .get_mut(&id)
        .unwrap();
    record.closed = true;
    record.phase = crate::agent_sleep::DormantPhase::Starting;
    record.transition_started_unix_ms = 1;
    runtime.refresh_dormant_rows();
    assert!(runtime.tick_agent_sleep(180_002));
    let sleeping = &runtime.snapshot.navigator.sleeping_sessions[0];
    assert_eq!(
        sleeping.phase,
        crate::agent_sleep::DormantPhase::WakeUnknown
    );
    assert!(!sleeping.wake_available);
    assert!(!runtime.tick_agent_sleep(180_003));
    assert!(runtime.close_operations.is_empty());
}

/// B2: the setting is one event, survives a restart, and a shared UI-state
/// save does not reset it; a value outside the choices is refused.
#[test]
fn the_sleep_setting_survives_a_restart_and_a_ui_state_update() {
    let mut runtime = runtime();
    let path = runtime.state_path.clone();
    // The test keeps the state folder: it restarts on that file after the
    // runtime is gone.
    let _state = hold_dirs(&mut runtime);
    assert!(runtime.dispatch_json(&event(
        "agent_sleep_set",
        serde_json::json!({"after_hours": 24})
    )));
    assert_eq!(
        serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]["agent_sleep_after_hours"],
        24
    );
    assert!(runtime.dispatch_json(&event(
        "ui_state_update",
        serde_json::json!({"accent_hex": "#7DD3FC", "font_size": 14})
    )));
    assert_eq!(
        runtime.snapshot().ui_state.agent_sleep_after_hours,
        Some(24)
    );

    runtime.dispatch_json(&event(
        "agent_sleep_set",
        serde_json::json!({"after_hours": 5}),
    ));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("agent_sleep.invalid_setting")
    );
    assert_eq!(
        runtime.snapshot().ui_state.agent_sleep_after_hours,
        Some(24)
    );
    drop(runtime);

    let mut restarted = Runtime::new(
        CoreOptions {
            schema_version: SCHEMA_VERSION,
            home: None,
            node_id: crate::node::test_node(),
            herdr_socket_path: Some("/tmp/herdr-core-pet-runtime.sock".to_owned()),
            herdr_bin_path: None,
            app_state_path: path.to_string_lossy().into_owned(),
            host_helper_root: None,
            host_cli_dir: None,
            workspace_views_path: None,
            shortcut_import_path: None,
            local_issues_path: None,
        },
        environment::EnvironmentReport {
            statuses: Vec::new(),
            home_path: None,
            codex_home: None,
        },
        std::sync::Arc::new(hide_node::Local::of_process()),
        crate::node::test_devices(),
    );
    assert_eq!(
        restarted.snapshot().ui_state.agent_sleep_after_hours,
        Some(24)
    );
    assert!(restarted.dispatch_json(&event(
        "agent_sleep_set",
        serde_json::json!({"after_hours": null})
    )));
    assert_eq!(restarted.snapshot().ui_state.agent_sleep_after_hours, None);
}

/// B10, B20: a sleeping agent keeps its row, name and place with the moon;
/// an awake row carries no `sleep` key, so an older reader's decode holds.
#[test]
fn a_slept_agent_keeps_its_row_after_herdr_forgets_it() {
    let (mut runtime, _) = asleep();
    runtime.ingest_session(Ok(session(None)));
    let slept = row(&runtime);
    assert_eq!(
        (
            &slept["id"],
            &slept["agent_kind"],
            &slept["identity_label"],
            &slept["symbol"],
            &slept["status_code"],
            &slept["group"],
            &slept["sleep"]["state"],
        ),
        (
            &serde_json::json!("reviewer"),
            &serde_json::json!("claude"),
            &serde_json::json!("Review the parser"),
            &serde_json::json!("\u{263e}"),
            &serde_json::json!("sleeping"),
            &serde_json::json!("seen"),
            &serde_json::json!("sleeping"),
        )
    );
    let pane = pane(&runtime);
    assert_eq!(pane["sleep"]["state"], "sleeping");
    assert!(
        pane.get("sleep_action").is_none(),
        "a sleeping pane offers Wake, not Sleep"
    );
    assert!(
        serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]
            .get("agent_sleep")
            .is_none(),
        "the records stay off the wire"
    );
}

/// A sleeping agent has ended its process, so no hook can speak from its
/// pane: the pane is not a session Hide cannot hear, and the agent's row in
/// Settings does not count it among the sessions running now.
#[test]
fn a_sleeping_agent_is_neither_counted_nor_called_not_connected() {
    use super::agent_connection::{connection_of, diagnosis, installed, kit_rows, sessions_of};
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    kit_rows(&mut runtime, None);
    runtime.ingest_hook_diagnosis(diagnosis(installed(), installed()));
    runtime.ingest_session(Ok(session(Some(4))));
    // Awake, the session started before the hook: it runs and is judged.
    assert_eq!(sessions_of(&runtime, "claude-code"), Some(1));
    assert_eq!(
        connection_of(&runtime, SLEEPER).and_then(|connection| connection.reason),
        Some(crate::model::PaneConnectionReason::StartedBeforeHide)
    );

    runtime.dispatch_json(&event(
        "agent_sleep",
        serde_json::json!({"pane_id": SLEEPER}),
    ));
    assert!(runtime.ingest_agent_sleep_end(SLEEPER, Ok(4)));
    runtime.ingest_session(Ok(session(None)));
    assert_eq!(pane(&runtime)["sleep"]["state"], "sleeping");
    assert_eq!(connection_of(&runtime, SLEEPER), None);
    assert_eq!(sessions_of(&runtime, "claude-code"), Some(0));
}

/// #268: a sleep record written before labels carried an owner proves no
/// session, so its row falls back to the provider title with no progress.
#[test]
fn a_legacy_sleep_record_without_a_label_owner_shows_no_stale_label() {
    let (mut runtime, _) = asleep();
    let record = runtime
        .snapshot
        .ui_state
        .agent_sleep
        .records
        .get_mut(SLEEPER)
        .expect("the sleep record");
    assert!(
        record.label_owner.is_some(),
        "a current row stamps its owner"
    );
    record.label_owner = None;
    record.progress = Some("Split the lexer".to_owned());
    runtime.ingest_session(Ok(session(None)));
    let slept = row(&runtime);
    assert_eq!(slept["sleep"]["state"], "sleeping");
    assert_eq!(slept["identity_label"], "Claude");
    assert!(slept.get("progress").is_none_or(serde_json::Value::is_null));
}

/// B12: typed input to a sleeping pane reaches no terminal.
#[test]
fn input_to_a_sleeping_pane_goes_nowhere() {
    let (mut runtime, _) = asleep();
    let generation = runtime.snapshot().input_generation;
    assert!(!runtime.dispatch_json(&event(
        "key",
        serde_json::json!({"pane_id": SLEEPER, "bytes_base64": "aGk="})
    )));
    assert_eq!(runtime.snapshot().input_generation, generation);
}

/// B12, D-12: a committed visit to the tab wakes the agent; another event
/// in the same tab is not a visit, and a second wake starts nothing.
#[test]
fn visiting_the_tab_wakes_the_agent_once() {
    let (mut runtime, checkout_id) = asleep();
    runtime.ingest_session(Ok(session(None)));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    assert_eq!(row(&runtime)["sleep"]["state"], "waking");
    assert_eq!(row(&runtime)["status_code"], "waking");
    assert!(!runtime.dispatch_json(&event(
        "agent_wake",
        serde_json::json!({"pane_id": SLEEPER})
    )));
}

/// B14: a failed wake says why in plain words and offers Retry and Start
/// new session; Herdr's own words stay in the log.
#[test]
fn a_failed_wake_says_why_and_can_be_retried_fresh() {
    let (mut runtime, _) = asleep();
    runtime.ingest_session(Ok(session(None)));
    assert!(runtime.dispatch_json(&event(
        "agent_wake",
        serde_json::json!({"pane_id": SLEEPER})
    )));
    assert!(runtime.ingest_agent_wake(
        SLEEPER,
        crate::agent_sleep::WakeMode::Resume,
        crate::agent_sleep_herdr::WakeOutcome::Failed {
            reason: "The conversation couldn\u{2019}t be resumed.".into(),
            detail: "agent_start_failed: exited".into(),
        },
    ));
    let failed = row(&runtime);
    assert_eq!(failed["sleep"]["state"], "failed");
    assert_eq!(
        failed["sleep"]["reason"],
        "The conversation couldn\u{2019}t be resumed."
    );
    assert_eq!(failed["status_code"], "sleep_failed");
    assert!(runtime.dispatch_json(&event(
        "agent_wake",
        serde_json::json!({"pane_id": SLEEPER, "fresh": true})
    )));
    assert_eq!(row(&runtime)["sleep"]["state"], "waking");
}

/// B16, B17, B18: a new agent in the pane is awake whatever started it,
/// and a closed pane takes its record with it.
#[test]
fn a_new_agent_clears_the_sleep_and_a_closed_pane_drops_it() {
    let (mut runtime, _) = asleep();
    runtime.ingest_session(Ok(session(Some(4))));
    assert_eq!(
        row(&runtime)["sleep"]["state"],
        "sleeping",
        "the ended agent still listed"
    );
    runtime.ingest_session(Ok(session(Some(9))));
    assert!(row(&runtime).get("sleep").is_none());
    assert_ne!(row(&runtime)["symbol"], "\u{263e}");

    let (mut runtime, _) = asleep();
    let mut closed = tab_order_payload(CHECKOUT, &TABS[..1], &TABS[..1], "w-order:t1");
    closed.agents.clear();
    runtime.ingest_session(Ok(closed));
    assert!(runtime.snapshot().ui_state.agent_sleep.records.is_empty());
}

/// B8: an end that did not land leaves the agent awake.
#[test]
fn an_end_that_fails_leaves_the_agent_awake() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    runtime.ingest_session(Ok(session(Some(4))));
    runtime.dispatch_json(&event(
        "agent_sleep",
        serde_json::json!({"pane_id": SLEEPER}),
    ));
    runtime.ingest_agent_sleep_end(SLEEPER, Err("did not hand the terminal back".into()));
    assert!(runtime.snapshot().ui_state.agent_sleep.records.is_empty());
    assert!(row(&runtime).get("sleep").is_none());
    assert_ne!(row(&runtime)["symbol"], "\u{263e}");
}

/// B4-B6: the minute decision ends an agent that sat seen and off screen
/// past the chosen hours, never the one on screen, at most once a minute,
/// and not at all while the setting is Never.
#[test]
fn the_minute_decision_sleeps_only_an_off_screen_agent_past_the_chosen_hours() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    let mut payload = session(Some(4));
    payload.agents.push(
        crate::sidebar::owned_label_fixture(serde_json::json!({
            "pane_id": "w-order:t1:p", "agent": "codex", "agent_status": "idle",
            "state_change_seq": 2, "cwd": CHECKOUT,
            "agent_session": {"kind": "id", "value": "on-screen-session"}
        }))
        .unwrap(),
    );
    runtime.ingest_session(Ok(payload));
    let later = unix_milliseconds() + 13 * 60 * 60 * 1000;
    let ending = |runtime: &Runtime| {
        runtime
            .snapshot()
            .ui_state
            .agent_sleep
            .records
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };

    assert!(!runtime.tick_agent_sleep(later));
    assert!(ending(&runtime).is_empty(), "Never sleeps nothing");

    runtime.dispatch_json(&event(
        "agent_sleep_set",
        serde_json::json!({"after_hours": 24}),
    ));
    runtime.tick_agent_sleep(later);
    assert!(ending(&runtime).is_empty(), "13 hours is not 24");

    runtime.dispatch_json(&event(
        "agent_sleep_set",
        serde_json::json!({"after_hours": 12}),
    ));
    runtime.tick_agent_sleep(later);
    assert_eq!(ending(&runtime), [SLEEPER], "t1 is on screen, t2 is not");
    assert!(
        row(&runtime).get("sleep").is_none(),
        "awake until the end lands"
    );
}
