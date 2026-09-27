use super::workspace_view::{views_path, with_views};
use super::*;

fn setup() -> (Runtime, String) {
    let (runtime, checkout) = tab_order_runtime("/agent-groups");
    let mut runtime = with_views(runtime, &views_path("agent-groups"));
    ingest(&mut runtime, &["w-order:t1", "w-order:t2", "w-order:t3"]);
    (runtime, checkout)
}
fn ingest(runtime: &mut Runtime, tabs: &[&str]) {
    runtime.ingest_session(Ok(tab_order_payload(
        "/agent-groups",
        tabs,
        tabs,
        "w-order:t1",
    )));
    runtime.sync_workspace_view();
}
fn action(runtime: &mut Runtime, mut payload: serde_json::Value) {
    payload["workspace"] = serde_json::json!({"device_id":"local","path":"/agent-groups"});
    runtime.dispatch_json(&explorer_event("agent_layout", payload));
}
fn layout(runtime: &Runtime) -> crate::agent_layout::Layout {
    runtime
        .workspace_views
        .as_ref()
        .unwrap()
        .views
        .get("local", "/agent-groups")
        .unwrap()
        .agent_layout
        .clone()
}
#[test]
fn agent_split_shows_both_tabs_retains_their_attaches_and_preserves_herdr_order() {
    let (mut runtime, checkout) = setup();
    let order = ordered_tab_ids(&runtime, &checkout);
    action(
        &mut runtime,
        serde_json::json!({"action":"split","tab_id":"w-order:t2","area_id":"a1","edge":"right","request_id":"split-1"}),
    );
    assert_eq!(layout(&runtime).shown(), ["w-order:t1", "w-order:t2"]);
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some("w-order:t2")
    );
    assert_eq!(ordered_tab_ids(&runtime, &checkout), order);
    assert!(runtime.recent_visible_tabs.contains(&"w-order:t1".into()));
    assert!(runtime.recent_visible_tabs.contains(&"w-order:t2".into()));
    let before = layout(&runtime);
    action(
        &mut runtime,
        serde_json::json!({"action":"split","tab_id":"w-order:t2","area_id":"a1","edge":"right","request_id":"split-1"}),
    );
    assert_eq!(layout(&runtime), before);
    ingest(
        &mut runtime,
        &["w-order:t3", "w-order:t2", "w-order:t1", "w-order:t4"],
    );
    assert_eq!(layout(&runtime).active(), Some("w-order:t2"));
    assert_eq!(
        layout(&runtime).tree.areas()[0]
            .displays
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        ["w-order:t1", "w-order:t3"]
    );
}
#[test]
fn agent_last_member_departure_collapses_its_area_and_stale_workspace_does_nothing() {
    let (mut runtime, _) = setup();
    action(
        &mut runtime,
        serde_json::json!({"action":"split","tab_id":"w-order:t2","area_id":"a1","edge":"down","request_id":"split-1"}),
    );
    action(
        &mut runtime,
        serde_json::json!({"action":"move","tab_id":"w-order:t2","area_id":"a1","index":0}),
    );
    assert_eq!(layout(&runtime).tree.area_count(), 1);
    let before = layout(&runtime);
    runtime.dispatch_json(&explorer_event("agent_layout", serde_json::json!({"workspace":{"device_id":"local","path":"/other"},"action":"split","tab_id":"w-order:t1","area_id":"a1","edge":"right","request_id":"stale"})));
    assert_eq!(layout(&runtime), before);
}

#[test]
fn requested_new_tab_area_survives_a_focus_change_before_topology_arrives() {
    let (mut runtime, _) = setup();
    action(
        &mut runtime,
        serde_json::json!({"action":"split","tab_id":"w-order:t2","area_id":"a1","edge":"right","request_id":"split-new"}),
    );
    runtime.ingest_local_control_result(
        RemoteControlAction::CreateTab {
            workspace_id: "w-order".into(),
            cwd: "/agent-groups".into(),
            label: "Tab".into(),
            area_id: Some("a1".into()),
            admission_id: None,
        },
        Ok(RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("w-order:t4".into()),
            created_pane_id: Some("w-order:p4".into()),
        }),
        1,
    );
    ingest(
        &mut runtime,
        &["w-order:t1", "w-order:t2", "w-order:t3", "w-order:t4"],
    );
    let arranged = layout(&runtime);
    assert_eq!(
        arranged
            .tree
            .area("a1")
            .unwrap()
            .displays
            .last()
            .unwrap()
            .id,
        "w-order:t4"
    );
    assert_eq!(arranged.tree.active_area, "a1");
    assert!(
        runtime
            .workspace_views
            .as_ref()
            .unwrap()
            .agent_placements
            .is_empty()
    );
}

