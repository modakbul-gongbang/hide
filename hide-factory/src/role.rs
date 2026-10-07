//! Who may run which Factory command (D-33, B56).
//!
//! The daemon decides the caller's role from its capability registry before
//! a command reaches the engine; a capability file holds only a token, so
//! editing it cannot change a role.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Role {
    /// A pane the Factory spawned, or a caller bound by cwd to a Factory
    /// worktree. It reports on its own Task only.
    Worker { factory: String, task: String },
    /// Any other pane or checkout caller; relays a person's words.
    Operator { pane: String },
    /// The engine itself (timers, deadline defaults). Never a CLI caller.
    Engine,
}

impl Role {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Worker { .. } => "worker",
            Self::Operator { .. } => "operator",
            Self::Engine => "engine",
        }
    }

    /// What is recorded as having relayed a person's words.
    pub fn relayed_by(&self) -> String {
        match self {
            Self::Worker { task, .. } => format!("worker:{task}"),
            Self::Operator { pane } => pane.clone(),
            Self::Engine => "engine".to_owned(),
        }
    }
}

/// Every command a caller may send, named by what it does to the Factory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Read status, show, inbox.
    Read,
    /// Report on one's own Task: ask, block, propose, done, decisions.
    Report,
    /// Add a dependency (safer and slower).
    AddDependency,
    /// Create a Factory, add or edit a Task.
    Intake,
    /// Answer, approve scope, relay a person's words.
    Answer,
    /// Remove a dependency, change priority (faster, looser).
    Loosen,
    /// Merge, request changes (release a gate).
    Merge,
    /// Pause, resume, retry, cancel, revive, close.
    Control,
    /// Change settings.
    Configure,
}

impl Permission {
    pub fn allowed(self, role: &Role) -> bool {
        match role {
            Role::Engine => true,
            Role::Operator { .. } => !matches!(self, Self::Report),
            Role::Worker { .. } => {
                matches!(self, Self::Read | Self::Report | Self::AddDependency)
            }
        }
    }
}

/// The refusal a disallowed command gets, with the role it was refused for.
pub const ROLE_NOT_ALLOWED: &str = "role_not_allowed";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worker_may_only_report_read_and_add_dependencies() {
        let worker = Role::Worker {
            factory: "f".into(),
            task: "T-1".into(),
        };
        for permission in [
            Permission::Read,
            Permission::Report,
            Permission::AddDependency,
        ] {
            assert!(permission.allowed(&worker), "{permission:?}");
        }
        for permission in [
            Permission::Intake,
            Permission::Answer,
            Permission::Loosen,
            Permission::Merge,
            Permission::Control,
            Permission::Configure,
        ] {
            assert!(!permission.allowed(&worker), "{permission:?}");
        }
    }

    #[test]
    fn an_operator_relays_a_person_but_does_not_report_as_a_worker() {
        let operator = Role::Operator { pane: "p1".into() };
        assert!(Permission::Answer.allowed(&operator));
        assert!(Permission::Merge.allowed(&operator));
        assert!(!Permission::Report.allowed(&operator));
        assert_eq!(operator.relayed_by(), "p1");
    }
}
