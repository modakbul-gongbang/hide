// PRD AC1-4: assertions describe the operator's tree and persisted choice,
// independently of how the projector builds its parent index.
fn lineage_rows() -> Vec<SidebarAgentSnapshot> {
    project_agents(serde_json::from_value(serde_json::json!({"agents": [
        {"id":"Parent","pane_id":"parent","agent_status":"working","state_change_seq":1},
        {"id":"Child","pane_id":"child","spawned_from_pane_id":"parent","agent_status":"working","state_change_seq":2},
        {"id":"Grandchild","pane_id":"grandchild","spawned_from_pane_id":"child","agent_status":"working","state_change_seq":3},
        {"id":"Newer sibling","pane_id":"sibling","spawned_from_pane_id":"parent","agent_status":"working","state_change_seq":4}
    ]})).unwrap()).agents
}

fn lineage_workspaces() -> Vec<WorkspaceSnapshot> {
    let main = checkout(
        "project",
        "main",
        "/fixture/main",
        Some(pane("parent", "/fixture/main")),
    );
    let mut feature = checkout(
        "project",
        "feature",
        "/fixture/feature",
        Some(pane("child", "/fixture/feature")),
    );
    feature.tabs[0].panes.extend([
        pane("grandchild", "/fixture/feature"),
        pane("sibling", "/fixture/feature"),
    ]);
    vec![workspace(
        "project",
        "Project",
        "/fixture/main",
        vec![main, feature],
    )]
}

