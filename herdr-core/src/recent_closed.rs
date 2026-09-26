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
    /// Herdr identities present before the close. A retry inspects only later
    /// identities for Hide's exact reopen-intent marker; being new, having the
    /// same label, or sharing a cwd never proves ownership by itself.
    pub workspace_ids_before_close: Vec<String>,
    pub tab_ids_before_close: Vec<String>,
    pub pane_ids_before_close: Vec<String>,
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
    /// Path from the tab root to the split that directly owned the closed
    /// pane. A split sibling has no single pane that `pane.split` can target,
    /// so reopen wraps the surviving subtree at this location instead.
    pub parent_path: Vec<ClosedLayoutBranch>,
    pub direction: ClosedSplitDirection,
    pub ratio: f32,
    pub target_was_first: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClosedLayoutBranch {
    First,
    Second,
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
    },
    File {
        key: String,
        /// The device the file is on; it reopens only while that device is
        /// the one in front, and only through that device's host.
        device_id: String,
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

    /// The device whose surface can reopen this item. Hide records a pane or
    /// tab close only on this machine's Herdr.
    pub fn device_id(&self) -> &str {
        match self {
            Self::Pane { .. } | Self::Tab { .. } => crate::workspace::LOCAL_DEVICE_ID,
            Self::File { device_id, .. } => device_id,
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
    pub fn pane_count(&self) -> usize {
        match self {
            Self::Pane { .. } => 1,
            Self::Split { first, second, .. } => first.pane_count() + second.pane_count(),
        }
    }

    pub fn pane_ids(&self, output: &mut Vec<String>) {
        match self {
            Self::Pane { pane_id, .. } => output.extend(pane_id.iter().cloned()),
            Self::Split { first, second, .. } => {
                first.pane_ids(output);
                second.pane_ids(output);
            }
        }
    }

    pub fn known_pane_ids(&self, panes: &BTreeMap<String, ClosedPane>, output: &mut Vec<String>) {
        match self {
            Self::Pane {
                pane_id: Some(pane_id),
                ..
            } if panes.contains_key(pane_id) => output.push(pane_id.clone()),
            Self::Pane { .. } => {}
            Self::Split { first, second, .. } => {
                first.known_pane_ids(panes, output);
                second.known_pane_ids(panes, output);
            }
        }
    }

    pub fn placement_for(&self, target: &str) -> Option<PanePlacement> {
        self.placement_for_at_path(target, &mut Vec::new())
    }

    fn placement_for_at_path(
        &self,
        target: &str,
        path: &mut Vec<ClosedLayoutBranch>,
    ) -> Option<PanePlacement> {
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
                        neighbor_pane_id: second.direct_pane_id(),
                        parent_path: path.clone(),
                        direction: *direction,
                        ratio: *ratio,
                        target_was_first: true,
                    });
                }
                if second.is_pane(target) {
                    return Some(PanePlacement {
                        neighbor_pane_id: first.direct_pane_id(),
                        parent_path: path.clone(),
                        direction: *direction,
                        // Herdr's pane.split ratio always describes the first
                        // child's share. The later swap restores which side the
                        // reopened pane occupied, so complementing here would
                        // invert the original geometry.
                        ratio: *ratio,
                        target_was_first: false,
                    });
                }
                path.push(ClosedLayoutBranch::First);
                let first_placement = first.placement_for_at_path(target, path);
                path.pop();
                if first_placement.is_some() {
                    return first_placement;
                }
                path.push(ClosedLayoutBranch::Second);
                let second_placement = second.placement_for_at_path(target, path);
                path.pop();
                second_placement
            }
        }
    }

    fn is_pane(&self, target: &str) -> bool {
        matches!(self, Self::Pane { pane_id: Some(pane_id), .. } if pane_id == target)
    }

    fn direct_pane_id(&self) -> Option<String> {
        match self {
            Self::Pane { pane_id, .. } => pane_id.clone(),
            Self::Split { .. } => None,
        }
    }

    pub fn resolve_panes(
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
                first.resolve_panes(panes, checkout_root, notices),
                second.resolve_panes(panes, checkout_root, notices),
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
            device_id: "local".into(),
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
    fn placement_preserves_original_split_ratio_for_either_side() {
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
                neighbor_pane_id: None,
                parent_path: vec![],
                direction: ClosedSplitDirection::Right,
                ratio: 0.35,
                target_was_first: true,
            })
        );
        let bottom = layout.placement_for("bottom").unwrap();
        assert_eq!(bottom.neighbor_pane_id.as_deref(), Some("top"));
        assert_eq!(bottom.parent_path, [ClosedLayoutBranch::Second]);
        assert_eq!(bottom.direction, ClosedSplitDirection::Down);
        assert!((bottom.ratio - 0.6).abs() < f32::EPSILON * 2.0);
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
    fn resolved_panes_restore_the_recorded_label_and_cwd() {
        let layout = pane("terminal");
        let panes = BTreeMap::from([(
            "terminal".into(),
            ClosedPane {
                pane_id: "terminal".into(),
                label: None,
                cwd: "/tmp".into(),
                agent: None,
            },
        )]);
        assert!(matches!(
            layout.resolve_panes(&panes, "/tmp", &mut Vec::new()),
            Some(ClosedLayoutNode::Pane { pane_id: None, .. })
        ));
    }

    #[test]
    fn an_unknown_pane_id_resolves_to_nothing() {
        let layout = pane("missing");
        assert_eq!(
            layout.resolve_panes(&BTreeMap::new(), "/tmp", &mut Vec::new()),
            None
        );
    }
}
