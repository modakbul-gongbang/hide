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
fn graph_cross_project_chips_preserve_order_counts_and_stable_device_context() {
    // Incoming main PR 749 screen: Build before Spec, then Docs; no local
    // delegation chip, and distinct device IDs retain equal-label context.
    let mut runtime = runtime();
    let local_id = runtime.snapshot.navigator.devices[0].id.clone();
    runtime.snapshot.navigator.devices[0].label = "mini".into();
    let mut agents = rows();
    agents[0].identity_label = "Lead".into();
    agents[1].pane_id = "spec".into();
    agents[1].identity_label = "Spec".into();
    agents[1].state.graph_rank = 1;
    agents[1].last_activity = "2026-10-08T01:00:00Z".into();
    agents[1].lineage_parent_pane_id = Some("root".into());
    let mut build = agents[1].clone();
    build.pane_id = "build".into();
    build.identity_label = "Build".into();
    build.last_activity = "2026-10-08T02:00:00Z".into();
    let mut docs = build.clone();
    docs.pane_id = "docs".into();
    docs.identity_label = "Docs".into();
    docs.state.graph_rank = 3;
    let mut helper = build.clone();
    helper.pane_id = "helper".into();
    agents.extend([build, docs, helper]);
    runtime.snapshot.navigator.agents = agents;
    let project = |id: &str, names: &[&str]| {
        let mut project = workspace(
            id,
            id,
            "/fixture",
            names
                .iter()
                .map(|name| checkout(id, name, "/fixture", Some(pane(name, "/fixture"))))
                .collect(),
        );
        project.device_id.clone_from(&local_id);
        project
    };
    runtime.snapshot.navigator.workspaces = vec![
        project("ide", &["root", "helper"]),
        project("sasu", &["spec", "build"]),
        project("docs", &["docs"]),
    ];
    assert!(runtime.refresh_agent_scopes());
    let chips = &runtime.snapshot.navigator.workspaces[0]
        .agent_scope
        .graph
        .cross["root"];
    assert_eq!(
        chips
            .iter()
            .map(|c| (c.project_id.as_str(), c.count))
            .collect::<Vec<_>>(),
        [("sasu", 2), ("docs", 1)]
    );
    assert_eq!(chips[0].names, ["Build", "Spec"]);
    assert_eq!(chips[0].box_id, "build");
    assert!(chips[0].device.is_none());
    assert!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .graph
            .cross["helper"]
            .is_empty()
    );
    assert_eq!(
        runtime.snapshot.navigator.workspaces[1]
            .agent_scope
            .graph
            .cross["spec"][0]
            .names,
        ["Lead"]
    );
    assert!(!runtime.refresh_agent_scopes());
    runtime.snapshot.navigator.agents[2].identity_label = "Builder".into();
    assert!(
        runtime.refresh_agent_scopes(),
        "a label change updates chip names"
    );
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .graph
            .cross["root"][0]
            .names,
        ["Builder", "Spec"]
    );

    let mut remote_device = crate::workspace::local_device(&crate::node::test_node());
    remote_device.id = "other-device".into();
    remote_device.label = "mini".into();
    remote_device.kind = "remote".into();
    runtime.snapshot.navigator.devices.push(remote_device);
    let mut remote_project = project("remote-docs", &["remote:other-device:pane:p1"]);
    remote_project.device_id = "other-device".into();
    let mut remote_agent = runtime.snapshot.navigator.agents[3].clone();
    remote_agent.pane_id = "remote:other-device:pane:p1".into();
    runtime.snapshot.status.remote = vec![RemoteStatusSnapshot {
        target_id: "other-device".into(),
        state: "connected".into(),
        message: None,
        herdr_version: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
        session: Some(RemoteSessionSnapshot {
            workspaces: vec![remote_project],
            agents: vec![remote_agent],
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
    let remote_chips = &runtime.snapshot.status.remote[0]
        .session
        .as_ref()
        .unwrap()
        .workspaces[0]
        .agent_scope
        .graph
        .cross["remote:other-device:pane:p1"];
    assert_eq!(remote_chips[0].project_device_id, local_id);
    assert_eq!(remote_chips[0].device.as_ref().unwrap().label, None);
    let chips = &runtime.snapshot.navigator.workspaces[0]
        .agent_scope
        .graph
        .cross["root"];
    assert_eq!(chips[2].project_id, "remote-docs");
    assert_eq!(
        chips[2].device.as_ref().unwrap().label.as_deref(),
        Some("mini")
    );
    runtime.snapshot.status.remote[0].state = "disconnected".into();
    assert!(runtime.refresh_agent_scopes());
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .graph
            .cross["root"][2]
            .count,
        1,
        "last-known disconnected chips remain visible"
    );
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
    let mut duplicate = rows[0].clone();
    duplicate.activity = "stopped".into();
    duplicate.group = "idle".into();
    duplicate.identity_label = "Later duplicate".into();
    duplicate.state = crate::agent_state::turn::row_state(&duplicate);
    rows.push(duplicate);
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
    assert_eq!(
        project
            .agent_scope
            .rows
            .iter()
            .map(|r| r.occurrence)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(
        project
            .agent_scope
            .sections
            .iter()
            .flat_map(|s| &s.rows)
            .map(|r| r.occurrence)
            .collect::<Vec<_>>(),
        [0, 1],
        "physical section rows keep their distinct source occurrence"
    );
    assert_eq!(project.agent_scope.overview_total, 1);
    assert_eq!(project.agent_scope.buckets.working, 1);
    assert_eq!(project.agent_scope.buckets.resting, 0);
    assert_eq!(
        project.agent_scope.requests.counts[&crate::agent_state::RequestVerb::Working],
        1
    );
    assert_eq!(project.agent_scope.graph.checkouts["first"].rank, 1);
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
    assert_eq!(
        board.rows[0]
            .agents
            .iter()
            .map(|r| r.pane_id.as_str())
            .collect::<Vec<_>>(),
        vec!["child", "root"]
    );
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
    assert_eq!(
        scope.raised[0]
            .shown
            .iter()
            .map(|r| r.pane_id.as_str())
            .collect::<Vec<_>>(),
        vec!["p0", "p1", "p2", "p3", "p4"]
    );
    assert_eq!(
        scope.raised[0]
            .more
            .iter()
            .map(|r| r.pane_id.as_str())
            .collect::<Vec<_>>(),
        vec!["p5", "p6"]
    );
    assert_eq!(
        scope.raised[1]
            .shown
            .iter()
            .map(|r| r.pane_id.as_str())
            .collect::<Vec<_>>(),
        vec!["p7", "p8", "p9"]
    );
    assert_eq!(
        scope.raised[1]
            .more
            .iter()
            .map(|r| r.pane_id.as_str())
            .collect::<Vec<_>>(),
        vec!["p10", "p11"]
    );
    assert_eq!(scope.owners["p0"], "first");
    assert!(
        !scope.owners.contains_key("p12"),
        "an agent outside every drawn checkout is not raised or numbered in Projects"
    );
}

#[test]
fn request_work_keeps_current_chips_and_expanded_history_in_source_order() {
    use crate::request_view::{
        AgentPullRequestSnapshot, AgentRequestSnapshot, RequestLineSnapshot, RequestSender,
    };
    let mut runtime = runtime();
    let mut agents = rows();
    let issue = |n: u32, open, closed| crate::tasks::TaskSnapshot {
        key: format!("github:acme/app#{n}"),
        source: "github".into(),
        id: Some(format!("#{n}")),
        url: None,
        title: format!("Issue {n}"),
        open,
        updated_at_unix_ms: Some(200),
        created_at_unix_ms: None,
        closed_at_unix_ms: closed,
        blocked_by: vec![],
        sub_issues: None,
        labels: vec![],
    };
    let pull = |n, live, issues: &[u32]| AgentPullRequestSnapshot {
        number: n,
        title: format!("PR {n}"),
        url: format!("https://github.com/acme/app/pull/{n}"),
        badge: crate::model::PullRequestBadge::Open,
        checks: PullRequestChecks::None,
        head_branch: "feature".into(),
        closing_issues: issues
            .iter()
            .map(|n| {
                serde_json::from_value(serde_json::json!({"repository":"acme/app","number":n}))
                    .unwrap()
            })
            .collect(),
        live,
        duty: false,
        created: false,
        settled_at_unix_ms: None,
    };
    agents[0].request = Some(AgentRequestSnapshot {
        verb: crate::agent_state::RequestVerb::Review,
        verb_since_unix_ms: 100,
        line: None,
        end: None,
        request: Some(RequestLineSnapshot {
            text: "Next work".into(),
            cut: false,
            images: 0,
            at_unix_ms: 100,
            sender: RequestSender::Operator,
        }),
        later_by: None,
        reply: None,
        pull_requests: vec![
            pull(1, false, &[7]),
            pull(2, true, &[8]),
            pull(3, true, &[9, 10, 11, 99]),
        ],
    });
    let mut main = checkout(
        "project",
        "main",
        "/fixture",
        Some(pane("root", "/fixture")),
    );
    main.task_key = Some("github:acme/app#7".into());
    let mut project = workspace("project", "Project", "/fixture", vec![main]);
    project.tasks.tasks = vec![
        issue(7, false, Some(99)),
        issue(8, false, Some(101)),
        issue(9, true, None),
        issue(10, false, None),
        issue(11, false, Some(100)),
    ];
    runtime.snapshot.navigator.workspaces = vec![project];
    runtime.snapshot.navigator.agents = agents;
    assert!(runtime.refresh_agent_scopes());
    let work = &runtime.snapshot.navigator.workspaces[0].agent_scope.work["root"];
    assert_eq!((work.pull, work.more), (Some(1), 1));
    assert_eq!(
        work.issues,
        [
            "github:acme/app#8",
            "github:acme/app#7",
            "github:acme/app#9",
            "github:acme/app#10",
            "github:acme/app#11"
        ]
    );
    assert_eq!(work.issue_chips, ["github:acme/app#8", "github:acme/app#9"]);
    runtime.snapshot.navigator.workspaces[0].tasks.tasks[1].closed_at_unix_ms = Some(100);
    assert!(runtime.refresh_agent_scopes());
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0].agent_scope.work["root"].issue_chips,
        ["github:acme/app#9"]
    );
}

#[test]
fn factory_workers_leave_requests_and_overview_count_without_leaving_physical_lists() {
    let mut runtime = runtime();
    let mut agents = rows();
    agents[0].group = "needs_you".into();
    agents[0].demand = "question".into();
    agents[0].unread = true;
    agents[0].state = crate::agent_state::turn::row_state(&agents[0]);
    runtime.snapshot.navigator.agents = agents;
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "project",
        "Project",
        "/fixture",
        vec![checkout(
            "project",
            "main",
            "/fixture",
            Some(pane("root", "/fixture")),
        )],
    )];
    runtime.refresh_agent_scopes();
    assert_eq!(
        runtime.snapshot.navigator.devices[0]
            .agent_scope
            .overview_needs_you,
        1
    );
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .requests
            .rows
            .len(),
        1
    );
    let summary = serde_json::from_value(serde_json::json!({
        "my_turn": 1, "inbox": [], "factories": [{
            "id": "f-1", "project": "/fixture", "project_name": "Fixture", "source": "local", "verification": "none", "closed": false,
            "flow": {"drafting": 0, "waiting": 0, "running": 1, "done_today": 0}, "my_turn": 1,
            "columns": [{"column": "running", "label": "", "cards": [{
                "task": "T-1", "display_id": "T-1", "column": "running", "title": "Fixture", "state": "running", "state_label": "", "needs_person": true,
                "waiting_on": [], "priority": 0, "since": 0, "unread": false, "folded": false, "archived": false, "failures": 0, "external": [], "worker_pane": "root"
            }]}], "cancelled": [], "graph": {"nodes": [], "edges": [], "unrelated": []}, "dependencies": [],
            "stale": false, "main_broken": false, "auto_merge_available": false, "merge_mode": "manual"
        }]
    })).unwrap();
    runtime.set_factory_screen(Some(std::sync::Arc::new(summary)), None);
    let scope = &runtime.snapshot.navigator.devices[0].agent_scope;
    assert_eq!(
        scope.groups.needs_you, 1,
        "the physical list still includes its worker"
    );
    assert_eq!(scope.overview_needs_you, 0);
    assert!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .requests
            .rows
            .is_empty()
    );
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .requests
            .todo,
        0
    );
    assert!(
        !runtime.refresh_agent_scopes(),
        "unchanged worker membership publishes nothing"
    );
    runtime.set_factory_screen(
        Some(std::sync::Arc::new(hide_factory::FactorySummary::default())),
        None,
    );
    assert_eq!(
        runtime.snapshot.navigator.devices[0]
            .agent_scope
            .overview_needs_you,
        1
    );
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .agent_scope
            .requests
            .rows
            .len(),
        1
    );
}