#[test]
fn lineage_cross_checkout_tree_and_orphan_keep_the_canonical_rows_and_axes() {
    let mut rows = lineage_rows();
    let mut records = std::collections::BTreeMap::new();
    crate::sidebar::apply_read_state(
        &mut rows,
        &mut records,
        Some("child"),
        ReadRecordScope::Local,
    );
    let original = rows
        .iter()
        .map(|row| {
            (
                row.pane_id.clone(),
                row.demand.clone(),
                row.unread,
                row.group.clone(),
            )
        })
        .collect::<Vec<_>>();
    let ledger = records.clone();
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    let row = |id: &str| rows.iter().find(|row| row.pane_id == id).unwrap();
    assert_eq!(row("parent").lineage_child_pane_ids, ["sibling", "child"]);
    assert_eq!(row("child").lineage_depth, 1);
    assert_eq!(row("grandchild").lineage_depth, 2);
    assert_eq!(
        row("child").lineage_root_checkout_id.as_deref(),
        Some("main")
    );
    assert_eq!(
        row("child").lineage_worktree_badge.as_deref(),
        Some("feature")
    );
    assert_eq!(row("grandchild").lineage_worktree_badge, None);
    assert_eq!(
        original,
        rows.iter()
            .map(|row| (
                row.pane_id.clone(),
                row.demand.clone(),
                row.unread,
                row.group.clone()
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(ledger, records);
    rows.retain(|row| row.pane_id != "parent");
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    let child = rows.iter().find(|row| row.pane_id == "child").unwrap();
    assert_eq!(child.lineage_depth, 0);
    assert_eq!(child.lineage_root_checkout_id.as_deref(), Some("feature"));
    assert!(child.lineage_orphan);
    // The parent is gone, so there is no name to give and the line says that
    // rather than printing the pane id it was keyed by. It used to read
    // "↳ from parent", which looked like a name and was an internal handle.
    assert_eq!(
        child.lineage_hint.as_deref(),
        Some("↳ from an agent that has since ended")
    );
    assert_eq!(child.spawn_origin_pane_id, None);
}

#[test]
fn lineage_depth_is_not_capped_and_cycles_are_visible_orphans() {
    let mut values = Vec::new();
    for index in 0..64 {
        values.push(
            serde_json::json!({"pane_id": format!("p{index}"), "state_change_seq": index,
            "spawned_from_pane_id": (index > 0).then(|| format!("p{}", index - 1))}),
        );
    }
    let mut rows =
        project_agents(serde_json::from_value(serde_json::json!({"agents":values})).unwrap())
            .agents;
    crate::sidebar::apply_lineage(&mut rows, &[], &[]);
    assert_eq!(rows[63].lineage_depth, 63);
    rows[0].spawned_from_pane_id = Some("p1".into());
    crate::sidebar::apply_lineage(&mut rows, &[], &[]);
    assert!(rows[0].lineage_orphan && rows[1].lineage_orphan);
    assert_eq!(rows[2].lineage_depth, 1);
}

#[test]
fn lineage_collapse_persists_without_attention_expanding_it_and_prunes_on_disappearance() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = lineage_workspaces();
    runtime.snapshot.navigator.agents = lineage_rows();
    runtime.refresh_agent_lineage();
    assert!(runtime.dispatch_json(
        br#"{"schema_version":2,"kind":"agent_tree_toggle","payload":{"pane_id":"parent"}}"#
    ));
    assert_eq!(
        runtime.snapshot.ui_state.collapsed_agent_pane_ids,
        ["parent"]
    );
    let (restored, _, disposition) = persistence::load(&runtime.state_path);
    assert_eq!(disposition, persistence::LoadDisposition::Loaded);
    assert_eq!(restored.collapsed_agent_pane_ids, ["parent"]);
    let options = CoreOptions {
        schema_version: SCHEMA_VERSION,
        herdr_socket_path: None,
        herdr_bin_path: None,
        remote_targets: vec![],
        app_state_path: runtime.state_path.to_string_lossy().into_owned(),
    };
    let restarted = Runtime::new(
        options,
        environment::EnvironmentReport {
            statuses: vec![],
            remote_enabled: false,
            chromux_enabled: false,
            herdr_socket_path_override: None,
            home_path: None,
        },
    );
    assert_eq!(
        restarted.snapshot.ui_state.collapsed_agent_pane_ids,
        ["parent"]
    );
    let mut attention = lineage_rows();
    attention
        .iter_mut()
        .find(|row| row.pane_id == "child")
        .unwrap()
        .demand = "question".into();
    crate::sidebar::apply_read_state(
        &mut attention,
        &mut runtime.snapshot.ui_state.pane_read_records,
        None,
        ReadRecordScope::Local,
    );
    crate::sidebar::apply_lineage(
        &mut attention,
        &lineage_workspaces(),
        &restored.collapsed_agent_pane_ids,
    );
    assert!(
        attention
            .iter()
            .find(|row| row.pane_id == "parent")
            .unwrap()
            .lineage_collapsed
    );
    // PRD B15, D-35: the child's question is the parent's problem. It keeps
    // the hint that says whose child asked it, and it stays out of the
    // operator's Needs You, which is what makes the delegation real.
    assert!(
        attention.iter().all(|row| row.group != "needs_you"),
        "a delegated child's question never raises the operator's own group"
    );
    let asking = attention
        .iter()
        .find(|row| row.pane_id == "child")
        .expect("the child is still a row");
    assert_eq!(asking.demand, "question");
    assert_eq!(asking.raised_hint.as_deref(), Some("↳ from Parent"));
    assert!(asking.delegated && !asking.emphasized);
    assert_eq!(
        runtime.snapshot.ui_state.collapsed_agent_pane_ids,
        ["parent"]
    );
    let snapshot_state = serde_json::to_value(&runtime.snapshot.ui_state).unwrap();
    assert!(
        runtime.dispatch_json(
            &serde_json::to_vec(&serde_json::json!({
                "schema_version": 2, "kind": "ui_state_update", "payload": snapshot_state
            }))
            .unwrap()
        )
    );
    assert_eq!(
        runtime.snapshot.ui_state.collapsed_agent_pane_ids,
        ["parent"]
    );
    let mut scoped = vec!["parent".to_owned(), "remote:mini:p1".to_owned()];
    assert!(!crate::sidebar::prune_lineage_collapse(
        &mut scoped,
        &[],
        ReadRecordScope::Retain
    ));
    assert!(crate::sidebar::prune_lineage_collapse(
        &mut scoped,
        &[],
        ReadRecordScope::Local
    ));
    assert_eq!(scoped, ["remote:mini:p1"]);
    // Fresh ingestion is the production cleanup boundary, not the toggle.
    runtime.ingest_session(Ok(
        serde_json::from_value(serde_json::json!({"agents":[]})).unwrap()
    ));
    assert!(
        runtime
            .snapshot
            .ui_state
            .collapsed_agent_pane_ids
            .is_empty()
    );
    let (restored, _, _) = persistence::load(&runtime.state_path);
    assert!(restored.collapsed_agent_pane_ids.is_empty());
    let _ = std::fs::remove_file(&runtime.state_path);
}

// PRD B11, B13, B14, D-36, D-52: ownership is the fourth derived axis, and it
// is read off the lineage rather than stored anywhere.
#[test]
fn ownership_marks_descendants_delegated_and_hands_an_orphan_back_to_the_operator() {
    let mut rows = lineage_rows();
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    let row = |rows: &[SidebarAgentSnapshot], id: &str| {
        rows.iter().find(|row| row.pane_id == id).unwrap().clone()
    };
    assert!(!row(&rows, "parent").delegated, "a lineage root is the operator's own work");
    assert!(row(&rows, "child").delegated);
    assert!(row(&rows, "grandchild").delegated);
    // The delegation source the row already carried is what names the owner.
    assert_eq!(row(&rows, "child").raised_hint.as_deref(), Some("↳ from Parent"));

    rows.retain(|row| row.pane_id != "parent");
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    let orphan = row(&rows, "child");
    assert!(!orphan.delegated, "a lost parent returns ownership to the operator");
    assert!(orphan.lineage_orphan);
    assert!(row(&rows, "grandchild").delegated, "its own descendants stay delegated");
}

#[test]
fn a_lost_origin_is_named_as_ended_rather_than_shown_as_a_pane_id() {
    let mut rows = lineage_rows();
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    let row = |rows: &[SidebarAgentSnapshot], id: &str| {
        rows.iter().find(|row| row.pane_id == id).unwrap().clone()
    };
    // While the origin is an agent Hide can see, the line names it and
    // carries the pane, which is what the shell turns into a jump.
    let child = row(&rows, "child");
    assert_eq!(child.raised_hint.as_deref(), Some("↳ from Parent"));
    assert_eq!(child.spawn_origin_pane_id.as_deref(), Some("parent"));

    // Once that agent is gone the line says so. A pane id is an internal
    // handle, not a name, and it is never rendered as one; with nothing to
    // go to, no destination is offered either.
    rows.retain(|row| row.pane_id != "parent");
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    let orphan = row(&rows, "child");
    assert_eq!(
        orphan.lineage_hint.as_deref(),
        Some("↳ from an agent that has since ended")
    );
    assert_eq!(orphan.spawn_origin_pane_id, None);
    for row in &rows {
        let shown = row.lineage_hint.iter().chain(row.raised_hint.iter());
        for hint in shown {
            assert!(
                !hint.contains("parent"),
                "a pane id never reaches the operator as a name: {hint}"
            );
        }
    }
}

// PRD B7, B9, B10, D-18, D-19: the breadcrumb and its per-step sibling list
// are derived every time from the lineage, so nothing can go stale.
#[test]
fn the_breadcrumb_path_and_each_steps_siblings_come_out_of_the_lineage() {
    let mut rows = lineage_rows();
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    let row = |id: &str| rows.iter().find(|row| row.pane_id == id).unwrap();

    assert_eq!(row("parent").lineage_path_pane_ids, Vec::<String>::new());
    assert_eq!(row("child").lineage_path_pane_ids, ["parent"]);
    assert_eq!(row("grandchild").lineage_path_pane_ids, ["parent", "child"]);
    assert_eq!(
        row("grandchild").lineage_parent_pane_id.as_deref(),
        Some("child")
    );

    // A step's dropdown offers that layer, in the parent's own child order,
    // and includes the step itself so the current position is visible.
    assert_eq!(row("child").lineage_sibling_pane_ids, ["sibling", "child"]);
    assert_eq!(row("sibling").lineage_sibling_pane_ids, ["sibling", "child"]);
    assert_eq!(row("grandchild").lineage_sibling_pane_ids, ["grandchild"]);
    assert_eq!(
        row("parent").lineage_sibling_pane_ids,
        Vec::<String>::new(),
        "the layer above a root is the sidebar, not the breadcrumb"
    );
}

// PRD D-18: the path is a function of the current list, so a child exiting
// leaves every surviving row's breadcrumb correct with no stored state to fix.
#[test]
fn a_departed_ancestor_shortens_every_descendants_path_on_the_next_projection() {
    let mut rows = lineage_rows();
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    assert_eq!(
        rows.iter().find(|row| row.pane_id == "grandchild").unwrap().lineage_path_pane_ids,
        ["parent", "child"]
    );
    rows.retain(|row| row.pane_id != "child");
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    let grandchild = rows.iter().find(|row| row.pane_id == "grandchild").unwrap();
    assert_eq!(grandchild.lineage_path_pane_ids, Vec::<String>::new());
    assert_eq!(grandchild.lineage_parent_pane_id, None);
    assert!(!grandchild.delegated, "an orphaned grandchild is a root of its own");
}

// PRD B5, B21-B24, B32, D-30, D-63: what a pane header is allowed to say
// about the work its agent delegated.
fn hook_status_of(
    status: Option<hide_agent_hooks::HookStatus>,
) -> impl Fn(hide_agent_hooks::AgentRuntime) -> Option<hide_agent_hooks::HookStatus> {
    move |_| status.clone()
}

fn instrumented_rows(agent_kind: &str) -> Vec<SidebarAgentSnapshot> {
    let mut rows = project_agents(
        serde_json::from_value(serde_json::json!({"agents": [
            {"id":"Observer","pane_id":"parent","agent":agent_kind,"agent_status":"working","state_change_seq":1},
            {"id":"Worker","pane_id":"child","agent":agent_kind,"spawned_from_pane_id":"parent",
             "agent_status":"idle","state_change_seq":2,"tokens":{"status_error_new":"x"}},
            {"id":"Runner","pane_id":"sibling","agent":agent_kind,"spawned_from_pane_id":"parent",
             "agent_status":"working","state_change_seq":3}
        ]}))
        .unwrap(),
    )
    .agents;
    crate::sidebar::apply_lineage(&mut rows, &[], &[]);
    rows
}

#[test]
fn a_pane_with_children_lists_them_and_names_the_one_that_speaks_for_them() {
    let rows = instrumented_rows("claude");
    let installed = hook_status_of(Some(hide_agent_hooks::HookStatus::Installed {
        version: hide_agent_hooks::HOOK_VERSION,
    }));
    let tokens = crate::agent_hooks::PaneHookTokens {
        version: Some(hide_agent_hooks::HOOK_VERSION),
        working: Some(2),
        done: Some(4),
        blocked: None,
    };
    let children =
        crate::sidebar::project_pane_children(&rows, "parent", tokens, &installed).unwrap();

    assert!(children.instrumented);
    assert_eq!(children.uninstrumented_reason, None);
    let labels: Vec<_> = children.chips.iter().map(|chip| chip.label.as_str()).collect();
    assert_eq!(labels, ["Runner", "Worker"], "chips follow the lineage's own child order");
    assert!(children.chips.iter().all(|chip| chip.delegated));
    // An unread error outranks a working sibling, by the same rule the
    // Workspace summary chip uses.
    assert_eq!(children.representative.as_ref().unwrap().label, "Worker");
    assert_eq!(children.representative.as_ref().unwrap().symbol, "\u{d7}");

    // In-process subagents are summarised separately and never folded into
    // the chip count.
    assert_eq!(children.subagents.working, Some(2));
    assert_eq!(children.subagents.done, Some(4));
    assert_eq!(children.subagents.blocked, None);
    assert_eq!(children.chips.len(), 2, "two pane children, whatever the subagent count says");
}

#[test]
fn an_instrumented_pane_with_no_children_is_a_different_answer_from_one_hide_cannot_see() {
    let rows = instrumented_rows("claude");
    let installed = hook_status_of(Some(hide_agent_hooks::HookStatus::Installed {
        version: hide_agent_hooks::HOOK_VERSION,
    }));
    let reported = crate::agent_hooks::PaneHookTokens {
        version: Some(hide_agent_hooks::HOOK_VERSION),
        working: Some(0),
        done: Some(0),
        blocked: None,
    };
    let alone =
        crate::sidebar::project_pane_children(&rows, "child", reported, &installed).unwrap();
    assert!(alone.instrumented);
    assert!(alone.chips.is_empty());
    assert!(alone.subagents.is_silent());
    assert_eq!(alone.uninstrumented_reason, None);

    // The same pane before its session ever ran the hook.
    let silent = crate::sidebar::project_pane_children(
        &rows,
        "child",
        crate::agent_hooks::PaneHookTokens::default(),
        &installed,
    )
    .unwrap();
    assert!(!silent.instrumented);
    assert!(
        silent
            .uninstrumented_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("Restart the agent")),
        "the operator is told which of the reasons applies"
    );
    assert!(silent.subagents.working.is_none(), "an unknown count is never a zero");

    // The hook only ever answered for in-process subagents. Pane children come
    // from Herdr's lineage, so an uninstrumented parent still lists them.
    let uninstrumented_parent = crate::sidebar::project_pane_children(
        &rows,
        "parent",
        crate::agent_hooks::PaneHookTokens::default(),
        &installed,
    )
    .unwrap();
    assert!(!uninstrumented_parent.instrumented);
    assert_eq!(uninstrumented_parent.chips.len(), 2);
    assert!(uninstrumented_parent.subagents.working.is_none());
}

#[test]
fn a_pane_with_no_agent_gets_neither_chips_nor_a_mark() {
    let rows = instrumented_rows("claude");
    let installed = hook_status_of(Some(hide_agent_hooks::HookStatus::Installed {
        version: hide_agent_hooks::HOOK_VERSION,
    }));
    assert!(
        crate::sidebar::project_pane_children(
            &rows,
            "a-plain-shell",
            crate::agent_hooks::PaneHookTokens::default(),
            &installed,
        )
        .is_none()
    );
}

#[test]
fn an_agent_hide_has_no_adapter_for_says_so_instead_of_guessing_a_runtime() {
    let rows = instrumented_rows("gemini");
    let children = crate::sidebar::project_pane_children(
        &rows,
        "parent",
        crate::agent_hooks::PaneHookTokens::default(),
        &hook_status_of(None),
    )
    .unwrap();
    assert!(!children.instrumented);
    assert_eq!(
        children.uninstrumented_reason.as_deref(),
        Some("Child information is unavailable for this pane.")
    );
    assert!(children.uninstrumented_label.is_some(), "the mark carries an accessible name");
}

#[test]
fn the_breadcrumb_is_the_ancestors_then_the_pane_with_each_layers_siblings() {
    let rows = instrumented_rows("claude");
    assert!(
        crate::sidebar::project_lineage_path(&rows, "parent").is_empty(),
        "a root has nowhere to go back to"
    );
    let path = crate::sidebar::project_lineage_path(&rows, "child");
    assert_eq!(
        path.iter().map(|step| step.pane_id.as_str()).collect::<Vec<_>>(),
        ["parent", "child"]
    );
    assert!(path[0].siblings.is_empty());
    assert_eq!(
        path[1].siblings.iter().map(|s| s.pane_id.as_str()).collect::<Vec<_>>(),
        ["sibling", "child"],
        "the step's dropdown offers that layer, including where the operator is"
    );
}

// PRD B1, B3, B4, D-15, D-43, D-44: the canvas keeps one pane, and the tab
// holding the delegated child never reaches the strip.
fn split_lineage_payload() -> crate::sidebar::SessionSnapshotPayload {
    serde_json::from_value(serde_json::json!({
        "agents": [
            {"id":"Observer","pane_id":"w1:p1","agent":"claude","agent_status":"working",
             "state_change_seq":1,"cwd":"/fixture","workspace_label":"Fixture"},
            {"id":"Implementor","pane_id":"w1:p2","agent":"claude","agent_status":"working",
             "state_change_seq":2,"cwd":"/fixture","workspace_label":"Fixture",
             "spawned_from_pane_id":"w1:p1"}
        ],
        "panes": [
            {"pane_id":"w1:p1","cwd":"/fixture"},
            {"pane_id":"w1:p2","cwd":"/fixture"}
        ],
        "tabs": [{"workspace_id":"w1","tab_id":"t1","label":""}],
        "layouts": [{
            "workspace_id":"w1","tab_id":"t1","zoomed":false,
            "area":{"x":0,"y":0,"width":80,"height":24},
            "focused_pane_id":"w1:p1",
            "panes":[
                {"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":40,"height":24}},
                {"pane_id":"w1:p2","rect":{"x":40,"y":0,"width":40,"height":24}}
            ],
            "splits":[{"direction":"right","ratio":0.5,
                       "rect":{"x":0,"y":0,"width":80,"height":24}}]
        }]
    }))
    .expect("session payload")
}

