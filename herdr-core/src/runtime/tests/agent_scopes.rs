use super::*;

fn rows() -> Vec<SidebarAgentSnapshot> {
    project_agents(
        serde_json::from_value(serde_json::json!({"agents": [
            {"pane_id":"root", "agent":"claude", "agent_status":"working", "state_change_seq":1},
            {"pane_id":"child", "agent":"claude", "agent_status":"idle", "state_change_seq":2}
        ]}))
        .unwrap(),
    )
    .agents
}

#[test]
fn scopes_keep_first_overview_owner_last_badge_owner_and_restore_rebuilt_catalog_values() {
    let mut runtime = runtime();
    let mut rows = rows();
    rows.push(rows[0].clone());
    runtime.snapshot.navigator.agents = rows;
    let first = checkout(
        "project",
        "first",
        "/fixture/first",
        Some(pane("root", "/fixture/first")),
    );
    let last = checkout(
        "project",
        "last",
        "/fixture/last",
        Some(pane("root", "/fixture/last")),
    );
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "project",
        "Project",
        "/fixture",
        vec![first, last],
    )];
    crate::agent_state::sync_checkout_agent_summaries(
        &mut runtime.snapshot.navigator.workspaces,
        &runtime.snapshot.navigator.agents,
    );
    assert!(runtime.refresh_agent_scopes());
    let project = &runtime.snapshot.navigator.workspaces[0];
    assert_eq!(
        project.agent_scope.total, 2,
        "physical rows are not deduplicated"
    );
    assert_eq!(project.agent_scope.overview_total, 1);
    assert_eq!(project.agent_scope.members[0].checkout_id, "first");
    assert_eq!(project.checkouts[0].agent_summary.working, 0);
    assert_eq!(project.checkouts[1].agent_summary.working, 1);
    assert_eq!(project.agent_scope.marks.working, 1);
    assert!(
        !runtime.refresh_agent_scopes(),
        "an unchanged projection publishes nothing"
    );
    let expected = runtime.snapshot.navigator.workspaces.clone();
    runtime.snapshot.navigator.workspaces[0].agent_scope = Default::default();
    runtime.snapshot.navigator.workspaces[0].checkouts[0].agent_scope = Default::default();
    assert!(
        runtime.refresh_agent_scopes(),
        "a catalog rebuilt from facts gets its cached scopes back"
    );
    assert_eq!(runtime.snapshot.navigator.workspaces, expected);
    assert!(!runtime.refresh_agent_scopes());
    runtime.snapshot.navigator.workspaces[0].checkouts[0]
        .tabs
        .clear();
    assert!(
        runtime.refresh_agent_scopes(),
        "pane ownership invalidates the scope"
    );
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0].agent_scope.members[0].checkout_id,
        "last"
    );
}

#[test]
fn disconnected_devices_zero_the_physical_tile_but_keep_overview_members_from_the_last_session() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.agents.clear();
    runtime.snapshot.navigator.workspaces.clear();
    let mut device = crate::workspace::local_device(&crate::node::test_node());
    device.id = "mini".into();
    device.kind = "remote".into();
    runtime.snapshot.navigator.devices.push(device);
    let mut project = workspace(
        "remote-project",
        "Remote",
        "/fixture",
        vec![checkout(
            "remote-project",
            "remote-checkout",
            "/fixture",
            Some(pane("root", "/fixture")),
        )],
    );
    project.device_id = "mini".into();
    runtime.snapshot.status.remote = vec![RemoteStatusSnapshot {
        target_id: "mini".into(),
        state: "connected".into(),
        message: None,
        herdr_version: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
        session: Some(RemoteSessionSnapshot {
            workspaces: vec![project],
            agents: rows(),
            active_tab_ids: Default::default(),
            focused_workspace_id: None,
            focused_checkout_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            pane_layouts: Vec::new(),
            pane_hook_tokens: Default::default(),
        }),
    }];
    assert!(runtime.refresh_agent_scopes());
    let scope = &runtime
        .snapshot
        .navigator
        .devices
        .iter()
        .find(|d| d.id == "mini")
        .unwrap()
        .agent_scope;
    assert_eq!((scope.total, scope.overview_total), (2, 1));
    runtime.snapshot.status.remote[0].state = "stale".into();
    assert!(runtime.refresh_agent_scopes());
    let scope = &runtime
        .snapshot
        .navigator
        .devices
        .iter()
        .find(|d| d.id == "mini")
        .unwrap()
        .agent_scope;
    assert_eq!(
        (scope.total, scope.groups.working, scope.overview_total),
        (0, 0, 1)
    );
    assert_eq!(runtime.snapshot.navigator.agent_scope.total, 0);
    assert_eq!(runtime.snapshot.navigator.agent_scope.overview_total, 1);
    assert!(!runtime.refresh_agent_scopes());
}

