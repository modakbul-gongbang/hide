use super::*;
use crate::agent_state::sessions::ResolveSource;
use crate::model::PullRequestSnapshot;
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
    assert!(runtime.snapshot.navigator.agents[0].resolved_recent);
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
fn a_resolution_older_than_a_day_leaves_resolved_without_closing_its_pane() {
    let mut runtime = with_session();
    runtime.resolve_session("session", ResolveSource::Operator);
    runtime
        .snapshot
        .ui_state
        .resolved_sessions
        .get_mut("session")
        .unwrap()
        .at_unix_ms -= crate::agent_state::sessions::RESOLVED_WINDOW_MS + 1;
    runtime.sync_session_state();
    let row = &runtime.snapshot.navigator.agents[0];
    assert!(row.resolved.is_some());
    assert!(!row.resolved_recent);
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
    assert!(runtime.snapshot.navigator.agents[0].resolved_recent);
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
fn session_resolution_leaves_resolved_24_hours_later_without_closing() {
    let mut runtime = with_session();
    let _dirs = hold_dirs(&mut runtime);
    assert!(runtime.resolve_session("session", ResolveSource::Operator));
    assert!(runtime.snapshot.navigator.agents[0].resolved_recent);
    let at = runtime.snapshot.ui_state.resolved_sessions["session"].at_unix_ms;
    let leaves = at + crate::agent_state::sessions::RESOLVED_WINDOW_MS;
    assert_eq!(runtime.session_window_deadline_unix_ms, leaves);
    assert!(
        !runtime.tick_session_window(leaves - 1),
        "an idle tick before the deadline compares one number"
    );
    assert!(runtime.snapshot.navigator.agents[0].resolved_recent);
    assert!(runtime.tick_session_window(leaves));
    assert!(!runtime.snapshot.navigator.agents[0].resolved_recent);
    assert!(runtime.snapshot.navigator.agents[0].resolved.is_some());
    assert_eq!(runtime.session_window_deadline_unix_ms, u64::MAX);
}

#[test]
fn pane_bands_prioritize_connection_then_own_demand_then_raised_descendants() {
    use crate::agent_state::{
        RequestVerb,
        escalation::{RaisedAsk, Verb},
        header,
    };
    let runtime = with_session();
    let mut agent = runtime.snapshot.navigator.agents[0].clone();
    let pane = pane("session", "/work/app");
    let project = workspace("app", "App", "/work/app", vec![]);
    agent.state.verb = RequestVerb::Working;
    let working = header::of(&pane, Some(&agent), None, &project, None);
    assert!(working.working);
    assert!(working.band.is_none());
    agent.raised = ["first", "second"]
        .map(|id| RaisedAsk {
            verb: Verb::Approval,
            what: Some("e2e 테스트 돌리던 중".into()),
            pane_id: id.into(),
            title: format!("Task {id}"),
            agent_kind: "claude".into(),
            open_pane_id: id.into(),
            since_unix_ms: Some(123),
            unreceived_by: None,
            path: vec!["Root".into(), format!("Task {id}")],
            checkout: None,
            human_notice: false,
        })
        .into();
    let raised = header::of(&pane, Some(&agent), None, &project, None);
    let band = raised.band.unwrap();
    assert_eq!(
        (
            band.kind.as_str(),
            band.more,
            band.raised.as_ref().map(|lead| lead.verb)
        ),
        ("raised", 1, Some(Verb::Approval)),
        "the lead ask and the rest as 외 N건"
    );
    assert_eq!(
        band.action,
        Some(header::Action::Child {
            pane_id: "first".into(),
            label: "Task first".into()
        })
    );
    assert!(!raised.working);
    // The root's own approval leads; its raised descendants are 외 N건.
    agent.demand = "question".into();
    agent.state.verb = RequestVerb::Answer;
    agent.blocked = true;
    let own = header::of(&pane, Some(&agent), None, &project, None)
        .band
        .unwrap();
    assert_eq!((own.kind.as_str(), own.more), ("approval", 2));
    agent.user_turn = Some(hide_session::turns::UserTurnFact {
        kind: hide_session::turns::UserTurnKind::Question,
        content: None,
    });
    assert_eq!(
        header::of(&pane, Some(&agent), None, &project, None)
            .band
            .unwrap()
            .kind,
        "answer",
        "the native question holds for a reply even after it is read"
    );
    agent.user_turn.as_mut().unwrap().kind = hide_session::turns::UserTurnKind::PlanApproval;
    assert_eq!(
        header::of(&pane, Some(&agent), None, &project, None)
            .band
            .unwrap()
            .kind,
        "approval",
        "a native plan wait still asks for approval"
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
fn pane_pr_band_targets_its_duty_instead_of_another_link_with_higher_sort_priority() {
    use crate::agent_state::{RequestVerb, header};
    let mut runtime = with_session();
    let agent = &mut runtime.snapshot.navigator.agents[0];
    let pane = pane("session", "/work/app");
    let mut project = workspace("app", "App", "/work/app", vec![]);
    let pr = |number, checks| {
        serde_json::from_value::<PullRequestSnapshot>(json!({
        "number":number,"title":"Duty","url":format!("https://github.com/acme/app/pull/{number}"),
        "head_branch":"feature","base_branch":"main","badge":"open","checks":checks,"is_draft":false,"closing_issues":[]
    })).unwrap()
    };
    project.pull_requests = vec![pr(1, "failed"), pr(2, "passing")];
    let link = |index: usize, duty| {
        let p = &project.pull_requests[index];
        crate::request_view::AgentPullRequestSnapshot {
            number: p.number,
            title: p.title.clone(),
            url: p.url.clone(),
            badge: p.badge,
            checks: p.checks,
            review: p.review,
            head_branch: p.head_branch.clone(),
            closing_issues: vec![],
            live: true,
            duty,
            created: true,
            settled_at_unix_ms: None,
        }
    };
    agent.request = Some(crate::request_view::AgentRequestSnapshot {
        verb: RequestVerb::Review,
        verb_since_unix_ms: 123,
        line: Some("Review duty".into()),
        end: None,
        request: None,
        later_by: None,
        reply: None,
        pull_requests: vec![link(0, false), link(1, true)],
    });
    agent.state = crate::agent_state::turn::row_state(agent);
    let projected = header::of(&pane, Some(agent), None, &project, None);
    let band = projected.band.unwrap();
    assert_eq!(band.kind, "merge");
    assert!(matches!(
        band.action,
        Some(header::Action::Pr { number: 2, .. })
    ));
    assert_eq!(band.since_unix_ms, Some(123));
    assert_eq!(band.reason, None);
    assert_eq!(
        band.facts,
        Some(header::ReasonFacts::PullRequest {
            checks: crate::model::PullRequestChecks::Passing,
            review: None
        })
    );
    // An earlier approved duty must not steal the action from the PR that
    // still needs a review, even though both checks are passing.
    let links = &mut agent.request.as_mut().unwrap().pull_requests;
    links[0].duty = true;
    links[0].checks = crate::model::PullRequestChecks::Passing;
    links[0].review = Some(crate::model::ReviewDecision::Approved);
    links[1].review = Some(crate::model::ReviewDecision::ReviewRequired);
    agent.state = crate::agent_state::turn::row_state(agent);
    let review = header::of(&pane, Some(agent), None, &project, None)
        .band
        .unwrap();
    assert_eq!(review.kind, "review");
    assert!(matches!(
        review.action,
        Some(header::Action::Pr { number: 2, .. })
    ));
    assert_eq!(
        review.facts,
        Some(header::ReasonFacts::PullRequest {
            checks: crate::model::PullRequestChecks::Passing,
            review: Some(crate::model::ReviewDecision::ReviewRequired)
        })
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