#[test]
fn a_tab_holding_only_delegated_children_is_kept_off_the_strip() {
    let mut runtime = runtime();
    runtime.ingest_session(Ok(split_lineage_payload()));

    let tabs = runtime
        .snapshot
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.checkouts)
        .flat_map(|checkout| &checkout.tabs)
        .collect::<Vec<_>>();
    assert!(!tabs.is_empty(), "the fixture placed its tab");
    // The parent and the child share one tab here, so that tab is still the
    // operator's and stays on the strip; the move is what separates them.
    assert!(
        tabs.iter().all(|tab| !tab.delegated),
        "a tab holding the operator's own agent is never hidden"
    );

    // Once the child has its own tab, that tab holds nothing but delegated
    // work and leaves the strip while staying in the checkout.
    for workspace in &mut runtime.snapshot.navigator.workspaces {
        for checkout in &mut workspace.checkouts {
            let child = checkout.tabs[0]
                .panes
                .iter()
                .position(|pane| pane.id == "w1:p2")
                .map(|index| checkout.tabs[0].panes.remove(index));
            if let Some(child) = child {
                let mut moved = checkout.tabs[0].clone();
                moved.id = Some("t2".to_owned());
                moved.panes = vec![child];
                checkout.tabs.push(moved);
            }
        }
    }
    runtime.sync_pane_lineage();

    let checkout = runtime.snapshot.navigator.workspaces[0].checkouts[0].clone();
    let child_tab = checkout
        .tabs
        .iter()
        .find(|tab| tab.id.as_deref() == Some("t2"))
        .expect("the child's own tab");
    assert!(child_tab.delegated);
    assert!(
        !checkout
            .strip
            .iter()
            .any(|entry| entry.source_id == "t2"),
        "the delegated tab is off the strip"
    );
    assert!(
        checkout.tabs.iter().any(|tab| tab.id.as_deref() == Some("t2")),
        "and still in the checkout, so the sidebar and breadcrumb reach it"
    );
}

