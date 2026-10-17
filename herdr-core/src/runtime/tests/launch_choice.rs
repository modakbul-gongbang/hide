//! Every restart Hide makes of an agent (a wake, a fork, a reopen) starts it
//! with the model and effort it was started with, from the pane's launch
//! record, and a record from a build that kept none restarts on the CLI's
//! defaults as before. The restarts run through their real workers against a
//! fake Herdr; the expected command lines are each CLI's own spelling
//! (`claude --model <m> --effort <e>`, `codex --model <m> -c
//! model_reasoning_effort=<e>`), measured against Claude Code 2.1.302 and
//! Codex 0.163.0, where a bare resume or fork fell back to the configured
//! default.

use super::*;
use crate::fake_herdr::FakeHerdr;
use crate::model::PaneLaunch;
use hide_agent_adapter::LaunchChoice;
use serde_json::json;

const TABS: [&str; 2] = ["w-order:t1", "w-order:t2"];
const AGENT: &str = "w-order:t2:p";
const SESSION: &str = "11111111-2222-3333-4444-555555555555";

fn choice(model: &str, effort: &str) -> LaunchChoice {
    LaunchChoice {
        model: Some(model.to_owned()),
        effort: Some(effort.to_owned()),
    }
}

/// Each kind's resume line and the words its launch choice adds after it.
fn resumed(kind: &str, launch: &[&str]) -> serde_json::Value {
    let resume: &[&str] = match kind {
        "codex" => &["resume", SESSION],
        _ => &["--resume", SESSION],
    };
    json!([resume, launch].concat())
}

fn event(kind: &str, payload: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "payload": payload
    }))
    .unwrap()
}

/// The two-tab session with t1 in front and, when `listed`, an idle `kind`
/// agent in t2 on [`SESSION`].
fn session(checkout: &str, kind: &str, listed: bool) -> SessionSnapshotPayload {
    let mut payload = tab_order_payload(checkout, &TABS, &TABS, "w-order:t1");
    if listed {
        let owned: SessionSnapshotPayload =
            crate::sidebar::owned_label_fixture(json!({"agents": [{
                "id": "reviewer", "pane_id": AGENT, "agent": kind,
                "agent_status": "idle", "state_change_seq": 4, "cwd": checkout,
                "agent_session": {"kind": "id", "value": SESSION},
                "tokens": {"task": "Review the parser"}
            }]}))
            .unwrap();
        payload.agents.extend(owned.agents);
    }
    payload
}

/// A live runtime on a real checkout folder whose t2 agent Hide started
/// with `launch` (none: an agent of an older build, or one started on the
/// CLI's defaults), on a machine whose Codex has no shared daemon.
fn started(kind: &str, launch: Option<&LaunchChoice>) -> (Runtime, tempfile::TempDir) {
    let folder = tempfile::tempdir().unwrap();
    let checkout = checkout_of(&folder);
    let (mut runtime, _) = live_tab_order_runtime(&checkout);
    let mut kit = runtime.kit_state(crate::node::TEST_NODE);
    kit.codex_daemon = Some(false);
    runtime.set_kit_state(crate::node::TEST_NODE, kit);
    if let Some(launch) = launch {
        // What the task start, the spawn executor and every restart do once
        // the agent is running in the pane.
        runtime.record_pane_launch(AGENT, kind, launch, None);
    }
    runtime.ingest_session(Ok(session(&checkout, kind, true)));
    (runtime, folder)
}

fn checkout_of(folder: &tempfile::TempDir) -> String {
    hide_platform::fs::identity::canonical(folder.path())
        .unwrap()
        .display()
        .to_string()
}

/// A fake Herdr that starts any agent in a pane whose shell holds the
/// terminal, and hands each `agent.start`'s arguments to the test.
fn starting_herdr(
    name: &str,
) -> (
    FakeHerdr,
    std::sync::mpsc::Receiver<(String, serde_json::Value)>,
) {
    let (sent, starts) = std::sync::mpsc::channel();
    let herdr = FakeHerdr::start_with_errors(name, move |method, params| match method {
        "pane.process_info" => Ok(json!({"type": "pane_process_info", "process_info": {
            "pane_id": params["pane_id"], "shell_pid": 42, "foreground_process_group_id": 42,
            "foreground_processes": [{"pid": 42, "name": "zsh"}]
        }})),
        "pane.split" => Ok(json!({"type": "pane_info", "pane": {
            "pane_id": "w-order:t2:fork", "terminal_id": "fork-terminal",
            "workspace_id": "w-order", "tab_id": "w-order:t2", "focused": false,
            "agent_status": "idle", "revision": 1
        }})),
        "agent.start" => {
            sent.send((
                params["pane_id"].as_str().unwrap().to_owned(),
                params["args"].clone(),
            ))
            .unwrap();
            Ok(json!({"type": "agent_started", "argv": [], "agent": {
                "pane_id": params["pane_id"], "terminal_id": "term", "workspace_id": "w-order",
                "tab_id": "w-order:t2", "focused": false, "agent_status": "idle", "revision": 2
            }}))
        }
        // What the fork's registration asks after its start is no part of
        // the start; refusing it leaves a working fork.
        other => Err(("unsupported".into(), format!("fixture answers no {other}"))),
    });
    (herdr, starts)
}

