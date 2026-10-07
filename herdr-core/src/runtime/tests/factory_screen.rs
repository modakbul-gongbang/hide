//! The Factory screens through the runtime (PRD software-factory-ui B10,
//! B21, B25): a screen action reaches the engine thread and its answer comes
//! back on the request id, and the `factory` and `factory_task` sections ride
//! their own delta revisions, sent only when the host handed a change over.

use super::*;
use crate::factory::screen::FactoryTaskSection;
use hide_factory::FactorySummary;

fn action(request_id: &str, command: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "factory_action",
        "payload": {"request_id": request_id, "command": command}
    }))
    .unwrap()
}

fn answer_for<'a>(runtime: &'a Runtime, request_id: &str) -> Option<&'a serde_json::Value> {
    runtime
        .snapshot
        .factory
        .as_deref()?
        .actions
        .iter()
        .find(|answer| answer.request_id == request_id)
        .map(|answer| &answer.answer)
}

#[test]
fn an_action_with_no_engine_is_refused_on_its_own_request_id() {
    let mut runtime = runtime();
    assert!(runtime.dispatch_json(&action(
        "r-1",
        serde_json::json!({"verb": "merge", "task": "T-1"})
    )));
    let answer = answer_for(&runtime, "r-1").expect("the refusal answers the request");
    assert_eq!(answer["ok"], false);
    assert_eq!(answer["reason"], "factory_unavailable");
    assert!(
        runtime
            .snapshot
            .factory
            .as_deref()
            .unwrap()
            .summary
            .is_none(),
        "no summary arrived, so the screen keeps its skeleton (B21)"
    );
}

#[test]
fn an_invalid_request_is_refused_without_reaching_the_engine() {
    let mut runtime = runtime();
    let long = "r".repeat(crate::factory::screen::REQUEST_ID_LIMIT + 1);
    assert!(!runtime.dispatch_json(&action(
        &long,
        serde_json::json!({"verb": "merge", "task": "T-1"})
    )));
    assert!(runtime.snapshot.factory.is_none());
    runtime.dispatch_json(&action("r-2", serde_json::json!({"verb": "merge"})));
    assert!(
        runtime.snapshot.status.last_error.is_some(),
        "a command the engine's contract does not accept is an invalid event"
    );
    assert!(runtime.snapshot.factory.is_none(), "and reaches no engine");
}

#[test]
fn the_sections_are_sent_once_per_change_on_their_own_revisions() {
    let mut runtime = runtime();
    let first = runtime.snapshot_delta_payload(0, 0);
    assert!(
        first.factory.is_none(),
        "absent until the host hands one over"
    );
    assert!(first.factory_task.is_none());

    runtime.set_factory_screen(
        Some(Arc::new(FactorySummary {
            my_turn: 2,
            ..FactorySummary::default()
        })),
        None,
    );
    let changed = runtime.snapshot_delta_payload(first.revision, 0);
    let section = changed.factory.as_ref().expect("the summary is sent");
    assert_eq!(section.summary.as_ref().unwrap().my_turn, 2);
    let idle = runtime.snapshot_delta_payload(changed.revision, 0);
    assert!(
        idle.factory.is_none(),
        "an unchanged section is not sent again (B25)"
    );
    assert_eq!(idle.revision, changed.revision, "and stamps no revision");

    let page = FactoryTaskSection {
        factory: "f-1".into(),
        task: "T-1".into(),
        detail: None,
    };
    runtime.set_factory_screen(None, Some(Some(page.clone())));
    let opened = runtime.snapshot_delta_payload(idle.revision, 0);
    assert!(
        opened.factory.is_none(),
        "a page does not resend the summary"
    );
    assert_eq!(opened.factory_task, Some(Some(page)));

    runtime.set_factory_screen(None, Some(None));
    let closed = runtime.snapshot_delta_payload(opened.revision, 0);
    assert_eq!(
        closed.factory_task,
        Some(None),
        "a closed page is sent as closed"
    );
    let fresh = runtime.snapshot_delta_payload(0, 0);
    assert_eq!(
        fresh.factory_task,
        Some(None),
        "a fresh reader learns it is closed"
    );
    assert!(fresh.factory.is_some(), "and gets the summary");
    let bytes = serialize_snapshot_delta(&fresh).unwrap();
    let wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(wire["factory"]["summary"]["my_turn"], 2);
    assert!(wire["factory_task"].is_null() && wire.get("factory_task").is_some());
}

/// A real host on a state folder with no store: the screen sees an empty
/// summary (B1), and a verb a screen does not send comes back refused on its
/// request id rather than run.
#[test]
fn the_host_answers_a_screen_and_publishes_an_empty_machine() {
    let state = scratch_dir("herdr-core-factory-screen-");
    let shared = Arc::new(Mutex::new(runtime()));
    let mut host = crate::factory::FactoryHost::start(
        state.path(),
        None,
        Arc::downgrade(&shared),
        ChangeNotifier::noop(),
    )
    .expect("the Factory host starts");
    shared
        .lock()
        .unwrap()
        .set_factory_screen_port(host.screen_port());
    wait(&shared, "the empty summary", |runtime| {
        runtime
            .snapshot
            .factory
            .as_deref()
            .is_some_and(|section| section.summary.is_some())
    });
    assert_eq!(
        **shared
            .lock()
            .unwrap()
            .snapshot
            .factory
            .as_deref()
            .unwrap()
            .summary
            .as_ref()
            .unwrap(),
        FactorySummary::default()
    );
    assert!(
        !shared
            .lock()
            .unwrap()
            .dispatch_json(&action("r-3", serde_json::json!({"verb": "inbox"})))
    );
    wait(&shared, "the refusal", |runtime| {
        answer_for(runtime, "r-3").is_some()
    });
    let runtime = shared.lock().unwrap();
    let answer = answer_for(&runtime, "r-3").unwrap();
    assert_eq!(answer["reason"], "factory_screen_verb");
    drop(runtime);
    host.shutdown();
}