#[test]
fn moving_a_delegated_child_asks_for_a_new_tab_in_the_workspace_it_is_already_in() {
    let params = crate::wire::pane_move_to_new_tab_params("w1:p2", "w1", "Implementor")
        .expect("params encode");
    assert_eq!(
        params,
        serde_json::json!({
            "pane_id": "w1:p2",
            "destination": {"type": "new_tab", "workspace_id": "w1", "label": "Implementor"},
            "focus": false
        }),
        "a new workspace would split one checkout into two rows for one path"
    );
}

#[test]
fn a_move_herdr_declined_is_an_error_rather_than_a_silent_success() {
    // Herdr reports a refusal as an unchanged move with a reason, not as an
    // error, so the decision has to read `changed` rather than assume that a
    // successful request moved anything.
    let refused = crate::wire::move_outcome(false, Some("SameTab".to_owned()), None)
        .expect_err("a refusal is not a move");
    assert!(refused.contains("declined") && refused.contains("SameTab"), "got {refused}");
    assert!(
        crate::wire::move_outcome(false, None, Some("t2".to_owned())).is_err(),
        "an unchanged move is a refusal even when a tab id came back"
    );
    assert_eq!(
        crate::wire::move_outcome(true, None, Some("t2".to_owned())).unwrap(),
        "t2"
    );
    assert!(
        crate::wire::move_outcome(true, None, None).is_err(),
        "a move with no tab to show for it is not a success"
    );
}

/// One root and one delegated child, so a clock assertion reads a single
/// wait rather than the whole fixture tree's.
fn one_child_rows() -> Vec<SidebarAgentSnapshot> {
    let mut rows = project_agents(
        serde_json::from_value(serde_json::json!({"agents": [
            {"id":"Parent","pane_id":"parent","agent_status":"working","state_change_seq":1},
            {"id":"Child","pane_id":"child","spawned_from_pane_id":"parent",
             "agent_status":"working","state_change_seq":2}
        ]}))
        .unwrap(),
    )
    .agents;
    crate::sidebar::apply_lineage(&mut rows, &[], &[]);
    rows
}

// PRD B15, B16, D-35, D-38: delegation is only real if the operator stops
// being called for the delegated work.
#[test]
fn a_delegated_childs_demand_and_completion_stay_off_the_operators_groups() {
    let mut rows = lineage_rows();
    for row in rows.iter_mut() {
        match row.pane_id.as_str() {
            // Asking a question, unread.
            "child" => {
                row.demand = "question".to_owned();
                row.activity = "stopped".to_owned();
                row.unread = true;
            }
            // Finished, unread: the operator's own row here would be Done.
            "grandchild" => {
                row.demand = "none".to_owned();
                row.activity = "stopped".to_owned();
                row.unread = true;
            }
            _ => {}
        }
    }
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    let row = |id: &str| {
        rows.iter()
            .find(|row| row.pane_id == id)
            .unwrap_or_else(|| panic!("{id} is a row"))
    };

    assert_eq!(row("child").group, "seen", "a child's question is its parent's");
    assert_eq!(
        row("grandchild").group,
        "seen",
        "a delegated completion is not the operator's Done"
    );
    assert!(!row("child").emphasized && !row("grandchild").emphasized);
    // The axes themselves are untouched, so the parent's badge can still read
    // the question off the row it points at.
    assert_eq!(row("child").demand, "question");
    assert_eq!(row("child").symbol, "?");
    // The operator's own root keeps its group and its brightness.
    assert_eq!(row("parent").group, "working");
    assert!(!row("parent").delegated);
}

// PRD B19, D-51: what the clock is allowed to count.
#[test]
fn the_stall_clock_runs_only_for_a_delegated_child_that_is_actually_waiting() {
    let mut runtime = runtime();
    runtime.snapshot.status.herdr.state = "connected".to_owned();
    let mut rows = lineage_rows();
    for row in rows.iter_mut() {
        if row.pane_id == "grandchild" {
            // Finished with nothing outstanding: not stuck, just done.
            row.activity = "stopped".to_owned();
        }
        if row.pane_id == "sibling" {
            row.activity = "unknown".to_owned();
        }
    }
    rows.push({
        let mut remote = rows[1].clone();
        remote.pane_id = "remote:mini:p9".to_owned();
        remote.id = "Remote child".to_owned();
        remote.spawned_from_pane_id = Some("parent".to_owned());
        remote
    });
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);
    runtime.apply_stall_escalation(&mut rows, 0);

    let clocked = runtime
        .stall_clocks
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        clocked,
        ["child".to_owned()].into_iter().collect(),
        "a root, a finished child, an unknown activity and a remote pane are all excluded"
    );
}

