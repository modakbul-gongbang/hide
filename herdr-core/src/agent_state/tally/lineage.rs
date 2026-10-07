//! Surface-specific lineage membership and marks. The desktop's locale-aware
//! alphabetical placement remains presentation; core resolves status priority
//! first and publishes only the tied candidates in each priority tier.
use super::scope::descendants;
use crate::model::{DescendantCountsSnapshot, SidebarAgentSnapshot, WorkspaceSnapshot};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Folded {
    pub tiers: Vec<Vec<Line>>,
    pub overflow: usize,
    pub badge_descendants: usize,
    pub badge_counts: DescendantCountsSnapshot,
    pub badge_children: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Line {
    pub key: String,
    pub candidates: Vec<String>,
    pub branch: Option<String>,
    pub pull_request: Option<u32>,
    pub device: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Tree {
    pub rows: Vec<TreeRow>,
    pub visible_rows: Vec<TreeRow>,
    pub shown: Vec<String>,
    pub more: usize,
    pub needs_you: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TreeRow {
    pub pane_id: String,
    pub depth: usize,
}

/// First physical checkout owns an agent in a project. The whole lineage
/// follows it there, including descendants working in another checkout.
pub(super) fn checkout_trees(
    project: &WorkspaceSnapshot,
    agents: &[&SidebarAgentSnapshot],
) -> HashMap<String, Tree> {
    let mut owners = HashMap::new();
    for checkout in &project.checkouts {
        for pane in checkout.tabs.iter().flat_map(|t| &t.panes) {
            owners
                .entry(pane.id.as_str())
                .or_insert(checkout.id.as_str());
        }
    }
    let mut by_pane = HashMap::new();
    for agent in agents {
        by_pane.entry(agent.pane_id.as_str()).or_insert(*agent);
    }
    fn visit(
        agent: &SidebarAgentSnapshot,
        depth: usize,
        visible: bool,
        index: &HashMap<&str, &SidebarAgentSnapshot>,
        seen: &mut HashSet<String>,
        tree: &mut Tree,
    ) {
        if !seen.insert(agent.pane_id.clone()) {
            return;
        }
        let row = TreeRow {
            pane_id: agent.pane_id.clone(),
            depth,
        };
        tree.rows.push(row.clone());
        if visible {
            tree.visible_rows.push(row);
        }
        tree.needs_you |= agent.state.needs_you || (depth == 0 && agent.group == "done");
        for id in &agent.lineage_child_pane_ids {
            if let Some(child) = index.get(id.as_str()) {
                visit(
                    child,
                    depth + 1,
                    visible && !agent.lineage_collapsed,
                    index,
                    seen,
                    tree,
                );
            }
        }
    }
    project
        .checkouts
        .iter()
        .map(|checkout| {
            let local: Vec<_> = agents
                .iter()
                .copied()
                .filter(|a| owners.get(a.pane_id.as_str()).copied() == Some(checkout.id.as_str()))
                .collect();
            let ids: HashSet<_> = local.iter().map(|a| a.pane_id.as_str()).collect();
            let mut tree = Tree::default();
            let mut seen = HashSet::new();
            for root in local.iter().filter(|a| {
                !a.lineage_parent_pane_id
                    .as_deref()
                    .is_some_and(|id| ids.contains(id))
            }) {
                visit(root, 0, true, &by_pane, &mut seen, &mut tree);
            }
            let mut shown = tree
                .rows
                .iter()
                .map(|r| by_pane[r.pane_id.as_str()])
                .collect::<Vec<_>>();
            shown.sort_by_key(|a| a.state.attention_rank);
            tree.more = shown.len().saturating_sub(2);
            tree.shown = shown.iter().take(2).map(|a| a.pane_id.clone()).collect();
            (checkout.id.clone(), tree)
        })
        .collect()
}

pub(super) fn folded(
    agents: &[&SidebarAgentSnapshot],
    workspaces: &[&WorkspaceSnapshot],
    places: &HashMap<&str, (&str, &str)>,
) -> BTreeMap<String, Folded> {
    let by_pane: HashMap<_, _> = agents
        .iter()
        .map(|row| (row.pane_id.as_str(), *row))
        .collect();
    let mut facts = HashMap::new();
    for workspace in workspaces {
        for checkout in &workspace.checkouts {
            for pane in checkout.tabs.iter().flat_map(|tab| &tab.panes) {
                facts.insert(
                    pane.id.as_str(),
                    (
                        format!("{}\0{}", workspace.device_id, checkout.id),
                        checkout,
                    ),
                );
            }
        }
    }
    agents
        .iter()
        .map(|parent| {
            let parent_fact = facts.get(parent.pane_id.as_str());
            let same = |row: &&SidebarAgentSnapshot| match parent_fact {
                Some((key, _)) => facts
                    .get(row.pane_id.as_str())
                    .is_some_and(|(other, _)| key == other),
                None => row.checkout_label == parent.checkout_label,
            };
            let descendants = descendants(parent, &by_pane);
            let mut result = Folded::default();
            for row in descendants.iter().filter(|row| same(row)) {
                result.badge_descendants += 1;
                match row.demand.as_str() {
                    "error" => result.badge_counts.error += 1,
                    "approval" => result.badge_counts.approval += 1,
                    "question" => result.badge_counts.question += 1,
                    _ if row.activity == "working" => result.badge_counts.working += 1,
                    _ if row.group == "done" => result.badge_counts.done += 1,
                    _ => {}
                }
            }
            result.badge_children = parent
                .lineage_child_pane_ids
                .iter()
                .filter_map(|id| by_pane.get(id.as_str()))
                .filter(|row| same(row))
                .map(|a| a.pane_id.clone())
                .collect();
            // A vector retains first encounter order, the old Map's order, when
            // both status and the locale's alphabetic comparison tie.
            let mut groups: Vec<(String, Vec<&SidebarAgentSnapshot>)> = Vec::new();
            for row in descendants.iter().filter(|row| !same(row)) {
                let key = facts
                    .get(row.pane_id.as_str())
                    .map(|(key, _)| key.clone())
                    .unwrap_or_else(|| {
                        format!(
                            "{}\0{}",
                            places.get(row.pane_id.as_str()).map_or("unknown", |p| p.0),
                            row.checkout_label.as_deref().unwrap_or(&row.pane_id)
                        )
                    });
                if let Some((_, rows)) = groups.iter_mut().find(|(k, _)| k == &key) {
                    rows.push(row);
                } else {
                    groups.push((key, vec![row]));
                }
            }
            result.overflow = groups.len().saturating_sub(3);
            let mut tiers = BTreeMap::<u8, Vec<Line>>::new();
            for (key, rows) in groups {
                let rank = rows
                    .iter()
                    .map(|a| a.state.attention_rank)
                    .min()
                    .expect("lineage groups are nonempty");
                let candidates: Vec<_> = rows
                    .iter()
                    .filter(|a| a.state.attention_rank == rank)
                    .collect();
                let row = candidates[0];
                let fact = facts.get(row.pane_id.as_str()).map(|(_, c)| *c);
                let device = places.get(row.pane_id.as_str());
                let parent_device = places.get(parent.pane_id.as_str());
                tiers.entry(rank).or_default().push(Line {
                    key,
                    candidates: candidates.iter().map(|a| a.pane_id.clone()).collect(),
                    branch: fact
                        .map(|c| c.branch.clone().unwrap_or_else(|| c.label.clone()))
                        .or_else(|| row.checkout_label.clone()),
                    pull_request: fact.and_then(|c| c.pull_request.as_ref().map(|p| p.number)),
                    device: device
                        .filter(|d| parent_device.map(|p| p.0) != Some(d.0))
                        .map(|d| d.1.to_owned()),
                });
            }
            result.tiers = tiers.into_values().collect();
            (parent.pane_id.clone(), result)
        })
        .collect()
}
