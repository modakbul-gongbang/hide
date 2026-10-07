use super::*;
use crate::model::{PullRequestChecks, PullRequestSnapshot};

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
fn graph_folds_count_hidden_marks_on_the_nearest_visible_ancestor() {
    let mut runtime = runtime();
    let mut agents = rows();
    agents[0].state.graph_rank = 1;
    agents[1].state.graph_rank = 3;
    agents[1].symbol = "○".into();
    agents[1].lineage_parent_pane_id = Some("root".into());
    agents[0].lineage_child_pane_ids = vec!["child".into()];
    agents[1].lineage_child_pane_ids = vec!["leaf".into()];
    let mut leaf = agents[1].clone();
    leaf.pane_id = "leaf".into();
    leaf.symbol = "✓".into();
    leaf.lineage_parent_pane_id = Some("child".into());
    agents.push(leaf);
    runtime.snapshot.navigator.agents = agents;
    let mut main = checkout(
        "project",
        "main",
        "/fixture",
        Some(pane("root", "/fixture")),
    );
    main.is_primary = true;
    let child = checkout("project", "child", "/child", Some(pane("child", "/child")));
    let mut leaf = checkout("project", "leaf", "/leaf", Some(pane("leaf", "/leaf")));
    leaf.is_worktree = true;
    leaf.landed = true;
    let mut project = workspace("project", "Project", "/fixture", vec![main, child, leaf]);
    project.is_git = true;
    runtime.snapshot.navigator.workspaces = vec![project];
    assert!(runtime.refresh_agent_scopes());
    let graph = &runtime.snapshot.navigator.workspaces[0].agent_scope.graph;
    assert_eq!(graph.attention, 1);
    assert_eq!(graph.checkouts["child"].fold, Some("resting"));
    assert_eq!(graph.checkouts["leaf"].fold, Some("cleanup"));
    assert_eq!(graph.tucked[graph.variants[8]]["root"]["idle"], 1);
    assert_eq!(graph.tucked[graph.variants[8]]["root"]["done"], 1);
    assert_eq!(graph.tucked[graph.variants[12]]["child"]["done"], 1);
    assert!(graph.tucked[graph.variants[15]].is_empty());
    let relations = &runtime.snapshot.navigator.devices[0].agent_scope.relations;
    assert_eq!(
        relations["root"]
            .iter()
            .map(|g| g.checkout_id.as_str())
            .collect::<Vec<_>>(),
        ["main", "child", "leaf"]
    );
    assert_eq!(
        relations["root"][1].rows[0].caption_parent.as_deref(),
        Some("root")
    );
    assert_eq!(
        relations["child"][0]
            .rows
            .iter()
            .map(|r| (r.pane_id.as_str(), r.depth, r.tag))
            .collect::<Vec<_>>(),
        [("child", 0, Some("here")), ("root", 1, Some("parent"))]
    );
    assert!(!runtime.refresh_agent_scopes());
    runtime.snapshot.navigator.workspaces[0].checkouts[2].landed = false;
    assert!(
        runtime.refresh_agent_scopes(),
        "Git-only cleanup changes invalidate folds"
    );
    let graph = &runtime.snapshot.navigator.workspaces[0].agent_scope.graph;
    assert_eq!(graph.checkouts["leaf"].fold, Some("resting"));
    assert!(graph.tucked[graph.variants[12]].is_empty());
}