// PRD B17, B18, D-42, D-62: the soft mark, then the handover, and the notice
// on the lineage root rather than one level at a time.
#[test]
fn a_stalled_grandchild_marks_the_root_then_hands_it_back_to_the_operator() {
    let mut runtime = runtime();
    runtime.snapshot.status.herdr.state = "connected".to_owned();
    let mut rows = lineage_rows();
    for row in rows.iter_mut() {
        if row.pane_id == "grandchild" {
            row.demand = "approval".to_owned();
            row.activity = "stopped".to_owned();
        }
    }
    crate::sidebar::apply_lineage(&mut rows, &lineage_workspaces(), &[]);

    runtime.apply_stall_escalation(&mut rows, 0);
    let level = |rows: &[SidebarAgentSnapshot], id: &str| {
        rows.iter()
            .find(|row| row.pane_id == id)
            .unwrap()
            .stall_level
            .clone()
    };
    assert_eq!(level(&rows, "parent"), "", "nothing has waited yet");

    // Five minutes: the root says so, and nobody has been reassigned.
    runtime.apply_stall_escalation(&mut rows, 5 * 60_000);
    let root = rows.iter().find(|row| row.pane_id == "parent").unwrap();
    assert_eq!(root.stall_level, "soft");
    assert_eq!(
        root.stall_notice.as_deref(),
        Some("Grandchild has been waiting 5 minutes on an approval"),
        "the notice names the descendant, not the root it is drawn on"
    );
    assert_eq!(root.group, "working", "a soft mark is not yet a summons");
    assert_eq!(level(&rows, "grandchild"), "", "the mark lands on the root");
    assert!(
        rows.iter()
            .find(|row| row.pane_id == "grandchild")
            .unwrap()
            .delegated,
        "still somebody else's work at five minutes"
    );

    // Fifteen minutes at depth two, without waiting fifteen more per level.
    runtime.apply_stall_escalation(&mut rows, 15 * 60_000);
    let root = rows.iter().find(|row| row.pane_id == "parent").unwrap();
    assert_eq!(root.stall_level, "hard");
    assert_eq!(root.group, "needs_you");
    assert!(root.emphasized);
    assert!(
        root.stall_notice
            .as_deref()
            .is_some_and(|notice| notice.contains("Grandchild") && notice.contains("15 minutes")),
        "got {:?}",
        root.stall_notice
    );
    let stuck = rows.iter().find(|row| row.pane_id == "grandchild").unwrap();
    assert!(
        !stuck.delegated,
        "the child that ran out of time is the operator's now, so its dimming lifts"
    );
    assert_eq!(stuck.group, "needs_you");
}

// PRD B20, D-54: a disconnection is Hide's blindness, not the agent being
// stuck, so the clocks hold their reading rather than counting through it.
#[test]
fn the_stall_clock_holds_its_reading_while_the_server_is_away() {
    let mut runtime = runtime();
    runtime.snapshot.status.herdr.state = "connected".to_owned();
    let mut rows = one_child_rows();
    runtime.apply_stall_escalation(&mut rows, 0);
    runtime.apply_stall_escalation(&mut rows, 4 * 60_000);
    assert_eq!(runtime.stall_clocks["child"].stalled_ms, 4 * 60_000);

    runtime.snapshot.status.herdr.state = "reconnecting".to_owned();
    runtime.apply_stall_escalation(&mut rows, 30 * 60_000);
    assert_eq!(
        runtime.stall_clocks["child"].stalled_ms,
        4 * 60_000,
        "half an hour offline is not half an hour stuck"
    );
    assert_eq!(
        rows.iter().find(|row| row.pane_id == "parent").unwrap().stall_level,
        "",
        "and no threshold is crossed on the strength of that gap"
    );

    // Counting resumes from where it stopped once the server is back.
    runtime.snapshot.status.herdr.state = "connected".to_owned();
    runtime.apply_stall_escalation(&mut rows, 31 * 60_000);
    assert_eq!(runtime.stall_clocks["child"].stalled_ms, 5 * 60_000);
}

// PRD B19: the clock measures one uninterrupted wait, so any move in the
// agent's own state starts it over.
#[test]
fn a_state_change_restarts_the_wait_rather_than_extending_it() {
    let mut runtime = runtime();
    runtime.snapshot.status.herdr.state = "connected".to_owned();
    let mut rows = one_child_rows();
    runtime.apply_stall_escalation(&mut rows, 0);
    runtime.apply_stall_escalation(&mut rows, 14 * 60_000);
    assert_eq!(runtime.stall_clocks["child"].stalled_ms, 14 * 60_000);

    rows.iter_mut()
        .find(|row| row.pane_id == "child")
        .unwrap()
        .state_change_seq = Some(99);
    runtime.apply_stall_escalation(&mut rows, 14 * 60_000);
    assert_eq!(runtime.stall_clocks["child"].stalled_ms, 0);
    runtime.apply_stall_escalation(&mut rows, 20 * 60_000);
    assert_eq!(
        rows.iter().find(|row| row.pane_id == "parent").unwrap().stall_level,
        "soft",
        "six minutes into the new wait, not twenty into the old one"
    );
}

// PRD B36: a stalled session reports nothing new, so the tick that already
// runs has to be the one that notices.
#[test]
fn the_agent_tick_publishes_when_a_threshold_is_crossed_and_not_otherwise() {
    let mut runtime = runtime();
    runtime.snapshot.status.herdr.state = "connected".to_owned();
    let mut rows = one_child_rows();
    runtime.apply_stall_escalation(&mut rows, 0);
    runtime.snapshot.navigator.agents = rows;

    assert!(
        !runtime.stall_publish_due_at(60_000),
        "a minute of the same wait is not news"
    );
    assert!(
        runtime.stall_publish_due_at(5 * 60_000),
        "crossing into soft is"
    );

    runtime.snapshot.status.herdr.state = "reconnecting".to_owned();
    assert!(
        !runtime.stall_publish_due_at(20 * 60_000),
        "a disconnected session publishes its own failure, not a stall verdict"
    );
}

