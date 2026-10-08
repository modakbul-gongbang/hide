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
