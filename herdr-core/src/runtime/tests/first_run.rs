//! The first-run wizard's progress as the core keeps it (PRD
//! first-run-onboarding D-07, D-08, D-21, B3, B6, B10, B12, B26, B43, B45).

use super::appearance::{runtime_at, ui_state_update};
use super::*;
use crate::model::{FirstRun, FirstRunStep, ScreenIntro};
use hide_kit::{AgentReport, Availability, ComponentState, KitReport, PieceReport};

fn agent(id: &str, enabled: bool, skill: ComponentState) -> AgentReport {
    AgentReport {
        id: id.to_owned(),
        label: id.to_owned(),
        availability: Availability::Available,
        enabled,
        chosen: false,
        skill: PieceReport {
            state: skill,
            reason: (skill == ComponentState::Failed).then(|| "permission denied".to_owned()),
            location: None,
        },
        hook: None,
        herdr: None,
        doc_url: "https://example.test/skills".to_owned(),
    }
}

/// This Mac's kit pass: `held` while its record waits for the first-run choice.
fn pass(held: bool, agents: Vec<AgentReport>) -> KitReport {
    KitReport {
        agents,
        held_for_onboarding: held,
        ..KitReport::default()
    }
}

fn first_run_pass() -> KitReport {
    pass(
        true,
        vec![
            agent("claude-code", false, ComponentState::Off),
            agent("codex", false, ComponentState::Off),
        ],
    )
}

fn event(kind: &str, payload: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": kind, "payload": payload,
    }))
    .expect("event")
}

fn step(runtime: &Runtime) -> Option<FirstRunStep> {
    runtime.snapshot().ui_state.first_run.map(|run| run.step)
}

fn report_here(runtime: &mut Runtime, report: &KitReport) {
    let node = runtime.node.clone();
    runtime.ingest_kit_report(node.as_str(), report);
}

/// The kit worker takes what Continue queued and answers with `report`.
fn run_queued_pass(runtime: &mut Runtime, report: &KitReport) -> hide_kit::Scope {
    let Some(KitJob::Apply(scope)) = runtime.take_local_kit_job(Instant::now()) else {
        panic!("Continue queues an install on this Mac");
    };
    report_here(runtime, report);
    scope
}

fn agent_ids(set: &std::collections::BTreeSet<String>) -> Vec<&str> {
    set.iter().map(String::as_str).collect()
}

/// B3, D-08: a Mac whose kit record waits opens the wizard on its first step;
/// an install that already had the kit never gets one.
#[test]
fn a_first_run_opens_the_wizard_and_an_existing_install_never_does() {
    let mut first = runtime();
    assert_eq!(step(&first), None);
    report_here(&mut first, &first_run_pass());
    assert_eq!(
        first.snapshot().ui_state.first_run,
        Some(FirstRun {
            version: crate::model::FIRST_RUN_VERSION,
            step: FirstRunStep::Agents
        })
    );
    assert_eq!(
        serde_json::to_value(first.snapshot()).unwrap()["ui_state"]["first_run"],
        serde_json::json!({"version": 1, "step": "agents"})
    );

    let mut existing = runtime();
    report_here(
        &mut existing,
        &pass(
            false,
            vec![agent("claude-code", true, ComponentState::Installed)],
        ),
    );
    assert_eq!(step(&existing), None);
    assert!(!existing.snapshot().ui_state.desktop_notifications);
}

