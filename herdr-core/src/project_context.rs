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
        .any(|(project, (id, checkouts))| {
            project.id != id || project.checkouts.iter().map(|c| &c.id).ne(checkouts.iter())
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
