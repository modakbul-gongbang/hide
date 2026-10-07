//! Cross-project graph chips preserve the first global pane occurrence and
//! stable device identity. The renderer supplies translations and navigation.
use crate::model::{SidebarAgentSnapshot, WorkspaceSnapshot};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CrossDevice {
    /// None names this machine; a remote label is translated only by the shell.
    pub label: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CrossChip {
    pub direction: &'static str,
    pub project_id: String,
    pub project_device_id: String,
    pub device: Option<CrossDevice>,
    pub pane_ids: Vec<String>,
    pub names: Vec<String>,
    pub box_id: String,
    pub count: usize,
}

pub(crate) struct GraphMember<'a> {
    pub project: &'a WorkspaceSnapshot,
    pub agent: &'a SidebarAgentSnapshot,
    pub checkout_id: String,
    pub device_label: Option<&'a str>,
}

pub(crate) struct Lineage<'a> {
    by_pane: HashMap<&'a str, GraphMember<'a>>,
    children: HashMap<&'a str, Vec<&'a str>>,
}

fn priority(a: &GraphMember<'_>, b: &GraphMember<'_>) -> Ordering {
    a.agent
        .state
        .graph_rank
        .cmp(&b.agent.state.graph_rank)
        .then_with(|| b.agent.last_activity.cmp(&a.agent.last_activity))
}

impl<'a> Lineage<'a> {
    pub(crate) fn new(members: impl IntoIterator<Item = GraphMember<'a>>) -> Self {
        let mut value = Self {
            by_pane: HashMap::new(),
            children: HashMap::new(),
        };
        for member in members {
            let id = member.agent.pane_id.as_str();
            if value.by_pane.contains_key(id) {
                continue;
            }
            if let Some(parent) = member.agent.lineage_parent_pane_id.as_deref()
                && parent != id
            {
                value.children.entry(parent).or_default().push(id);
            }
            value.by_pane.insert(id, member);
        }
        value
    }

    pub(crate) fn chips(
        &self,
        project: &WorkspaceSnapshot,
        agent: &SidebarAgentSnapshot,
    ) -> Vec<CrossChip> {
        let mut groups: Vec<Vec<&GraphMember<'_>>> = Vec::new();
        let mut group_index = HashMap::new();
        for id in self
            .children
            .get(agent.pane_id.as_str())
            .into_iter()
            .flatten()
        {
            let other = &self.by_pane[id];
            if other.project.id == project.id {
                continue;
            }
            let index = *group_index
                .entry(other.project.id.as_str())
                .or_insert_with(|| {
                    groups.push(Vec::new());
                    groups.len() - 1
                });
            groups[index].push(other);
        }
        for group in &mut groups {
            group.sort_by(|a, b| priority(a, b));
        }
        // Stable ties retain the incoming project and agent order, as on main.
        groups.sort_by(|a, b| priority(a[0], b[0]));
        let mut chips: Vec<_> = groups
            .iter()
            .map(|g| Self::chip("out", project, g))
            .collect();
        if let Some(parent) = agent
            .lineage_parent_pane_id
            .as_deref()
            .and_then(|id| self.by_pane.get(id))
            && parent.project.id != project.id
        {
            chips.push(Self::chip("in", project, &[parent]));
        }
        chips
    }

    fn chip(
        direction: &'static str,
        project: &WorkspaceSnapshot,
        others: &[&GraphMember<'_>],
    ) -> CrossChip {
        let first = others[0];
        CrossChip {
            direction,
            project_id: first.project.id.clone(),
            project_device_id: first.project.device_id.clone(),
            device: (project.device_id != first.project.device_id).then(|| CrossDevice {
                label: first.device_label.map(str::to_owned),
            }),
            pane_ids: others.iter().map(|m| m.agent.pane_id.clone()).collect(),
            names: others
                .iter()
                .map(|m| m.agent.identity_label.clone())
                .collect(),
            box_id: first.checkout_id.clone(),
            count: others.len(),
        }
    }
}
