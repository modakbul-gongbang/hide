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

/// What the operator allowed on a device: install and update Hide's node
/// and its `hide` command under `helper_root`, link `hide` in `cli_dir` when
/// that name is free or already Hide's, run the node only for the life of an
/// SSH connection, and perform file and Git work inside registered
/// checkouts, with trash moves and worktree removals still confirmed one by
/// one. `contract` names that scope; a build whose scope differs asks again,
/// and so does a device that answers with another identity than the one the
/// consent was first used on.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostConsent {
    pub contract: u32,
    pub helper_root: String,
    /// Absent in a consent given before contract 2, which never covered it.
    #[serde(default)]
    pub cli_dir: Option<String>,
    pub granted_at_unix_ms: u64,
    /// The account, address and host key the node first ran on; bound on
    /// the first connection after consent and never rewritten by one.
    #[serde(default)]
    pub identity: Option<HostIdentity>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostIdentity {
    pub user: String,
    pub hostname: String,
    pub port: u16,
    pub host_key_sha256: String,
}

impl HostIdentity {
    pub fn describe(&self) -> String {
        format!(
            "{}@{}:{} ({})",
            self.user, self.hostname, self.port, self.host_key_sha256
        )
    }
}

/// The scope the operator agrees to, versioned. A build that needs more than
/// this contract describes bumps it, and every device asks again (B51).
/// Contract 2 added the `hide` command installed beside the helper and its
/// link in the consented command folder. Contract 3 is the whole install kit
/// (PRD device-parity D-12): the hook entries besides the command
/// (it also held the labels plugin, which labels-in-hided retired, so the
/// scope only narrowed); a contract-2 consent with the same folders is
/// carried to 3 on its next connection without asking (D-13).
pub const HOST_CONSENT_CONTRACT: u32 = 3;

/// The contract a consent may be carried forward from without asking.
pub const HOST_CONSENT_CARRIED_FROM: u32 = 2;

/// Where the node is installed on the device unless the daemon was started
/// with another root; `~` is the remote account's home.
pub const DEFAULT_HELPER_ROOT: &str = hide_kit::layout::HELPER_ROOT;

/// Where the device's `hide` command is linked unless the daemon was started
/// with another folder; `~` is the remote account's home.
pub const DEFAULT_CLI_DIR: &str = "~/.local/bin";

#[derive(Debug)]
pub enum EstablishError {
    Connect(RemoteError),
    /// The device answering the alias is not the one consent was given for.
    IdentityChanged {
        bound: Box<HostIdentity>,
        observed: Box<HostIdentity>,
    },
    /// This build cannot serve the device; nothing was installed.
    Unsupported(String),
    Install(String),
    Helper(String),
}

impl fmt::Display for EstablishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(error) => write!(formatter, "{}", error.diagnostic().reason),
            Self::IdentityChanged { bound, observed } => write!(
                formatter,
                "The device now answers as {}, not {} that the helper was allowed on; allow it again in Settings to continue",
                observed.describe(),
                bound.describe()
            ),
            Self::Unsupported(reason) | Self::Install(reason) | Self::Helper(reason) => {
                formatter.write_str(reason)
            }
        }
    }
}

/// A device's node, started and answering on its link.
pub struct Established {
    pub host: std::sync::Arc<dyn crate::NodeLink>,
    pub identity: HostIdentity,
    pub hello: crate::protocol::Hello,
    /// The node was installed or replaced on this connection.
    pub installed: bool,
    pub helper_path: String,
    /// How the build's files reached the device on this connection.
    pub upload: Upload,
}

/// What one connection's install did with the build's files: how many it
/// sent, how many were already there, and the kit parts left out with why.
/// The kit on the device reports a left-out part on the device's row; this
/// is the diagnostic detail.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Upload {
    pub sent: usize,
    pub reused: usize,
    pub missing: Vec<String>,
}

/// A device terminal session's ends: its output, its input when the session
/// takes input, and what ends it.
pub type TerminalSessionParts = (
    Box<dyn std::io::Read + Send>,
    Option<Box<dyn std::io::Write + Send>>,
    Box<dyn FnOnce() + Send>,
);

/// Opens the transport to a registered device. The node that holds the SSH
/// configuration and keys implements it (`hide_node::ssh`); the core asks it
/// by alias and never reads `~/.ssh` itself (PRD core-host-node D-21).
pub trait DeviceConnector: Send + Sync {
    /// The transport for the device behind `alias`, resolved through this
    /// node's SSH configuration; nothing connects until it is used.
    fn transport(
        &self,
        alias: &str,
        herdr_socket: Option<String>,
    ) -> RemoteResult<std::sync::Arc<dyn DeviceTransport>>;
}

/// One registered device as the core reaches it: its Herdr API, its node
/// link, its terminal sessions and attachment staging, each over SSH.
pub trait DeviceTransport: Send + Sync {
    /// The device's Herdr API, over this transport.
    fn herdr_api_connector(&self) -> std::sync::Arc<dyn hide_herdr_client::ApiConnector>;
    /// The Herdr version the device reported last, if it has.
    fn cached_herdr_version(&self) -> Option<String>;
    /// Probes each stage of a connection for the operator's test.
    fn capability_test(&self, operation_id: &str) -> CapabilityReport;
    /// Connects, checks `consent` against the device that answered, installs
    /// this build's node when the device lacks it, and starts it. Blocking;
    /// run it off the runtime lock. `on_close` hears why the link ended.
    fn establish(
        &self,
        consent: &HostConsent,
        retirement_projects: &[String],
        on_close: Box<dyn FnOnce(String) + Send + 'static>,
    ) -> Result<Established, EstablishError>;
    /// A Herdr terminal session for `pane_id` on the device.
    fn open_terminal_session(
        &self,
        pane_id: &str,
        mode: &str,
        rows: u16,
        cols: u16,
    ) -> RemoteResult<TerminalSessionParts>;
    /// Stages `files` on the device under `request_id`; answers each one's
    /// path there.
    fn stage_attachments(
        &self,
        request_id: &str,
        files: &[crate::attachments::AttachmentFile],
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<String>, String>;
    /// Removes what `request_id` staged.
    fn remove_attachments(&self, request_id: &str, files: &[crate::attachments::AttachmentFile]);
    /// The concrete transport, for the shell that built it and reaches parts
    /// the core does not use (browser and return-route forwards).
    fn as_any(&self) -> &dyn std::any::Any;
}
