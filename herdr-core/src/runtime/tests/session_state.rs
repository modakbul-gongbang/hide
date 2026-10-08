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

#[test]
fn pane_bands_prioritize_connection_then_own_demand_then_raised_children() {
    use crate::agent_state::{RequestVerb, escalation::RaisedChild, header, sessions::Tag};
    let runtime = with_session();
    let mut agent = runtime.snapshot.navigator.agents[0].clone();
    let pane = pane("session", "/work/app");
    let project = workspace("app", "App", "/work/app", vec![]);
    agent.state.verb = RequestVerb::Working;
    let working = header::of(&pane, Some(&agent), None, &project, None);
    assert!(working.working);
    assert!(working.band.is_none());
    agent.raised_children = ["first", "second"]
        .map(|id| RaisedChild {
            pane_id: id.into(),
            title: format!("Task {id}"),
            tag: Tag::Answer,
            reason: Some("Please answer".into()),
            since_unix_ms: Some(123),
        })
        .into();
    let raised = header::of(&pane, Some(&agent), None, &project, None);
    let band = raised.band.unwrap();
    assert_eq!(
        (band.kind.as_str(), band.more, band.child_tag),
        ("raised_child", 1, Some(Tag::Answer))
    );
    assert_eq!(
        band.action,
        Some(header::Action::Child {
            pane_id: "first".into(),
            label: "Task first".into()
        })
    );
    assert!(!raised.working);
    agent.state.verb = RequestVerb::Answer;
    agent.blocked = true;
    assert_eq!(
        header::of(&pane, Some(&agent), None, &project, None)
            .band
            .unwrap()
            .kind,
        "approval"
    );
    let offline = header::of(&pane, Some(&agent), None, &project, Some("mini"))
        .band
        .unwrap();
    assert_eq!(
        (offline.kind.as_str(), offline.tone, offline.action),
        ("device_offline", "muted", None)
    );
}

#[test]
fn pane_bands_keep_idle_and_ci_wait_quiet_and_distinguish_failed_exits() {
    use crate::agent_state::{RequestVerb, header};
    let runtime = with_session();
    let mut agent = runtime.snapshot.navigator.agents[0].clone();
    let pane = pane("session", "/work/app");
    let project = workspace("app", "App", "/work/app", vec![]);
    for verb in [RequestVerb::Idle, RequestVerb::Waiting] {
        agent.state.verb = verb;
        let result = header::of(&pane, Some(&agent), None, &project, None);
        assert!(!result.working);
        assert!(result.band.is_none());
    }
    for (code, kind, tone) in [(0, "terminated", "muted"), (7, "exit", "error")] {
        let terminal = TerminalPaneSnapshot {
            closed: true,
            exit_code: Some(code),
            ..Default::default()
        };
        let band = header::of(&pane, Some(&agent), Some(&terminal), &project, None)
            .band
            .unwrap();
        assert_eq!(
            (band.kind.as_str(), band.tone, band.exit_code),
            (kind, tone, Some(code))
        );
    }
}