#[test]
fn close_consequences_keep_unknown_priority_and_outside_descendant_counts() {
    let mut runtime = runtime();
    let mut agents = rows();
    let mut neighbour = agents[0].clone();
    neighbour.pane_id = "neighbour".into();
    neighbour.requires_close_status_check = true;
    neighbour.state.subtree = "unknown";
    agents[0].close_descendant_pane_ids = vec!["child".into(), "gone".into()];
    agents[0].lineage_child_pane_ids = vec!["child".into()];
    agents[1].lineage_parent_pane_id = Some("root".into());
    agents[1].lineage_depth = 2;
    agents[1].state.subtree = "waiting";
    agents.push(neighbour);
    runtime.snapshot.navigator.agents = agents;
    let mut root = pane("root", "/fixture");
    root.requires_close_confirmation = true;
    let mut neighbour = pane("neighbour", "/fixture");
    neighbour.herdr_label = Some("second pane".into());
    let mut inside = checkout("project", "inside", "/fixture", Some(root));
    inside.tabs[0].panes.push(neighbour);
    let outside = checkout(
        "project",
        "outside",
        "/outside",
        Some(pane("child", "/outside")),
    );
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "project",
        "Project",
        "/fixture",
        vec![inside, outside],
    )];
    assert!(runtime.refresh_agent_scopes());
    let close = &runtime.snapshot.navigator.agent_scope.closes["root\0neighbour"];
    assert_eq!(
        serde_json::to_value(&close.decision).unwrap(),
        serde_json::json!({"action":"status_unknown", "label":"second pane"})
    );
    assert_eq!(close.stop_work.unknown, Some(1));
    let subtree = close.subtree.as_ref().unwrap();
    assert_eq!(subtree.ids, ["child"]);
    assert_eq!(
        subtree
            .rows
            .iter()
            .map(|r| (r.pane_id.as_str(), r.depth, r.target))
            .collect::<Vec<_>>(),
        [("root", 0, true), ("child", 2, false)]
    );
    assert_eq!(subtree.counts.waiting, 1);
    assert!(!subtree.unknown);
    assert!(!subtree.target_unknown);
    assert!(close.subtree_all.as_ref().unwrap().target_unknown);
    assert_eq!(close.subtree_all.as_ref().unwrap().rows.len(), 3);
    assert!(
        runtime.snapshot.navigator.agent_scope.closes["root\0neighbour\0child"]
            .subtree
            .is_none()
    );
    assert!(!runtime.refresh_agent_scopes());
    runtime.snapshot.navigator.workspaces[0].checkouts[0].tabs[0].panes[0]
        .requires_close_status_check = true;
    assert!(
        runtime.refresh_agent_scopes(),
        "a pane-only status change refreshes the close notice"
    );
    let close = &runtime.snapshot.navigator.agent_scope.closes["root\0neighbour"];
    assert_eq!(close.stop_work.unknown, Some(0));
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
    assert_eq!(project.agent_scope.places["root"].checkout_id, "first");
    assert_eq!(runtime.snapshot.navigator.agent_scope.listed.len(), 3);
    assert_eq!(runtime.snapshot.navigator.agent_scope.listed[2].index, 2);
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
    assert_eq!(scope.listed.len(), 2);
    assert!(scope.places_live);
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
    assert!(scope.listed.is_empty());
    assert!(!scope.places_live);
    assert!(
        scope.places.contains_key("root"),
        "search context retains the catalog's place"
    );
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