/// B6, B10: Continue moves the wizard only once the kit pass it queued has
/// landed, a second press meanwhile and a report already on its way move
/// nothing, and a restart reopens the step the wizard stood on.
#[test]
fn the_first_step_moves_on_when_its_pass_lands_and_a_restart_reopens_there() {
    let folder = scratch_dir("herdr-core-first-run-");
    let path = folder.path().join("state.json");
    let mut runtime = runtime_at(&path);
    report_here(&mut runtime, &first_run_pass());

    assert!(runtime.dispatch_json(&event(
        "agent_onboarding_apply",
        serde_json::json!({"agents": ["claude-code"]}),
    )));
    assert_eq!(step(&runtime), Some(FirstRunStep::Agents));
    assert!(
        !runtime.dispatch_json(&event(
            "agent_onboarding_apply",
            serde_json::json!({"agents": ["codex"]}),
        )),
        "a second press while the pass runs is the same intent"
    );
    // A pass that started before Continue answers while the install waits.
    report_here(&mut runtime, &first_run_pass());
    assert_eq!(step(&runtime), Some(FirstRunStep::Agents));

    let scope = run_queued_pass(
        &mut runtime,
        &pass(
            false,
            vec![
                agent("claude-code", true, ComponentState::Installed),
                agent("codex", false, ComponentState::Off),
            ],
        ),
    );
    assert_eq!(agent_ids(&scope.agent_on), ["claude-code"]);
    assert_eq!(step(&runtime), Some(FirstRunStep::Notifications));
    drop(runtime);

    let restarted = runtime_at(&path);
    assert_eq!(step(&restarted), Some(FirstRunStep::Notifications));
}

/// B12: a chosen agent whose piece failed keeps the first step on screen, and
/// Continue again sends that agent again and moves on once it is in place.
#[test]
fn a_failed_agent_keeps_the_first_step_and_continue_again_retries_it() {
    let mut runtime = runtime();
    report_here(&mut runtime, &first_run_pass());
    runtime.dispatch_json(&event(
        "agent_onboarding_apply",
        serde_json::json!({"agents": ["claude-code"]}),
    ));
    run_queued_pass(
        &mut runtime,
        &pass(
            false,
            vec![
                agent("claude-code", true, ComponentState::Failed),
                agent("codex", false, ComponentState::Off),
            ],
        ),
    );
    assert_eq!(step(&runtime), Some(FirstRunStep::Agents));

    assert!(runtime.dispatch_json(&event(
        "agent_onboarding_apply",
        serde_json::json!({"agents": ["claude-code"]}),
    )));
    let scope = run_queued_pass(
        &mut runtime,
        &pass(
            false,
            vec![
                agent("claude-code", true, ComponentState::Installed),
                agent("codex", false, ComponentState::Off),
            ],
        ),
    );
    assert_eq!(agent_ids(&scope.agent_on), ["claude-code"]);
    assert!(scope.agent_off.is_empty(), "codex was already off");
    assert_eq!(step(&runtime), Some(FirstRunStep::Notifications));
}

/// A record that did not save asks the question again, and the wizard stays
/// on the step that asks it (B6).
#[test]
fn an_unsaved_answer_keeps_the_first_step() {
    let mut runtime = runtime();
    report_here(&mut runtime, &first_run_pass());
    runtime.dispatch_json(&event(
        "agent_onboarding_apply",
        serde_json::json!({"agents": []}),
    ));
    run_queued_pass(&mut runtime, &first_run_pass());
    assert_eq!(
        runtime.snapshot().ui_state.agent_onboarding,
        Some(crate::model::AgentOnboarding::Pending)
    );
    assert_eq!(step(&runtime), Some(FirstRunStep::Agents));
}

/// B26: a browser tab's wizard is its first step alone, so applying it ends it.
#[test]
fn a_browser_tab_finishes_the_wizard_with_its_first_step() {
    let mut runtime = runtime();
    report_here(&mut runtime, &first_run_pass());
    runtime.dispatch_json(&event(
        "agent_onboarding_apply",
        serde_json::json!({"agents": ["codex"], "finish": true}),
    ));
    run_queued_pass(
        &mut runtime,
        &pass(false, vec![agent("codex", true, ComponentState::Installed)]),
    );
    assert_eq!(step(&runtime), Some(FirstRunStep::Done));
}

