//! Derived project reading order and current checkout context. No I/O or clock.
use std::collections::{HashMap, HashSet};

use crate::model::{
    CheckoutPaneContext, CheckoutSnapshot, SidebarAgentSnapshot, WorkspaceSnapshot,
};

/// Wall time and server sequence are separate domains. A sequence is only a
/// tie-breaker, never interpreted as milliseconds. Missing facts remain absent.
#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
struct Activity {
    unix_ms: Option<u64>,
    sequence: Option<u64>,
}

pub(crate) fn sort_projects(
    projects: &mut [WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
) -> bool {
    let by_pane: HashMap<_, _> = agents
        .iter()
        .map(|agent| (agent.pane_id.as_str(), agent))
        .collect();
    let before: Vec<_> = projects
        .iter()
        .map(|project| {
            (
                project.id.clone(),
                project.last_activity_unix_ms,
                project
                    .checkouts
                    .iter()
                    .map(|c| c.id.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let mut project_activity = HashMap::new();
    for project in projects.iter_mut() {
        let mut keys = HashMap::new();
        let mut recent = Activity::default();
        for checkout in &project.checkouts {
            let mut key = Activity {
                unix_ms: checkout
                    .worktree
                    .as_ref()
                    .and_then(|w| w.last_commit_unix_seconds)
                    .and_then(|s| s.checked_mul(1000)),
                sequence: None,
            };
            for pane in checkout.tabs.iter().flat_map(|tab| &tab.panes) {
                if let Some(agent) = by_pane.get(pane.id.as_str()) {
                    // The existing sidebar validates the plugin's 13-digit
                    // activity token; its padded sequence has 20 digits.
                    let timestamp = (agent.last_activity.len() == 13)
                        .then(|| agent.last_activity.parse::<u64>().ok())
                        .flatten();
                    key.unix_ms = key.unix_ms.max(timestamp);
                    key.sequence = key.sequence.max(agent.state_change_seq);
                }
            }
            recent = recent.max(key);
            keys.insert(checkout.id.clone(), key);
        }
        project.checkouts.sort_by(|left, right| {
            keys[&right.id]
                .cmp(&keys[&left.id])
                .then_with(|| left.id.cmp(&right.id))
        });
        // The order's own reason, carried to the shell so the row can show it.
        // Only the wall time travels: a server sequence is a tie-breaker, not
        // a date, and rendering it as one would invent recency.
        project.last_activity_unix_ms = recent.unix_ms;
        project_activity.insert(project.id.clone(), recent);
    }
    projects.sort_by(|left, right| {
        left.device_id
            .cmp(&right.device_id)
            .then_with(|| project_activity[&right.id].cmp(&project_activity[&left.id]))
            .then_with(|| left.id.cmp(&right.id))
    });
    projects
        .iter()
        .zip(before)
        .any(|(project, (id, last_activity_unix_ms, checkouts))| {
            project.id != id
                || project.last_activity_unix_ms != last_activity_unix_ms
                || project.checkouts.iter().map(|c| &c.id).ne(checkouts.iter())
        })
}

pub(crate) fn checkout_panes(
    checkout: &CheckoutSnapshot,
    projects: &[WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
) -> Vec<CheckoutPaneContext> {
    let live: HashSet<_> = projects
        .iter()
        .flat_map(|w| &w.checkouts)
        .flat_map(|c| &c.tabs)
        .flat_map(|t| &t.panes)
        .map(|p| p.id.as_str())
        .collect();
    let agents: HashMap<_, _> = agents.iter().map(|a| (a.pane_id.as_str(), a)).collect();
    checkout
        .tabs
        .iter()
        .flat_map(|tab| {
            tab.panes.iter().map(|pane| {
                let agent = agents.get(pane.id.as_str());
                CheckoutPaneContext {
                    pane_id: pane.id.clone(),
                    title: pane
                        .herdr_label
                        .as_ref()
                        .or(pane.terminal_title.as_ref())
                        .or(pane.summary.as_ref())
                        .cloned()
                        .unwrap_or_else(|| pane.id.clone()),
                    status: pane.status_label.clone(),
                    session_id: agent.and_then(|a| a.session_id.clone()),
                    parent_pane_id: agent
                        .and_then(|a| a.spawned_from_pane_id.as_ref())
                        .filter(|id| live.contains(id.as_str()))
                        .cloned(),
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::model::{PaneSnapshot, TabSnapshot, WorktreeSnapshot};

    /// A project row is a project id, the commit time of its one checkout, and
    /// the panes an agent may be running in. Everything else the ordering does
    /// not read stays at its default.
    fn project(
        id: &str,
        device_id: &str,
        last_commit_unix_seconds: Option<u64>,
        pane_ids: &[&str],
    ) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            id: id.to_owned(),
            label: id.to_owned(),
            path: format!("/fixture/{id}"),
            remote_target_id: None,
            expanded: true,
            device_id: device_id.to_owned(),
            repo_name: id.to_owned(),
            is_git: true,
            default_branch: None,
            branches: Vec::new(),
            registered: true,
            temporary: false,
            session_workspace_ids: Vec::new(),
            last_activity_unix_ms: None,
            checkouts: vec![checkout(id, last_commit_unix_seconds, pane_ids)],
        }
    }

    fn checkout(
        id: &str,
        last_commit_unix_seconds: Option<u64>,
        pane_ids: &[&str],
    ) -> CheckoutSnapshot {
        CheckoutSnapshot {
            id: id.to_owned(),
            worktree: last_commit_unix_seconds.map(|seconds| WorktreeSnapshot {
                last_commit_unix_seconds: Some(seconds),
                ..Default::default()
            }),
            tabs: vec![TabSnapshot {
                id: None,
                workspace_id: None,
                checkout_id: None,
                label: None,
                empty: pane_ids.is_empty(),
                delegated: false,
                panes: pane_ids
                    .iter()
                    .map(|pane_id| PaneSnapshot {
                        id: (*pane_id).to_owned(),
                        content: Default::default(),
                        herdr_label: None,
                        terminal_title: None,
                        workspace_label: None,
                        cwd: "/fixture".to_owned(),
                        status_label: "Unknown".to_owned(),
                        requires_close_confirmation: false,
                        summary: None,
                        activity_at_unix_ms: None,
                        fork: Default::default(),
                        ports: vec![],
                        children: None,
                        lineage_path: Vec::new(),
                    })
                    .collect(),
            }],
            ..Default::default()
        }
    }

    /// Agents come through the real projection rather than a handwritten
    /// struct, so the tests read the same 13-digit activity token the sidebar
    /// validates.
    fn agents(rows: Value) -> Vec<SidebarAgentSnapshot> {
        crate::sidebar::project_agents(
            serde_json::from_value(json!({ "agents": rows })).expect("valid fixture"),
        )
        .agents
    }

    fn ids(projects: &[WorkspaceSnapshot]) -> Vec<&str> {
        projects.iter().map(|p| p.id.as_str()).collect()
    }

    /// B1, B2. The newest activity in a project comes first inside its device
    /// group, whether that activity is a commit or an agent, and the carried
    /// timestamp is the same one the order was decided by.
    #[test]
    fn projects_order_by_newest_activity_within_a_device() {
        let mut projects = vec![
            project("stale", "local", Some(1_000), &[]),
            project("committed", "local", Some(3_000), &[]),
            project("agent", "local", Some(2_000), &["pane-agent"]),
        ];
        let agents = agents(json!([
            {"pane_id": "pane-agent", "state_change_seq": 1, "tokens": {"activity": "0000004000000"}},
        ]));

        assert!(sort_projects(&mut projects, &agents));

        assert_eq!(ids(&projects), ["agent", "committed", "stale"]);
        assert_eq!(projects[0].last_activity_unix_ms, Some(4_000_000));
        assert_eq!(projects[1].last_activity_unix_ms, Some(3_000_000));
        assert_eq!(projects[2].last_activity_unix_ms, Some(1_000_000));
    }

    /// B1. Equal activity keeps the existing order, which is the project id.
    /// A tie is not an invitation to shuffle the list on every projection.
    #[test]
    fn equal_activity_falls_back_to_the_project_id() {
        let mut projects = vec![
            project("beta", "local", Some(2_000), &[]),
            project("alpha", "local", Some(2_000), &[]),
        ];

        assert!(sort_projects(&mut projects, &[]));
        assert_eq!(ids(&projects), ["alpha", "beta"]);

        // Already ordered: the same input reports no change, so an idle tick
        // publishes nothing.
        assert!(!sort_projects(&mut projects, &[]));
    }

    /// B1, B4. A project with neither a commit nor an agent has no activity to
    /// claim. It sorts below every project that has one and carries no
    /// timestamp, which is what lets the row leave its time blank.
    #[test]
    fn projects_without_activity_sort_last_and_carry_no_time() {
        let mut projects = vec![
            project("quiet", "local", None, &[]),
            project("active", "local", Some(5_000), &[]),
        ];

        assert!(sort_projects(&mut projects, &[]));

        assert_eq!(ids(&projects), ["active", "quiet"]);
        assert_eq!(projects[0].last_activity_unix_ms, Some(5_000_000));
        assert_eq!(projects[1].last_activity_unix_ms, None);
    }

    /// B1. Device groups stay whole: activity orders projects inside a device,
    /// never across two of them.
    #[test]
    fn device_groups_are_ordered_before_activity() {
        let mut projects = vec![
            project("remote-old", "mini", Some(1_000), &[]),
            project("local-old", "local", Some(2_000), &[]),
            project("remote-new", "mini", Some(9_000), &[]),
            project("local-new", "local", Some(8_000), &[]),
        ];

        sort_projects(&mut projects, &[]);

        assert_eq!(
            ids(&projects),
            ["local-new", "local-old", "remote-new", "remote-old"]
        );
    }

    /// B2. A pane's agent moving is enough to raise its project, and the move
    /// is reported so the snapshot publishes.
    #[test]
    fn new_agent_activity_raises_its_project_and_reports_the_change() {
        let mut projects = vec![
            project("busy", "local", Some(9_000), &[]),
            project("waking", "local", Some(1_000), &["pane-waking"]),
        ];
        sort_projects(&mut projects, &[]);
        assert_eq!(ids(&projects), ["busy", "waking"]);

        let awake = agents(json!([
            {"pane_id": "pane-waking", "state_change_seq": 2, "tokens": {"activity": "0000010000000"}},
        ]));

        assert!(sort_projects(&mut projects, &awake));
        assert_eq!(ids(&projects), ["waking", "busy"]);
        assert_eq!(projects[0].last_activity_unix_ms, Some(10_000_000));
    }
}
