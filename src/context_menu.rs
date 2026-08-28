use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessDisclosure {
    pub process_id: String,
    pub label: String,
    pub phase: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ContextTarget {
    Workspace {
        host_id: String,
        workspace_id: String,
        name: String,
        affected_processes: Vec<ProcessDisclosure>,
    },
    Agent {
        host_id: String,
        workspace_id: String,
        agent_id: String,
        pane_id: String,
        name: String,
        phase: String,
    },
    Worktree {
        host_id: String,
        workspace_id: String,
        repo_key: String,
        checkout_path: String,
        dirty: bool,
        manifest_owned: bool,
    },
}

impl ContextTarget {
    pub fn stable_key(&self) -> String {
        match self {
            Self::Workspace {
                host_id,
                workspace_id,
                ..
            } => format!("workspace:{host_id}:{workspace_id}"),
            Self::Agent {
                host_id, agent_id, ..
            } => format!("agent:{host_id}:{agent_id}"),
            Self::Worktree {
                host_id,
                repo_key,
                checkout_path,
                ..
            } => format!("worktree:{host_id}:{repo_key}:{checkout_path}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextAction {
    Rename,
    CloseWorkspace,
    StopAgentAndClosePane,
    RevealCheckout,
    RemoveCheckout,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MenuItem {
    pub action: ContextAction,
    pub label: String,
    pub enabled: bool,
    pub destructive: bool,
}

pub fn menu_items(target: &ContextTarget) -> Vec<MenuItem> {
    match target {
        ContextTarget::Workspace { .. } => vec![
            MenuItem {
                action: ContextAction::Rename,
                label: "Rename workspace".to_owned(),
                enabled: true,
                destructive: false,
            },
            MenuItem {
                action: ContextAction::CloseWorkspace,
                label: "Close workspace".to_owned(),
                enabled: true,
                destructive: true,
            },
        ],
        ContextTarget::Agent { .. } => vec![
            MenuItem {
                action: ContextAction::Rename,
                label: "Rename agent".to_owned(),
                enabled: true,
                destructive: false,
            },
            MenuItem {
                action: ContextAction::StopAgentAndClosePane,
                label: "Stop agent and close pane".to_owned(),
                enabled: true,
                destructive: true,
            },
        ],
        ContextTarget::Worktree { .. } => vec![
            MenuItem {
                action: ContextAction::RevealCheckout,
                label: "Reveal checkout".to_owned(),
                enabled: true,
                destructive: false,
            },
            MenuItem {
                action: ContextAction::RemoveCheckout,
                label: "Remove checkout".to_owned(),
                enabled: true,
                destructive: true,
            },
        ],
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MutationIntent {
    RenameWorkspace {
        host_id: String,
        workspace_id: String,
        new_name: String,
    },
    CloseWorkspace {
        host_id: String,
        workspace_id: String,
        affected_processes: Vec<ProcessDisclosure>,
    },
    RenameAgent {
        host_id: String,
        workspace_id: String,
        agent_id: String,
        new_name: String,
    },
    StopAgentAndClosePane {
        host_id: String,
        workspace_id: String,
        agent_id: String,
        pane_id: String,
    },
    RevealCheckout {
        host_id: String,
        workspace_id: String,
        repo_key: String,
        checkout_path: String,
    },
    RemoveCheckout {
        host_id: String,
        workspace_id: String,
        repo_key: String,
        checkout_path: String,
        dirty: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfirmationDisclosure {
    pub title: String,
    pub exact_target: String,
    pub consequence: String,
    pub affected_processes: Vec<ProcessDisclosure>,
    pub dirty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MutationPlan {
    pub target_key: String,
    pub action: ContextAction,
    pub intent: MutationIntent,
    pub confirmation: Option<ConfirmationDisclosure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextMenuError {
    UnsupportedAction {
        target: String,
        action: ContextAction,
    },
    EmptyName,
    ConfirmationRequired,
    UnownedCheckout(String),
    DirtyCheckoutRequiresConfirmation(String),
    ServerRejected {
        operation: String,
        reason: String,
    },
    RevealFailed {
        path: String,
        reason: String,
    },
}

impl std::fmt::Display for ContextMenuError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedAction { target, action } => {
                write!(formatter, "action {action:?} is not valid for {target}")
            }
            Self::EmptyName => formatter.write_str("name cannot be empty"),
            Self::ConfirmationRequired => formatter.write_str("explicit confirmation is required"),
            Self::UnownedCheckout(path) => {
                write!(formatter, "checkout is not manifest-owned: {path}")
            }
            Self::DirtyCheckoutRequiresConfirmation(path) => {
                write!(formatter, "dirty checkout requires confirmation: {path}")
            }
            Self::ServerRejected { operation, reason } => {
                write!(formatter, "server rejected operation={operation}: {reason}")
            }
            Self::RevealFailed { path, reason } => {
                write!(formatter, "reveal failed path={path}: {reason}")
            }
        }
    }
}

impl std::error::Error for ContextMenuError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MutationResult {
    pub operation: String,
    pub target_key: String,
    pub confirmed: bool,
    pub server_sequence: Option<u64>,
}

pub trait ContextMenuServer {
    fn apply(&mut self, intent: &MutationIntent) -> Result<MutationResult, String>;
}

pub struct ContextMenuController {
    owned_checkouts: BTreeSet<String>,
}

impl ContextMenuController {
    pub fn new<I, S>(owned_checkouts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            owned_checkouts: owned_checkouts.into_iter().map(Into::into).collect(),
        }
    }

    pub fn plan(
        &self,
        target: &ContextTarget,
        action: ContextAction,
        new_name: Option<&str>,
    ) -> Result<MutationPlan, ContextMenuError> {
        let target_key = target.stable_key();
        let plan = match (target, action) {
            (
                ContextTarget::Workspace {
                    host_id,
                    workspace_id,
                    affected_processes: _,
                    ..
                },
                ContextAction::Rename,
            ) => MutationPlan {
                target_key: target_key.clone(),
                action,
                intent: MutationIntent::RenameWorkspace {
                    host_id: host_id.clone(),
                    workspace_id: workspace_id.clone(),
                    new_name: required_name(new_name)?,
                },
                confirmation: None,
            },
            (
                ContextTarget::Workspace {
                    host_id,
                    workspace_id,
                    name,
                    affected_processes,
                },
                ContextAction::CloseWorkspace,
            ) => MutationPlan {
                target_key: target_key.clone(),
                action,
                intent: MutationIntent::CloseWorkspace {
                    host_id: host_id.clone(),
                    workspace_id: workspace_id.clone(),
                    affected_processes: affected_processes.clone(),
                },
                confirmation: Some(ConfirmationDisclosure {
                    title: "Close workspace?".to_owned(),
                    exact_target: name.clone(),
                    consequence: "Working and attention processes in this workspace will end and their panes will close.".to_owned(),
                    affected_processes: affected_processes.clone(),
                    dirty: false,
                }),
            },
            (
                ContextTarget::Agent {
                    host_id,
                    workspace_id,
                    agent_id,
                    pane_id: _,
                    ..
                },
                ContextAction::Rename,
            ) => MutationPlan {
                target_key: target_key.clone(),
                action,
                intent: MutationIntent::RenameAgent {
                    host_id: host_id.clone(),
                    workspace_id: workspace_id.clone(),
                    agent_id: agent_id.clone(),
                    new_name: required_name(new_name)?,
                },
                confirmation: None,
            },
            (
                ContextTarget::Agent {
                    host_id,
                    workspace_id,
                    agent_id,
                    pane_id,
                    name,
                    phase,
                },
                ContextAction::StopAgentAndClosePane,
            ) => MutationPlan {
                target_key: target_key.clone(),
                action,
                intent: MutationIntent::StopAgentAndClosePane {
                    host_id: host_id.clone(),
                    workspace_id: workspace_id.clone(),
                    agent_id: agent_id.clone(),
                    pane_id: pane_id.clone(),
                },
                confirmation: Some(ConfirmationDisclosure {
                    title: "Stop agent and close pane?".to_owned(),
                    exact_target: format!("{name} ({agent_id}) / pane {pane_id}"),
                    consequence: format!("The {phase} agent process will stop and its pane will close."),
                    affected_processes: vec![ProcessDisclosure {
                        process_id: agent_id.clone(),
                        label: name.clone(),
                        phase: phase.clone(),
                    }],
                    dirty: false,
                }),
            },
            (
                ContextTarget::Worktree {
                    host_id,
                    workspace_id,
                    repo_key,
                    checkout_path,
                    ..
                },
                ContextAction::RevealCheckout,
            ) => MutationPlan {
                target_key: target_key.clone(),
                action,
                intent: MutationIntent::RevealCheckout {
                    host_id: host_id.clone(),
                    workspace_id: workspace_id.clone(),
                    repo_key: repo_key.clone(),
                    checkout_path: checkout_path.clone(),
                },
                confirmation: None,
            },
            (
                ContextTarget::Worktree {
                    host_id,
                    workspace_id,
                    repo_key,
                    checkout_path,
                    dirty,
                    manifest_owned,
                },
                ContextAction::RemoveCheckout,
            ) => {
                if !manifest_owned || !self.owned_checkouts.contains(checkout_path) {
                    return Err(ContextMenuError::UnownedCheckout(checkout_path.clone()));
                }
                MutationPlan {
                    target_key: target_key.clone(),
                    action,
                    intent: MutationIntent::RemoveCheckout {
                        host_id: host_id.clone(),
                        workspace_id: workspace_id.clone(),
                        repo_key: repo_key.clone(),
                        checkout_path: checkout_path.clone(),
                        dirty: *dirty,
                    },
                    confirmation: Some(ConfirmationDisclosure {
                        title: "Remove worktree checkout?".to_owned(),
                        exact_target: checkout_path.clone(),
                        consequence: if *dirty {
                            "Uncommitted changes may be lost; the affected workspace will no longer resolve this checkout.".to_owned()
                        } else {
                            "The exact manifest-owned checkout will be removed and its workspace reference will disappear after server confirmation.".to_owned()
                        },
                        affected_processes: Vec::new(),
                        dirty: *dirty,
                    }),
                }
            }
            _ => {
                return Err(ContextMenuError::UnsupportedAction { target: target_key, action });
            }
        };
        Ok(plan)
    }

    pub fn execute<S: ContextMenuServer>(
        &self,
        plan: &MutationPlan,
        confirmed: bool,
        server: &mut S,
    ) -> Result<MutationResult, ContextMenuError> {
        if plan.confirmation.is_some() && !confirmed {
            return Err(ContextMenuError::ConfirmationRequired);
        }
        let result =
            server
                .apply(&plan.intent)
                .map_err(|reason| ContextMenuError::ServerRejected {
                    operation: operation_name(&plan.intent).to_owned(),
                    reason,
                })?;
        if result.target_key != plan.target_key || !result.confirmed {
            return Err(ContextMenuError::ServerRejected {
                operation: result.operation,
                reason: "server result did not confirm the exact target".to_owned(),
            });
        }
        Ok(result)
    }
}

fn required_name(value: Option<&str>) -> Result<String, ContextMenuError> {
    let value = value.unwrap_or_default().trim();
    if value.is_empty() {
        return Err(ContextMenuError::EmptyName);
    }
    Ok(value.to_owned())
}

fn operation_name(intent: &MutationIntent) -> &'static str {
    match intent {
        MutationIntent::RenameWorkspace { .. } => "workspace.rename",
        MutationIntent::CloseWorkspace { .. } => "workspace.close",
        MutationIntent::RenameAgent { .. } => "agent.rename",
        MutationIntent::StopAgentAndClosePane { .. } => "agent.stop_and_pane.close",
        MutationIntent::RevealCheckout { .. } => "worktree.reveal",
        MutationIntent::RemoveCheckout { .. } => "worktree.remove",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ContextAction, ContextMenuController, ContextMenuError, ContextMenuServer, ContextTarget,
        MutationIntent, MutationResult, ProcessDisclosure, menu_items,
    };

    #[derive(Default)]
    struct RecordingServer {
        last: Option<MutationIntent>,
        reject: bool,
    }

    impl ContextMenuServer for RecordingServer {
        fn apply(&mut self, intent: &MutationIntent) -> Result<MutationResult, String> {
            if self.reject {
                return Err("fixture rejected mutation".to_owned());
            }
            self.last = Some(intent.clone());
            Ok(MutationResult {
                operation: "context.mutation".to_owned(),
                target_key: match intent {
                    MutationIntent::RenameWorkspace {
                        host_id,
                        workspace_id,
                        ..
                    }
                    | MutationIntent::CloseWorkspace {
                        host_id,
                        workspace_id,
                        ..
                    } => format!("workspace:{host_id}:{workspace_id}"),
                    MutationIntent::RenameAgent {
                        host_id, agent_id, ..
                    }
                    | MutationIntent::StopAgentAndClosePane {
                        host_id, agent_id, ..
                    } => format!("agent:{host_id}:{agent_id}"),
                    MutationIntent::RevealCheckout {
                        host_id,
                        repo_key,
                        checkout_path,
                        ..
                    }
                    | MutationIntent::RemoveCheckout {
                        host_id,
                        repo_key,
                        checkout_path,
                        ..
                    } => format!("worktree:{host_id}:{repo_key}:{checkout_path}"),
                },
                confirmed: true,
                server_sequence: Some(9),
            })
        }
    }

    #[test]
    fn menu_surface_is_target_specific_and_destructive_actions_are_marked() {
        let workspace = ContextTarget::Workspace {
            host_id: "local".to_owned(),
            workspace_id: "w1".to_owned(),
            name: "Workspace".to_owned(),
            affected_processes: vec![ProcessDisclosure {
                process_id: "agent-1".to_owned(),
                label: "Codex".to_owned(),
                phase: "working".to_owned(),
            }],
        };
        let items = menu_items(&workspace);
        assert_eq!(items.len(), 2);
        assert!(items[1].destructive);
        assert_eq!(items[1].action, ContextAction::CloseWorkspace);
    }

    #[test]
    fn close_requires_consequence_confirmation_before_server_call() {
        let target = ContextTarget::Agent {
            host_id: "local".to_owned(),
            workspace_id: "w1".to_owned(),
            agent_id: "a1".to_owned(),
            pane_id: "p1".to_owned(),
            name: "Agent".to_owned(),
            phase: "attention".to_owned(),
        };
        let controller = ContextMenuController::new(std::iter::empty::<String>());
        let plan = controller
            .plan(&target, ContextAction::StopAgentAndClosePane, None)
            .unwrap();
        let mut server = RecordingServer::default();
        assert_eq!(
            controller.execute(&plan, false, &mut server),
            Err(ContextMenuError::ConfirmationRequired)
        );
        assert!(server.last.is_none());
        let result = controller.execute(&plan, true, &mut server).unwrap();
        assert!(result.confirmed);
        assert!(server.last.is_some());
    }

    #[test]
    fn worktree_remove_requires_exact_manifest_path_and_discloses_dirty_state() {
        let path = "/fixtures/owned/worktree";
        let target = ContextTarget::Worktree {
            host_id: "local".to_owned(),
            workspace_id: "w1".to_owned(),
            repo_key: "repo".to_owned(),
            checkout_path: path.to_owned(),
            dirty: true,
            manifest_owned: true,
        };
        let controller = ContextMenuController::new([path]);
        let plan = controller
            .plan(&target, ContextAction::RemoveCheckout, None)
            .unwrap();
        let disclosure = plan.confirmation.as_ref().unwrap();
        assert!(disclosure.dirty);
        assert!(disclosure.consequence.contains("Uncommitted"));
        let unowned = ContextTarget::Worktree {
            host_id: "local".to_owned(),
            workspace_id: "w1".to_owned(),
            repo_key: "repo".to_owned(),
            checkout_path: "/fixtures/other".to_owned(),
            dirty: true,
            manifest_owned: false,
        };
        assert!(matches!(
            controller.plan(&unowned, ContextAction::RemoveCheckout, None),
            Err(ContextMenuError::UnownedCheckout(_))
        ));
    }

    #[test]
    fn rename_and_server_rejection_keep_exact_target_observable() {
        let target = ContextTarget::Workspace {
            host_id: "mini".to_owned(),
            workspace_id: "w1".to_owned(),
            name: "Remote".to_owned(),
            affected_processes: Vec::new(),
        };
        let controller = ContextMenuController::new(std::iter::empty::<String>());
        let plan = controller
            .plan(&target, ContextAction::Rename, Some("Renamed"))
            .unwrap();
        assert!(plan.confirmation.is_none());
        let mut server = RecordingServer {
            reject: true,
            ..RecordingServer::default()
        };
        assert!(matches!(
            controller.execute(&plan, true, &mut server),
            Err(ContextMenuError::ServerRejected { operation, .. }) if operation == "workspace.rename"
        ));
    }
}