/// B21, B25, B6: the later steps move one at a time from the step on screen;
/// a window still showing an earlier step moves nothing, and the first step
/// ends only through its apply.
#[test]
fn the_wizard_leaves_only_the_step_on_screen() {
    let mut runtime = runtime();
    runtime.snapshot.ui_state.first_run = Some(FirstRun {
        version: crate::model::FIRST_RUN_VERSION,
        step: FirstRunStep::Notifications,
    });
    let advance = |from: &str| event("first_run_advance", serde_json::json!({"from": from}));
    assert!(!runtime.dispatch_json(&advance("project")));
    assert_eq!(step(&runtime), Some(FirstRunStep::Notifications));
    assert!(runtime.dispatch_json(&advance("notifications")));
    assert_eq!(step(&runtime), Some(FirstRunStep::Project));
    assert!(!runtime.dispatch_json(&advance("notifications")));
    assert!(!runtime.dispatch_json(&advance("agents")));
    assert!(runtime.dispatch_json(&advance("project")));
    assert_eq!(step(&runtime), Some(FirstRunStep::Done));
    assert!(!runtime.dispatch_json(&advance("done")));
}

/// B45: the first step reopened from Help on an install whose question was
/// answered switches only the agents whose switch it changed, the way
/// Settings > Agents does, and opens no wizard progress.
#[test]
fn a_reopened_first_step_switches_only_what_it_changed() {
    let mut runtime = runtime();
    let mut omp = agent("omp", false, ComponentState::Off);
    omp.availability = Availability::Available;
    let mut cursor = agent("cursor", true, ComponentState::Installed);
    cursor.availability = Availability::NotInstalled;
    report_here(
        &mut runtime,
        &pass(
            false,
            vec![
                agent("claude-code", true, ComponentState::Installed),
                agent("codex", true, ComponentState::Installed),
                omp,
                cursor,
            ],
        ),
    );
    assert!(runtime.dispatch_json(&event(
        "agent_onboarding_apply",
        serde_json::json!({"agents": ["claude-code", "omp"]}),
    )));
    let Some(KitJob::Apply(scope)) = runtime.take_local_kit_job(Instant::now()) else {
        panic!("the changed agents are queued");
    };
    assert_eq!(agent_ids(&scope.agent_on), ["omp"]);
    assert_eq!(
        agent_ids(&scope.agent_off),
        ["codex"],
        "an agent that is not installed here has no switch, so it stays on"
    );
    assert_eq!(step(&runtime), None);
}

/// D-21, B30, B43: the notification switch and the screens introduced once
/// survive a shared UI save and a restart.
#[test]
fn the_notification_switch_and_screen_intros_survive_a_save_and_a_restart() {
    let folder = scratch_dir("herdr-core-first-run-switch-");
    let path = folder.path().join("state.json");
    let mut runtime = runtime_at(&path);
    let notifications = |enabled: bool| {
        event(
            "desktop_notifications_set",
            serde_json::json!({"enabled": enabled}),
        )
    };
    assert!(runtime.dispatch_json(&notifications(true)));
    assert!(!runtime.dispatch_json(&notifications(true)));
    let intro = event(
        "screen_intro_seen",
        serde_json::json!({"screen": "factory"}),
    );
    assert!(runtime.dispatch_json(&intro));
    assert!(!runtime.dispatch_json(&intro));

    // A save built from a snapshot older than both carries neither back.
    let stale = ui_state_update(
        &runtime,
        serde_json::json!({"desktop_notifications": false, "screen_intros_seen": []}),
    );
    runtime.dispatch_json(&stale);
    assert!(runtime.snapshot().ui_state.desktop_notifications);
    drop(runtime);

    let mut restarted = runtime_at(&path);
    assert!(restarted.snapshot().ui_state.desktop_notifications);
    restarted.dispatch_json(&event(
        "screen_intro_seen",
        serde_json::json!({"screen": "settings"}),
    ));
    assert_eq!(
        restarted
            .snapshot()
            .ui_state
            .screen_intros_seen
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [ScreenIntro::Factory],
        "an unknown screen is refused"
    );
}
