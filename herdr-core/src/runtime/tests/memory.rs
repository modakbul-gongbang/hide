use super::*;
use crate::model::{
    ArchiveDetailSnapshot, MemoryDetailSnapshot, MemoryRowSnapshot, RightPanelSection,
    SessionRowSnapshot, SessionsMode, SessionsProviderFilter,
};

fn session(id: &str, provider: &str, request: &str, updated_at_unix_ms: u64) -> SessionRowSnapshot {
    SessionRowSnapshot {
        id: id.to_owned(),
        provider: provider.to_owned(),
        provider_label: if provider == "codex" {
            "Codex"
        } else {
            "Claude Code"
        }
        .to_owned(),
        locator: format!("/fixture/{id}.jsonl"),
        checkout_path: "/fixture/project".to_owned(),
        first_human_request: Some(request.to_owned()),
        started_at_unix_ms: Some(updated_at_unix_ms.saturating_sub(1)),
        updated_at_unix_ms,
        title: None,
        unavailable_reason: None,
    }
}

fn memory(id: &str, body: &str) -> MemoryRowSnapshot {
    MemoryRowSnapshot {
        id: id.to_owned(),
        body: body.to_owned(),
        lifecycle: "active".to_owned(),
        revision: 1,
        source_count: 1,
        provided_session_count: 0,
        updated_at_unix_ms: 1,
    }
}

#[test]
fn focusing_a_checkout_refreshes_an_already_visible_sessions_panel() {
    let mut runtime = runtime();
    runtime.ingest_session(Ok(context_payload()));
    runtime.snapshot.ui_state.right_panel_visible = true;
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Sessions;
    runtime.snapshot.sessions.unavailable_reason =
        Some("Choose a local Project to view sessions".to_owned());
    let project = runtime.snapshot.navigator.workspaces[0].clone();

    assert!(runtime.focus_checkout(&project.id, &project.checkouts[0].id));

    assert_eq!(
        runtime.snapshot.sessions.unavailable_reason.as_deref(),
        Some("Session reader is unavailable")
    );
}

#[test]
fn a_catalog_change_refreshes_an_already_visible_sessions_panel() {
    let mut runtime = runtime();
    runtime.ingest_session(Ok(context_payload()));
    let project = runtime.snapshot.navigator.workspaces[0].clone();
    runtime.focus_checkout(&project.id, &project.checkouts[0].id);
    runtime.snapshot.ui_state.right_panel_visible = true;
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Sessions;
    runtime.snapshot.sessions.unavailable_reason =
        Some("Choose a local Project to view sessions".to_owned());

    assert!(runtime.refresh_sessions_after_catalog_change(true));

    assert_eq!(
        runtime.snapshot.sessions.unavailable_reason.as_deref(),
        Some("Session reader is unavailable")
    );
}

#[test]
fn reopening_an_archive_tab_replaces_its_cached_memory_detail() {
    let mut runtime = runtime();
    let detail = |revision: u64, body: &str| ArchiveDetailSnapshot {
        id: "memory-1".to_owned(),
        kind: "memory".to_owned(),
        title: "Memory".to_owned(),
        provider: None,
        unavailable_reason: None,
        events: Vec::new(),
        memory: Some(MemoryDetailSnapshot {
            id: "memory-1".to_owned(),
            body: body.to_owned(),
            lifecycle: "active".to_owned(),
            revision,
            source_count: 0,
            provided_session_count: 0,
            learned_at_unix_ms: 1,
            conflict_existing_id: None,
            conflict_candidate_id: None,
            sources: Vec::new(),
            revisions: Vec::new(),
        }),
    };

    runtime.show_archive_tab("workspace", "checkout", detail(1, "First"), true);
    runtime.show_archive_tab("workspace", "checkout", detail(2, "Updated"), false);

    let memory = runtime
        .snapshot
        .editor
        .archive_detail
        .as_ref()
        .and_then(|archive| archive.memory.as_ref())
        .expect("active memory detail");
    assert_eq!(memory.revision, 2);
    assert_eq!(memory.body, "Updated");
}

