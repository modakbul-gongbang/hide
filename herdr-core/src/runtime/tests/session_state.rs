use super::*;
use crate::agent_state::sessions::ResolveSource;
use serde_json::json;

fn with_session() -> Runtime {
    let mut runtime = runtime();
    runtime.snapshot.navigator.agents = project_agents(serde_json::from_value(json!({
        "agents": [{"pane_id":"session", "agent":"codex", "agent_status":"done", "state_change_seq":1}]
    })).unwrap()).agents;
    runtime
}

#[test]
fn session_resolution_survives_restart_and_new_input_restores_the_live_pane() {
    let mut runtime = with_session();
    let _dirs = hold_dirs(&mut runtime);
    assert!(runtime.resolve_session("session", ResolveSource::Operator));
    assert!(runtime.snapshot.navigator.agents[0].resolved_today);
    let loaded = persistence::load(&runtime.state_path).0;
    assert_eq!(
        loaded.resolved_sessions["session"].source,
        ResolveSource::Operator
    );
    let at = loaded.resolved_sessions["session"].at_unix_ms;
    runtime.note_delivery_key("session");
    assert!(runtime.snapshot.navigator.agents[0].resolved.is_none());
    assert!(
        persistence::load(&runtime.state_path)
            .0
            .resolved_sessions
            .is_empty()
    );
    assert!(runtime.snapshot.ui_state.session_resolution_inputs["session"] >= at);
    assert!(
        !runtime.sync_session_state(),
        "unchanged state publishes no new frame"
    );
}

#[test]
fn session_resolution_save_failure_keeps_the_row_visible() {
    let mut runtime = with_session();
    let occupied = runtime.state_path.with_file_name("occupied");
    std::fs::write(&occupied, b"not a directory").unwrap();
    runtime.state_path = occupied.join("state.json");
    assert!(!runtime.resolve_session("session", ResolveSource::Operator));
    assert!(runtime.snapshot.navigator.agents[0].resolved.is_none());
    assert!(runtime.snapshot.ui_state.resolved_sessions.is_empty());
    assert!(runtime.pending_session_resolutions.is_empty());
}

#[test]
fn older_resolution_is_hidden_from_today_without_closing_its_pane() {
    let mut runtime = with_session();
    runtime.resolve_session("session", ResolveSource::Operator);
    runtime
        .snapshot
        .ui_state
        .resolved_sessions
        .get_mut("session")
        .unwrap()
        .local_date = "2000-01-01".into();
    runtime.sync_session_state();
    let row = &runtime.snapshot.navigator.agents[0];
    assert!(row.resolved.is_some());
    assert!(!row.resolved_today);
    assert_eq!(runtime.snapshot.navigator.agents.len(), 1);
}

#[test]
fn session_resolve_waits_for_save_and_ignores_an_ack_after_new_input() {
    let mut runtime = with_session();
    let _dirs = hold_dirs(&mut runtime);
    // Keep the coalesced writer queued so this test controls the save boundary.
    runtime.install_worker_context(Weak::new(), ChangeNotifier::noop());
    runtime.state_save_active = true;
    assert!(runtime.resolve_session("session", ResolveSource::Operator));
    assert!(runtime.snapshot.navigator.agents[0].resolved.is_none());
    let pending = runtime.ui_state_to_save();
    persistence::save(&runtime.state_path, &pending, &BTreeMap::new()).unwrap();
    runtime.note_delivery_key("session");
    assert!(!runtime.acknowledge_session_save(&pending, true));
    assert!(runtime.snapshot.navigator.agents[0].resolved.is_none());

    assert!(runtime.resolve_session("session", ResolveSource::Operator));
    let current = runtime.ui_state_to_save();
    persistence::save(&runtime.state_path, &current, &BTreeMap::new()).unwrap();
    assert!(runtime.acknowledge_session_save(&current, true));
    assert!(runtime.snapshot.navigator.agents[0].resolved_today);
    assert!(
        !runtime.acknowledge_session_save(&current, true),
        "duplicate acknowledgements do not publish"
    );
    runtime.prune_resolved_sessions(|pane| pane == "session");
    assert!(runtime.snapshot.ui_state.resolved_sessions.is_empty());
    assert!(
        runtime
            .snapshot
            .ui_state
            .session_resolution_inputs
            .is_empty()
    );
}

#[test]
fn session_resolution_expires_at_the_local_day_boundary_without_closing() {
    let mut runtime = with_session();
    let _dirs = hold_dirs(&mut runtime);
    assert!(runtime.resolve_session("session", ResolveSource::Operator));
    assert!(runtime.snapshot.navigator.agents[0].resolved_today);
    assert!(runtime.tick_session_day(runtime.session_next_day_unix_ms));
    assert!(!runtime.snapshot.navigator.agents[0].resolved_today);
    assert!(runtime.snapshot.navigator.agents[0].resolved.is_some());
    assert!(!runtime.tick_session_day(runtime.session_next_day_unix_ms - 1));
}