#[test]
fn replacement_predicate_requires_primary_provenance_a_linked_sibling_and_its_last_tab() {
    let (mut runtime, _) = setup();
    let primary = crate::domain::WorktreeProjection {
        repo_key: "repo".into(),
        repo_name: "repo".into(),
        repo_root: "/repo".into(),
        checkout_path: "/repo".into(),
        is_linked_worktree: false,
    };
    runtime
        .herdr_worktrees
        .insert("w-order".into(), primary.clone());
    runtime.herdr_worktrees.insert(
        "linked".into(),
        crate::domain::WorktreeProjection {
            is_linked_worktree: true,
            ..primary
        },
    );
    assert!(!runtime.primary_needs_shell("w-order", "w-order:t1"));
    runtime
        .herdr_workspace_tab_order
        .insert("w-order".into(), vec!["w-order:t1".into()]);
    assert!(runtime.primary_needs_shell("w-order", "w-order:t1"));
    assert!(!runtime.primary_needs_shell("linked", "w-order:t1"));
    runtime.herdr_worktrees.remove("linked");
    assert!(!runtime.primary_needs_shell("w-order", "w-order:t1"));
}

#[test]
fn external_focused_new_tab_preserves_canvas_then_existing_tab_focus_is_followed() {
    let (mut runtime, checkout) = setup();
    let before_pane = runtime.snapshot.terminal.pane_id.clone();
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3", "w-order:t4"];
    runtime.ingest_session(Ok(tab_order_payload(
        "/agent-groups",
        &tabs,
        &tabs,
        "w-order:t4",
    )));
    assert_eq!(layout(&runtime).active(), Some("w-order:t1"));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some("w-order:t1")
    );
    assert_eq!(runtime.snapshot.terminal.pane_id, before_pane);
    assert_eq!(layout(&runtime).tree.displays().count(), 4);
    runtime.ingest_session(Ok(tab_order_payload(
        "/agent-groups",
        &tabs,
        &tabs,
        "w-order:t2",
    )));
    runtime.ingest_session(Ok(tab_order_payload(
        "/agent-groups",
        &tabs,
        &tabs,
        "w-order:t4",
    )));
    runtime.sync_workspace_view();
    assert_eq!(layout(&runtime).active(), Some("w-order:t4"));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some("w-order:t4")
    );
}

#[test]
fn creation_provenance_preserves_canvas_after_admission_and_same_tab_refocus_is_followed() {
    let (mut runtime, checkout) = setup();
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3", "w-order:t4"];
    // Membership may already be known before a delayed creation focus arrives.
    ingest(&mut runtime, &tabs);
    let mut payload = tab_order_payload("/agent-groups", &tabs, &tabs, "w-order:t4");
    payload.tab_focus = Some(crate::sidebar::SessionTabFocus {
        generation: 1,
        workspace_id: "w-order".into(),
        tab_id: "w-order:t4".into(),
        revision: 1,
        creation: true,
    });
    runtime.ingest_session(Ok(payload.clone()));
    runtime.ingest_session(Ok(payload.clone()));
    assert_eq!(layout(&runtime).active(), Some("w-order:t1"));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some("w-order:t1")
    );
    let focus = payload.tab_focus.as_mut().unwrap();
    focus.creation = false;
    focus.revision = 2;
    runtime.ingest_session(Ok(payload));
    runtime.sync_workspace_view();
    assert_eq!(layout(&runtime).active(), Some("w-order:t4"));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some("w-order:t4")
    );
}