#[test]
fn session_filter_and_query_keep_the_catalogs_existing_order() {
    let mut runtime = runtime();
    runtime.session_catalog_rows = vec![
        session("new-codex", "codex", "Alpha decision", 30),
        session("claude", "claude", "Alpha review", 20),
        session("old-codex", "codex", "Beta work", 10),
    ];

    assert!(runtime.set_sessions_filter("codex", "alpha".to_owned()));
    assert_eq!(
        runtime.snapshot.sessions.provider_filter,
        SessionsProviderFilter::Codex
    );
    assert_eq!(
        runtime
            .snapshot
            .sessions
            .rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["new-codex"]
    );

    assert!(runtime.set_sessions_filter("all", "alpha".to_owned()));
    assert_eq!(
        runtime
            .snapshot
            .sessions
            .rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["new-codex", "claude"]
    );
}

#[test]
fn opening_memory_for_a_turn_is_one_bounded_panel_transition() {
    let mut runtime = runtime();
    runtime.snapshot.sessions.project_id = Some("project-1".to_owned());
    runtime.snapshot.sessions.query = "old query".to_owned();
    runtime.memory_catalog_rows = vec![
        memory("a", "Alpha"),
        memory("b", "Beta"),
        memory("c", "Gamma"),
        memory("d", "Delta"),
    ];

    assert!(runtime.open_memory_for_turn(vec![
        "d".to_owned(),
        "b".to_owned(),
        "a".to_owned(),
        "c".to_owned(),
        "a".to_owned(),
    ]));

    assert!(runtime.snapshot.ui_state.right_panel_visible);
    assert_eq!(
        runtime.snapshot.ui_state.right_panel_section,
        RightPanelSection::Sessions
    );
    assert_eq!(runtime.snapshot.sessions.mode, SessionsMode::Memory);
    assert_eq!(runtime.snapshot.sessions.query, "");
    assert_eq!(
        runtime.snapshot.sessions.this_turn_memory_ids,
        ["a", "b", "c"]
    );
    assert_eq!(
        runtime
            .snapshot
            .sessions
            .memories
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert_eq!(
        runtime
            .snapshot
            .ui_state
            .sessions_mode_by_project
            .get("project-1"),
        Some(&SessionsMode::Memory)
    );
}

#[test]
fn changing_modes_clears_the_turn_filter_and_persists_per_project() {
    let mut runtime = runtime();
    runtime.snapshot.sessions.project_id = Some("project-1".to_owned());
    runtime.snapshot.sessions.mode = SessionsMode::Memory;
    runtime.snapshot.sessions.this_turn_memory_ids = vec!["a".to_owned()];

    assert!(runtime.set_sessions_mode("sessions"));
    assert_eq!(runtime.snapshot.sessions.mode, SessionsMode::Sessions);
    assert!(runtime.snapshot.sessions.this_turn_memory_ids.is_empty());
    assert_eq!(
        runtime
            .snapshot
            .ui_state
            .sessions_mode_by_project
            .get("project-1"),
        Some(&SessionsMode::Sessions)
    );
}

#[test]
fn analysis_event_groups_preserve_every_event_exactly_once() {
    let events = (0..3)
        .map(|index| {
            serde_json::json!({
                "offset": index,
                "kind": "human",
                "text": format!("{index}{}", "x".repeat(18 * 1024)),
            })
        })
        .collect::<Vec<_>>();

    let groups = crate::runtime::memory::event_groups(&events).expect("bounded events group");
    let flattened = groups.into_iter().flatten().collect::<Vec<_>>();

    assert_eq!(flattened, events);
}

#[test]
fn one_analysis_event_over_the_group_budget_is_rejected() {
    let events = vec![serde_json::json!({
        "offset": 1,
        "kind": "human",
        "text": "x".repeat(37 * 1024),
    })];

    let groups = crate::runtime::memory::event_groups(&events).unwrap();
    assert!(groups.is_empty(), "the oversized event is quarantined");
}

#[test]
fn an_oversized_event_does_not_block_a_later_valid_turn() {
    let events = vec![
        serde_json::json!({"offset": 1, "text": "x".repeat(40 * 1024)}),
        serde_json::json!({"offset": 2, "text": "Keep later turns analyzable"}),
    ];
    let groups = crate::runtime::memory::event_groups(&events).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0][0]["offset"], 2);
}