#[test]
fn pr_board_keeps_branch_turn_separate_from_its_maker_and_tracks_issue_changes() {
    use crate::request_view::{AgentPullRequestSnapshot, AgentRequestSnapshot};
    let mut runtime = runtime();
    let pr: PullRequestSnapshot = serde_json::from_value(serde_json::json!({
        "number":42,"title":"Ship","url":"https://github.com/acme/app/pull/42",
        "head_branch":"feature","base_branch":"main","badge":"open","checks":"failed","is_draft":false,
        "closing_issues":[{"repository":"acme/app","number":7}]
    })).unwrap();
    let mut agents = rows();
    // The PR maker moved to another checkout and is still working there.
    agents[0].request = Some(AgentRequestSnapshot {
        verb: crate::agent_state::RequestVerb::Working,
        verb_since_unix_ms: 1,
        line: None,
        end: None,
        request: None,
        later_by: None,
        reply: None,
        pull_requests: vec![AgentPullRequestSnapshot {
            number: pr.number,
            title: pr.title.clone(),
            url: pr.url.clone(),
            badge: pr.badge,
            checks: pr.checks,
            head_branch: pr.head_branch.clone(),
            closing_issues: pr.closing_issues.clone(),
            live: true,
            duty: true,
            created: true,
            settled_at_unix_ms: None,
        }],
    });
    agents[1].group = "done".into();
    agents[1].activity = "stopped".into();
    agents[1].state = crate::agent_state::turn::row_state(&agents[1]);
    let mut branch = checkout(
        "project",
        "feature",
        "/fixture/feature",
        Some(pane("child", "/fixture/feature")),
    );
    branch.pull_request = Some(pr.clone());
    branch.exists = true;
    let mut project = workspace("project", "Project", "/fixture", vec![branch]);
    project.home_issues.repository = Some("acme/app".into());
    project.pull_requests = vec![pr];
    runtime.snapshot.navigator.workspaces = vec![project];
    runtime.snapshot.navigator.agents = agents;
    assert!(runtime.refresh_agent_scopes());
    let board = &runtime.snapshot.navigator.workspaces[0].agent_scope.prs;
    assert_eq!(board.rows[0].agents, vec!["child", "root"]);
    assert_eq!(board.rows[0].group, "blocked");
    assert!(board.rows[0].needs_look);
    assert_eq!(board.rows[0].issue.as_ref().unwrap().label, "#7");
    assert_eq!(board.open, 1);
    assert_eq!(board.counts.blocked, 1);
    assert_eq!(board.groups[0].numbers, vec![42]);
    runtime.snapshot.navigator.workspaces[0].pull_requests[0].checks = PullRequestChecks::Passing;
    assert!(
        runtime.refresh_agent_scopes(),
        "GitHub changes invalidate work scope even when agents stay still"
    );
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .prs
            .rows[0]
            .group,
        "turn"
    );
    runtime.snapshot.navigator.agents[1].activity = "working".into();
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .prs
            .counts
            .look,
        1
    );
    runtime.snapshot.navigator.agents[1].group = "working".into();
    runtime.snapshot.navigator.agents[1].state =
        crate::agent_state::turn::row_state(&runtime.snapshot.navigator.agents[1]);
    assert!(runtime.refresh_agent_scopes());
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .prs
            .rows[0]
            .group,
        "fixing"
    );
    assert!(!runtime.refresh_agent_scopes());
}

#[test]
fn raised_sections_keep_five_questions_three_completions_and_first_number_owner() {
    let mut runtime = runtime();
    let prototype = rows()[0].clone();
    let agents: Vec<_> = (0..13)
        .map(|i| {
            let mut row = prototype.clone();
            row.pane_id = format!("p{i}");
            row.group = if i < 7 { "needs_you" } else { "done" }.into();
            row.state = crate::agent_state::turn::row_state(&row);
            row
        })
        .collect();
    let mut first = checkout(
        "project",
        "first",
        "/fixture/first",
        Some(pane("p0", "/fixture/first")),
    );
    first.tabs[0].panes = (0..12)
        .map(|i| pane(&format!("p{i}"), "/fixture/first"))
        .collect();
    let duplicate = checkout(
        "project",
        "second",
        "/fixture/second",
        Some(pane("p0", "/fixture/second")),
    );
    runtime.snapshot.navigator.agents = agents;
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "project",
        "Project",
        "/fixture",
        vec![first, duplicate],
    )];
    assert!(runtime.refresh_agent_scopes());
    let scope = &runtime
        .snapshot
        .navigator
        .devices
        .iter()
        .find(|d| d.kind != "remote")
        .unwrap()
        .agent_scope;
    assert_eq!(scope.raised[0].shown, vec!["p0", "p1", "p2", "p3", "p4"]);
    assert_eq!(scope.raised[0].more, vec!["p5", "p6"]);
    assert_eq!(scope.raised[1].shown, vec!["p7", "p8", "p9"]);
    assert_eq!(scope.raised[1].more, vec!["p10", "p11"]);
    assert_eq!(scope.owners["p0"], "first");
    assert!(
        !scope.owners.contains_key("p12"),
        "an agent outside every drawn checkout is not raised or numbered in Projects"
    );
}
