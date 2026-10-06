//! Whether Hide hears each Claude Code and Codex session, counted per
//! machine and named per pane (PRD settings-cleanup B16, B17, B19, B26 to
//! B28, D-23). Expected answers come from the PRD, not from the projector.

use super::*;
use crate::model::{PaneConnectionReason, PaneConnectionSnapshot};
use hide_agent_hooks::{AgentRuntime, HookStatus};

/// One Herdr session with one pane per `(pane id, agent, hook token)`.
fn session(panes: &[(&str, &str, bool)]) -> serde_json::Value {
    let agents = panes
        .iter()
        .enumerate()
        .map(|(index, (pane, agent, _))| {
            serde_json::json!({
                "id": format!("agent-{pane}"), "pane_id": pane, "agent": agent,
                "agent_status": "idle", "state_change_seq": index + 1,
                "cwd": "/fixture", "workspace_label": "Fixture",
                "tokens": {"task": format!("Task of {pane}")},
            })
        })
        .collect::<Vec<_>>();
    let pane_rows = panes
        .iter()
        .map(|(pane, _, hooked)| {
            let tokens = if *hooked {
                serde_json::json!({"hide_hooks": hide_agent_hooks::HOOK_VERSION.to_string()})
            } else {
                serde_json::json!({})
            };
            serde_json::json!({"pane_id": pane, "cwd": "/fixture", "tokens": tokens})
        })
        .collect::<Vec<_>>();
    // One pane per tab: a tab of several panes needs an authoritative split.
    let tabs = (0..panes.len())
        .map(|index| serde_json::json!({"workspace_id": "w1", "tab_id": format!("t{index}"), "label": ""}))
        .collect::<Vec<_>>();
    let layouts = panes
        .iter()
        .enumerate()
        .map(|(index, (pane, _, _))| {
            serde_json::json!({
                "workspace_id": "w1", "tab_id": format!("t{index}"), "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": pane,
                "panes": [{"pane_id": pane, "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": [],
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "agents": agents,
        "panes": pane_rows,
        "tabs": tabs,
        "layouts": layouts,
    })
}

fn diagnosis(claude: HookStatus, codex: HookStatus) -> hide_agent_hooks::Diagnosis {
    let row = |runtime: AgentRuntime, status| hide_agent_hooks::diagnosis::RuntimeDiagnosis {
        runtime,
        label: runtime.label().to_owned(),
        path: "/fixture/hooks".to_owned(),
        status,
        current_version: hide_agent_hooks::HOOK_VERSION,
        memory_compatibility: hide_agent_hooks::MemoryCompatibility::Supported {
            version: "999.0.0".to_owned(),
        },
    };
    hide_agent_hooks::Diagnosis {
        runtimes: vec![
            row(AgentRuntime::ClaudeCode, claude),
            row(AgentRuntime::Codex, codex),
        ],
        last_report_failure: None,
    }
}

fn installed() -> HookStatus {
    HookStatus::Installed {
        version: hide_agent_hooks::HOOK_VERSION,
    }
}

/// This Mac's kit as the first check reported it: every adapter's row, on.
fn kit_rows(runtime: &mut Runtime, codex_daemon_on: Option<bool>) {
    let mut kit = runtime.kit_state(crate::workspace::LOCAL_DEVICE_ID);
    kit.codex_daemon = Some(true);
    kit.codex_daemon_on = codex_daemon_on;
    kit.agents = hide_kit::agents::ADAPTERS
        .iter()
        .map(|adapter| {
            let piece = crate::model::KitPieceSnapshot {
                state: hide_kit::ComponentState::Installed,
                reason: None,
                location: None,
            };
            crate::model::KitAgentSnapshot {
                id: adapter.id.to_owned(),
                label: adapter.label.to_owned(),
                availability: hide_kit::Availability::Available,
                enabled: true,
                skill: piece,
                hook: None,
                herdr: None,
                partial: adapter.partial(),
                features: Vec::new(),
                sessions: None,
                doc_url: String::new(),
            }
        })
        .collect();
    runtime.set_kit_state(crate::workspace::LOCAL_DEVICE_ID, kit);
}

fn sessions_of(runtime: &Runtime, agent: &str) -> Option<crate::model::KitAgentSessionsSnapshot> {
    runtime.snapshot.navigator.devices[0]
        .kit
        .agents
        .iter()
        .find(|row| row.id == agent)
        .unwrap_or_else(|| panic!("no kit row for {agent}"))
        .sessions
        .clone()
}

fn connection_of(runtime: &Runtime, pane_id: &str) -> Option<PaneConnectionSnapshot> {
    runtime
        .snapshot
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .flat_map(|checkout| checkout.tabs.iter())
        .flat_map(|tab| tab.panes.iter())
        .find(|pane| pane.id == pane_id)
        .and_then(|pane| pane.children.as_ref())
        .and_then(|children| children.connection)
}

fn feed(runtime: &mut Runtime, panes: &[(&str, &str, bool)]) {
    runtime.ingest_session(Ok(
        crate::sidebar::owned_label_fixture(session(panes)).expect("session payload")
    ));
}

#[test]
fn the_counts_follow_the_sessions_open_now_and_never_accumulate() {
    let mut runtime = runtime();
    kit_rows(&mut runtime, None);
    runtime.ingest_hook_diagnosis(diagnosis(installed(), installed()));

    feed(
        &mut runtime,
        &[
            ("w1:p1", "claude", true),
            ("w1:p2", "claude", false),
            ("w1:p3", "claude", false),
        ],
    );
    let claude = sessions_of(&runtime, "claude-code").expect("Claude Code is judged");
    assert_eq!(claude.connected, 1);
    assert_eq!(
        claude
            .not_connected
            .iter()
            .map(|session| (
                session.pane_id.as_str(),
                session.title.as_str(),
                session.project.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("w1:p2", "Task of w1:p2", "fixture"),
            ("w1:p3", "Task of w1:p3", "fixture")
        ]
    );
    assert!(
        claude
            .not_connected
            .iter()
            .all(|session| session.reason == PaneConnectionReason::StartedBeforeHide)
    );

    // Two sessions close: the count shrinks with them (D-23).
    feed(
        &mut runtime,
        &[("w1:p1", "claude", true), ("w1:p2", "claude", false)],
    );
    let claude = sessions_of(&runtime, "claude-code").unwrap();
    assert_eq!((claude.connected, claude.not_connected.len()), (1, 1));
    feed(&mut runtime, &[("w1:p1", "claude", true)]);
    let claude = sessions_of(&runtime, "claude-code").unwrap();
    assert_eq!((claude.connected, claude.not_connected.len()), (1, 0));
    feed(&mut runtime, &[("w1:p4", "claude", true)]);
    let claude = sessions_of(&runtime, "claude-code").unwrap();
    assert_eq!(
        (claude.connected, claude.not_connected.len()),
        (1, 0),
        "a different open session replaces the closed one"
    );
}

#[test]
fn an_agent_hide_cannot_hear_is_never_given_a_count() {
    let mut runtime = runtime();
    kit_rows(&mut runtime, None);
    runtime.ingest_hook_diagnosis(diagnosis(installed(), installed()));
    feed(
        &mut runtime,
        &[("w1:p1", "gemini", false), ("w1:p2", "claude", true)],
    );

    assert_eq!(sessions_of(&runtime, "gemini-cli"), None, "B19");
    assert_eq!(connection_of(&runtime, "w1:p1"), None);
    // An on agent with no session is "Ready": a count of zero, not an absent one.
    assert_eq!(
        sessions_of(&runtime, "codex"),
        Some(crate::model::KitAgentSessionsSnapshot::default())
    );
}

#[test]
fn a_codex_pane_on_the_shared_server_says_so_and_one_started_earlier_says_that() {
    let mut runtime = runtime();
    runtime.ingest_hook_diagnosis(diagnosis(installed(), installed()));
    kit_rows(&mut runtime, Some(true));
    feed(&mut runtime, &[("w1:p1", "codex", false)]);
    assert_eq!(
        connection_of(&runtime, "w1:p1"),
        Some(PaneConnectionSnapshot {
            connected: false,
            reason: Some(PaneConnectionReason::CodexSharedServer),
            reopen: None,
        })
    );
    let codex = sessions_of(&runtime, "codex").unwrap();
    assert_eq!(
        codex.not_connected[0].reason,
        PaneConnectionReason::CodexSharedServer
    );

    // The same pane with the shared server off is a session that started
    // before Hide's hook.
    kit_rows(&mut runtime, Some(false));
    feed(&mut runtime, &[("w1:p1", "codex", false)]);
    assert_eq!(
        connection_of(&runtime, "w1:p1").and_then(|connection| connection.reason),
        Some(PaneConnectionReason::StartedBeforeHide)
    );
}

#[test]
fn a_missing_hook_asks_for_setup_and_a_switched_off_agent_claims_nothing() {
    let mut runtime = runtime();
    kit_rows(&mut runtime, None);
    runtime.ingest_hook_diagnosis(diagnosis(HookStatus::NotInstalled, installed()));
    feed(&mut runtime, &[("w1:p1", "claude", false)]);
    assert_eq!(
        connection_of(&runtime, "w1:p1").and_then(|connection| connection.reason),
        Some(PaneConnectionReason::SetupNeeded),
        "the fix is the agent's row in Settings, not Reopen"
    );

    runtime.ingest_hook_diagnosis(diagnosis(HookStatus::Off, installed()));
    feed(&mut runtime, &[("w1:p1", "claude", false)]);
    assert_eq!(
        connection_of(&runtime, "w1:p1"),
        None,
        "B16: off has no state"
    );
}
