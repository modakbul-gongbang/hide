//! Status, membership and fold badges for the Agents graph.
//! Pixel geometry, text search and placement remain in the renderer.
use super::scope::Member;
use crate::model::{SidebarAgentSnapshot, WorkspaceSnapshot};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

type Badges = BTreeMap<String, BTreeMap<&'static str, usize>>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Graph {
    pub attention: u8,
    pub recency: String,
    pub checkouts: BTreeMap<String, Checkout>,
    /// The three opened folds plus project scope form a fixed 16-way selector.
    /// Identical badge maps share one payload, including the empty map.
    pub variants: [usize; 16],
    pub tucked: Vec<Badges>,
}
impl Default for Graph {
    fn default() -> Self {
        Self {
            attention: 4,
            recency: String::new(),
            checkouts: BTreeMap::new(),
            variants: [0; 16],
            tucked: vec![BTreeMap::new()],
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Checkout {
    pub primary: bool,
    pub cleanup: Option<&'static str>,
    pub fold: Option<&'static str>,
    pub members: Vec<String>,
    pub rank: u8,
    pub resting: bool,
}

pub(super) fn project(
    project: &WorkspaceSnapshot,
    members: &[Member],
    agents: &[&SidebarAgentSnapshot],
) -> Graph {
    let by_pane: HashMap<_, _> = agents.iter().map(|a| (a.pane_id.as_str(), *a)).collect();
    let primary = project.checkouts.iter().find(|c| c.is_primary).or_else(|| {
        if project.is_git {
            None
        } else {
            project.checkouts.first()
        }
    });
    let mut value = Graph::default();
    for checkout in &project.checkouts {
        let mut own: Vec<_> = members
            .iter()
            .filter(|m| m.checkout_id == checkout.id)
            .map(|m| by_pane[m.pane_id.as_str()])
            .collect();
        own.sort_by(|a, b| {
            a.state
                .graph_rank
                .cmp(&b.state.graph_rank)
                .then_with(|| b.last_activity.cmp(&a.last_activity))
        });
        let rank = own.first().map_or(4, |a| a.state.graph_rank);
        let resting = own.iter().all(|a| a.state.graph_rank == 3);
        let cleanup = if !checkout.is_worktree || checkout.is_primary || !project.is_git {
            None
        } else if !checkout.exists || checkout.worktree.as_ref().is_some_and(|w| w.missing) {
            Some("missing")
        } else if checkout.landed
            || checkout
                .pull_request
                .as_ref()
                .is_some_and(|p| p.badge == crate::model::PullRequestBadge::Merged)
        {
            Some("merged")
        } else {
            None
        };
        let fold = if cleanup.is_some() && resting {
            Some("cleanup")
        } else if own.is_empty() {
            Some("empty")
        } else if resting {
            Some("resting")
        } else {
            None
        };
        value.attention = value.attention.min(rank);
        for agent in &own {
            if agent.last_activity > value.recency {
                value.recency.clone_from(&agent.last_activity);
            }
        }
        value.checkouts.insert(
            checkout.id.clone(),
            Checkout {
                primary: primary.is_some_and(|p| p.id == checkout.id),
                cleanup,
                fold,
                members: own.iter().map(|a| a.pane_id.clone()).collect(),
                rank,
                resting,
            },
        );
    }
    let membership: HashMap<_, _> = members
        .iter()
        .map(|m| (m.pane_id.as_str(), m.checkout_id.as_str()))
        .collect();
    for selector in 0..16 {
        let shown: HashSet<_> = value
            .checkouts
            .iter()
            .filter(|(_, c)| {
                c.primary && selector & 8 != 0
                    || match c.fold {
                        None => true,
                        Some("empty") => selector & 1 != 0,
                        Some("cleanup") => selector & 2 != 0,
                        Some("resting") => selector & 4 != 0,
                        Some(other) => unreachable!("unknown graph fold {other}"),
                    }
            })
            .map(|(id, _)| id.as_str())
            .collect();
        let mut badges = Badges::new();
        for member in members {
            if shown.contains(member.checkout_id.as_str()) {
                continue;
            }
            let agent = by_pane[member.pane_id.as_str()];
            let state = match agent.symbol.as_str() {
                "×" => "error",
                "!" => "approval",
                "?" => "question",
                "●" => "working",
                "✓" => "done",
                "○" => "idle",
                _ => continue,
            };
            let mut ancestor = agent.lineage_parent_pane_id.as_deref();
            let mut seen = HashSet::new();
            while let Some(id) = ancestor {
                let Some(checkout) = membership.get(id) else {
                    break;
                };
                if shown.contains(checkout) {
                    *badges
                        .entry(id.to_owned())
                        .or_default()
                        .entry(state)
                        .or_default() += 1;
                    break;
                }
                if !seen.insert(id) {
                    break;
                }
                ancestor = by_pane[id].lineage_parent_pane_id.as_deref();
            }
        }
        value.variants[selector] =
            if let Some(index) = value.tucked.iter().position(|v| v == &badges) {
                index
            } else {
                let index = value.tucked.len();
                value.tucked.push(badges);
                index
            };
    }
    value
}