#[test]
fn checkout_trees_and_folded_badges_preserve_cross_checkout_lineage_and_priority() {
    let mut runtime = runtime();
    let mut agents = rows();
    let mut done = agents[1].clone();
    done.pane_id = "done".into();
    done.group = "done".into();
    done.demand = "none".into();
    done.activity = "stopped".into();
    done.state = crate::agent_state::turn::row_state(&done);
    agents.push(done);
    agents[0].lineage_child_pane_ids = vec!["child".into(), "done".into(), "gone".into()];
    agents[0].lineage_collapsed = true;
    agents[1].lineage_parent_pane_id = Some("root".into());
    agents[1].delegated = true;
    agents[1].group = "seen".into();
    agents[1].demand = "question".into();
    agents[1].state = crate::agent_state::turn::row_state(&agents[1]);
    agents[2].lineage_parent_pane_id = Some("root".into());
    let mut first = checkout(
        "project",
        "first",
        "/fixture/first",
        Some(pane("root", "/fixture/first")),
    );
    first.tabs[0].panes.push(pane("child", "/fixture/first"));
    let second = checkout(
        "project",
        "second",
        "/fixture/second",
        Some(pane("done", "/fixture/second")),
    );
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "project",
        "Project",
        "/fixture",
        vec![first, second],
    )];
    runtime.snapshot.navigator.agents = agents;
    assert!(runtime.refresh_agent_scopes());
    let project = &runtime.snapshot.navigator.workspaces[0];
    let tree = &project.checkouts[0].agent_scope.tree;
    assert_eq!(
        tree.rows
            .iter()
            .map(|r| (r.pane_id.as_str(), r.depth))
            .collect::<Vec<_>>(),
        vec![("root", 0), ("child", 1), ("done", 1)]
    );
    assert_eq!(
        tree.visible_rows
            .iter()
            .map(|r| r.pane_id.as_str())
            .collect::<Vec<_>>(),
        vec!["root"]
    );
    assert_eq!(tree.shown, vec!["done", "root"]);
    assert_eq!(tree.more, 1);
    assert!(
        !tree.needs_you,
        "delegated question and a done descendant do not turn the card yellow"
    );
    assert!(
        project.checkouts[1].agent_scope.tree.needs_you,
        "that done row is a root relative to its own checkout"
    );
    let scope = &runtime.snapshot.navigator.agent_scope;
    let folded = &scope.folded["root"];
    assert_eq!(folded.badge_descendants, 1);
    assert_eq!(folded.badge_counts.question, 1);
    assert_eq!(folded.badge_children, vec!["child"]);
    assert_eq!(scope.children["root"], vec!["child", "done"]);
    assert_eq!(scope.descendants["root"], 2);
    assert_eq!(folded.tiers.len(), 1);
    assert_eq!(folded.tiers[0][0].candidates, vec!["done"]);
    assert_eq!(folded.overflow, 0);
    runtime.snapshot.navigator.agents[0].lineage_collapsed = false;
    assert!(runtime.refresh_agent_scopes());
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0].checkouts[0]
            .agent_scope
            .tree
            .visible_rows
            .len(),
        3
    );
    assert!(!runtime.refresh_agent_scopes());
}
