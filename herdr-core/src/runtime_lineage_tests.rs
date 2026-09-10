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
    assert_eq!(child.lineage_hint.as_deref(), Some("↳ from parent"));
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
    let raised = attention
        .iter()
        .filter(|row| row.group == "needs_you")
        .collect::<Vec<_>>();
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].raised_hint.as_deref(), Some("↳ from Parent"));
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