// PRD B31, D-53: a dead session's numbers never stay on screen.
#[test]
fn a_pane_whose_session_ended_draws_no_counts_even_though_its_tokens_remain() {
    let rows = instrumented_rows("claude");
    let installed = hook_status_of(Some(hide_agent_hooks::HookStatus::Installed {
        version: hide_agent_hooks::HOOK_VERSION,
    }));
    // Herdr's pane metadata is durable, so the last session's tokens are
    // still on the pane after it exits.
    let leftover = crate::agent_hooks::PaneHookTokens {
        version: Some(hide_agent_hooks::HOOK_VERSION),
        working: Some(3),
        done: Some(9),
        blocked: None,
    };
    assert!(
        crate::sidebar::project_pane_children(&rows, "no-such-agent", leftover, &installed)
            .is_none(),
        "the counts belong to a session, and there is no session in that pane"
    );
    // The same tokens on a pane that does have a session still read.
    let live = crate::sidebar::project_pane_children(&rows, "parent", leftover, &installed)
        .expect("a pane with an agent projects");
    assert_eq!(live.subagents.done, Some(9));
}

// PRD B27, D-31, D-48, D-61: the Settings diagnosis says what each runtime's
// hook is, and names the panes a restart would fix.
#[test]
fn the_settings_diagnosis_reports_each_runtime_and_the_sessions_that_predate_the_install() {
    let mut runtime = runtime();
    runtime.ingest_session(Ok(serde_json::from_value(serde_json::json!({
        "agents": [
            {"id":"Instrumented","pane_id":"w1:p1","agent":"claude","agent_status":"working",
             "state_change_seq":1,"cwd":"/fixture","workspace_label":"Fixture"},
            {"id":"Older","pane_id":"w1:p2","agent":"claude","agent_status":"working",
             "state_change_seq":2,"cwd":"/fixture","workspace_label":"Fixture"}
        ],
        "panes": [
            {"pane_id":"w1:p1","cwd":"/fixture","tokens":{"hide_hooks":"1","hide_sub_done":"2"}},
            {"pane_id":"w1:p2","cwd":"/fixture"}
        ],
        "tabs": [{"workspace_id":"w1","tab_id":"t1","label":""}],
        "layouts": [{
            "workspace_id":"w1","tab_id":"t1","zoomed":false,
            "area":{"x":0,"y":0,"width":80,"height":24},
            "focused_pane_id":"w1:p1",
            "panes":[
                {"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":40,"height":24}},
                {"pane_id":"w1:p2","rect":{"x":40,"y":0,"width":40,"height":24}}
            ],
            "splits":[{"direction":"right","ratio":0.5,
                       "rect":{"x":0,"y":0,"width":80,"height":24}}]
        }]
    }))
    .expect("session payload")));

    // Before anything has been read, the screen has nothing to claim.
    assert!(runtime.snapshot.status.agent_hooks.runtimes.is_empty());

    assert!(runtime.ingest_hook_diagnosis(hide_agent_hooks::Diagnosis {
        runtimes: vec![
            hide_agent_hooks::diagnosis::RuntimeDiagnosis {
                runtime: hide_agent_hooks::AgentRuntime::ClaudeCode,
                label: "Claude Code".to_owned(),
                path: "/fixture/.claude/settings.json".to_owned(),
                status: hide_agent_hooks::HookStatus::Installed {
                    version: hide_agent_hooks::HOOK_VERSION,
                },
                current_version: hide_agent_hooks::HOOK_VERSION,
            },
            hide_agent_hooks::diagnosis::RuntimeDiagnosis {
                runtime: hide_agent_hooks::AgentRuntime::Codex,
                label: "Codex".to_owned(),
                path: "/fixture/.codex/hooks.json".to_owned(),
                status: hide_agent_hooks::HookStatus::NotInstalled,
                current_version: hide_agent_hooks::HOOK_VERSION,
            },
        ],
    }));

    let hooks = &runtime.snapshot.status.agent_hooks;
    assert_eq!(
        hooks
            .runtimes
            .iter()
            .map(|row| (row.id.as_str(), row.headline.as_str(), row.offers_install))
            .collect::<Vec<_>>(),
        vec![
            ("claude-code", "Installed (v1)", false),
            ("codex", "Not installed", true),
        ],
        "a runtime that is fine is not offered a reinstall (PRD B28)"
    );
    assert_eq!(hooks.runtimes[0].path, "/fixture/.claude/settings.json");

    // The hook is installed and one pane still carries none of its tokens,
    // so that session started first and a restart is what fixes it.
    assert_eq!(
        hooks
            .sessions_predating_install
            .iter()
            .map(|pane| pane.pane_id.as_str())
            .collect::<Vec<_>>(),
        ["w1:p2"],
        "the instrumented pane is not on the list"
    );
    assert!(
        hooks.sessions_predating_install[0]
            .message
            .contains("Restart the agent"),
        "got {:?}",
        hooks.sessions_predating_install[0].message
    );
    assert_eq!(hooks.sessions_predating_install[0].label, "Older");
}

// PRD B28, D-31: Hide installs on approval, never on its own initiative, and
// an approval it cannot act on says so rather than being dropped.
#[test]
fn an_approved_install_is_queued_once_and_an_unknown_runtime_is_reported() {
    let mut runtime = runtime();
    let install = |runtime_id: &str| {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2, "kind": "install_agent_hooks",
            "payload": {"runtime_id": runtime_id}
        }))
        .unwrap()
    };

    assert!(
        runtime.take_agent_hook_installs().is_empty(),
        "nothing is installed until the operator asks"
    );

    assert!(runtime.dispatch_json(&install("claude-code")));
    // Approving twice is one install: the queue is a set and the write itself
    // rewrites the same hook group either way (engineering rule 11).
    assert!(runtime.dispatch_json(&install("claude-code")));
    assert!(runtime.dispatch_json(&install("codex")));
    assert_eq!(
        runtime.take_agent_hook_installs(),
        vec![
            hide_agent_hooks::AgentRuntime::ClaudeCode,
            hide_agent_hooks::AgentRuntime::Codex
        ]
    );
    assert!(
        runtime.take_agent_hook_installs().is_empty(),
        "a taken request is not performed twice"
    );

    assert!(runtime.dispatch_json(&install("emacs")));
    assert!(
        runtime.take_agent_hook_installs().is_empty(),
        "a runtime Hide has no adapter for installs nothing"
    );
    let error = runtime
        .snapshot
        .status
        .last_error
        .as_ref()
        .expect("the refusal is visible rather than silent");
    assert_eq!(error.kind, "agent_hooks.unknown_runtime");
    assert!(error.message.contains("emacs"), "got {}", error.message);
}