#[test]
fn raw_snapshot_does_not_rearm_a_consumed_focus_event_after_local_timeout() {
    let (mut runtime, checkout) = setup();
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    let mut event = tab_order_payload("/agent-groups", &tabs, &tabs, "w-order:t2");
    event.tab_focus = Some(crate::sidebar::SessionTabFocus {
        generation: 1,
        workspace_id: "w-order".into(),
        tab_id: "w-order:t2".into(),
        revision: 1,
        creation: false,
    });
    runtime.ingest_session(Ok(event.clone()));
    runtime.dispatch_json(&focus_tab_event(&checkout, "w-order:t1"));
    runtime.expire_pending_view_focus(u64::MAX);
    let mut raw = event.clone();
    raw.tab_focus = None;
    runtime.ingest_session(Ok(raw));
    runtime.ingest_session(Ok(event.clone()));
    runtime.sync_workspace_view();
    assert_eq!(layout(&runtime).active(), Some("w-order:t1"));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some("w-order:t1")
    );
    // A reconnect's new stream cannot collide with the previous revision.
    event.tab_focus.as_mut().unwrap().generation = 2;
    runtime.ingest_session(Ok(event));
    runtime.sync_workspace_view();
    assert_eq!(layout(&runtime).active(), Some("w-order:t2"));
}

#[test]
fn authoritative_overflow_waits_without_hidden_focus_and_admits_when_a_slot_opens() {
    let (mut runtime, checkout) = setup();
    let ids = (1..=65)
        .map(|n| format!("w-order:t{n}"))
        .collect::<Vec<_>>();
    let tabs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    runtime.ingest_session(Ok(tab_order_payload(
        "/agent-groups",
        &tabs,
        &tabs,
        "w-order:t65",
    )));
    runtime.sync_workspace_view();
    assert_eq!(layout(&runtime).tree.display_count(), 64);
    assert_eq!(layout(&runtime).waiting, 1);
    assert_eq!(
        runtime
            .snapshot
            .workspace_view
            .as_ref()
            .unwrap()
            .agent_layout
            .waiting,
        1
    );
    assert_eq!(ordered_tab_ids(&runtime, &checkout).len(), 65);
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some("w-order:t1")
    );
    assert_eq!(
        runtime.snapshot.terminal.pane_id.as_deref(),
        Some("w-order:t1:p")
    );
    runtime.dispatch_json(&focus_tab_event(&checkout, "w-order:t65"));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some("w-order:t1")
    );
    runtime.focus_pane("w-order:t65:p".into(), PaneFocusOrigin::Operator, None);
    assert_eq!(
        runtime.snapshot.terminal.pane_id.as_deref(),
        Some("w-order:t1:p")
    );
    let workspace = runtime
        .snapshot
        .navigator
        .focused_workspace_id
        .clone()
        .unwrap();
    runtime.apply(Event::CreateTab(CreateTabPayload {
        workspace_id: workspace,
        checkout_id: Some(checkout.clone()),
        label: "Tab".into(),
        area_id: None,
    }));
    // There is no live control worker in this fixture. Admission must fail
    // before even trying to obtain one or changing the current selection.
    assert_eq!(
        runtime.snapshot.status.last_error.as_ref().unwrap().kind,
        "agent_layout.display_limit"
    );
    assert!(
        runtime
            .workspace_views
            .as_ref()
            .unwrap()
            .agent_admissions
            .is_empty()
    );
    let remaining = tabs
        .iter()
        .copied()
        .filter(|id| *id != "w-order:t2")
        .collect::<Vec<_>>();
    runtime.ingest_session(Ok(tab_order_payload(
        "/agent-groups",
        &remaining,
        &remaining,
        "w-order:t65",
    )));
    runtime.sync_workspace_view();
    let arranged = layout(&runtime);
    assert_eq!(arranged.waiting, 0);
    assert_eq!(arranged.tree.display_count(), 64);
    assert_eq!(
        arranged.tree.active_area().displays.last().unwrap().id,
        "w-order:t65"
    );
    assert_eq!(arranged.active(), Some("w-order:t1"));
    let mut restored: crate::agent_layout::Layout =
        serde_json::from_value(serde_json::to_value(&arranged).unwrap()).unwrap();
    restored.repair();
    assert!(restored.tree.display("w-order:t65").is_some());
}

