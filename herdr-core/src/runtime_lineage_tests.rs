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
