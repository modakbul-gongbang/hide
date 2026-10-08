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

/// B12: typed input to a sleeping pane reaches no terminal: the pane's node
/// is told it sleeps, and drops its keys until it wakes
/// (`hide-node` `keys_for_a_sleeping_pane_are_dropped_until_it_wakes`).
#[test]
fn input_to_a_sleeping_pane_goes_nowhere() {
    let (mut runtime, _) = live_tab_order_runtime(CHECKOUT);
    let terminals = record_terminals(&mut runtime);
    runtime.ingest_session(Ok(session(Some(4))));
    runtime.dispatch_json(&event(
        "agent_sleep",
        serde_json::json!({"pane_id": SLEEPER}),
    ));
    let asleep = TerminalControl::Asleep {
        pane: SLEEPER.into(),
        asleep: true,
    };
    assert!(
        !terminals.take().contains(&asleep),
        "the agent takes keys until its end lands (B8)"
    );
    assert!(runtime.ingest_agent_sleep_end(SLEEPER, Ok(4)));
    assert!(terminals.take().contains(&asleep));
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