#[test]
fn pending_agent_admissions_are_counted_before_the_next_effect() {
    let (mut runtime, _) = setup();
    let ids = (1..=63)
        .map(|n| format!("w-order:t{n}"))
        .collect::<Vec<_>>();
    let tabs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    ingest(&mut runtime, &tabs);
    assert!(runtime.admit_agent_tab("/agent-groups"));
    runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .agent_admissions
        .insert(
            ("local".into(), "/agent-groups".into()),
            HashSet::from(["create:1".into()]),
        );
    assert!(!runtime.admit_agent_tab("/agent-groups"));
    runtime.finish_agent_admission("/agent-groups", 1);
    assert!(runtime.admit_agent_tab("/agent-groups"));
}

#[test]
fn protected_close_and_reopen_at_capacity_refuse_before_any_worker_or_selection_change() {
    let (mut runtime, _) = setup();
    let ids = (1..=64)
        .map(|n| format!("w-order:t{n}"))
        .collect::<Vec<_>>();
    let tabs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    ingest(&mut runtime, &tabs);
    let primary = crate::domain::WorktreeProjection {
        repo_key: "repo".into(),
        repo_name: "repo".into(),
        repo_root: "/agent-groups".into(),
        checkout_path: "/agent-groups".into(),
        is_linked_worktree: false,
    };
    runtime
        .herdr_worktrees
        .insert("w-order".into(), primary.clone());
    runtime.herdr_worktrees.insert(
        "linked".into(),
        crate::domain::WorktreeProjection {
            is_linked_worktree: true,
            ..primary
        },
    );
    // The other checkout tabs may belong to other Herdr workspaces. Its
    // protected primary workspace has only the target remaining.
    runtime
        .herdr_workspace_tab_order
        .insert("w-order".into(), vec!["w-order:t1".into()]);
    let tab = runtime
        .snapshot
        .navigator
        .workspaces
        .iter()
        .flat_map(|w| &w.checkouts)
        .flat_map(|c| &c.tabs)
        .find(|tab| tab.id.as_deref() == Some("w-order:t1"))
        .unwrap()
        .clone();
    let before = runtime.snapshot.terminal.pane_id.clone();
    let context = runtime.close_context(&tab).unwrap();
    assert!(context.replacement_shell);
    runtime.start_close_capture(
        live::CloseCaptureTarget::Tab {
            tab_id: "w-order:t1".into(),
        },
        tab.clone(),
    );
    assert!(runtime.close_operations.is_empty());
    assert!(runtime.panes_closing.is_empty());
    assert_eq!(
        runtime.snapshot.status.last_error.as_ref().unwrap().kind,
        "agent_layout.display_limit"
    );
    assert_eq!(runtime.snapshot.terminal.pane_id, before);
    runtime.push_recent_closed(ClosedItem::Tab {
        key: "capacity-reopen".into(),
        context,
        layout: crate::recent_closed::ClosedLayout {
            workspace_id: "w-order".into(),
            tab_id: "retired".into(),
            zoomed: false,
            focused_pane_id: "retired:p".into(),
            root: crate::recent_closed::ClosedLayoutNode::Pane {
                pane_id: Some("retired:p".into()),
                label: None,
                cwd: Some("/agent-groups".into()),
                command: None,
                env: BTreeMap::new(),
            },
        },
        panes: runtime.closed_panes(&tab),
    });
    runtime.reopen_closed();
    assert!(runtime.reopen_in_flight.is_none());
    assert_eq!(
        runtime.recent_closed.back().unwrap().key(),
        "capacity-reopen"
    );
    assert_eq!(
        runtime.snapshot.status.last_error.as_ref().unwrap().kind,
        "agent_layout.display_limit"
    );
}