#[test]
fn palette_and_pr_lineage_keep_first_anchor_and_last_ancestor_occurrences() {
    let mut runtime = runtime();
    let mut agents = rows();
    agents[1].lineage_parent_pane_id = Some("root".into());
    let mut later_child = agents[1].clone();
    later_child.identity_label = "Later child".into();
    later_child.lineage_parent_pane_id = None;
    let mut later_root = agents[0].clone();
    later_root.identity_label = "Later parent".into();
    agents.extend([later_child, later_root]);
    let pr: PullRequestSnapshot = serde_json::from_value(serde_json::json!({
        "number":42,"title":"Ship","url":"https://github.com/acme/app/pull/42",
        "head_branch":"feature","base_branch":"main","badge":"open","checks":"passing","is_draft":false,"closing_issues":[]
    })).unwrap();
    let mut branch = checkout(
        "project",
        "feature",
        "/fixture/feature",
        Some(pane("child", "/fixture/feature")),
    );
    branch.pull_request = Some(pr.clone());
    branch.exists = true;
    let mut project = workspace("project", "Project", "/fixture", vec![branch]);
    project.pull_requests = vec![pr];
    runtime.snapshot.navigator.workspaces = vec![project];
    runtime.snapshot.navigator.agents = agents;
    assert!(runtime.refresh_agent_scopes());
    let scope = &runtime.snapshot.navigator.workspaces[0].agent_scope;
    assert_eq!(
        scope.prs.rows[0]
            .lineage
            .iter()
            .map(|r| (r.pane_id.as_str(), r.occurrence, r.depth))
            .collect::<Vec<_>>(),
        [("root", 1, 0), ("child", 0, 1)]
    );
    let relation = &runtime.snapshot.navigator.devices[0].agent_scope.relations["child"][0];
    assert_eq!(
        relation
            .rows
            .iter()
            .map(|r| (r.pane_id.as_str(), r.occurrence, r.tag))
            .collect::<Vec<_>>(),
        [("child", 0, Some("here")), ("root", 1, Some("parent"))]
    );
    assert!(!runtime.refresh_agent_scopes());
}
