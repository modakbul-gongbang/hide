//! Projects authoritative Herdr snapshots into the shell-facing session payload.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedWorkspace {
    pub(crate) workspace_id: String,
    pub(crate) label: String,
    pub(crate) active_tab_id: String,
    pub(crate) worktree: Option<ProjectedWorktree>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedWorktree {
    pub(crate) repo_key: String,
    pub(crate) repo_name: String,
    pub(crate) repo_root: String,
    pub(crate) checkout_path: String,
    pub(crate) is_linked_worktree: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedTab {
    pub(crate) tab_id: String,
    pub(crate) workspace_id: String,
    pub(crate) label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedPane {
    pub(crate) pane_id: String,
    pub(crate) workspace_id: String,
    pub(crate) tab_id: String,
    pub(crate) cwd: Option<String>,
    pub(crate) tokens: BTreeMap<String, Value>,
    pub(crate) label: Option<String>,
    pub(crate) terminal_title: Option<String>,
    pub(crate) terminal_title_stripped: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedAgent {
    pub(crate) pane_id: String,
    pub(crate) name: Option<String>,
    pub(crate) workspace_id: String,
    pub(crate) tab_id: String,
    pub(crate) cwd: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) agent_status: Option<String>,
    pub(crate) agent_session: Option<crate::sidebar::SessionAgentSessionPayload>,
    pub(crate) spawned_from_pane_id: Option<String>,
    pub(crate) state_change_seq: u64,
    pub(crate) tokens: BTreeMap<String, Value>,
    pub(crate) ambient: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectionState {
    pub(crate) focused_pane_id: Option<String>,
    /// Herdr's focused workspace, read from `session.snapshot` and kept
    /// current from the focus events. A `tab_focused` or `pane_focused` in
    /// another workspace moves it too, because Herdr focuses the workspace
    /// along with the tab and does not always send `workspace_focused` first.
    pub(crate) focused_workspace_id: Option<String>,
    pub(crate) workspaces: Vec<ProjectedWorkspace>,
    pub(crate) tabs: Vec<ProjectedTab>,
    pub(crate) panes: Vec<ProjectedPane>,
    pub(crate) layouts: Vec<SessionLayoutPayload>,
    pub(crate) agents: Vec<ProjectedAgent>,
}

impl ProjectionState {
    pub(crate) fn project(&self) -> SessionSnapshotPayload {
        let workspace_labels = self
            .workspaces
            .iter()
            .map(|workspace| (workspace.workspace_id.as_str(), workspace.label.as_str()))
            .collect::<BTreeMap<_, _>>();
        let agents = self
            .agents
            .iter()
            .map(|agent| {
                let workspace_label = workspace_labels
                    .get(agent.workspace_id.as_str())
                    .copied()
                    .or_else(|| {
                        (!agent.workspace_id.trim().is_empty())
                            .then_some(agent.workspace_id.as_str())
                    })
                    .map(str::to_owned);
                SessionAgentPayload {
                    id: agent.name.clone().or_else(|| Some(agent.pane_id.clone())),
                    pane_id: Some(agent.pane_id.clone()),
                    workspace_label,
                    cwd: agent.cwd.clone(),
                    agent: agent.agent.clone(),
                    agent_status: agent.agent_status.clone(),
                    agent_session: agent.agent_session.clone(),
                    spawned_from_pane_id: agent.spawned_from_pane_id.clone(),
                    state_change_seq: Some(agent.state_change_seq),
                    tokens: agent.tokens.clone(),
                    ambient: agent.ambient.clone(),
                }
            })
            .collect();
        let tabs = self
            .tabs
            .iter()
            .map(|tab| SessionTabPayload {
                tab_id: tab.tab_id.clone(),
                workspace_id: tab.workspace_id.clone(),
                label: tab.label.clone(),
            })
            .collect();
        let panes = self
            .panes
            .iter()
            .map(|pane| SessionPanePayload {
                pane_id: pane.pane_id.clone(),
                tokens: pane.tokens.clone(),
                cwd: pane.cwd.clone(),
                label: non_blank(pane.label.as_deref()),
                terminal_title: non_blank(
                    pane.terminal_title_stripped
                        .as_deref()
                        .or(pane.terminal_title.as_deref()),
                ),
            })
            .collect();
        // Herdr order, not map order: the active tab id rides with the
        // workspace it belongs to, and the navigator reads it as the only
        // authority for which tab is active.
        let workspaces = self
            .workspaces
            .iter()
            .map(|workspace| SessionWorkspacePayload {
                workspace_id: workspace.workspace_id.clone(),
                label: workspace.label.clone(),
                active_tab_id: non_blank(Some(workspace.active_tab_id.as_str())),
            })
            .collect();
        SessionSnapshotPayload {
            focused_pane_id: self.focused_pane_id.clone(),
            focused_workspace_id: self.focused_workspace_id.clone(),
            tabs,
            layouts: self.layouts.clone(),
            agents,
            panes,
            workspaces,
        }
    }
}

pub(crate) fn project_snapshot(
    snapshot: &Value,
) -> Result<SessionSnapshotPayload, SessionFetchError> {
    Ok(wire::snapshot(snapshot.clone())?.2.project())
}

/// A Herdr label or terminal title that is present but blank carries no more
/// information than an absent one, and a header ladder that treated the two
/// differently would show an empty title instead of falling through.
pub(crate) fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