// PRD B34, B35, D-32, D-55, D-60: Overview says who is working here, and an
// empty line is not the same answer as one Hide cannot fill.
#[test]
fn the_overview_agent_line_separates_nobody_working_here_from_cannot_see() {
    let rows = instrumented_rows("claude");
    let chips = rows
        .iter()
        .map(|agent| (agent.pane_id.clone(), crate::sidebar::agent_chip(agent)))
        .collect::<HashMap<_, _>>();
    let reason_for = |code: hide_agent_hooks::diagnosis::UninstrumentedReason| {
        crate::model::PaneChildrenSnapshot {
            instrumented: false,
            uninstrumented_reason: Some(code.message().to_owned()),
            uninstrumented_label: Some(code.accessibility_label().to_owned()),
            uninstrumented_code: Some(code.code().to_owned()),
            ..Default::default()
        }
    };
    let panes = |ids: &[&str]| {
        ids.iter()
            .map(|id| (*id).to_owned())
            .collect::<HashSet<String>>()
    };

    // A worktree Hide has no pane rows for at all.
    let none = worktree_agent_line(None, &chips, &HashMap::new(), &rows);
    assert!(none.agents.is_empty() && none.uninstrumented_code.is_none());

    // Panes, but nobody working in them: an empty line and no claim.
    let quiet = worktree_agent_line(Some(&panes(&["idle"])), &chips, &HashMap::new(), &rows);
    assert!(quiet.agents.is_empty());
    assert_eq!(
        quiet.uninstrumented_code, None,
        "no agents is an answer, not a gap"
    );

    // Two agents here and one elsewhere, all instrumented.
    let instrumented = HashMap::from([
        (
            "parent".to_owned(),
            crate::model::PaneChildrenSnapshot {
                instrumented: true,
                ..Default::default()
            },
        ),
        (
            "child".to_owned(),
            crate::model::PaneChildrenSnapshot {
                instrumented: true,
                ..Default::default()
            },
        ),
    ]);
    let working = worktree_agent_line(
        Some(&panes(&["parent", "child"])),
        &chips,
        &instrumented,
        &rows,
    );
    assert_eq!(
        working
            .agents
            .iter()
            .map(|agent| agent.pane_id.as_str())
            .collect::<Vec<_>>(),
        rows.iter()
            .filter(|agent| agent.pane_id != "sibling")
            .map(|agent| agent.pane_id.as_str())
            .collect::<Vec<_>>(),
        "the line follows the sidebar's own order"
    );
    assert!(working.agents.iter().any(|agent| !agent.label.is_empty()));
    assert_eq!(working.uninstrumented_code, None);

    // One of them is uninstrumented, and two reasons are in play: the line
    // shows the first in the crate's resolution order, not the first pane.
    let mixed = HashMap::from([
        (
            "parent".to_owned(),
            reason_for(hide_agent_hooks::diagnosis::UninstrumentedReason::SessionPredatesInstall),
        ),
        (
            "child".to_owned(),
            reason_for(hide_agent_hooks::diagnosis::UninstrumentedReason::HooksNotInstalled),
        ),
    ]);
    let unseen = worktree_agent_line(Some(&panes(&["parent", "child"])), &chips, &mixed, &rows);
    assert_eq!(unseen.agents.len(), 2, "the agents are still named");
    assert_eq!(
        unseen.uninstrumented_code.as_deref(),
        Some("hooks_not_installed")
    );
    assert_eq!(
        unseen.uninstrumented_reason,
        Some(
            hide_agent_hooks::diagnosis::UninstrumentedReason::HooksNotInstalled
                .message()
                .to_owned()
        ),
        "the same sentence the pane header shows (PRD B21)"
    );
    assert!(unseen.uninstrumented_label.is_some(), "the mark is named");
}

