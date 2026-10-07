//! What the core and a node share about a registered SSH device: the stage
//! a device operation failed at, its diagnostic, and the capability report
//! of a connection test (PRD core-host-node D-21). The SSH transport that
//! produces them is the node's (`hide_node::ssh`); the core only reads them.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The prefixes a host-key refusal starts with, so a caller can tell a
/// changed key from a missing one without parsing the rest of the sentence.
pub const HOST_KEY_CHANGED: &str = "host key changed";
pub const HOST_KEY_UNKNOWN: &str = "host key unknown";

/// Which trust or sign-in step a connection failure names, read off the
/// words this module wrote into it: a changed host key, an unknown one, or a
/// refused authentication (PRD S5.5 B38). Any other failure is `None`.
pub fn connection_problem(message: &str) -> Option<&'static str> {
    if message.contains(HOST_KEY_CHANGED) {
        Some("host_key_changed")
    } else if message.contains(HOST_KEY_UNKNOWN) {
        Some("host_key_unknown")
    } else if message.contains("stage=auth ") {
        Some("authentication")
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteStage {
    Alias,
    Ssh,
    Auth,
    Herdr,
    Protocol,
    Pty,
    Sftp,
    Git,
    Tunnel,
    Reconnect,
    Cleanup,
}

impl fmt::Display for RemoteStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Alias => "alias",
            Self::Ssh => "ssh",
            Self::Auth => "auth",
            Self::Herdr => "herdr",
            Self::Protocol => "protocol",
            Self::Pty => "pty",
            Self::Sftp => "sftp",
            Self::Git => "git",
            Self::Tunnel => "tunnel",
            Self::Reconnect => "reconnect",
            Self::Cleanup => "cleanup",
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteDiagnostic {
    pub operation_id: String,
    pub target: String,
    pub stage: RemoteStage,
    pub retryable: bool,
    pub action_required: bool,
    pub reason: String,
}

impl fmt::Display for RemoteDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "stage={} target={} operation={} retryable={} action_required={} cause={}",
            self.stage,
            self.target,
            self.operation_id,
            self.retryable,
            self.action_required,
            self.reason
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteError {
    diagnostic: RemoteDiagnostic,
}

impl RemoteError {
    pub fn new(
        operation_id: impl Into<String>,
        target: impl Into<String>,
        stage: RemoteStage,
        reason: impl Into<String>,
        retryable: bool,
        action_required: bool,
    ) -> Self {
        Self {
            diagnostic: RemoteDiagnostic {
                operation_id: operation_id.into(),
                target: target.into(),
                stage,
                retryable,
                action_required,
                reason: reason.into(),
            },
        }
    }

    pub fn diagnostic(&self) -> &RemoteDiagnostic {
        &self.diagnostic
    }

    pub fn stage(&self) -> RemoteStage {
        self.diagnostic.stage
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diagnostic.fmt(formatter)
    }
}

impl std::error::Error for RemoteError {}

pub type RemoteResult<T> = Result<T, RemoteError>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteHostIdentity {
    pub host_id: String,
    pub alias: String,
    pub hostname: String,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemoteConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Stale { reason: String },
    Failed { reason: String },
    ActionRequired { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CapabilityState {
    Pending,
    Passed,
    Failed {
        retryable: bool,
        action_required: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityResult {
    pub stage: RemoteStage,
    pub state: CapabilityState,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub operation_id: String,
    pub host: RemoteHostIdentity,
    pub connection: RemoteConnectionState,
    pub stages: Vec<CapabilityResult>,
}

impl CapabilityReport {
    pub fn new(operation_id: impl Into<String>, host: RemoteHostIdentity) -> Self {
        let stages = [
            RemoteStage::Ssh,
            RemoteStage::Auth,
            RemoteStage::Herdr,
            RemoteStage::Protocol,
            RemoteStage::Pty,
            RemoteStage::Sftp,
            RemoteStage::Git,
            RemoteStage::Tunnel,
        ]
        .into_iter()
        .map(|stage| CapabilityResult {
            stage,
            state: CapabilityState::Pending,
            detail: String::new(),
        })
        .collect();
        Self {
            operation_id: operation_id.into(),
            host,
            connection: RemoteConnectionState::Connecting,
            stages,
        }
    }

    pub fn stage(&self, stage: RemoteStage) -> Option<&CapabilityResult> {
        self.stages.iter().find(|result| result.stage == stage)
    }

    pub fn pass(&mut self, stage: RemoteStage, detail: impl Into<String>) {
        if let Some(result) = self.stages.iter_mut().find(|result| result.stage == stage) {
            result.state = CapabilityState::Passed;
            result.detail = detail.into();
        } else {
            self.stages.push(CapabilityResult {
                stage,
                state: CapabilityState::Passed,
                detail: detail.into(),
            });
        }
    }

    pub fn fail(
        &mut self,
        stage: RemoteStage,
        detail: impl Into<String>,
        retryable: bool,
        action_required: bool,
    ) {
        if let Some(result) = self.stages.iter_mut().find(|result| result.stage == stage) {
            result.state = CapabilityState::Failed {
                retryable,
                action_required,
            };
            result.detail = detail.into();
        } else {
            self.stages.push(CapabilityResult {
                stage,
                state: CapabilityState::Failed {
                    retryable,
                    action_required,
                },
                detail: detail.into(),
            });
        }
        self.connection = if action_required {
            RemoteConnectionState::ActionRequired {
                reason: format!("{stage} capability requires an explicit user action"),
            }
        } else {
            RemoteConnectionState::Failed {
                reason: format!("{stage} capability failed"),
            }
        };
    }

    pub fn connected(&mut self) {
        self.connection = RemoteConnectionState::Connected;
    }

    pub fn all_required_passed(&self) -> bool {
        self.stages
            .iter()
            .all(|result| matches!(result.state, CapabilityState::Passed))
    }
}

/// A socket path a device registration may name: absolute, one line.
pub fn valid_remote_socket_path(path: &str) -> bool {
    path.starts_with('/') && path.len() > 1 && !path.bytes().any(|byte| byte.is_ascii_control())
}