fn next_start(
    starts: &std::sync::mpsc::Receiver<(String, serde_json::Value)>,
) -> (String, serde_json::Value) {
    starts
        .recv_timeout(Duration::from_secs(10))
        .expect("the worker started the agent")
}

/// Puts the t2 agent to sleep and has Herdr forget it, as the end does.
fn sleep(runtime: &mut Runtime, folder: &tempfile::TempDir, kind: &str) {
    runtime.dispatch_json(&event("agent_sleep", json!({"pane_id": AGENT})));
    assert!(runtime.snapshot().status.last_error.is_none());
    assert!(runtime.ingest_agent_sleep_end(AGENT, Ok(4)));
    runtime.ingest_session(Ok(session(&checkout_of(folder), kind, false)));
}

fn wake(runtime: &mut Runtime, herdr: &FakeHerdr) {
    runtime.live.as_mut().unwrap().api_connector = Arc::new(herdr.connector());
    assert!(runtime.dispatch_json(&event("agent_wake", json!({"pane_id": AGENT}))));
}

#[test]
fn a_wake_resumes_the_agent_with_the_model_and_effort_it_was_started_with() {
    for (kind, launch, words) in [
        (
            "claude",
            choice("sonnet", "high"),
            ["--model", "sonnet", "--effort", "high"],
        ),
        (
            "codex",
            choice("gpt-6-luna", "low"),
            ["--model", "gpt-6-luna", "-c", "model_reasoning_effort=low"],
        ),
    ] {
        let (mut runtime, folder) = started(kind, Some(&launch));
        sleep(&mut runtime, &folder, kind);
        let (herdr, starts) = starting_herdr("launch-choice-wake");
        wake(&mut runtime, &herdr);
        assert_eq!(
            next_start(&starts),
            (AGENT.to_owned(), resumed(kind, &words)),
            "{kind}"
        );
        // The woken agent is the same conversation on the same choice, so
        // the next restart carries it again.
        assert_eq!(
            runtime.snapshot().ui_state.pane_launches.get(AGENT),
            Some(&PaneLaunch {
                kind: kind.to_owned(),
                choice: launch,
                session: Some(SESSION.to_owned()),
            }),
            "{kind}"
        );
    }
}

/// An agent of an older build left no launch record and a sleep record
/// without the field; it wakes on the resume alone, as before.
#[test]
fn a_wake_of_an_agent_with_no_recorded_choice_resumes_as_before() {
    let (mut runtime, folder) = started("claude", None);
    sleep(&mut runtime, &folder, "claude");
    let record = runtime
        .snapshot
        .ui_state
        .agent_sleep
        .records
        .get_mut(AGENT)
        .unwrap();
    let mut saved = serde_json::to_value(&*record).unwrap();
    assert!(
        saved.get("launch").is_none(),
        "no choice is written as none"
    );
    saved.as_object_mut().unwrap().remove("launch");
    *record = serde_json::from_value(saved).unwrap();
    let (herdr, starts) = starting_herdr("launch-choice-legacy-wake");
    wake(&mut runtime, &herdr);
    assert_eq!(next_start(&starts).1, resumed("claude", &[]));
    assert!(runtime.snapshot().ui_state.pane_launches.is_empty());
}

#[test]
fn a_fork_starts_with_the_model_and_effort_its_source_was_started_with() {
    for (kind, launch, resume, words) in [
        (
            "claude",
            choice("sonnet", "high"),
            vec!["--resume", SESSION, "--fork-session"],
            vec!["--model", "sonnet", "--effort", "high"],
        ),
        (
            "codex",
            choice("gpt-6-luna", "low"),
            vec!["fork", SESSION],
            vec!["--model", "gpt-6-luna", "-c", "model_reasoning_effort=low"],
        ),
    ] {
        let (runtime, _folder) = started(kind, Some(&launch));
        let (herdr, starts) = starting_herdr("launch-choice-fork");
        let shared = SharedRuntime::new(runtime);
        {
            let mut runtime = shared.lock().unwrap();
            let live = runtime.live.as_mut().unwrap();
            live.api_connector = Arc::new(herdr.connector());
            live.runtime = shared.weak();
            assert!(runtime.dispatch_json(&event(
                "fork_pane",
                json!({"pane_id": AGENT, "placement": "side"})
            )));
        }
        assert_eq!(
            next_start(&starts),
            (
                "w-order:t2:fork".to_owned(),
                json!([resume, words].concat())
            ),
            "{kind}"
        );
        wait(&shared, "the fork to settle", |runtime| {
            runtime.forks_in_flight.is_empty()
        });
        // The fork is a new conversation on its source's choice; the
        // record binds to the session the fork reports.
        assert_eq!(
            shared
                .lock()
                .unwrap()
                .snapshot()
                .ui_state
                .pane_launches
                .get("w-order:t2:fork"),
            Some(&PaneLaunch {
                kind: kind.to_owned(),
                choice: launch,
                session: None,
            }),
            "{kind}"
        );
    }
}