/// One realistic delegation session, projected end to end.
///
/// It asserts what the operator's screen says in the four states this change
/// exists to separate - a parent with children, a pane Hide cannot see into,
/// an instrumented pane working alone, and a pane with no agent at all - and,
/// when `HIDE_SNAPSHOT_OUT` names a path, writes the snapshot the shell would
/// draw so the same states can be captured for human visual judgement
/// (PRD D-34) without a live server or the operator's own panes.
#[test]
fn a_delegation_session_projects_every_state_the_operator_has_to_tell_apart() {
    let mut runtime = runtime();
    runtime.ingest_session(Ok(serde_json::from_value(serde_json::json!({
        "agents": [
            {"id":"Observer","pane_id":"w1:p1","agent":"claude","agent_status":"working",
             "state_change_seq":1,"cwd":"/fixture","workspace_label":"hide",
             "tokens":{"summary":"delegating the orchestrator work"}},
            {"id":"Implementor","pane_id":"w1:p2","agent":"claude","agent_status":"working",
             "state_change_seq":2,"cwd":"/fixture","workspace_label":"hide",
             "spawned_from_pane_id":"w1:p1","tokens":{"summary":"구현 중: 계보 투영과 위임 표시"}},
            {"id":"Reviewer","pane_id":"w1:p3","agent":"claude","agent_status":"idle",
             "state_change_seq":3,"cwd":"/fixture","workspace_label":"hide",
             "spawned_from_pane_id":"w1:p1",
             "tokens":{"status_question_new":"?","summary":"정체 임계값을 물어보는 중"}},
            {"id":"Uninstrumented","pane_id":"w1:p4","agent":"claude","agent_status":"working",
             "state_change_seq":4,"cwd":"/fixture","workspace_label":"hide",
             "tokens":{"summary":"started before the hook was installed"}},
            {"id":"Alone","pane_id":"w1:p5","agent":"claude","agent_status":"working",
             "state_change_seq":5,"cwd":"/fixture","workspace_label":"hide",
             "tokens":{"summary":"working with no children"}}
        ],
        "panes": [
            {"pane_id":"w1:p1","cwd":"/fixture",
             "tokens":{"hide_hooks":"1","hide_sub_working":"2","hide_sub_done":"4"}},
            {"pane_id":"w1:p2","cwd":"/fixture","tokens":{"hide_hooks":"1"}},
            {"pane_id":"w1:p3","cwd":"/fixture","tokens":{"hide_hooks":"1"}},
            {"pane_id":"w1:p4","cwd":"/fixture"},
            {"pane_id":"w1:p5","cwd":"/fixture","tokens":{"hide_hooks":"1","hide_sub_working":"0","hide_sub_done":"0"}},
            {"pane_id":"w1:p6","cwd":"/fixture"}
        ],
        "tabs": [
            {"workspace_id":"w1","tab_id":"t1","label":""},
            {"workspace_id":"w1","tab_id":"t2","label":""},
            {"workspace_id":"w1","tab_id":"t3","label":""},
            {"workspace_id":"w1","tab_id":"t4","label":""},
            {"workspace_id":"w1","tab_id":"t5","label":""},
            {"workspace_id":"w1","tab_id":"t6","label":""}
        ],
        "layouts": [
            {
                "workspace_id":"w1","tab_id":"t1","zoomed":false,
                "area":{"x":0,"y":0,"width":120,"height":40},
                "focused_pane_id":"w1:p1",
                "panes":[{"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":120,"height":40}}],
                "splits":[]
            },
            {
                "workspace_id":"w1","tab_id":"t2","zoomed":false,
                "area":{"x":0,"y":0,"width":120,"height":40},
                "focused_pane_id":"w1:p2",
                "panes":[{"pane_id":"w1:p2","rect":{"x":0,"y":0,"width":120,"height":40}}],
                "splits":[]
            },
            {
                "workspace_id":"w1","tab_id":"t6","zoomed":false,
                "area":{"x":0,"y":0,"width":120,"height":40},
                "focused_pane_id":"w1:p3",
                "panes":[{"pane_id":"w1:p3","rect":{"x":0,"y":0,"width":120,"height":40}}],
                "splits":[]
            },
            {
                "workspace_id":"w1","tab_id":"t3","zoomed":false,
                "area":{"x":0,"y":0,"width":120,"height":40},
                "focused_pane_id":"w1:p4",
                "panes":[{"pane_id":"w1:p4","rect":{"x":0,"y":0,"width":120,"height":40}}],
                "splits":[]
            },
            {
                "workspace_id":"w1","tab_id":"t4","zoomed":false,
                "area":{"x":0,"y":0,"width":120,"height":40},
                "focused_pane_id":"w1:p5",
                "panes":[{"pane_id":"w1:p5","rect":{"x":0,"y":0,"width":120,"height":40}}],
                "splits":[]
            },
            {
                "workspace_id":"w1","tab_id":"t5","zoomed":false,
                "area":{"x":0,"y":0,"width":120,"height":40},
                "focused_pane_id":"w1:p6",
                "panes":[{"pane_id":"w1:p6","rect":{"x":0,"y":0,"width":120,"height":40}}],
                "splits":[]
            }
        ]
    }))
    .expect("session payload")));

    // The hook is installed, which is what turns "no tokens on this pane"
    // into "this session started first" rather than "no hook anywhere".
    runtime.ingest_hook_diagnosis(hide_agent_hooks::Diagnosis {
        runtimes: vec![
            hide_agent_hooks::diagnosis::RuntimeDiagnosis {
                runtime: hide_agent_hooks::AgentRuntime::ClaudeCode,
                label: "Claude Code".to_owned(),
                path: "/fixture/.claude/settings.json".to_owned(),
                status: hide_agent_hooks::HookStatus::Installed {
                    version: hide_agent_hooks::HOOK_VERSION,
                },
                current_version: hide_agent_hooks::HOOK_VERSION,
            },
            hide_agent_hooks::diagnosis::RuntimeDiagnosis {
                runtime: hide_agent_hooks::AgentRuntime::Codex,
                label: "Codex".to_owned(),
                path: "/fixture/.codex/hooks.json".to_owned(),
                status: hide_agent_hooks::HookStatus::NotInstalled,
                current_version: hide_agent_hooks::HOOK_VERSION,
            },
        ],
    });

    let pane = |id: &str| {
        runtime
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .find(|pane| pane.id == id)
            .unwrap_or_else(|| panic!("{id} is a pane"))
            .clone()
    };

    // A parent with children: chips for the pane children, the in-process
    // count separate from them, and a breadcrumb its children carry.
    let parent = pane("w1:p1").children.expect("the parent has a session");
    assert!(parent.instrumented);
    assert_eq!(
        parent.chips.iter().map(|chip| chip.label.as_str()).collect::<Vec<_>>(),
        ["Reviewer", "Implementor"],
        "chips follow the lineage's own child order"
    );
    assert_eq!(parent.representative.as_ref().unwrap().label, "Reviewer");
    assert_eq!((parent.subagents.working, parent.subagents.done), (Some(2), Some(4)));
    assert!(pane("w1:p2").lineage_path.iter().any(|step| step.label == "Observer"));

    // A pane Hide cannot see into, and the reason a restart would fix it.
    let unseen = pane("w1:p4").children.expect("it has a session");
    assert!(!unseen.instrumented);
    assert_eq!(
        unseen.uninstrumented_code.as_deref(),
        Some("session_predates_install")
    );

    // An instrumented session working alone: a confirmed answer, and a
    // different screen from the one above.
    let alone = pane("w1:p5").children.expect("it has a session");
    assert!(alone.instrumented && alone.chips.is_empty());
    assert_eq!(alone.uninstrumented_reason, None);
    assert_eq!(alone.subagents.working, Some(0));

    // A pane with no agent says nothing at all.
    assert!(pane("w1:p6").children.is_none());

    // The Settings diagnosis names both runtimes and the pane a restart fixes.
    let hooks = &runtime.snapshot.status.agent_hooks;
    assert_eq!(hooks.runtimes.len(), 2);
    assert_eq!(
        hooks
            .sessions_predating_install
            .iter()
            .map(|pane| pane.label.as_str())
            .collect::<Vec<_>>(),
        ["Uninstrumented"]
    );

    // Overview reads the same rows. A catalog is handed in directly because
    // the real one comes from `git`, which a projection test does not run,
    // and Overview draws the focused checkout's project.
    let checkout_id = runtime.snapshot.navigator.workspaces[0].checkouts[0].id.clone();
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id);
    let workspace_id = runtime.snapshot.navigator.workspaces[0].id.clone();
    runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id);
    runtime.ingest_worktrees(crate::model::WorktreeCatalogSnapshot {
        projects: vec![crate::model::ProjectWorktreesSnapshot {
            root_path: "/fixture".to_owned(),
            base_source: "default_branch".to_owned(),
            default_branch: Some("main".to_owned()),
            worktrees: vec![
                crate::model::WorktreeSnapshot {
                    path: "/fixture".to_owned(),
                    branch: Some("main".to_owned()),
                    is_main: true,
                    upstream_state: "pushed".to_owned(),
                    ..Default::default()
                },
                crate::model::WorktreeSnapshot {
                    path: "/fixture/quiet".to_owned(),
                    branch: Some("quiet".to_owned()),
                    upstream_state: "pushed".to_owned(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
    });
    let overview = runtime
        .snapshot
        .git_worktrees
        .as_ref()
        .expect("the catalog projects");
    let line = |path: &str| {
        overview
            .worktrees
            .iter()
            .find(|worktree| worktree.path == path)
            .map(|worktree| worktree.agent_line.clone())
            .unwrap_or_else(|| panic!("{path} is a worktree row"))
    };
    // The worktree the session runs in names its agents and carries the mark
    // of the one Hide cannot see into.
    let busy = line("/fixture");
    assert!(!busy.agents.is_empty());
    assert_eq!(
        busy.uninstrumented_code.as_deref(),
        Some("session_predates_install")
    );
    // A worktree nobody is working in says nothing, which is a different
    // screen from one Hide cannot see into (PRD B35, D-60).
    let quiet = line("/fixture/quiet");
    assert!(quiet.agents.is_empty());
    assert_eq!(quiet.uninstrumented_code, None);

    if let Ok(path) = std::env::var("HIDE_SNAPSHOT_OUT") {
        let payload = runtime.snapshot_delta_payload(0, 0);
        let bytes = crate::runtime::serialize_snapshot_delta(&payload).expect("snapshot encodes");
        std::fs::write(&path, bytes).expect("snapshot is written");
    }
}
