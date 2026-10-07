//! The PR list's session ownership and whose move it is, preserving the
//! branch-only decision even when the maker is now working elsewhere.
use crate::agent_state::tally::lineage::{Tree, TreeRow};
use crate::model::{
    PullRequestBadge, PullRequestChecks, ReviewDecision, SidebarAgentSnapshot, WorkspaceSnapshot,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Board {
    pub rows: Vec<Row>,
    pub groups: Vec<Group>,
    pub open: usize,
    pub counts: Counts,
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Counts {
    pub turn: usize,
    pub fixing: usize,
    pub blocked: usize,
    pub review: usize,
    pub draft: usize,
    pub look: usize,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Group {
    pub group: &'static str,
    pub numbers: Vec<u32>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Row {
    pub number: u32,
    pub checkout_id: Option<String>,
    pub agents: Vec<crate::agent_state::tally::scope::RowRef>,
    pub lineage: Vec<TreeRow>,
    pub needs_look: bool,
    pub group: &'static str,
    pub issue: Option<Issue>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Issue {
    pub key: String,
    pub label: String,
    pub url: Option<String>,
    pub task_key: Option<String>,
}

fn lineage(rows: &[TreeRow], agents: &[&SidebarAgentSnapshot]) -> Vec<TreeRow> {
    let by_pane: HashMap<_, _> = agents.iter().map(|a| (a.pane_id.as_str(), *a)).collect();
    let mut shown: HashSet<_> = rows.iter().map(|r| r.pane_id.clone()).collect();
    let mut result = Vec::new();
    for (start, root) in rows.iter().enumerate().filter(|(_, r)| r.depth == 0) {
        let mut ancestors = Vec::new();
        let mut parent = by_pane[root.pane_id.as_str()]
            .lineage_parent_pane_id
            .as_deref()
            .and_then(|id| by_pane.get(id));
        while let Some(row) = parent {
            if shown.contains(&row.pane_id) || ancestors.len() >= 8 {
                break;
            }
            ancestors.insert(0, row.pane_id.clone());
            parent = row
                .lineage_parent_pane_id
                .as_deref()
                .and_then(|id| by_pane.get(id));
        }
        for (depth, id) in ancestors.iter().enumerate() {
            shown.insert(id.clone());
            result.push(TreeRow {
                pane_id: id.clone(),
                depth,
            });
        }
        for (offset, row) in rows[start..].iter().enumerate() {
            if offset > 0 && row.depth == 0 {
                break;
            }
            result.push(TreeRow {
                pane_id: row.pane_id.clone(),
                depth: row.depth + ancestors.len(),
            });
        }
    }
    result
}

pub(crate) fn project(
    workspace: &WorkspaceSnapshot,
    agents: &[&SidebarAgentSnapshot],
    trees: &HashMap<String, Tree>,
) -> Board {
    let tasks: HashMap<_, _> = workspace
        .tasks
        .tasks
        .iter()
        .map(|t| (t.key.as_str(), t))
        .collect();
    let mut result = Board::default();
    let references = crate::agent_state::tally::scope::row_references(agents);
    for pr in &workspace.pull_requests {
        let checkout = workspace.checkouts.iter().find(|c| {
            c.pull_request.as_ref().is_some_and(|p| p.url == pr.url) && (c.is_worktree || c.exists)
        });
        let panes: HashSet<_> = checkout
            .into_iter()
            .flat_map(|c| &c.tabs)
            .flat_map(|t| &t.panes)
            .map(|p| p.id.as_str())
            .collect();
        let mut here: Vec<_> = agents
            .iter()
            .copied()
            .filter(|a| {
                panes.contains(a.pane_id.as_str())
                    || a.request.as_ref().is_some_and(|r| {
                        r.pull_requests.iter().any(|p| p.created && p.url == pr.url)
                    })
            })
            .collect();
        here.sort_by_key(|a| {
            if a.state.needs_you {
                0
            } else if a.group == "done" {
                1
            } else if a.state.working {
                2
            } else {
                3
            }
        });
        let branch: Vec<_> = here
            .iter()
            .filter(|a| panes.contains(a.pane_id.as_str()))
            .collect();
        let merged = pr.badge == PullRequestBadge::Merged;
        let group = if merged {
            "merged"
        } else if branch.iter().any(|a| a.state.working) {
            "fixing"
        } else if pr.checks == PullRequestChecks::Failed
            || pr.review == Some(ReviewDecision::ChangesRequested)
        {
            "blocked"
        } else {
            "turn"
        };
        let linked = checkout
            .and_then(|c| c.task_key.as_deref())
            .and_then(|key| tasks.get(key));
        let issue = linked
            .map(|task| Issue {
                key: task.key.clone(),
                label: task.id.clone().unwrap_or_else(|| task.title.clone()),
                url: task.url.clone(),
                task_key: Some(task.key.clone()),
            })
            .or_else(|| {
                pr.closing_issues.first().map(|reference| {
                    let key = format!("github:{}#{}", reference.repository, reference.number);
                    let task = tasks.get(key.as_str());
                    let fallback = if workspace.home_issues.repository.as_deref()
                        == Some(reference.repository.as_str())
                    {
                        format!("#{}", reference.number)
                    } else {
                        format!("{}#{}", reference.repository, reference.number)
                    };
                    Issue {
                        key,
                        label: task.and_then(|t| t.id.clone()).unwrap_or(fallback),
                        url: Some(task.and_then(|t| t.url.clone()).unwrap_or_else(|| {
                            format!(
                                "https://github.com/{}/issues/{}",
                                reference.repository, reference.number
                            )
                        })),
                        task_key: task.map(|t| t.key.clone()),
                    }
                })
            });
        result.rows.push(Row {
            number: pr.number,
            checkout_id: checkout.map(|c| c.id.clone()),
            agents: here
                .iter()
                .map(|a| references[&(*a as *const _)].clone())
                .collect(),
            lineage: checkout
                .map(|c| lineage(&trees[&c.id].rows, agents))
                .unwrap_or_default(),
            needs_look: branch.iter().any(|a| a.group == "done"),
            group,
            issue,
        });
        result.open += usize::from(!merged);
        match group {
            "turn" => {
                result.counts.turn += 1;
                if result.rows.last().expect("row inserted").needs_look {
                    result.counts.look += 1;
                } else if pr.is_draft
                    && !matches!(
                        pr.badge,
                        PullRequestBadge::Merged | PullRequestBadge::Closed
                    )
                {
                    result.counts.draft += 1;
                } else {
                    result.counts.review += 1;
                }
            }
            "fixing" => result.counts.fixing += 1,
            "blocked" => result.counts.blocked += 1,
            "merged" => {}
            other => unreachable!("unknown PR group {other}"),
        }
    }
    for group in ["turn", "fixing", "blocked", "merged"] {
        let mut prs: Vec<_> = workspace
            .pull_requests
            .iter()
            .zip(&result.rows)
            .filter(|(_, r)| r.group == group)
            .map(|(p, _)| p)
            .collect();
        prs.sort_by_key(|p| {
            std::cmp::Reverse((
                if group == "merged" {
                    p.merged_at_unix_ms
                } else {
                    p.updated_at_unix_ms
                }
                .unwrap_or(0),
                p.number,
            ))
        });
        if !prs.is_empty() {
            result.groups.push(Group {
                group,
                numbers: prs.iter().map(|p| p.number).collect(),
            });
        }
    }
    result
}