/// Closing a pane keeps its agent's choice with the closed pane, and the
/// reopen's answer records it on the pane the reopen made.
#[test]
fn a_closed_agent_takes_its_choice_to_the_pane_its_reopen_makes() {
    let launch = choice("sonnet", "high");
    let (mut runtime, _folder) = started("claude", Some(&launch));
    let tab = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .flat_map(|checkout| checkout.tabs.iter())
        .find(|tab| tab.id.as_deref() == Some("w-order:t2"))
        .cloned()
        .unwrap();
    let closed = runtime.closed_panes(&tab);
    assert_eq!(
        closed[0].agent,
        Some(ClosedAgent {
            kind: "claude".into(),
            session_id: Some(SESSION.into()),
            launch: launch.clone(),
        })
    );

    let item = closed_file("closed", "/repo/closed.rs");
    runtime.push_recent_closed(item.clone());
    runtime.reopen_in_flight = Some("closed".to_owned());
    let request = live::ReopenRequest {
        codex_daemon: Default::default(),
        item,
        workspace_exists: true,
        tab_exists: true,
        fallback_pane_id: None,
        owner: None,
    };
    assert!(runtime.ingest_reopen_result(
        &request,
        Ok(live::FileReopenResultOrHerdr::Herdr(live::ReopenOutcome {
            tab_id: None,
            consumed: true,
            focused_pane_id: None,
            notices: vec![],
            agents: vec![live::ReopenedAgent {
                pane_id: "w-order:t3:p".into(),
                kind: "claude".into(),
                launch: launch.clone(),
                session_id: Some(SESSION.into()),
            }],
        })),
    ));
    assert_eq!(
        runtime
            .snapshot()
            .ui_state
            .pane_launches
            .get("w-order:t3:p"),
        Some(&PaneLaunch {
            kind: "claude".into(),
            choice: launch,
            session: Some(SESSION.into()),
        })
    );
}

/// A record describes the conversation Hide started: it binds to the first
/// session the pane reports, and a pane that then hosts another session,
/// another kind, or no longer exists drops it, so an agent started there by
/// hand never inherits the choice.
#[test]
fn a_launch_record_binds_to_its_first_conversation_and_leaves_with_it() {
    let launch = choice("sonnet", "high");
    let (mut runtime, folder) = started("claude", Some(&launch));
    let checkout = checkout_of(&folder);
    let bound = PaneLaunch {
        kind: "claude".into(),
        choice: launch.clone(),
        session: Some(SESSION.into()),
    };
    assert_eq!(
        runtime.snapshot().ui_state.pane_launches.get(AGENT),
        Some(&bound)
    );

    let mut other = session(&checkout, "claude", true);
    other.agents[0].agent_session = Some(crate::sidebar::SessionAgentSessionPayload {
        kind: "id".into(),
        value: "99999999-2222-3333-4444-555555555555".into(),
    });
    runtime.ingest_session(Ok(other));
    assert!(runtime.snapshot().ui_state.pane_launches.is_empty());

    runtime.record_pane_launch(AGENT, "claude", &launch, None);
    runtime.ingest_session(Ok(session(&checkout, "codex", true)));
    assert!(runtime.snapshot().ui_state.pane_launches.is_empty());

    runtime.record_pane_launch(AGENT, "claude", &launch, Some(SESSION));
    runtime.ingest_session(Ok(tab_order_payload(
        &checkout,
        &TABS[..1],
        &TABS[..1],
        "w-order:t1",
    )));
    assert!(runtime.snapshot().ui_state.pane_launches.is_empty());
}

/// The record outlives the daemon, so a wake after a restart still has it.
#[test]
fn a_launch_record_survives_a_restart() {
    let launch = choice("gpt-6-luna", "low");
    let (mut runtime, _folder) = started("codex", Some(&launch));
    let path = runtime.state_path.clone();
    let _state = hold_dirs(&mut runtime);
    runtime.write_ui_state().unwrap();
    let saved = runtime.snapshot().ui_state.pane_launches.clone();
    assert_eq!(saved.len(), 1);
    drop(runtime);
    let restarted = Runtime::new(
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
            effects_held: false,
            machine_name: None,
            account_state_folder: false,
        },
        environment::EnvironmentReport {
            statuses: Vec::new(),
            home_path: None,
            codex_home: None,
        },
        Arc::new(hide_node::Local::of_process()),
        crate::node::test_devices(),
    );
    assert_eq!(restarted.snapshot().ui_state.pane_launches, saved);
}
