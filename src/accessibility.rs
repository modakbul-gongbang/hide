use crate::layout::PaneId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticRole {
    Window,
    List,
    TabGroup,
    Group,
    TextArea,
    WebArea,
    Status,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticNode {
    pub role: SemanticRole,
    pub label: String,
    pub value: Option<String>,
    pub selected: bool,
    pub expanded: Option<bool>,
    pub disabled: bool,
    pub children: Vec<SemanticNode>,
}

impl SemanticNode {
    pub fn leaf(role: SemanticRole, label: impl Into<String>) -> Self {
        Self {
            role,
            label: label.into(),
            value: None,
            selected: false,
            expanded: None,
            disabled: false,
            children: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.label.trim().is_empty() {
            return Err(format!("semantic node {:?} has an empty label", self.role));
        }
        for child in &self.children {
            child.validate()?;
        }
        Ok(())
    }
}

/// Build the single semantic model consumed by both deterministic tests and
/// the live AppKit adapter. `zoomed_pane` and `focused_pane` use the stable
/// semantic names (`terminal`, `editor`, or `browser`), while the optional
/// overlay flag controls whether the agent switcher is exposed to AX.
pub fn workbench_tree_with_state(
    zoomed_pane: Option<&str>,
    focused_pane: Option<&str>,
    overlay_open: bool,
) -> SemanticNode {
    let terminal = SemanticNode {
        role: SemanticRole::TextArea,
        label: "Terminal pane A".to_owned(),
        value: None,
        selected: matches!(focused_pane, Some("terminal")),
        expanded: None,
        disabled: false,
        children: Vec::new(),
    };
    let editor = SemanticNode {
        role: SemanticRole::TextArea,
        label: "Editor pane B".to_owned(),
        value: None,
        selected: matches!(focused_pane, Some("editor")),
        expanded: None,
        disabled: false,
        children: Vec::new(),
    };
    let pane_children = match zoomed_pane {
        Some("terminal") => vec![terminal],
        Some("editor") => vec![editor],
        _ => vec![terminal, editor],
    };
    let mut children = vec![
        SemanticNode::leaf(SemanticRole::List, "Workspace navigator"),
        SemanticNode {
            role: SemanticRole::TabGroup,
            label: "Workspace tabs".to_owned(),
            value: Some(if zoomed_pane.is_some() {
                "Pane zoomed".to_owned()
            } else {
                "Split layout".to_owned()
            }),
            selected: true,
            expanded: None,
            disabled: false,
            children: Vec::new(),
        },
        SemanticNode {
            role: SemanticRole::Group,
            label: "Split canvas".to_owned(),
            value: None,
            selected: false,
            expanded: None,
            disabled: false,
            children: pane_children,
        },
        SemanticNode::leaf(SemanticRole::Status, "Connection status"),
    ];
    if overlay_open {
        children.push(SemanticNode::leaf(SemanticRole::Group, "Agent switcher"));
    }
    SemanticNode {
        role: SemanticRole::Window,
        label: "Herdr IDE".to_owned(),
        value: None,
        selected: true,
        expanded: None,
        disabled: false,
        children,
    }
}

pub fn semantic_pane_name(pane: PaneId) -> &'static str {
    match pane {
        PaneId::Editor => "editor",
        PaneId::Browser => "browser",
        PaneId::TerminalA | PaneId::TerminalB => "terminal",
    }
}

pub fn workbench_tree(zoomed_pane: Option<&str>) -> SemanticNode {
    workbench_tree_with_state(zoomed_pane, None, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_node_has_a_meaningful_role_and_label() {
        let tree = workbench_tree(None);
        tree.validate().unwrap();
        assert_eq!(tree.children[2].children.len(), 2);
    }

    #[test]
    fn zoom_keeps_navigator_and_tabs_but_hides_sibling_semantics() {
        let tree = workbench_tree(Some("editor"));
        tree.validate().unwrap();
        assert_eq!(tree.children[0].label, "Workspace navigator");
        assert_eq!(tree.children[1].value.as_deref(), Some("Pane zoomed"));
        assert_eq!(tree.children[2].children.len(), 1);
        assert_eq!(tree.children[2].children[0].label, "Editor pane B");
    }

    #[test]
    fn live_adapter_state_is_canonical_and_marks_focus_and_overlay() {
        let tree = workbench_tree_with_state(Some("editor"), Some("editor"), true);
        tree.validate().unwrap();
        assert_eq!(tree.children[0].label, "Workspace navigator");
        assert_eq!(tree.children[1].value.as_deref(), Some("Pane zoomed"));
        assert_eq!(tree.children[2].children.len(), 1);
        assert!(tree.children[2].children[0].selected);
        assert_eq!(tree.children[2].children[0].label, "Editor pane B");
        assert_eq!(tree.children[4].label, "Agent switcher");
    }
}
