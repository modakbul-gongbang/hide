use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

pub const RECENT_CLOSED_LIMIT: usize = 20;

pub fn push_bounded(stack: &mut VecDeque<ClosedItem>, item: ClosedItem) {
    if stack.len() == RECENT_CLOSED_LIMIT {
        stack.pop_front();
    }
    stack.push_back(item);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosedContext {
    pub workspace_id: String,
    pub workspace_label: String,
    pub checkout_id: String,
    pub checkout_path: String,
    pub tab_id: String,
    pub tab_label: String,
    pub tab_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosedAgent {
    pub kind: String,
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosedPane {
    pub pane_id: String,
    pub label: Option<String>,
    pub cwd: String,
    pub agent: Option<ClosedAgent>,
    pub browser: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClosedLayout {
    pub workspace_id: String,
    pub tab_id: String,
    pub zoomed: bool,
    pub focused_pane_id: String,
    pub root: ClosedLayoutNode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClosedLayoutNode {
    Pane {
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        command: Option<Vec<String>>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Split {
        direction: ClosedSplitDirection,
        ratio: f32,
        first: Box<ClosedLayoutNode>,
        second: Box<ClosedLayoutNode>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosedSplitDirection {
    Right,
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanePlacement {
    pub neighbor_pane_id: Option<String>,
    pub direction: ClosedSplitDirection,
    pub ratio: f32,
    pub target_was_first: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClosedItem {
    Pane {
        key: String,
        context: ClosedContext,
        pane: ClosedPane,
        placement: PanePlacement,
    },
    Tab {
        key: String,
        context: ClosedContext,
        layout: ClosedLayout,
        panes: Vec<ClosedPane>,
        browser_count: usize,
    },
    File {
        key: String,
        workspace_id: String,
        checkout_id: String,
        checkout_path: String,
        path: String,
        label: String,
    },
}

impl ClosedItem {
    pub fn key(&self) -> &str {
        match self {
            Self::Pane { key, .. } | Self::Tab { key, .. } | Self::File { key, .. } => key,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Pane { pane, .. } => pane.label.as_deref().unwrap_or("Pane"),
            Self::Tab { context, .. } => &context.tab_label,
            Self::File { label, .. } => label,
        }
    }
}

impl ClosedLayoutNode {
    pub fn pane_ids(&self, output: &mut Vec<String>) {
        match self {
            Self::Pane { pane_id, .. } => output.extend(pane_id.iter().cloned()),
            Self::Split { first, second, .. } => {
                first.pane_ids(output);
                second.pane_ids(output);
            }
        }
    }

    pub fn terminal_pane_ids(
        &self,
        panes: &BTreeMap<String, ClosedPane>,
        output: &mut Vec<String>,
    ) {
        match self {
            Self::Pane {
                pane_id: Some(pane_id),
                ..
            } if panes.get(pane_id).is_some_and(|pane| !pane.browser) => {
                output.push(pane_id.clone())
            }
            Self::Pane { .. } => {}
            Self::Split { first, second, .. } => {
                first.terminal_pane_ids(panes, output);
                second.terminal_pane_ids(panes, output);
            }
        }
    }

    pub fn placement_for(&self, target: &str) -> Option<PanePlacement> {
        match self {
            Self::Pane { .. } => None,
            Self::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                if first.is_pane(target) {
                    return Some(PanePlacement {
                        neighbor_pane_id: second.first_pane_id(),
                        direction: *direction,
                        ratio: *ratio,
                        target_was_first: true,
                    });
                }
                if second.is_pane(target) {
                    return Some(PanePlacement {
                        neighbor_pane_id: first.first_pane_id(),
                        direction: *direction,
                        ratio: 1.0 - *ratio,
                        target_was_first: false,
                    });
                }
                first
                    .placement_for(target)
                    .or_else(|| second.placement_for(target))
            }
        }
    }

    fn is_pane(&self, target: &str) -> bool {
        matches!(self, Self::Pane { pane_id: Some(pane_id), .. } if pane_id == target)
    }

    fn first_pane_id(&self) -> Option<String> {
        match self {
            Self::Pane { pane_id, .. } => pane_id.clone(),
            Self::Split { first, second, .. } => {
                first.first_pane_id().or_else(|| second.first_pane_id())
            }
        }
    }

    pub fn prune_browser_panes(
        &self,
        panes: &BTreeMap<String, ClosedPane>,
        checkout_root: &str,
        notices: &mut Vec<String>,
    ) -> Option<Self> {
        match self {
            Self::Pane {
                pane_id,
                label,
                cwd: _,
                command: _,
                env,
            } => {
                let pane = pane_id.as_ref().and_then(|id| panes.get(id))?;
                if pane.browser {
                    return None;
                }
                let restored_cwd = if std::path::Path::new(&pane.cwd).is_dir() {
                    pane.cwd.clone()
                } else {
                    notices.push(format!(
                        "{} no longer exists; reopened in the checkout root",
                        pane.cwd
                    ));
                    checkout_root.to_owned()
                };
                Some(Self::Pane {
                    pane_id: None,
                    label: pane.label.clone().or_else(|| label.clone()),
                    cwd: Some(restored_cwd),
                    command: None,
                    env: env.clone(),
                })
            }
            Self::Split {
                direction,
                ratio,
                first,
                second,
            } => match (
                first.prune_browser_panes(panes, checkout_root, notices),
                second.prune_browser_panes(panes, checkout_root, notices),
            ) {
                (Some(first), Some(second)) => Some(Self::Split {
                    direction: *direction,
                    ratio: *ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(node), None) | (None, Some(node)) => Some(node),
                (None, None) => None,
            },
        }
    }
}

pub fn resume_arguments(agent: &ClosedAgent) -> Option<Vec<String>> {
    let session_id = agent.session_id.as_ref()?;
    match agent.kind.to_ascii_lowercase().as_str() {
        "claude" | "claude-code" => Some(vec!["--resume".into(), session_id.clone()]),
        "codex" => Some(vec!["resume".into(), session_id.clone()]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str) -> ClosedLayoutNode {
        ClosedLayoutNode::Pane {
            pane_id: Some(id.into()),
            label: None,
            cwd: Some("/tmp".into()),
            command: None,
            env: BTreeMap::new(),
        }
    }

    fn file(key: &str) -> ClosedItem {
        ClosedItem::File {
            key: key.into(),
            workspace_id: "workspace".into(),
            checkout_id: "checkout".into(),
            checkout_path: "/tmp".into(),
            path: format!("/tmp/{key}"),
            label: key.into(),
        }
    }

    #[test]
    fn bounded_stack_drops_only_the_oldest_and_remains_lifo() {
        let mut stack = VecDeque::new();
        for index in 0..=RECENT_CLOSED_LIMIT {
            push_bounded(&mut stack, file(&format!("file-{index}")));
        }
        assert_eq!(stack.len(), RECENT_CLOSED_LIMIT);
        assert_eq!(stack.front().unwrap().label(), "file-1");
        assert_eq!(stack.pop_back().unwrap().label(), "file-20");
        assert_eq!(stack.pop_back().unwrap().label(), "file-19");
    }

    #[test]
    fn placement_preserves_neighbor_direction_ratio_and_side() {
        let layout = ClosedLayoutNode::Split {
            direction: ClosedSplitDirection::Right,
            ratio: 0.35,
            first: Box::new(pane("left")),
            second: Box::new(ClosedLayoutNode::Split {
                direction: ClosedSplitDirection::Down,
                ratio: 0.6,
                first: Box::new(pane("top")),
                second: Box::new(pane("bottom")),
            }),
        };
        assert_eq!(
            layout.placement_for("left"),
            Some(PanePlacement {
                neighbor_pane_id: Some("top".into()),
                direction: ClosedSplitDirection::Right,
                ratio: 0.35,
                target_was_first: true,
            })
        );
        let bottom = layout.placement_for("bottom").unwrap();
        assert_eq!(bottom.neighbor_pane_id.as_deref(), Some("top"));
        assert_eq!(bottom.direction, ClosedSplitDirection::Down);
        assert!((bottom.ratio - 0.4).abs() < f32::EPSILON * 2.0);
        assert!(!bottom.target_was_first);
    }

    #[test]
    fn resume_arguments_resume_without_forking() {
        assert_eq!(
            resume_arguments(&ClosedAgent {
                kind: "claude".into(),
                session_id: Some("session-c".into()),
            }),
            Some(
                vec!["--resume", "session-c"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            )
        );
        assert_eq!(
            resume_arguments(&ClosedAgent {
                kind: "codex".into(),
                session_id: Some("session-x".into()),
            }),
            Some(
                vec!["resume", "session-x"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            )
        );
    }

    #[test]
    fn browser_leaves_are_pruned_without_redrawing_the_remaining_topology() {
        let layout = ClosedLayoutNode::Split {
            direction: ClosedSplitDirection::Right,
            ratio: 0.5,
            first: Box::new(pane("terminal")),
            second: Box::new(pane("browser")),
        };
        let panes = BTreeMap::from([
            (
                "terminal".into(),
                ClosedPane {
                    pane_id: "terminal".into(),
                    label: None,
                    cwd: "/tmp".into(),
                    agent: None,
                    browser: false,
                },
            ),
            (
                "browser".into(),
                ClosedPane {
                    pane_id: "browser".into(),
                    label: None,
                    cwd: "/tmp".into(),
                    agent: None,
                    browser: true,
                },
            ),
        ]);
        assert!(matches!(
            layout.prune_browser_panes(&panes, "/tmp", &mut Vec::new()),
            Some(ClosedLayoutNode::Pane { pane_id: None, .. })
        ));
    }
}