#[test]
fn hook_repair_resumes_only_the_enable_intent_the_operator_approved() {
    let mut runtime = runtime();
    let diagnosis = |status| hide_agent_hooks::Diagnosis {
        runtimes: vec![hide_agent_hooks::diagnosis::RuntimeDiagnosis {
            runtime: hide_agent_hooks::AgentRuntime::Codex,
            label: "Codex".to_owned(),
            path: "/fixture/.codex/hooks.json".to_owned(),
            status,
            current_version: hide_agent_hooks::HOOK_VERSION,
            memory_compatibility: hide_agent_hooks::MemoryCompatibility::Supported {
                version: "999.0.0".to_owned(),
            },
        }],
        last_report_failure: None,
    };
    runtime.ingest_hook_diagnosis(diagnosis(hide_agent_hooks::HookStatus::NotInstalled));

    runtime.apply_memory_action(crate::runtime::events::MemoryActionPayload {
        action: "enable".to_owned(),
        item_id: None,
        candidate_id: None,
        body: None,
        batch_id: None,
        conflict_choice: None,
    });
    assert!(!runtime.memory_enable_after_hook_update);
    assert!(runtime.take_agent_hook_installs().is_empty());

    runtime.apply_memory_action(crate::runtime::events::MemoryActionPayload {
        action: "update_hooks".to_owned(),
        item_id: None,
        candidate_id: None,
        body: None,
        batch_id: None,
        conflict_choice: None,
    });
    assert!(runtime.memory_enable_after_hook_update);
    assert_eq!(
        runtime.take_agent_hook_installs(),
        [hide_agent_hooks::AgentRuntime::Codex]
    );

    runtime.ingest_hook_diagnosis(diagnosis(hide_agent_hooks::HookStatus::Installed {
        version: hide_agent_hooks::HOOK_VERSION,
    }));
    assert!(!runtime.memory_enable_after_hook_update);
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .expect("the approved enable intent resumed")
            .kind,
        "memory.project_unavailable"
    );
}

#[test]
fn one_unsupported_runtime_does_not_disable_another_supported_runtime() {
    let mut runtime = runtime();
    runtime.ingest_hook_diagnosis(hide_agent_hooks::Diagnosis {
        runtimes: vec![
            hide_agent_hooks::RuntimeDiagnosis {
                runtime: hide_agent_hooks::AgentRuntime::ClaudeCode,
                label: "Claude Code".to_owned(),
                path: "/fixture/.claude/settings.json".to_owned(),
                status: hide_agent_hooks::HookStatus::Installed {
                    version: hide_agent_hooks::HOOK_VERSION,
                },
                current_version: hide_agent_hooks::HOOK_VERSION,
                memory_compatibility: hide_agent_hooks::MemoryCompatibility::UpdateRequired {
                    installed_version: Some("2.1.277".to_owned()),
                    minimum_version: "2.1.278".to_owned(),
                },
            },
            hide_agent_hooks::RuntimeDiagnosis {
                runtime: hide_agent_hooks::AgentRuntime::Codex,
                label: "Codex".to_owned(),
                path: "/fixture/.codex/hooks.json".to_owned(),
                status: hide_agent_hooks::HookStatus::Installed {
                    version: hide_agent_hooks::HOOK_VERSION,
                },
                current_version: hide_agent_hooks::HOOK_VERSION,
                memory_compatibility: hide_agent_hooks::MemoryCompatibility::Supported {
                    version: "0.155.1".to_owned(),
                },
            },
        ],
        last_report_failure: None,
    });

    assert!(runtime.hooks_support_memory());
    assert_eq!(
        runtime.snapshot.status.agent_hooks.runtimes[0].headline,
        "Update required"
    );
    assert_eq!(
        runtime.snapshot.status.agent_hooks.runtimes[1].headline,
        "Installed (v3)"
    );
}