#[test]
fn admitted_tab_keeps_its_slot_when_external_topology_arrives_first() {
    let (mut runtime, _) = setup();
    let mut ids = (1..=63)
        .map(|n| format!("w-order:t{n}"))
        .collect::<Vec<_>>();
    ingest(
        &mut runtime,
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .agent_admissions
        .insert(
            ("local".into(), "/agent-groups".into()),
            HashSet::from(["create:71".into()]),
        );
    ids.push("w-order:external".into());
    ingest(
        &mut runtime,
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_eq!(layout(&runtime).tree.display_count(), 63);
    assert_eq!(layout(&runtime).waiting, 1);
    runtime.ingest_local_control_result(
        RemoteControlAction::CreateTab {
            workspace_id: "w-order".into(),
            cwd: "/agent-groups".into(),
            label: "Tab".into(),
            area_id: Some("a1".into()),
            admission_id: Some(71),
        },
        Ok(RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("w-order:admitted".into()),
            created_pane_id: None,
        }),
        1,
    );
    // The known ID also retains its slot while still absent from topology.
    ingest(
        &mut runtime,
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_eq!(layout(&runtime).tree.display_count(), 63);
    ids.push("w-order:admitted".into());
    ingest(
        &mut runtime,
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert!(layout(&runtime).tree.display("w-order:admitted").is_some());
    assert!(layout(&runtime).tree.display("w-order:external").is_none());
    assert_eq!(layout(&runtime).waiting, 1);
}

#[test]
fn uncertain_create_retains_its_own_claim_until_a_definite_result() {
    let (mut runtime, _) = setup();
    let ids = (1..=62)
        .map(|n| format!("w-order:t{n}"))
        .collect::<Vec<_>>();
    let tabs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    ingest(&mut runtime, &tabs);
    runtime
        .workspace_views
        .as_mut()
        .unwrap()
        .agent_admissions
        .insert(
            ("local".into(), "/agent-groups".into()),
            HashSet::from(["create:71".into(), "create:72".into()]),
        );
    let create = |id| RemoteControlAction::CreateTab {
        workspace_id: "w-order".into(),
        cwd: "/agent-groups".into(),
        label: "Tab".into(),
        area_id: Some("a1".into()),
        admission_id: Some(id),
    };
    runtime.ingest_local_control_failure(
        create(71),
        Err(live::ControlFailure::Ambiguous(
            "lost acknowledgement".into(),
        )),
        1,
    );
    ingest(&mut runtime, &tabs);
    assert!(!runtime.admit_agent_tab("/agent-groups"));
    runtime.ingest_local_control_failure(
        create(72),
        Err(live::ControlFailure::Definite("refused".into())),
        1,
    );
    let pending = &runtime.workspace_views.as_ref().unwrap().agent_admissions;
    assert_eq!(
        pending
            .get(&("local".into(), "/agent-groups".into()))
            .unwrap(),
        &HashSet::from(["create:71".into()])
    );
    assert!(runtime.admit_agent_tab("/agent-groups"));
    runtime.ingest_local_control_result(
        create(71),
        Ok(RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("w-order:recovered".into()),
            created_pane_id: None,
        }),
        1,
    );
    assert!(
        runtime
            .workspace_views
            .as_ref()
            .unwrap()
            .agent_admissions
            .is_empty()
    );
    let mut recovered = tabs.clone();
    recovered.push("w-order:recovered");
    ingest(&mut runtime, &recovered);
    assert_eq!(layout(&runtime).tree.display_count(), 63);
    assert!(runtime.admit_agent_tab("/agent-groups"));
}

#[test]
fn replacement_retry_with_missing_shell_at_capacity_sends_no_effect() {
    let (mut runtime, _) = setup();
    let ids = (1..=64)
        .map(|n| format!("w-order:t{n}"))
        .collect::<Vec<_>>();
    ingest(
        &mut runtime,
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let tab = runtime
        .snapshot
        .navigator
        .workspaces
        .iter()
        .flat_map(|w| &w.checkouts)
        .flat_map(|c| &c.tabs)
        .find(|tab| tab.id.as_deref() == Some("w-order:t1"))
        .unwrap()
        .clone();
    let mut context = runtime.close_context(&tab).unwrap();
    context.replacement_shell = true;
    let request = live::CloseCaptureRequest {
        key: "retained-close".into(),
        connection_generation: runtime.live_generation,
        context,
        panes: runtime.closed_panes(&tab),
        target: live::CloseCaptureTarget::Tab {
            tab_id: "w-order:t1".into(),
        },
    };
    runtime.ensure_pending_close_from_request(&request);
    let operation = runtime.close_operations.get_mut("retained-close").unwrap();
    operation.phase = "refused".into();
    operation.stage = "close_request".into();
    operation.replacement_tab_id = Some("w-order:removed-shell".into());
    assert!(runtime.retry_agent_close("retained-close"));
    assert_eq!(
        runtime.snapshot.status.last_error.as_ref().unwrap().kind,
        "agent_layout.display_limit"
    );
    assert_eq!(runtime.close_operations["retained-close"].phase, "refused");
    assert!(runtime.panes_closing.is_empty());
}

fn capacity_close_request(runtime: &Runtime, key: &str) -> live::CloseCaptureRequest {
    let tab = runtime
        .snapshot
        .navigator
        .workspaces
        .iter()
        .flat_map(|w| &w.checkouts)
        .flat_map(|c| &c.tabs)
        .find(|tab| tab.id.as_deref() == Some("w-order:t1"))
        .unwrap();
    let mut context = runtime.close_context(tab).unwrap();
    context.replacement_shell = true;
    live::CloseCaptureRequest {
        key: key.into(),
        connection_generation: runtime.live_generation,
        context,
        panes: runtime.closed_panes(tab),
        target: live::CloseCaptureTarget::Tab {
            tab_id: "w-order:t1".into(),
        },
    }
}

#[test]
fn replacement_unknown_claim_survives_refusal_and_dismissal() {
    let (mut runtime, _) = setup();
    let ids = (1..=63)
        .map(|n| format!("w-order:t{n}"))
        .collect::<Vec<_>>();
    let tabs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    ingest(&mut runtime, &tabs);
    let request = capacity_close_request(&runtime, "uncertain-replacement");
    assert!(runtime.reserve_agent_effect("/agent-groups", "close:uncertain-replacement"));
    runtime.ensure_pending_close_from_request(&request);
    let operation = runtime.close_operations.get_mut(&request.key).unwrap();
    operation.phase = "transmitting".into();
    operation.replacement_effect_started = true;
    runtime.ingest_close_effect_result(
        &live::CloseEffectRequest {
            key: request.key.clone(),
            connection_generation: runtime.live_generation,
            target: request.target.clone(),
            replacement: Some(request.context.clone()),
            allow_replacement_create: true,
        },
        Err(hide_herdr_client::ApiError::Remote {
            code: "replacement_failed".into(),
            message: "created but acknowledgement lost".into(),
        }),
    );
    ingest(&mut runtime, &tabs);
    assert!(!runtime.admit_agent_tab("/agent-groups"));
    assert!(
        runtime.reserve_agent_effect("/agent-groups", "close:uncertain-replacement"),
        "the same intent reuses its own claim"
    );
    runtime.dismiss_agent_close(&request.key);
    assert!(
        !runtime.admit_agent_tab("/agent-groups"),
        "dismiss is not proof that the effect did not happen"
    );
}

#[test]
fn reopen_unknown_claim_survives_error_and_retry_launch_failure_then_transfers_to_placement() {
    let (mut runtime, _) = setup();
    let ids = (1..=63)
        .map(|n| format!("w-order:t{n}"))
        .collect::<Vec<_>>();
    let tabs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    ingest(&mut runtime, &tabs);
    let close = capacity_close_request(&runtime, "uncertain-reopen");
    let item = ClosedItem::Tab {
        key: close.key.clone(),
        context: close.context,
        panes: close.panes,
        layout: crate::recent_closed::ClosedLayout {
            workspace_id: "w-order".into(),
            tab_id: "retired".into(),
            zoomed: false,
            focused_pane_id: "retired:p".into(),
            root: crate::recent_closed::ClosedLayoutNode::Pane {
                pane_id: Some("retired:p".into()),
                label: None,
                cwd: Some("/agent-groups".into()),
                command: None,
                env: BTreeMap::new(),
            },
        },
    };
    runtime.push_recent_closed(item.clone());
    let request = live::ReopenRequest {
        item,
        workspace_exists: true,
        tab_exists: false,
        fallback_pane_id: None,
    };
    assert!(runtime.reserve_agent_effect("/agent-groups", "reopen:uncertain-reopen"));
    runtime.reopen_in_flight = Some(close.key.clone());
    runtime.ingest_reopen_result(
        &request,
        Err("layout applied but acknowledgement lost".into()),
    );
    ingest(&mut runtime, &tabs);
    assert!(!runtime.admit_agent_tab("/agent-groups"));
    runtime.reopen_closed(); // No fixture live worker: this retry cannot start.
    assert!(!runtime.admit_agent_tab("/agent-groups"));
    runtime.reopen_in_flight = Some(close.key);
    runtime.ingest_reopen_result(
        &request,
        Ok(live::FileReopenResultOrHerdr::Herdr(live::ReopenOutcome {
            tab_id: Some("w-order:restored".into()),
            consumed: true,
            focused_pane_id: None,
            notices: vec![],
        })),
    );
    assert_eq!(runtime.pending_agent_admissions("/agent-groups"), 0);
    assert!(
        !runtime.admit_agent_tab("/agent-groups"),
        "known placement keeps the slot until topology arrives"
    );
    let mut restored = tabs;
    restored.push("w-order:restored");
    ingest(&mut runtime, &restored);
    assert_eq!(layout(&runtime).waiting, 0);
    assert!(layout(&runtime).tree.display("w-order:restored").is_some());
}

#[test]
fn cross_machine_lineage_updates_agent_tab_placement_in_every_arrival_order() {
    // B16: a resolved child is a sidebar canvas, never a normal Agent tab.
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let (mut runtime, _) = setup();
        let mut local = tab_order_payload(
            "/agent-groups",
            &["w-order:t1", "w-order:t2"],
            &["w-order:t1", "w-order:t2"],
            "w-order:t1",
        );
        local.agents = serde_json::from_value(serde_json::json!([{
            "id":"child", "pane_id":"w-order:t2:p", "agent_status":"working",
            "state_change_seq":1, "spawned_from_pane_id":"parent",
            "spawned_from_machine_id":"machine-mini", "tokens":{"task":"Child"}
        }]))
        .unwrap();
        let remote = RemoteSessionSnapshot {
            workspaces: vec![],
            agents: project_agents(
                serde_json::from_value(serde_json::json!({"agents":[{
                    "id":"parent", "pane_id":"remote:mini:pane:parent", "agent_status":"working",
                    "state_change_seq":1, "tokens":{"task":"Parent"}
                }]}))
                .unwrap(),
            )
            .agents,
            active_tab_ids: Default::default(),
            focused_workspace_id: None,
            focused_checkout_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            pane_layouts: vec![],
        };
        runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
            target_id: "mini".into(),
            state: "connected".into(),
            message: None,
            herdr_version: Some("0.9.1".into()),
            session: None,
            files: RemoteFileListSnapshot::idle(),
            catalog: Default::default(),
        });
        for step in order {
            match step {
                0 => {
                    runtime.ingest_session(Ok(local.clone()));
                }
                1 => {
                    runtime.ingest_remote_session("mini", Ok(remote.clone()));
                }
                _ => {
                    runtime.ingest_device_machine_id("mini", Ok("machine-mini".into()));
                }
            }
        }
        let assert_placement = |runtime: &Runtime, delegated: bool| {
            let child = runtime
                .snapshot
                .navigator
                .agents
                .iter()
                .find(|agent| agent.pane_id == "w-order:t2:p")
                .unwrap();
            assert_eq!(child.delegated, delegated, "arrival order {order:?}");
            let checkout = runtime
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|w| &w.checkouts)
                .find(|c| c.path == "/agent-groups")
                .unwrap();
            let tab = checkout
                .tabs
                .iter()
                .find(|t| t.id.as_deref() == Some("w-order:t2"))
                .unwrap();
            assert_eq!(tab.delegated, delegated, "tab ownership {order:?}");
            assert_eq!(
                checkout.strip.iter().any(|t| t.source_id == "w-order:t2"),
                !delegated
            );
            assert_eq!(
                layout(runtime).tree.display("w-order:t2").is_some(),
                !delegated
            );
        };
        assert_placement(&runtime, true);
        runtime.dispatch_json(&operator_focus_event("w-order:t2:p"));
        runtime.sync_workspace_view();
        assert_eq!(layout(&runtime).active(), Some("w-order:t2"));
        assert!(layout(&runtime).tree.display("w-order:t2").is_none());
        runtime.ingest_device_machine_id("mini", Err("disconnected".into()));
        assert_placement(&runtime, false);
        runtime.ingest_device_machine_id("mini", Ok("machine-mini".into()));
        assert_placement(&runtime, true);
        // A later local topology pass must not temporarily re-admit the child.
        runtime.ingest_session(Ok(local));
        assert_placement(&runtime, true);
    }
}
