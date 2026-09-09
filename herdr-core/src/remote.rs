//! Remote host, transport, and projection boundaries for the IDE.
//!
//! The module intentionally keeps the product contract separate from the SSH
//! implementation. Alias import and state projection are deterministic and
//! testable without a network. The russh adapter is the only production
//! transport; no OpenSSH subprocess is used by this path.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use russh::client::{self, Handle, Handler, Msg};
use russh::keys::{
    PrivateKeyWithHashAlg, PublicKeyOrCertificate, agent::client::AgentClient,
    check_known_hosts_path, load_secret_key,
};
use russh::{Channel, ChannelMsg, ChannelOpenFailure, Disconnect, Pty};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileType as SftpFileType, OpenFlags};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::Value;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::runtime::{Builder, Runtime};

use crate::domain::{
    DomainEvent, DomainProjection, DomainSnapshot, EnvironmentContract, HostScope,
};
use crate::herdr_api::{ApiConnector, ApiError, ApiStream, ConnectionShutdown};
use crate::remote_files::{FileEntry, FileKind, FileResult, FileServiceError, SftpTransport};
#[cfg(test)]
use crate::remote_files::{FileService, RemoteFileService};

pub use crate::herdr_contract::HERDR_PROTOCOL_REVISION as REMOTE_PROTOCOL_REVISION;
const SSH_OPERATION_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_REMOTE_TERM: &str = "xterm-256color";

/// Environment names read by this module. Values remain in the process
/// environment and are never copied into diagnostics or remote commands.
pub const REMOTE_PROCESS_ENVIRONMENT: &[EnvironmentContract] = &[
    EnvironmentContract {
        key: "HOME",
        value: None,
        requirement: "required",
        missing_behavior: "known_hosts resolution fails visibly",
    },
    EnvironmentContract {
        key: "SSH_AUTH_SOCK",
        value: None,
        requirement: "optional",
        missing_behavior: "agent authentication uses the ssh config IdentityAgent, and reports an explicit action-required failure when neither is set",
    },
];

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

fn remote_error(
    operation_id: &str,
    target: &str,
    stage: RemoteStage,
    error: impl fmt::Display,
    retryable: bool,
    action_required: bool,
) -> RemoteError {
    RemoteError::new(
        operation_id,
        target,
        stage,
        error.to_string(),
        retryable,
        action_required,
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SshAlias {
    /// Stable local identity based on the alias, not a resolved IP address.
    pub host_id: String,
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub identity_file: Option<PathBuf>,
    /// Where agent authentication looks for its socket, resolved from the SSH
    /// config the same way OpenSSH resolves it.
    pub agent_socket: AgentSocket,
    pub known_hosts_file: PathBuf,
}

/// The agent socket an alias authenticates through.
///
/// OpenSSH resolves this from `IdentityAgent` and only falls back to
/// `SSH_AUTH_SOCK` when no directive matched. Reading `SSH_AUTH_SOCK` alone
/// reaches whichever agent the launching process happened to carry, which for a
/// Finder launch is the empty launchd agent rather than the one the user
/// configured.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AgentSocket {
    /// No `IdentityAgent` matched the alias, so `SSH_AUTH_SOCK` decides.
    Environment,
    /// `IdentityAgent none`: this host authenticates without an agent.
    Disabled,
    /// `IdentityAgent <path>`: this socket, whatever the environment holds.
    Path(PathBuf),
}

impl AgentSocket {
    /// Names the source in diagnostics. The socket path itself never appears,
    /// because it is a routing value the environment contract keeps out of
    /// diagnostics.
    fn source(&self) -> &'static str {
        match self {
            Self::Environment => "SSH_AUTH_SOCK",
            Self::Disabled => "IdentityAgent none",
            Self::Path(_) => "the ssh config IdentityAgent",
        }
    }
}

impl SshAlias {
    pub fn from_config_contents(
        alias: &str,
        contents: &str,
        known_hosts_file: impl Into<PathBuf>,
    ) -> RemoteResult<Self> {
        validate_alias(alias)?;
        let config = russh_config::parse(contents, alias).map_err(|error| {
            remote_error(
                "ssh-alias-import",
                alias,
                RemoteStage::Alias,
                error,
                false,
                true,
            )
        })?;
        let hostname = config.host().trim().to_owned();
        if hostname.is_empty() || hostname.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(remote_error(
                "ssh-alias-import",
                alias,
                RemoteStage::Alias,
                "Host entry does not resolve a hostname",
                false,
                true,
            ));
        }
        let user = config.user().trim().to_owned();
        if user.is_empty() || user.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(remote_error(
                "ssh-alias-import",
                alias,
                RemoteStage::Alias,
                "Host entry does not resolve a usable user",
                false,
                true,
            ));
        }
        let identity_file = config
            .host_config
            .identity_file
            .as_ref()
            .and_then(|files| files.first())
            .cloned();
        let agent_socket = parse_identity_agent(alias, contents)?;
        Ok(Self {
            host_id: format!("ssh:{alias}"),
            alias: alias.to_owned(),
            hostname,
            user,
            port: config.port(),
            identity_file,
            agent_socket,
            known_hosts_file: known_hosts_file.into(),
        })
    }

    pub fn from_config_file(path: &Path, alias: &str) -> RemoteResult<Self> {
        let contents = std::fs::read_to_string(path).map_err(|error| {
            remote_error(
                "ssh-alias-import",
                &path.display().to_string(),
                RemoteStage::Alias,
                error,
                true,
                false,
            )
        })?;
        let known_hosts = default_known_hosts_path().map_err(|error| {
            remote_error(
                "ssh-alias-import",
                alias,
                RemoteStage::Alias,
                error,
                false,
                true,
            )
        })?;
        Self::from_config_contents(alias, &contents, known_hosts)
    }

    pub fn identity(&self) -> RemoteHostIdentity {
        RemoteHostIdentity {
            host_id: self.host_id.clone(),
            alias: self.alias.clone(),
            hostname: self.hostname.clone(),
            port: self.port,
        }
    }

    pub fn target(&self) -> String {
        format!("{}@{}:{}", self.user, self.hostname, self.port)
    }
}

pub fn import_ssh_aliases(path: &Path) -> RemoteResult<Vec<SshAlias>> {
    let contents = std::fs::read_to_string(path).map_err(|error| {
        remote_error(
            "ssh-alias-import",
            &path.display().to_string(),
            RemoteStage::Alias,
            error,
            true,
            false,
        )
    })?;
    let known_hosts = default_known_hosts_path().map_err(|error| {
        remote_error(
            "ssh-alias-import",
            &path.display().to_string(),
            RemoteStage::Alias,
            error,
            false,
            true,
        )
    })?;
    import_ssh_aliases_from_str(&contents, known_hosts)
}

pub fn import_ssh_aliases_from_str(
    contents: &str,
    known_hosts_file: impl Into<PathBuf>,
) -> RemoteResult<Vec<SshAlias>> {
    let known_hosts_file = known_hosts_file.into();
    let mut aliases = scan_alias_names(contents);
    aliases.sort();
    aliases.dedup();
    aliases
        .into_iter()
        .map(|alias| SshAlias::from_config_contents(&alias, contents, known_hosts_file.clone()))
        .collect()
}

fn scan_alias_names(contents: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    for line in contents.lines() {
        let line = line.trim();
        let Some((keyword, values)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if !keyword.eq_ignore_ascii_case("host") {
            continue;
        }
        for value in values.split_whitespace() {
            if value.is_empty()
                || value.starts_with('!')
                || value.contains('*')
                || value.contains('?')
            {
                continue;
            }
            names.insert(value.to_owned());
        }
    }
    names.into_iter().collect()
}

/// Resolves `IdentityAgent` for one alias.
///
/// `russh_config` drops the directive, so the `Host` block matching that
/// already decides hostname and user is repeated here. `Match` blocks are
/// ignored, exactly as they are for every other directive this module reads.
fn parse_identity_agent(alias: &str, contents: &str) -> RemoteResult<AgentSocket> {
    let mut applies = false;
    for line in contents.lines() {
        let Some((keyword, value)) = split_config_line(line) else {
            continue;
        };
        if keyword.eq_ignore_ascii_case("host") {
            applies = host_patterns_match(alias, value);
        } else if keyword.eq_ignore_ascii_case("match") {
            applies = false;
        } else if applies && keyword.eq_ignore_ascii_case("identityagent") {
            // OpenSSH keeps the first value it obtains for a keyword.
            return resolve_identity_agent(alias, value);
        }
    }
    Ok(AgentSocket::Environment)
}

fn split_config_line(line: &str) -> Option<(&str, &str)> {
    let line = line.split('#').next().unwrap_or_default().trim();
    let index = line.find(|character: char| character.is_whitespace() || character == '=')?;
    let (keyword, remainder) = line.split_at(index);
    let value = remainder
        .trim_start_matches(|character: char| character.is_whitespace() || character == '=')
        .trim_end();
    (!value.is_empty()).then_some((keyword, value))
}

/// OpenSSH `Host` matching: one positive pattern has to match and no negated
/// pattern may.
fn host_patterns_match(alias: &str, patterns: &str) -> bool {
    let mut matched = false;
    for pattern in patterns.split_whitespace() {
        if let Some(negated) = pattern.strip_prefix('!') {
            if matches_host_pattern(alias, negated) {
                return false;
            }
        } else if matches_host_pattern(alias, pattern) {
            matched = true;
        }
    }
    matched
}

fn matches_host_pattern(candidate: &str, pattern: &str) -> bool {
    let candidate: Vec<char> = candidate.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    // Iterative wildcard match: `star` remembers the last `*` so a failed tail
    // can retry one character later without recursing.
    let (mut c, mut p) = (0usize, 0usize);
    let (mut star, mut retry) = (None, 0usize);
    while c < candidate.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == candidate[c]) {
            c += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            retry = c;
            p += 1;
        } else if let Some(star) = star {
            p = star + 1;
            retry += 1;
            c = retry;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|character| *character == '*')
}

fn resolve_identity_agent(alias: &str, value: &str) -> RemoteResult<AgentSocket> {
    let value = value.trim_matches('"');
    if value.eq_ignore_ascii_case("none") {
        return Ok(AgentSocket::Disabled);
    }
    if value == "SSH_AUTH_SOCK" || value == "$SSH_AUTH_SOCK" {
        return Ok(AgentSocket::Environment);
    }
    if value.starts_with('$') {
        return Err(alias_error(
            alias,
            "IdentityAgent names an environment variable outside the declared remote environment contract",
        ));
    }
    let socket = expand_identity_agent_path(alias, value)?;
    if !socket.is_absolute() {
        return Err(alias_error(
            alias,
            "IdentityAgent must resolve to an absolute Unix-domain socket path",
        ));
    }
    Ok(AgentSocket::Path(socket))
}

fn expand_identity_agent_path(alias: &str, value: &str) -> RemoteResult<PathBuf> {
    if let Some(rest) = value.strip_prefix("~/").or(value.strip_prefix("%d/")) {
        let home = read_remote_environment("HOME")
            .map_err(|error| alias_error(alias, error))?
            .map(PathBuf::from)
            .ok_or_else(|| {
                alias_error(
                    alias,
                    "IdentityAgent is home-relative and HOME is not set, so it cannot be resolved",
                )
            })?;
        return Ok(home.join(expand_percent_escapes(alias, rest)?));
    }
    Ok(PathBuf::from(expand_percent_escapes(alias, value)?))
}

fn expand_percent_escapes(alias: &str, value: &str) -> RemoteResult<String> {
    let mut expanded = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '%' {
            expanded.push(character);
            continue;
        }
        match characters.next() {
            Some('%') => expanded.push('%'),
            _ => {
                return Err(alias_error(
                    alias,
                    "IdentityAgent carries a percent token this shell does not expand",
                ));
            }
        }
    }
    Ok(expanded)
}

fn alias_error(alias: &str, cause: impl fmt::Display) -> RemoteError {
    remote_error(
        "ssh-alias-import",
        alias,
        RemoteStage::Alias,
        cause,
        false,
        true,
    )
}

fn validate_alias(alias: &str) -> RemoteResult<()> {
    if alias.is_empty()
        || alias.chars().any(char::is_whitespace)
        || alias.bytes().any(|byte| byte.is_ascii_control())
        || alias.contains('/')
        || alias.starts_with('-')
    {
        return Err(remote_error(
            "ssh-alias-import",
            alias,
            RemoteStage::Alias,
            "alias must be one non-empty SSH config token",
            false,
            true,
        ));
    }
    Ok(())
}

fn default_known_hosts_path() -> io::Result<PathBuf> {
    read_remote_environment("HOME")?
        .map(PathBuf::from)
        .map(|home| home.join(".ssh").join("known_hosts"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

fn read_remote_environment(key: &str) -> io::Result<Option<OsString>> {
    if !REMOTE_PROCESS_ENVIRONMENT
        .iter()
        .any(|contract| contract.key == key)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unregistered remote environment key {key}"),
        ));
    }
    Ok(std::env::var_os(key))
}

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteSnapshotEnvelope {
    pub host: HostScope,
    pub protocol: u32,
    pub event_sequence: u64,
    pub workspace_ids: Vec<String>,
    pub pane_ids: Vec<String>,
    pub agent_ids: Vec<String>,
}

#[cfg(test)]
pub fn decode_remote_snapshot(
    value: &Value,
    operation_id: &str,
) -> RemoteResult<RemoteSnapshotEnvelope> {
    crate::wire::remote_snapshot(value, operation_id)
}

pub struct RemoteHerdrProjection {
    host: RemoteHostIdentity,
    host_scope: HostScope,
    projection: DomainProjection,
    state: RemoteConnectionState,
    last_snapshot: Option<RemoteSnapshotEnvelope>,
    wire_only_snapshot: bool,
    disconnect_reason: Option<String>,
}

impl RemoteHerdrProjection {
    pub fn new(host: RemoteHostIdentity, session_id: impl Into<String>) -> Self {
        let host_scope = HostScope {
            host_id: host.host_id.clone(),
            session_id: session_id.into(),
        };
        Self {
            host,
            host_scope,
            projection: DomainProjection::default(),
            state: RemoteConnectionState::Reconnecting { attempt: 0 },
            last_snapshot: None,
            wire_only_snapshot: false,
            disconnect_reason: None,
        }
    }

    pub fn host(&self) -> &RemoteHostIdentity {
        &self.host
    }

    pub fn host_scope(&self) -> &HostScope {
        &self.host_scope
    }

    pub fn state(&self) -> &RemoteConnectionState {
        &self.state
    }

    pub fn domain(&self) -> &DomainProjection {
        &self.projection
    }

    pub fn last_snapshot(&self) -> Option<&RemoteSnapshotEnvelope> {
        self.last_snapshot.as_ref()
    }

    pub fn disconnect_reason(&self) -> Option<&str> {
        self.disconnect_reason.as_deref()
    }

    pub fn apply_snapshot(&mut self, snapshot: DomainSnapshot) -> RemoteResult<()> {
        validate_snapshot_host(&snapshot, &self.host_scope).map_err(|reason| {
            self.state = RemoteConnectionState::Failed {
                reason: reason.clone(),
            };
            remote_error(
                "remote-snapshot",
                &self.host.host_id,
                RemoteStage::Protocol,
                reason,
                false,
                true,
            )
        })?;
        self.projection.apply_snapshot(snapshot).map_err(|error| {
            self.state = RemoteConnectionState::Failed {
                reason: error.to_string(),
            };
            remote_error(
                "remote-snapshot",
                &self.host.host_id,
                RemoteStage::Protocol,
                error,
                true,
                false,
            )
        })?;
        self.last_snapshot = Some(snapshot_envelope(&self.projection, &self.host_scope));
        self.wire_only_snapshot = false;
        self.disconnect_reason = None;
        self.state = RemoteConnectionState::Connected;
        Ok(())
    }

    pub fn apply_event(&mut self, event: DomainEvent) -> RemoteResult<()> {
        if self.wire_only_snapshot {
            let reason =
                "typed domain snapshot is required before applying remote events".to_owned();
            self.state = RemoteConnectionState::Stale {
                reason: reason.clone(),
            };
            return Err(remote_error(
                "remote-event",
                &self.host.host_id,
                RemoteStage::Protocol,
                reason,
                true,
                false,
            ));
        }
        self.projection.apply_event(event).map_err(|error| {
            self.state = RemoteConnectionState::Stale {
                reason: error.to_string(),
            };
            remote_error(
                "remote-event",
                &self.host.host_id,
                RemoteStage::Protocol,
                error,
                true,
                false,
            )
        })?;
        self.disconnect_reason = None;
        self.state = RemoteConnectionState::Connected;
        Ok(())
    }

    /// Applies the remote snapshot identity envelope. A full `DomainSnapshot` can be
    /// installed with [`Self::apply_snapshot`] after the caller has decoded the
    /// server-specific layout payload. Keeping that distinction explicit prevents a
    /// decoded ID envelope from being mistaken for a complete event baseline.
    #[cfg(test)]
    pub fn apply_wire_snapshot(&mut self, value: &Value, operation_id: &str) -> RemoteResult<()> {
        let envelope = match decode_remote_snapshot(value, operation_id) {
            Ok(envelope) => envelope,
            Err(error) => {
                self.state = RemoteConnectionState::Stale {
                    reason: error.to_string(),
                };
                return Err(error);
            }
        };
        if envelope.host.host_id != self.host_scope.host_id
            || envelope.host.session_id != self.host_scope.session_id
        {
            let reason = format!(
                "snapshot host mismatch expected={}:{} received={}:{}",
                self.host_scope.host_id,
                self.host_scope.session_id,
                envelope.host.host_id,
                envelope.host.session_id
            );
            self.state = RemoteConnectionState::Stale {
                reason: reason.clone(),
            };
            return Err(remote_error(
                operation_id,
                &self.host.host_id,
                RemoteStage::Protocol,
                reason,
                true,
                true,
            ));
        }
        if let Some(previous) = self.last_snapshot.as_ref() {
            if envelope.event_sequence < previous.event_sequence {
                let reason = format!(
                    "snapshot sequence regressed previous={} received={}",
                    previous.event_sequence, envelope.event_sequence
                );
                self.state = RemoteConnectionState::Stale {
                    reason: reason.clone(),
                };
                return Err(remote_error(
                    operation_id,
                    &self.host.host_id,
                    RemoteStage::Protocol,
                    reason,
                    true,
                    false,
                ));
            }
            if envelope.event_sequence == previous.event_sequence && envelope != *previous {
                let reason = format!(
                    "snapshot identity changed without sequence advance sequence={}",
                    envelope.event_sequence
                );
                self.state = RemoteConnectionState::Stale {
                    reason: reason.clone(),
                };
                return Err(remote_error(
                    operation_id,
                    &self.host.host_id,
                    RemoteStage::Protocol,
                    reason,
                    true,
                    false,
                ));
            }
        }
        self.last_snapshot = Some(envelope);
        self.wire_only_snapshot = true;
        self.disconnect_reason = None;
        self.state = RemoteConnectionState::Connected;
        Ok(())
    }

    pub fn disconnected(&mut self, reason: impl Into<String>) {
        self.disconnect_reason = Some(reason.into());
        self.state = RemoteConnectionState::Reconnecting { attempt: 0 };
    }

    pub fn reconnecting(&mut self, attempt: u32) {
        self.state = RemoteConnectionState::Reconnecting { attempt };
    }

    pub fn stale_tunnel(&mut self, reason: impl Into<String>) {
        self.state = RemoteConnectionState::Stale {
            reason: reason.into(),
        };
    }

    pub fn protocol_mismatch(&mut self, reason: impl Into<String>) {
        self.state = RemoteConnectionState::ActionRequired {
            reason: reason.into(),
        };
    }
}

fn snapshot_envelope(projection: &DomainProjection, host: &HostScope) -> RemoteSnapshotEnvelope {
    RemoteSnapshotEnvelope {
        host: host.clone(),
        protocol: REMOTE_PROTOCOL_REVISION,
        event_sequence: projection.sequence(),
        workspace_ids: projection
            .workspaces()
            .map(|workspace| workspace.workspace_id.clone())
            .collect(),
        pane_ids: projection
            .workspaces()
            .flat_map(|workspace| workspace.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .map(|pane| pane.pane_id.clone())
            .collect(),
        agent_ids: projection
            .agents()
            .map(|agent| agent.agent_instance_id.clone())
            .collect(),
    }
}

fn validate_snapshot_host(snapshot: &DomainSnapshot, expected: &HostScope) -> Result<(), String> {
    for workspace in &snapshot.workspaces {
        if workspace.host != *expected {
            return Err(format!(
                "workspace {} belongs to {}:{}, expected {}:{}",
                workspace.workspace_id,
                workspace.host.host_id,
                workspace.host.session_id,
                expected.host_id,
                expected.session_id
            ));
        }
    }
    for agent in &snapshot.agents {
        if agent.host != *expected {
            return Err(format!(
                "agent {} belongs to {}:{}, expected {}:{}",
                agent.agent_instance_id,
                agent.host.host_id,
                agent.host.session_id,
                expected.host_id,
                expected.session_id
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemotePtyEndpoint {
    pub host: HostScope,
    pub pane_id: String,
    pub agent_instance_id: Option<String>,
    pub terminal_id: String,
    pub protocol: u32,
    pub cols: u32,
    pub rows: u32,
    pub term: String,
}

impl RemotePtyEndpoint {
    pub fn validate(&self) -> RemoteResult<()> {
        if self.host.host_id.is_empty()
            || self.host.session_id.is_empty()
            || self.pane_id.is_empty()
            || self.terminal_id.is_empty()
        {
            return Err(remote_error(
                "remote-pty-route",
                "terminal",
                RemoteStage::Pty,
                "pane_id and terminal_id are required",
                false,
                true,
            ));
        }
        if self.protocol != REMOTE_PROTOCOL_REVISION {
            return Err(remote_error(
                "remote-pty-route",
                &self.pane_id,
                RemoteStage::Protocol,
                format!(
                    "protocol mismatch expected={REMOTE_PROTOCOL_REVISION} received={}",
                    self.protocol
                ),
                false,
                true,
            ));
        }
        if self.cols == 0
            || self.rows == 0
            || self.term.trim().is_empty()
            || self.term.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(remote_error(
                "remote-pty-route",
                &self.pane_id,
                RemoteStage::Pty,
                "terminal dimensions and TERM are required",
                false,
                true,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemoteSurface {
    Terminal(RemotePtyEndpoint),
    Editor {
        editor_id: String,
    },
    Browser {
        view_id: String,
        source_pane_id: String,
    },
}

pub fn require_terminal_surface(surface: &RemoteSurface) -> RemoteResult<&RemotePtyEndpoint> {
    match surface {
        RemoteSurface::Terminal(endpoint) => {
            endpoint.validate()?;
            Ok(endpoint)
        }
        RemoteSurface::Editor { editor_id } => Err(remote_error(
            "remote-pty-route",
            editor_id,
            RemoteStage::Pty,
            "editor surface has no terminal attach endpoint",
            false,
            false,
        )),
        RemoteSurface::Browser { view_id, .. } => Err(remote_error(
            "remote-pty-route",
            view_id,
            RemoteStage::Pty,
            "browser surface has no terminal attach endpoint",
            false,
            false,
        )),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemoteReadCommand {
    GitStatus { root: String },
}

impl RemoteReadCommand {
    fn operation_id(&self) -> &'static str {
        match self {
            Self::GitStatus { .. } => "remote-git-status",
        }
    }

    fn stage(&self) -> RemoteStage {
        match self {
            Self::GitStatus { .. } => RemoteStage::Git,
        }
    }

    fn command_line(&self) -> RemoteResult<String> {
        match self {
            Self::GitStatus { root } => {
                if !root.starts_with('/')
                    || root
                        .bytes()
                        .any(|byte| matches!(byte, b'\n' | b'\r' | b'\0'))
                {
                    return Err(remote_error(
                        self.operation_id(),
                        root,
                        self.stage(),
                        "Git root must be an absolute single-line path",
                        false,
                        true,
                    ));
                }
                Ok(format!(
                    "git -C {} status --short --porcelain=v1",
                    shell_quote(root)
                ))
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteCommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: u32,
}

#[derive(Clone)]
pub struct RusshRemoteClient {
    host: SshAlias,
    runtime: Arc<Runtime>,
}

#[derive(Clone)]
pub(crate) struct RusshApiConnector {
    connection: Arc<RusshApiConnection>,
    socket_path: String,
}

impl fmt::Debug for RusshApiConnector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RusshApiConnector")
            .field("host_id", &self.connection.client.host.host_id)
            .field("socket_path", &self.socket_path)
            .finish()
    }
}

struct RusshApiConnection {
    client: RusshRemoteClient,
    session: Mutex<Option<Handle<KnownHostHandler>>>,
}

impl RusshApiConnection {
    fn open_stream(&self, socket_path: &str) -> Result<russh::ChannelStream<Msg>, ApiError> {
        let mut session = self.session.lock().map_err(|_| {
            ApiError::Transport("remote Herdr SSH session state is poisoned".to_owned())
        })?;
        if session.is_none() {
            *session = Some(
                self.client
                    .runtime
                    .block_on(
                        self.client
                            .connect(KnownHostHandler::new(&self.client.host, None)),
                    )
                    .map_err(|error| ApiError::Transport(error.to_string()))?,
            );
        }
        let channel = self
            .client
            .runtime
            .block_on(
                session
                    .as_ref()
                    .expect("remote Herdr SSH session was initialized")
                    .channel_open_direct_streamlocal(socket_path.to_owned()),
            )
            .map_err(|error| {
                let stale = session.take();
                if let Some(stale) = stale {
                    let _ = self.client.runtime.block_on(stale.disconnect(
                        Disconnect::ByApplication,
                        "Herdr socket open failed",
                        "en",
                    ));
                }
                ApiError::Transport(format!(
                    "remote Herdr socket open failed for {}: {error}",
                    self.client.host.host_id
                ))
            })?;
        Ok(channel.into_stream())
    }
}

impl Drop for RusshApiConnection {
    fn drop(&mut self) {
        let session = match self.session.get_mut() {
            Ok(session) => session.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        let Some(session) = session else { return };
        if let Err(error) = self.client.runtime.block_on(session.disconnect(
            Disconnect::ByApplication,
            "Herdr socket connection complete",
            "en",
        )) {
            crate::diagnostic!(serde_json::json!({
                "component": "remote_herdr_api",
                "kind": "disconnect.failed",
                "target": self.client.host.host_id,
                "message": error.to_string(),
            }));
        }
    }
}

struct RusshApiShutdown {
    stopped: Arc<AtomicBool>,
}

impl ConnectionShutdown for RusshApiShutdown {
    fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
    }
}

struct RusshApiStream {
    runtime: Arc<Runtime>,
    _connection: Arc<RusshApiConnection>,
    stream: Option<russh::ChannelStream<Msg>>,
    stopped: Arc<AtomicBool>,
    read_timeout: Mutex<Option<Duration>>,
    write_timeout: Mutex<Option<Duration>>,
}

impl RusshApiStream {
    fn timeout(slot: &Mutex<Option<Duration>>, operation: &str) -> io::Result<Option<Duration>> {
        slot.lock().map(|timeout| *timeout).map_err(|_| {
            io::Error::other(format!(
                "remote Herdr {operation} timeout state is poisoned"
            ))
        })
    }

    fn stream(&mut self) -> io::Result<&mut russh::ChannelStream<Msg>> {
        self.stream.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "remote Herdr socket stream is closed",
            )
        })
    }
}

impl Read for RusshApiStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let deadline =
            Self::timeout(&self.read_timeout, "read")?.map(|value| Instant::now() + value);
        loop {
            if self.stopped.load(Ordering::Acquire) {
                return Ok(0);
            }
            let wait = match deadline {
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "remote Herdr socket read timed out",
                        ));
                    }
                    remaining.min(Duration::from_millis(100))
                }
                None => Duration::from_millis(100),
            };
            let runtime = Arc::clone(&self.runtime);
            let stream = self.stream()?;
            match runtime.block_on(async { tokio::time::timeout(wait, stream.read(buffer)).await })
            {
                Ok(result) => return result,
                Err(_) => continue,
            }
        }
    }
}

impl Write for RusshApiStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "remote Herdr socket stream is closed",
            ));
        }
        let timeout = Self::timeout(&self.write_timeout, "write")?.unwrap_or(SSH_OPERATION_TIMEOUT);
        let runtime = Arc::clone(&self.runtime);
        let stream = self.stream()?;
        runtime
            .block_on(async { tokio::time::timeout(timeout, stream.write(buffer)).await })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "remote Herdr socket write timed out",
                )
            })?
    }

    fn flush(&mut self) -> io::Result<()> {
        let timeout = Self::timeout(&self.write_timeout, "write")?.unwrap_or(SSH_OPERATION_TIMEOUT);
        let runtime = Arc::clone(&self.runtime);
        let stream = self.stream()?;
        runtime
            .block_on(async { tokio::time::timeout(timeout, stream.flush()).await })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "remote Herdr socket flush timed out",
                )
            })?
    }
}

impl ApiStream for RusshApiStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError> {
        *self.read_timeout.lock().map_err(|_| {
            ApiError::Transport("remote Herdr read timeout state is poisoned".to_owned())
        })? = timeout;
        Ok(())
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError> {
        *self.write_timeout.lock().map_err(|_| {
            ApiError::Transport("remote Herdr write timeout state is poisoned".to_owned())
        })? = timeout;
        Ok(())
    }

    fn read_line_with_timeout(&mut self, timeout: Duration) -> Result<String, ApiError> {
        let previous = Self::timeout(&self.read_timeout, "read")
            .map_err(|error| ApiError::Transport(error.to_string()))?;
        let deadline = Instant::now() + timeout;
        let mut bytes = Vec::new();
        let result = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break Err(ApiError::Transport(
                    "subscription acknowledgement timed out".to_owned(),
                ));
            }
            self.set_read_timeout(Some(remaining))?;
            let mut byte = [0_u8; 1];
            match self.read(&mut byte) {
                Ok(0) => {
                    break Err(ApiError::Transport(
                        "subscription acknowledgement reached EOF".to_owned(),
                    ));
                }
                Ok(_) => {
                    bytes.push(byte[0]);
                    if byte[0] == b'\n' {
                        break String::from_utf8(bytes).map_err(|error| {
                            ApiError::Malformed(format!(
                                "subscription acknowledgement was not UTF-8: {error}"
                            ))
                        });
                    }
                    if bytes.len() > 64 * 1024 {
                        break Err(ApiError::Malformed(
                            "subscription acknowledgement exceeds 64 KiB".to_owned(),
                        ));
                    }
                }
                Err(error) => break Err(ApiError::Transport(error.to_string())),
            }
        };
        self.set_read_timeout(previous)?;
        result
    }

    fn shutdown_handle(&self) -> Result<Box<dyn ConnectionShutdown>, ApiError> {
        Ok(Box::new(RusshApiShutdown {
            stopped: Arc::clone(&self.stopped),
        }))
    }
}

impl Drop for RusshApiStream {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let stream = self.stream.take();
        self.runtime.block_on(async { drop(stream) });
    }
}

impl ApiConnector for RusshApiConnector {
    fn connect(&self) -> Result<Box<dyn ApiStream>, ApiError> {
        let stream = self.connection.open_stream(&self.socket_path)?;
        Ok(Box::new(RusshApiStream {
            runtime: Arc::clone(&self.connection.client.runtime),
            _connection: Arc::clone(&self.connection),
            stream: Some(stream),
            stopped: Arc::new(AtomicBool::new(false)),
            read_timeout: Mutex::new(None),
            write_timeout: Mutex::new(None),
        }))
    }
}

impl fmt::Debug for RusshRemoteClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RusshRemoteClient")
            .field("host_id", &self.host.host_id)
            .field("hostname", &self.host.hostname)
            .field("port", &self.host.port)
            .finish_non_exhaustive()
    }
}

impl RusshRemoteClient {
    pub fn new(host: SshAlias) -> RemoteResult<Self> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|error| {
                remote_error(
                    "remote-runtime",
                    &host.host_id,
                    RemoteStage::Ssh,
                    error,
                    false,
                    true,
                )
            })?;
        Ok(Self {
            host,
            runtime: Arc::new(runtime),
        })
    }

    pub fn host(&self) -> &SshAlias {
        &self.host
    }

    pub(crate) fn herdr_api_connector(
        &self,
        socket_path: impl Into<String>,
    ) -> RemoteResult<RusshApiConnector> {
        let socket_path = socket_path.into();
        if !Path::new(&socket_path).is_absolute()
            || socket_path.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(remote_error(
                "remote-herdr-socket",
                &self.host.host_id,
                RemoteStage::Herdr,
                "remote Herdr socket path must be absolute and single-line",
                false,
                true,
            ));
        }
        Ok(RusshApiConnector {
            connection: Arc::new(RusshApiConnection {
                client: self.clone(),
                session: Mutex::new(None),
            }),
            socket_path,
        })
    }

    pub(crate) fn open_terminal_session(
        &self,
        socket_path: &str,
        pane_id: &str,
        mode: &str,
        rows: u16,
        cols: u16,
    ) -> RemoteResult<RemoteTerminalProcess> {
        let command = remote_terminal_command(socket_path, pane_id, mode, rows, cols)?;
        let session = self
            .runtime
            .block_on(self.connect(KnownHostHandler::new(&self.host, None)))?;
        let operation = self.runtime.block_on(async {
            let channel = session.channel_open_session().await.map_err(|error| {
                remote_error(
                    "remote-terminal-session",
                    pane_id,
                    RemoteStage::Herdr,
                    error,
                    true,
                    false,
                )
            })?;
            channel.exec(true, command).await.map_err(|error| {
                remote_error(
                    "remote-terminal-session",
                    pane_id,
                    RemoteStage::Herdr,
                    error,
                    true,
                    false,
                )
            })?;
            let writer = (mode == "control").then(|| {
                Box::new(RemoteTerminalWriter {
                    runtime: Arc::clone(&self.runtime),
                    writer: Box::pin(channel.make_writer()),
                }) as Box<dyn Write + Send>
            });
            let reader = Box::new(RemoteTerminalReader {
                runtime: Arc::clone(&self.runtime),
                channel,
                pending: Vec::new(),
                pending_offset: 0,
            }) as Box<dyn Read + Send>;
            Ok::<_, RemoteError>((reader, writer))
        });
        match operation {
            Ok((reader, writer)) => {
                let connection = RemoteTerminalConnection {
                    runtime: Arc::clone(&self.runtime),
                    session: Some(session),
                    target_id: self.host.host_id.clone(),
                    pane_id: pane_id.to_owned(),
                };
                Ok(RemoteTerminalProcess {
                    reader,
                    writer,
                    shutdown: Box::new(move || connection.shutdown()),
                })
            }
            Err(primary) => {
                let cleanup = self
                    .runtime
                    .block_on(session.disconnect(
                        Disconnect::ByApplication,
                        "remote terminal session open failed",
                        "en",
                    ))
                    .map_err(|error| {
                        remote_error(
                            "remote-terminal-session",
                            pane_id,
                            RemoteStage::Cleanup,
                            error,
                            true,
                            false,
                        )
                    });
                combine_cleanup("remote-terminal-session", pane_id, Err(primary), cleanup)
            }
        }
    }

    pub fn staged_capability_test(
        &self,
        operation_id: &str,
        herdr_socket_path: &str,
        probe_tunnel: bool,
    ) -> CapabilityReport {
        let mut report = CapabilityReport::new(operation_id, self.host.identity());
        let connection = self
            .runtime
            .block_on(self.connect(KnownHostHandler::new(&self.host, None)));
        match connection {
            Ok(session) => {
                report.pass(
                    RemoteStage::Ssh,
                    "TCP SSH handshake and known_hosts verification passed",
                );
                report.pass(
                    RemoteStage::Auth,
                    "configured key or SSH agent authentication passed",
                );
                let _ = self.runtime.block_on(session.disconnect(
                    Disconnect::ByApplication,
                    "capability probe complete",
                    "en",
                ));
            }
            Err(error) => {
                report.fail(
                    RemoteStage::Ssh,
                    error.to_string(),
                    error.diagnostic().retryable,
                    error.diagnostic().action_required,
                );
                if error.stage() != RemoteStage::Ssh {
                    report.fail(
                        error.stage(),
                        error.to_string(),
                        error.diagnostic().retryable,
                        error.diagnostic().action_required,
                    );
                }
                return report;
            }
        }

        match self.fetch_herdr_snapshot(herdr_socket_path) {
            Ok(snapshot) => {
                report.pass(
                    RemoteStage::Herdr,
                    "official Herdr Socket API snapshot responded",
                );
                report.pass(
                    RemoteStage::Protocol,
                    format!(
                        "Herdr protocol={} event_sequence={}",
                        snapshot.protocol, snapshot.event_sequence
                    ),
                );
            }
            Err(error) => {
                report.fail(
                    error.stage(),
                    error.to_string(),
                    error.diagnostic().retryable,
                    error.diagnostic().action_required,
                );
                if error.stage() != RemoteStage::Protocol {
                    report.fail(
                        RemoteStage::Protocol,
                        error.to_string(),
                        error.diagnostic().retryable,
                        error.diagnostic().action_required,
                    );
                }
            }
        }

        match self.probe_pty() {
            Ok(()) => report.pass(
                RemoteStage::Pty,
                "remote PTY allocation and teardown passed",
            ),
            Err(error) => report.fail(
                RemoteStage::Pty,
                error.to_string(),
                error.diagnostic().retryable,
                error.diagnostic().action_required,
            ),
        }

        match self.probe_sftp() {
            Ok(path) => report.pass(RemoteStage::Sftp, format!("canonical root {path}")),
            Err(error) => report.fail(
                error.stage(),
                error.to_string(),
                error.diagnostic().retryable,
                error.diagnostic().action_required,
            ),
        }

        match self.exec_read_only(RemoteReadCommand::GitStatus {
            root: "/".to_owned(),
        }) {
            Ok(output) if output.exit_status == 0 || output.exit_status == 128 => {
                report.pass(
                    RemoteStage::Git,
                    "read-only git status command reached remote host",
                );
            }
            Ok(output) => report.fail(
                RemoteStage::Git,
                format!(
                    "exit={} stderr={}",
                    output.exit_status,
                    redact_output(&output.stderr)
                ),
                true,
                false,
            ),
            Err(error) => report.fail(
                error.stage(),
                error.to_string(),
                error.diagnostic().retryable,
                error.diagnostic().action_required,
            ),
        }

        if probe_tunnel {
            let spec = ReverseBrowserBridgeSpec::loopback_probe();
            match self.start_reverse_browser_bridge(spec) {
                Ok(tunnel) => match tunnel.close() {
                    Ok(()) => report.pass(
                        RemoteStage::Tunnel,
                        "owned loopback reverse forward allocated and released",
                    ),
                    Err(error) => report.fail(
                        RemoteStage::Tunnel,
                        error.to_string(),
                        error.diagnostic().retryable,
                        error.diagnostic().action_required,
                    ),
                },
                Err(error) => report.fail(
                    error.stage(),
                    error.to_string(),
                    error.diagnostic().retryable,
                    error.diagnostic().action_required,
                ),
            }
        } else {
            report.fail(
                RemoteStage::Tunnel,
                "tunnel probe is explicit because it changes remote forwarding state",
                false,
                true,
            );
        }
        if report.all_required_passed() {
            report.connected();
        }
        report
    }

    fn probe_pty(&self) -> RemoteResult<()> {
        let session = self
            .runtime
            .block_on(self.connect(KnownHostHandler::new(&self.host, None)))?;
        let result = self.runtime.block_on(async {
            let channel = session.channel_open_session().await.map_err(|error| {
                remote_error(
                    "remote-pty-probe",
                    &self.host.host_id,
                    RemoteStage::Pty,
                    error,
                    true,
                    false,
                )
            })?;
            channel
                .request_pty(
                    true,
                    DEFAULT_REMOTE_TERM,
                    120,
                    40,
                    0,
                    0,
                    &[] as &[(Pty, u32)],
                )
                .await
                .map_err(|error| {
                    remote_error(
                        "remote-pty-probe",
                        &self.host.host_id,
                        RemoteStage::Pty,
                        error,
                        true,
                        false,
                    )
                })?;
            channel.close().await.map_err(|error| {
                remote_error(
                    "remote-pty-probe",
                    &self.host.host_id,
                    RemoteStage::Pty,
                    error,
                    true,
                    false,
                )
            })?;
            Ok::<(), RemoteError>(())
        });
        let disconnect = self.runtime.block_on(session.disconnect(
            Disconnect::ByApplication,
            "PTY capability probe complete",
            "en",
        ));
        let disconnect = disconnect.map_err(|error| {
            remote_error(
                "remote-pty-probe",
                &self.host.host_id,
                RemoteStage::Cleanup,
                error,
                true,
                false,
            )
        });
        match (result, disconnect) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(primary), Ok(())) => Err(primary),
            (Ok(()), Err(cleanup)) => Err(cleanup),
            (Err(primary), Err(cleanup)) => Err(remote_error(
                "remote-pty-probe",
                &self.host.host_id,
                primary.stage(),
                format!("{}; cleanup failed: {cleanup}", primary.diagnostic().reason),
                primary.diagnostic().retryable,
                primary.diagnostic().action_required,
            )),
        }
    }

    pub fn exec_read_only(&self, command: RemoteReadCommand) -> RemoteResult<RemoteCommandOutput> {
        let operation_id = command.operation_id();
        let stage = command.stage();
        let command_line = command.command_line()?;
        let mut session = self
            .runtime
            .block_on(self.connect(KnownHostHandler::new(&self.host, None)))?;
        let output = self.runtime.block_on(async {
            execute_channel(
                &mut session,
                &command_line,
                operation_id,
                &self.host.host_id,
                stage,
            )
            .await
        });
        let disconnect = self.runtime.block_on(session.disconnect(
            Disconnect::ByApplication,
            "read-only operation complete",
            "en",
        ));
        match (output, disconnect) {
            (Ok(output), Ok(())) => Ok(output),
            (Err(primary), Ok(())) => Err(primary),
            (Ok(_), Err(error)) => Err(remote_error(
                operation_id,
                &self.host.host_id,
                RemoteStage::Cleanup,
                error,
                true,
                false,
            )),
            (Err(primary), Err(cleanup)) => Err(remote_error(
                operation_id,
                &self.host.host_id,
                primary.stage(),
                format!("{}; cleanup failed: {cleanup}", primary.diagnostic().reason),
                primary.diagnostic().retryable,
                primary.diagnostic().action_required,
            )),
        }
    }

    pub fn fetch_herdr_snapshot(&self, socket_path: &str) -> RemoteResult<RemoteSnapshotEnvelope> {
        let connector = self.herdr_api_connector(socket_path)?;
        let response = crate::herdr_api::request_with_connector(
            &connector,
            "session.snapshot",
            crate::wire::empty_params(),
            SSH_OPERATION_TIMEOUT,
        )
        .map_err(|error| self.remote_snapshot_error(error))?;
        crate::wire::remote_snapshot(&response, "remote-herdr-snapshot")
    }

    #[cfg(test)]
    pub fn fetch_herdr_snapshot_value(&self, socket_path: &str) -> RemoteResult<Value> {
        let connector = self.herdr_api_connector(socket_path)?;
        crate::herdr_api::request_with_connector(
            &connector,
            "session.snapshot",
            crate::wire::empty_params(),
            SSH_OPERATION_TIMEOUT,
        )
        .map_err(|error| self.remote_snapshot_error(error))
    }

    fn remote_snapshot_error(&self, error: ApiError) -> RemoteError {
        let (stage, retryable, action_required) = match &error {
            ApiError::Malformed(_) => (RemoteStage::Protocol, false, true),
            ApiError::Transport(_) | ApiError::Remote { .. } => (RemoteStage::Herdr, true, false),
        };
        remote_error(
            "remote-herdr-snapshot",
            &self.host.host_id,
            stage,
            error,
            retryable,
            action_required,
        )
    }

    pub fn open_pty(&self, endpoint: RemotePtyEndpoint) -> RemoteResult<RemotePtySession> {
        endpoint.validate()?;
        if endpoint.host.host_id != self.host.host_id {
            return Err(remote_error(
                "remote-pty-open",
                &endpoint.pane_id,
                RemoteStage::Pty,
                "terminal endpoint is owned by another host",
                false,
                true,
            ));
        }
        let session = self
            .runtime
            .block_on(self.connect(KnownHostHandler::new(&self.host, None)))?;
        let operation = self.runtime.block_on(async {
            let channel = session.channel_open_session().await.map_err(|error| {
                remote_error(
                    "remote-pty-open",
                    &endpoint.pane_id,
                    RemoteStage::Pty,
                    error,
                    true,
                    false,
                )
            })?;
            channel
                .request_pty(
                    true,
                    &endpoint.term,
                    endpoint.cols,
                    endpoint.rows,
                    0,
                    0,
                    &[] as &[(Pty, u32)],
                )
                .await
                .map_err(|error| {
                    remote_error(
                        "remote-pty-open",
                        &endpoint.pane_id,
                        RemoteStage::Pty,
                        error,
                        true,
                        false,
                    )
                })?;
            channel.request_shell(true).await.map_err(|error| {
                remote_error(
                    "remote-pty-open",
                    &endpoint.pane_id,
                    RemoteStage::Pty,
                    error,
                    true,
                    false,
                )
            })?;
            Ok::<Channel<Msg>, RemoteError>(channel)
        });
        match operation {
            Ok(channel) => Ok(RemotePtySession {
                runtime: Arc::clone(&self.runtime),
                session: Mutex::new(Some(session)),
                channel: Mutex::new(Some(channel)),
                endpoint,
            }),
            Err(primary) => {
                let cleanup = self
                    .runtime
                    .block_on(session.disconnect(
                        Disconnect::ByApplication,
                        "PTY open failed",
                        "en",
                    ))
                    .map_err(|error| {
                        remote_error(
                            "remote-pty-open",
                            &endpoint.pane_id,
                            RemoteStage::Cleanup,
                            error,
                            true,
                            false,
                        )
                    });
                combine_cleanup("remote-pty-open", &endpoint.pane_id, Err(primary), cleanup)
            }
        }
    }

    fn probe_sftp(&self) -> RemoteResult<String> {
        let session = self
            .runtime
            .block_on(self.connect(KnownHostHandler::new(&self.host, None)))?;
        let operation = self.runtime.block_on(async {
            let channel = session.channel_open_session().await.map_err(|error| {
                remote_error(
                    "remote-sftp-probe",
                    &self.host.host_id,
                    RemoteStage::Sftp,
                    error,
                    true,
                    false,
                )
            })?;
            channel
                .request_subsystem(true, "sftp")
                .await
                .map_err(|error| {
                    remote_error(
                        "remote-sftp-probe",
                        &self.host.host_id,
                        RemoteStage::Sftp,
                        error,
                        true,
                        false,
                    )
                })?;
            let sftp = SftpSession::new(channel.into_stream())
                .await
                .map_err(|error| {
                    remote_error(
                        "remote-sftp-probe",
                        &self.host.host_id,
                        RemoteStage::Sftp,
                        error,
                        true,
                        false,
                    )
                })?;
            let result = sftp.canonicalize(".").await.map_err(|error| {
                remote_error(
                    "remote-sftp-probe",
                    &self.host.host_id,
                    RemoteStage::Sftp,
                    error,
                    true,
                    false,
                )
            })?;
            let cleanup = sftp.close().await.map_err(|error| {
                remote_error(
                    "remote-sftp-probe",
                    &self.host.host_id,
                    RemoteStage::Cleanup,
                    error,
                    true,
                    false,
                )
            });
            combine_cleanup("remote-sftp-probe", &self.host.host_id, Ok(result), cleanup)
        });
        let disconnect = self
            .runtime
            .block_on(session.disconnect(Disconnect::ByApplication, "SFTP probe complete", "en"))
            .map_err(|error| {
                remote_error(
                    "remote-sftp-probe",
                    &self.host.host_id,
                    RemoteStage::Cleanup,
                    error,
                    true,
                    false,
                )
            });
        combine_cleanup(
            "remote-sftp-probe",
            &self.host.host_id,
            operation,
            disconnect,
        )
    }

    async fn connect(&self, handler: KnownHostHandler) -> RemoteResult<Handle<KnownHostHandler>> {
        let config = client::Config {
            inactivity_timeout: Some(SSH_OPERATION_TIMEOUT),
            keepalive_interval: Some(Duration::from_secs(5)),
            ..client::Config::default()
        };
        let mut session = client::connect(
            Arc::new(config),
            (self.host.hostname.as_str(), self.host.port),
            handler,
        )
        .await
        .map_err(|error| {
            remote_error(
                "remote-connect",
                &self.host.host_id,
                RemoteStage::Ssh,
                error,
                true,
                false,
            )
        })?;
        authenticate(&mut session, &self.host).await?;
        Ok(session)
    }

    fn sftp_read(&self, path: &str) -> RemoteResult<Vec<u8>> {
        let mut session = self
            .runtime
            .block_on(self.connect(KnownHostHandler::new(&self.host, None)))?;
        let operation = self.runtime.block_on(async {
            let sftp = open_sftp(&mut session, &self.host.host_id).await?;
            let result = sftp.read(path).await.map_err(|error| {
                remote_error(
                    "remote-sftp-read",
                    path,
                    RemoteStage::Sftp,
                    error,
                    true,
                    false,
                )
            })?;
            let cleanup = sftp.close().await.map_err(|error| {
                remote_error(
                    "remote-sftp-read",
                    path,
                    RemoteStage::Cleanup,
                    error,
                    true,
                    false,
                )
            });
            combine_cleanup("remote-sftp-read", path, Ok(result), cleanup)
        });
        let disconnect = self
            .runtime
            .block_on(session.disconnect(Disconnect::ByApplication, "SFTP read complete", "en"))
            .map_err(|error| {
                remote_error(
                    "remote-sftp-read",
                    path,
                    RemoteStage::Cleanup,
                    error,
                    true,
                    false,
                )
            });
        combine_cleanup("remote-sftp-read", path, operation, disconnect)
    }

    fn sftp_list(&self, path: &str) -> RemoteResult<Vec<FileEntry>> {
        let mut session = self
            .runtime
            .block_on(self.connect(KnownHostHandler::new(&self.host, None)))?;
        let operation = self.runtime.block_on(async {
            let sftp = open_sftp(&mut session, &self.host.host_id).await?;
            let directory = sftp.read_dir(path).await.map_err(|error| {
                remote_error(
                    "remote-sftp-list",
                    path,
                    RemoteStage::Sftp,
                    error,
                    true,
                    false,
                )
            })?;
            let mut entries = Vec::new();
            for entry in directory {
                let file_type = match entry.file_type() {
                    SftpFileType::Dir => FileKind::Directory,
                    SftpFileType::File => FileKind::File,
                    SftpFileType::Symlink | SftpFileType::Other => continue,
                };
                entries.push(FileEntry {
                    path: entry.path(),
                    name: entry.file_name(),
                    kind: file_type,
                    size_bytes: entry.metadata().len(),
                });
            }
            entries.sort_by(|left, right| left.path.cmp(&right.path));
            let cleanup = sftp.close().await.map_err(|error| {
                remote_error(
                    "remote-sftp-list",
                    path,
                    RemoteStage::Cleanup,
                    error,
                    true,
                    false,
                )
            });
            combine_cleanup("remote-sftp-list", path, Ok(entries), cleanup)
        });
        let disconnect = self
            .runtime
            .block_on(session.disconnect(Disconnect::ByApplication, "SFTP list complete", "en"))
            .map_err(|error| {
                remote_error(
                    "remote-sftp-list",
                    path,
                    RemoteStage::Cleanup,
                    error,
                    true,
                    false,
                )
            });
        combine_cleanup("remote-sftp-list", path, operation, disconnect)
    }

    fn sftp_write(&self, path: &str, bytes: &[u8]) -> RemoteResult<()> {
        let mut session = self
            .runtime
            .block_on(self.connect(KnownHostHandler::new(&self.host, None)))?;
        let operation = self.runtime.block_on(async {
            let sftp = open_sftp(&mut session, &self.host.host_id).await?;
            let mut file = sftp
                .open_with_flags(
                    path,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                )
                .await
                .map_err(|error| {
                    remote_error(
                        "remote-sftp-write",
                        path,
                        RemoteStage::Sftp,
                        error,
                        true,
                        false,
                    )
                })?;
            let result = file.write_all(bytes).await.map_err(|error| {
                remote_error(
                    "remote-sftp-write",
                    path,
                    RemoteStage::Sftp,
                    error,
                    true,
                    false,
                )
            });
            if let Err(error) = result {
                let cleanup = sftp.close().await.map_err(|close_error| {
                    remote_error(
                        "remote-sftp-write",
                        path,
                        RemoteStage::Cleanup,
                        close_error,
                        true,
                        false,
                    )
                });
                return combine_cleanup("remote-sftp-write", path, Err(error), cleanup);
            }
            let result = file.shutdown().await.map_err(|error| {
                remote_error(
                    "remote-sftp-write",
                    path,
                    RemoteStage::Sftp,
                    error,
                    true,
                    false,
                )
            });
            let cleanup = sftp.close().await.map_err(|error| {
                remote_error(
                    "remote-sftp-write",
                    path,
                    RemoteStage::Cleanup,
                    error,
                    true,
                    false,
                )
            });
            combine_cleanup("remote-sftp-write", path, result, cleanup)
        });
        let disconnect = self
            .runtime
            .block_on(session.disconnect(Disconnect::ByApplication, "SFTP write complete", "en"))
            .map_err(|error| {
                remote_error(
                    "remote-sftp-write",
                    path,
                    RemoteStage::Cleanup,
                    error,
                    true,
                    false,
                )
            });
        combine_cleanup("remote-sftp-write", path, operation, disconnect)
    }

    pub fn start_reverse_browser_bridge(
        &self,
        spec: ReverseBrowserBridgeSpec,
    ) -> RemoteResult<RemoteTunnelHandle> {
        spec.validate()?;
        let forward_error = Arc::new(Mutex::new(None));
        let handler = KnownHostHandler::new(&self.host, Some(spec.local_cdp))
            .with_forward_error(Arc::clone(&forward_error));
        let session = self.runtime.block_on(self.connect(handler))?;
        let forward = self.runtime.block_on(session.tcpip_forward(
            spec.remote_bind.ip().to_string(),
            u32::from(spec.remote_bind.port()),
        ));
        let remote_port = match forward {
            Ok(port) if port <= u32::from(u16::MAX) => port,
            Ok(port) => {
                let primary = remote_error(
                    "remote-browser-tunnel",
                    &self.host.host_id,
                    RemoteStage::Tunnel,
                    format!("remote forward port exceeds u16: {port}"),
                    false,
                    true,
                );
                let cleanup = self
                    .runtime
                    .block_on(session.disconnect(
                        Disconnect::ByApplication,
                        "invalid remote forward port",
                        "en",
                    ))
                    .map_err(|error| {
                        remote_error(
                            "remote-browser-tunnel",
                            &self.host.host_id,
                            RemoteStage::Cleanup,
                            error,
                            true,
                            false,
                        )
                    });
                return combine_cleanup(
                    "remote-browser-tunnel",
                    &self.host.host_id,
                    Err(primary),
                    cleanup,
                );
            }
            Err(error) => {
                let primary = remote_error(
                    "remote-browser-tunnel",
                    &self.host.host_id,
                    RemoteStage::Tunnel,
                    error,
                    true,
                    false,
                );
                let cleanup = self
                    .runtime
                    .block_on(session.disconnect(
                        Disconnect::ByApplication,
                        "forward setup failed",
                        "en",
                    ))
                    .map_err(|error| {
                        remote_error(
                            "remote-browser-tunnel",
                            &self.host.host_id,
                            RemoteStage::Cleanup,
                            error,
                            true,
                            false,
                        )
                    });
                return combine_cleanup(
                    "remote-browser-tunnel",
                    &self.host.host_id,
                    Err(primary),
                    cleanup,
                );
            }
        };
        let descriptor = RemoteTunnelDescriptor {
            tunnel_id: format!("tunnel:{}:{}", self.host.host_id, spec.browser_view_id),
            host_id: self.host.host_id.clone(),
            browser_view_id: spec.browser_view_id,
            local_cdp: spec.local_cdp,
            remote_bind: SocketAddr::new(spec.remote_bind.ip(), remote_port as u16),
            state: RemoteTunnelState::Ready,
            owned: true,
        };
        Ok(RemoteTunnelHandle {
            runtime: Arc::clone(&self.runtime),
            session: Mutex::new(Some(session)),
            descriptor,
            forward_error,
            closed: AtomicBool::new(false),
        })
    }
}

pub(crate) struct RemoteTerminalProcess {
    reader: Box<dyn Read + Send>,
    writer: Option<Box<dyn Write + Send>>,
    shutdown: Box<dyn FnOnce() + Send>,
}

type RemoteTerminalParts = (
    Box<dyn Read + Send>,
    Option<Box<dyn Write + Send>>,
    Box<dyn FnOnce() + Send>,
);

impl RemoteTerminalProcess {
    pub(crate) fn into_parts(self) -> RemoteTerminalParts {
        (self.reader, self.writer, self.shutdown)
    }
}

struct RemoteTerminalReader {
    runtime: Arc<Runtime>,
    channel: Channel<Msg>,
    pending: Vec<u8>,
    pending_offset: usize,
}

impl RemoteTerminalReader {
    fn copy_pending(&mut self, buffer: &mut [u8]) -> usize {
        let available = self.pending.len().saturating_sub(self.pending_offset);
        let copied = available.min(buffer.len());
        buffer[..copied].copy_from_slice(
            &self.pending[self.pending_offset..self.pending_offset.saturating_add(copied)],
        );
        self.pending_offset = self.pending_offset.saturating_add(copied);
        if self.pending_offset == self.pending.len() {
            self.pending.clear();
            self.pending_offset = 0;
        }
        copied
    }
}

impl Read for RemoteTerminalReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.pending_offset < self.pending.len() {
            return Ok(self.copy_pending(buffer));
        }
        loop {
            match self.runtime.block_on(self.channel.wait()) {
                Some(ChannelMsg::Data { data }) if !data.is_empty() => {
                    self.pending = data.to_vec();
                    return Ok(self.copy_pending(buffer));
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    let detail = String::from_utf8_lossy(&data);
                    let detail = detail.trim();
                    return Err(io::Error::other(if detail.is_empty() {
                        "remote Herdr terminal session wrote an empty stderr record".to_owned()
                    } else {
                        format!("remote Herdr terminal session failed: {detail}")
                    }));
                }
                Some(ChannelMsg::ExitStatus { exit_status }) if exit_status != 0 => {
                    return Err(io::Error::other(format!(
                        "remote Herdr terminal session exited with status {exit_status}"
                    )));
                }
                Some(ChannelMsg::Eof | ChannelMsg::Close) | None => return Ok(0),
                Some(_) => continue,
            }
        }
    }
}

struct RemoteTerminalWriter {
    runtime: Arc<Runtime>,
    writer: Pin<Box<dyn AsyncWrite + Send>>,
}

impl Write for RemoteTerminalWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.runtime.block_on(self.writer.write(buffer))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.runtime.block_on(self.writer.flush())
    }
}

struct RemoteTerminalConnection {
    runtime: Arc<Runtime>,
    session: Option<Handle<KnownHostHandler>>,
    target_id: String,
    pane_id: String,
}

impl RemoteTerminalConnection {
    fn shutdown(mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        if let Err(error) = self.runtime.block_on(session.disconnect(
            Disconnect::ByApplication,
            "remote terminal session complete",
            "en",
        )) {
            crate::diagnostic!(json!({
                "component": "remote_terminal_session",
                "kind": "disconnect.failed",
                "target": self.target_id,
                "pane_id": self.pane_id,
                "message": error.to_string(),
            }));
        }
    }
}

fn remote_terminal_command(
    socket_path: &str,
    pane_id: &str,
    mode: &str,
    rows: u16,
    cols: u16,
) -> RemoteResult<String> {
    if !Path::new(socket_path).is_absolute()
        || socket_path.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(remote_error(
            "remote-terminal-session",
            pane_id,
            RemoteStage::Herdr,
            "remote Herdr socket path must be absolute and single-line",
            false,
            true,
        ));
    }
    if pane_id.trim().is_empty() || pane_id.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(remote_error(
            "remote-terminal-session",
            pane_id,
            RemoteStage::Herdr,
            "remote Herdr pane id must be non-empty and single-line",
            false,
            true,
        ));
    }
    if !matches!(mode, "control" | "observe") {
        return Err(remote_error(
            "remote-terminal-session",
            pane_id,
            RemoteStage::Herdr,
            "remote Herdr terminal mode must be control or observe",
            false,
            true,
        ));
    }
    if rows == 0 || cols == 0 {
        return Err(remote_error(
            "remote-terminal-session",
            pane_id,
            RemoteStage::Herdr,
            "remote Herdr terminal dimensions must be positive",
            false,
            true,
        ));
    }
    Ok(format!(
        "env HERDR_SOCKET_PATH={} PATH=\"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin\" herdr terminal session {} {} --cols {cols} --rows {rows}",
        shell_quote(socket_path),
        shell_quote(mode),
        shell_quote(pane_id),
    ))
}

async fn authenticate(session: &mut Handle<KnownHostHandler>, host: &SshAlias) -> RemoteResult<()> {
    if let Some(identity_file) = host.identity_file.as_ref() {
        let key = load_secret_key(identity_file, None).map_err(|error| {
            remote_error(
                "remote-auth",
                &host.host_id,
                RemoteStage::Auth,
                error,
                false,
                true,
            )
        })?;
        let hash = session
            .best_supported_rsa_hash()
            .await
            .map_err(|error| {
                remote_error(
                    "remote-auth",
                    &host.host_id,
                    RemoteStage::Auth,
                    error,
                    false,
                    true,
                )
            })?
            .flatten();
        let result = session
            .authenticate_publickey(
                host.user.clone(),
                PrivateKeyWithHashAlg::new(Arc::new(key), hash),
            )
            .await
            .map_err(|error| {
                remote_error(
                    "remote-auth",
                    &host.host_id,
                    RemoteStage::Auth,
                    error,
                    true,
                    true,
                )
            })?;
        if result.success() {
            return Ok(());
        }
    }

    let source = host.agent_socket.source();
    let socket = match &host.agent_socket {
        AgentSocket::Disabled => {
            return Err(remote_error(
                "remote-auth",
                &host.host_id,
                RemoteStage::Auth,
                "the ssh config sets IdentityAgent none and no IdentityFile authenticated",
                false,
                true,
            ));
        }
        AgentSocket::Path(socket) => socket.clone().into_os_string(),
        AgentSocket::Environment => read_remote_environment("SSH_AUTH_SOCK")
            .map_err(|error| {
                remote_error(
                    "remote-auth",
                    &host.host_id,
                    RemoteStage::Auth,
                    error,
                    false,
                    true,
                )
            })?
            .ok_or_else(|| {
                remote_error(
                    "remote-auth",
                    &host.host_id,
                    RemoteStage::Auth,
                    "no IdentityFile, no IdentityAgent, and SSH_AUTH_SOCK is not set",
                    false,
                    true,
                )
            })?,
    };
    let mut agent = AgentClient::connect_uds(socket).await.map_err(|error| {
        remote_error(
            "remote-auth",
            &host.host_id,
            RemoteStage::Auth,
            error,
            true,
            true,
        )
    })?;
    let identities = agent.request_identities().await.map_err(|error| {
        remote_error(
            "remote-auth",
            &host.host_id,
            RemoteStage::Auth,
            error,
            true,
            true,
        )
    })?;
    if identities.is_empty() {
        // An agent that holds nothing is a different repair from an agent whose
        // keys the server refused: one is "load a key", the other is "authorize
        // this key".
        return Err(remote_error(
            "remote-auth",
            &host.host_id,
            RemoteStage::Auth,
            format!("the SSH agent reached through {source} holds no identities"),
            false,
            true,
        ));
    }
    let offered = identities.len();
    // Without the server's preferred hash an RSA identity is signed as ssh-rsa
    // (SHA-1), which every current sshd refuses.
    let hash = session
        .best_supported_rsa_hash()
        .await
        .map_err(|error| {
            remote_error(
                "remote-auth",
                &host.host_id,
                RemoteStage::Auth,
                error,
                true,
                true,
            )
        })?
        .flatten();
    for identity in identities {
        let result = session
            .authenticate_publickey_with(
                host.user.clone(),
                identity.public_key().into_owned(),
                hash,
                &mut agent,
            )
            .await
            .map_err(|error| {
                remote_error(
                    "remote-auth",
                    &host.host_id,
                    RemoteStage::Auth,
                    error,
                    true,
                    true,
                )
            })?;
        if result.success() {
            return Ok(());
        }
    }
    Err(remote_error(
        "remote-auth",
        &host.host_id,
        RemoteStage::Auth,
        format!(
            "the SSH agent reached through {source} offered {offered} identities and the server rejected every one"
        ),
        true,
        true,
    ))
}

async fn execute_channel(
    session: &mut Handle<KnownHostHandler>,
    command: &str,
    operation_id: &str,
    target: &str,
    stage: RemoteStage,
) -> RemoteResult<RemoteCommandOutput> {
    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|error| remote_error(operation_id, target, stage, error, true, false))?;
    channel
        .exec(true, command)
        .await
        .map_err(|error| remote_error(operation_id, target, stage, error, true, false))?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_status = None;
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
            ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
            ChannelMsg::ExitStatus {
                exit_status: status,
            } => exit_status = Some(status),
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    Ok(RemoteCommandOutput {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_status: exit_status.ok_or_else(|| {
            remote_error(
                operation_id,
                target,
                stage,
                "remote command closed without an exit status",
                true,
                false,
            )
        })?,
    })
}

async fn open_sftp(
    session: &mut Handle<KnownHostHandler>,
    target: &str,
) -> RemoteResult<SftpSession> {
    let channel = session.channel_open_session().await.map_err(|error| {
        remote_error(
            "remote-sftp-open",
            target,
            RemoteStage::Sftp,
            error,
            true,
            false,
        )
    })?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|error| {
            remote_error(
                "remote-sftp-open",
                target,
                RemoteStage::Sftp,
                error,
                true,
                false,
            )
        })?;
    SftpSession::new(channel.into_stream())
        .await
        .map_err(|error| {
            remote_error(
                "remote-sftp-open",
                target,
                RemoteStage::Sftp,
                error,
                true,
                false,
            )
        })
}

#[derive(Clone, Debug)]
struct KnownHostHandler {
    host: String,
    port: u16,
    known_hosts_file: PathBuf,
    local_forward: Option<SocketAddr>,
    forward_error: Option<Arc<Mutex<Option<String>>>>,
}

impl KnownHostHandler {
    fn new(host: &SshAlias, local_forward: Option<SocketAddr>) -> Self {
        Self {
            host: host.hostname.clone(),
            port: host.port,
            known_hosts_file: host.known_hosts_file.clone(),
            local_forward,
            forward_error: None,
        }
    }

    fn with_forward_error(mut self, forward_error: Arc<Mutex<Option<String>>>) -> Self {
        self.forward_error = Some(forward_error);
        self
    }
}

impl Handler for KnownHostHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let public_key = server_public_key.public_key();
        let trusted =
            check_known_hosts_path(&self.host, self.port, &public_key, &self.known_hosts_file)
                .map_err(|error| anyhow!("known_hosts verification failed: {error}"))?;
        if !trusted {
            return Err(anyhow!(
                "server key is not present in known_hosts for {}:{}",
                self.host,
                self.port
            ));
        }
        Ok(true)
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        _connected_address: &str,
        _connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let Some(local_forward) = self.local_forward else {
            reply
                .reject(ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        };
        reply.accept().await;
        let result = async {
            let mut remote = channel.into_stream();
            let mut local = TcpStream::connect(local_forward).await?;
            tokio::io::copy_bidirectional(&mut remote, &mut local).await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            if let Some(forward_error) = self.forward_error.as_ref()
                && let Ok(mut slot) = forward_error.lock()
            {
                *slot = Some(error.to_string());
            }
            return Err(error);
        }
        Ok(())
    }
}

pub struct RemotePtySession {
    runtime: Arc<Runtime>,
    session: Mutex<Option<Handle<KnownHostHandler>>>,
    channel: Mutex<Option<Channel<Msg>>>,
    endpoint: RemotePtyEndpoint,
}

impl fmt::Debug for RemotePtySession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemotePtySession")
            .field("pane_id", &self.endpoint.pane_id)
            .field("terminal_id", &self.endpoint.terminal_id)
            .finish_non_exhaustive()
    }
}

impl RemotePtySession {
    pub fn endpoint(&self) -> &RemotePtyEndpoint {
        &self.endpoint
    }

    pub fn write(&self, bytes: &[u8]) -> RemoteResult<()> {
        let channel = self
            .channel
            .lock()
            .map_err(|_| {
                remote_error(
                    "remote-pty-write",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "channel lock poisoned",
                    false,
                    true,
                )
            })?
            .take()
            .ok_or_else(|| {
                remote_error(
                    "remote-pty-write",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "PTY is closed",
                    false,
                    false,
                )
            })?;
        let result = self
            .runtime
            .block_on(channel.data_bytes(bytes.to_vec()))
            .map_err(|error| {
                remote_error(
                    "remote-pty-write",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    error,
                    true,
                    false,
                )
            });
        self.channel
            .lock()
            .map_err(|_| {
                remote_error(
                    "remote-pty-write",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "channel lock poisoned",
                    false,
                    true,
                )
            })?
            .replace(channel);
        result
    }

    pub fn read(&self, timeout: Duration) -> RemoteResult<Vec<u8>> {
        let mut channel = self
            .channel
            .lock()
            .map_err(|_| {
                remote_error(
                    "remote-pty-read",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "channel lock poisoned",
                    false,
                    true,
                )
            })?
            .take()
            .ok_or_else(|| {
                remote_error(
                    "remote-pty-read",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "PTY is closed",
                    false,
                    false,
                )
            })?;
        let result = self.runtime.block_on(async {
            match tokio::time::timeout(timeout, channel.wait()).await {
                Ok(Some(ChannelMsg::Data { data })) => Ok(data.to_vec()),
                Ok(Some(ChannelMsg::ExtendedData { data, .. })) => Ok(data.to_vec()),
                Ok(Some(ChannelMsg::ExitStatus { exit_status })) => Err(remote_error(
                    "remote-pty-read",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    format!("remote PTY child exited status={exit_status}"),
                    false,
                    false,
                )),
                Ok(Some(ChannelMsg::Close | ChannelMsg::Eof)) => Err(remote_error(
                    "remote-pty-read",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "remote PTY exited",
                    false,
                    false,
                )),
                Ok(Some(_)) => Ok(Vec::new()),
                Ok(None) => Err(remote_error(
                    "remote-pty-read",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "remote PTY channel ended",
                    false,
                    false,
                )),
                Err(_) => Ok(Vec::new()),
            }
        });
        self.channel
            .lock()
            .map_err(|_| {
                remote_error(
                    "remote-pty-read",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "channel lock poisoned",
                    false,
                    true,
                )
            })?
            .replace(channel);
        result
    }

    pub fn resize(&self, cols: u32, rows: u32) -> RemoteResult<()> {
        if cols == 0 || rows == 0 {
            return Err(remote_error(
                "remote-pty-resize",
                &self.endpoint.pane_id,
                RemoteStage::Pty,
                "PTY dimensions must be non-zero",
                false,
                true,
            ));
        }
        let channel = self
            .channel
            .lock()
            .map_err(|_| {
                remote_error(
                    "remote-pty-resize",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "channel lock poisoned",
                    false,
                    true,
                )
            })?
            .take()
            .ok_or_else(|| {
                remote_error(
                    "remote-pty-resize",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "PTY is closed",
                    false,
                    false,
                )
            })?;
        let result = self
            .runtime
            .block_on(channel.window_change(cols, rows, 0, 0))
            .map_err(|error| {
                remote_error(
                    "remote-pty-resize",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    error,
                    true,
                    false,
                )
            });
        self.channel
            .lock()
            .map_err(|_| {
                remote_error(
                    "remote-pty-resize",
                    &self.endpoint.pane_id,
                    RemoteStage::Pty,
                    "channel lock poisoned",
                    false,
                    true,
                )
            })?
            .replace(channel);
        result
    }

    pub fn close(&self) -> RemoteResult<()> {
        let channel = self
            .channel
            .lock()
            .map_err(|_| {
                remote_error(
                    "remote-pty-close",
                    &self.endpoint.pane_id,
                    RemoteStage::Cleanup,
                    "channel lock poisoned",
                    false,
                    true,
                )
            })?
            .take();
        let mut first_error = None;
        if let Some(channel) = channel
            && let Err(error) = self.runtime.block_on(channel.eof())
        {
            first_error = Some(remote_error(
                "remote-pty-close",
                &self.endpoint.pane_id,
                RemoteStage::Cleanup,
                format!("PTY EOF failed: {error}"),
                true,
                false,
            ));
        }
        let session = self
            .session
            .lock()
            .map_err(|_| {
                remote_error(
                    "remote-pty-close",
                    &self.endpoint.pane_id,
                    RemoteStage::Cleanup,
                    "session lock poisoned",
                    false,
                    true,
                )
            })?
            .take();
        if let Some(session) = session
            && let Err(error) = self.runtime.block_on(session.disconnect(
                Disconnect::ByApplication,
                "PTY closed",
                "en",
            ))
            && first_error.is_none()
        {
            first_error = Some(remote_error(
                "remote-pty-close",
                &self.endpoint.pane_id,
                RemoteStage::Cleanup,
                error,
                true,
                false,
            ));
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReverseBrowserBridgeSpec {
    pub browser_view_id: String,
    pub capability: String,
    pub local_cdp: SocketAddr,
    pub remote_bind: SocketAddr,
}

impl ReverseBrowserBridgeSpec {
    pub fn loopback_probe() -> Self {
        Self {
            browser_view_id: "probe".to_owned(),
            capability: "probe-capability".to_owned(),
            local_cdp: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9222),
            remote_bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        }
    }

    pub fn validate(&self) -> RemoteResult<()> {
        if self.browser_view_id.trim().is_empty()
            || self.capability.trim().is_empty()
            || self
                .browser_view_id
                .bytes()
                .any(|byte| byte.is_ascii_control())
            || self.capability.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(remote_error(
                "remote-browser-tunnel",
                "browser",
                RemoteStage::Tunnel,
                "view ID and capability are required",
                false,
                true,
            ));
        }
        if !self.local_cdp.ip().is_loopback() || !self.remote_bind.ip().is_loopback() {
            return Err(remote_error(
                "remote-browser-tunnel",
                &self.browser_view_id,
                RemoteStage::Tunnel,
                "reverse Browser bridge must stay on loopback",
                false,
                true,
            ));
        }
        if self.local_cdp.port() == 0 {
            return Err(remote_error(
                "remote-browser-tunnel",
                &self.browser_view_id,
                RemoteStage::Tunnel,
                "local CDP endpoint must use a non-zero port",
                false,
                true,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteTunnelState {
    Connecting,
    Ready,
    Stale,
    Closed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteTunnelDescriptor {
    pub tunnel_id: String,
    pub host_id: String,
    pub browser_view_id: String,
    pub local_cdp: SocketAddr,
    pub remote_bind: SocketAddr,
    pub state: RemoteTunnelState,
    pub owned: bool,
}

pub struct RemoteTunnelHandle {
    runtime: Arc<Runtime>,
    session: Mutex<Option<Handle<KnownHostHandler>>>,
    descriptor: RemoteTunnelDescriptor,
    forward_error: Arc<Mutex<Option<String>>>,
    closed: AtomicBool,
}

impl fmt::Debug for RemoteTunnelHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteTunnelHandle")
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

impl RemoteTunnelHandle {
    pub fn descriptor(&self) -> &RemoteTunnelDescriptor {
        &self.descriptor
    }

    pub fn descriptor_snapshot(&self) -> RemoteTunnelDescriptor {
        let mut descriptor = self.descriptor.clone();
        descriptor.state = self.state();
        descriptor
    }

    pub fn state(&self) -> RemoteTunnelState {
        if self.closed.load(Ordering::Acquire) {
            return RemoteTunnelState::Closed;
        }
        match self.forward_error.lock() {
            Ok(error) if error.is_some() => RemoteTunnelState::Failed,
            Ok(_) => self.descriptor.state,
            Err(_) => RemoteTunnelState::Failed,
        }
    }

    pub fn forward_error(&self) -> RemoteResult<Option<String>> {
        self.forward_error
            .lock()
            .map(|error| error.clone())
            .map_err(|_| {
                remote_error(
                    "remote-tunnel-state",
                    &self.descriptor.tunnel_id,
                    RemoteStage::Tunnel,
                    "forward error lock poisoned",
                    false,
                    true,
                )
            })
    }

    pub fn close(&self) -> RemoteResult<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let session = self
            .session
            .lock()
            .map_err(|_| {
                remote_error(
                    "remote-tunnel-close",
                    &self.descriptor.tunnel_id,
                    RemoteStage::Cleanup,
                    "session lock poisoned",
                    false,
                    true,
                )
            })?
            .take();
        if let Some(session) = session {
            let cancel = self.runtime.block_on(session.cancel_tcpip_forward(
                self.descriptor.remote_bind.ip().to_string(),
                u32::from(self.descriptor.remote_bind.port()),
            ));
            let disconnect = self.runtime.block_on(session.disconnect(
                Disconnect::ByApplication,
                "owned tunnel closed",
                "en",
            ));
            if let Err(error) = cancel {
                return Err(remote_error(
                    "remote-tunnel-close",
                    &self.descriptor.tunnel_id,
                    RemoteStage::Cleanup,
                    format!("cancel forward failed: {error}"),
                    true,
                    false,
                ));
            }
            if let Err(error) = disconnect {
                return Err(remote_error(
                    "remote-tunnel-close",
                    &self.descriptor.tunnel_id,
                    RemoteStage::Cleanup,
                    error,
                    true,
                    false,
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OwnedTunnelLease {
    pub descriptor: RemoteTunnelDescriptor,
    pub owner_id: String,
}

#[derive(Default)]
pub struct RemoteTunnelRegistry {
    leases: BTreeMap<String, OwnedTunnelLease>,
}

impl RemoteTunnelRegistry {
    pub fn claim(
        &mut self,
        spec: &ReverseBrowserBridgeSpec,
        host_id: &str,
        owner_id: &str,
    ) -> RemoteResult<OwnedTunnelLease> {
        spec.validate()?;
        if host_id.trim().is_empty() || owner_id.trim().is_empty() {
            return Err(remote_error(
                "remote-tunnel-claim",
                host_id,
                RemoteStage::Tunnel,
                "host and owner identities are required",
                false,
                true,
            ));
        }
        let tunnel_id = format!("tunnel:{host_id}:{}", spec.browser_view_id);
        if let Some(existing) = self.leases.get(&tunnel_id) {
            if existing.owner_id == owner_id
                && existing.descriptor.local_cdp == spec.local_cdp
                && existing.descriptor.remote_bind == spec.remote_bind
            {
                return Ok(existing.clone());
            }
            return Err(remote_error(
                "remote-tunnel-claim",
                &tunnel_id,
                RemoteStage::Tunnel,
                "tunnel identity is already owned by another operation",
                false,
                true,
            ));
        }
        let lease = OwnedTunnelLease {
            descriptor: RemoteTunnelDescriptor {
                tunnel_id: tunnel_id.clone(),
                host_id: host_id.to_owned(),
                browser_view_id: spec.browser_view_id.clone(),
                local_cdp: spec.local_cdp,
                remote_bind: spec.remote_bind,
                state: RemoteTunnelState::Connecting,
                owned: true,
            },
            owner_id: owner_id.to_owned(),
        };
        self.leases.insert(tunnel_id, lease.clone());
        Ok(lease)
    }

    pub fn mark_ready(&mut self, tunnel_id: &str, owner_id: &str) -> RemoteResult<()> {
        let lease = self.leases.get_mut(tunnel_id).ok_or_else(|| {
            remote_error(
                "remote-tunnel-state",
                tunnel_id,
                RemoteStage::Tunnel,
                "unknown tunnel",
                false,
                true,
            )
        })?;
        if lease.owner_id != owner_id {
            return Err(remote_error(
                "remote-tunnel-state",
                tunnel_id,
                RemoteStage::Tunnel,
                "owner mismatch",
                false,
                true,
            ));
        }
        if matches!(
            lease.descriptor.state,
            RemoteTunnelState::Closed | RemoteTunnelState::Failed
        ) {
            return Err(remote_error(
                "remote-tunnel-state",
                tunnel_id,
                RemoteStage::Tunnel,
                "closed or failed tunnel cannot become ready",
                false,
                true,
            ));
        }
        lease.descriptor.state = RemoteTunnelState::Ready;
        Ok(())
    }

    pub fn mark_stale(&mut self, tunnel_id: &str, owner_id: &str) -> RemoteResult<()> {
        let lease = self.leases.get_mut(tunnel_id).ok_or_else(|| {
            remote_error(
                "remote-tunnel-state",
                tunnel_id,
                RemoteStage::Tunnel,
                "unknown tunnel",
                false,
                true,
            )
        })?;
        if lease.owner_id != owner_id {
            return Err(remote_error(
                "remote-tunnel-state",
                tunnel_id,
                RemoteStage::Tunnel,
                "owner mismatch",
                false,
                true,
            ));
        }
        if lease.descriptor.state == RemoteTunnelState::Closed {
            return Err(remote_error(
                "remote-tunnel-state",
                tunnel_id,
                RemoteStage::Tunnel,
                "closed tunnel cannot become stale",
                false,
                true,
            ));
        }
        lease.descriptor.state = RemoteTunnelState::Stale;
        Ok(())
    }

    pub fn release(&mut self, tunnel_id: &str, owner_id: &str) -> RemoteResult<OwnedTunnelLease> {
        let lease = self.leases.get(tunnel_id).ok_or_else(|| {
            remote_error(
                "remote-tunnel-cleanup",
                tunnel_id,
                RemoteStage::Cleanup,
                "unknown tunnel",
                false,
                true,
            )
        })?;
        if lease.owner_id != owner_id {
            return Err(remote_error(
                "remote-tunnel-cleanup",
                tunnel_id,
                RemoteStage::Cleanup,
                "owner mismatch; refusing to release another operation's tunnel",
                false,
                true,
            ));
        }
        let Some(mut lease) = self.leases.remove(tunnel_id) else {
            return Err(remote_error(
                "remote-tunnel-cleanup",
                tunnel_id,
                RemoteStage::Cleanup,
                "tunnel disappeared before release",
                true,
                false,
            ));
        };
        lease.descriptor.state = RemoteTunnelState::Closed;
        Ok(lease)
    }

    pub fn len(&self) -> usize {
        self.leases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.leases.is_empty()
    }
}

#[derive(Clone)]
pub struct RusshSftpTransport {
    client: Arc<RusshRemoteClient>,
}

impl RusshSftpTransport {
    pub fn new(client: Arc<RusshRemoteClient>) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &Arc<RusshRemoteClient> {
        &self.client
    }

    fn map_error(&self, operation: &str, error: RemoteError) -> FileServiceError {
        FileServiceError::Remote {
            operation: operation.to_owned(),
            target: self.client.host().host_id.clone(),
            reason: error.to_string(),
        }
    }
}

impl SftpTransport for RusshSftpTransport {
    fn list(&self, path: &str) -> FileResult<Vec<FileEntry>> {
        self.client
            .sftp_list(path)
            .map_err(|error| self.map_error("list", error))
    }

    fn read(&self, path: &str) -> FileResult<Vec<u8>> {
        self.client
            .sftp_read(path)
            .map_err(|error| self.map_error("read", error))
    }

    fn write(&self, path: &str, bytes: &[u8]) -> FileResult<()> {
        self.client
            .sftp_write(path, bytes)
            .map_err(|error| self.map_error("write", error))
    }

    fn git_status(&self, root: &str) -> FileResult<String> {
        let output = self
            .client
            .exec_read_only(RemoteReadCommand::GitStatus {
                root: root.to_owned(),
            })
            .map_err(|error| self.map_error("git status", error))?;
        if output.exit_status != 0 && output.exit_status != 128 {
            return Err(FileServiceError::Remote {
                operation: "git status".to_owned(),
                target: self.client.host().host_id.clone(),
                reason: format!(
                    "exit={} stderr={}",
                    output.exit_status,
                    redact_output(&output.stderr)
                ),
            });
        }
        Ok(output.stdout)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteConnectionDescriptor {
    pub host: RemoteHostIdentity,
    pub state: RemoteConnectionState,
    pub last_operation_id: Option<String>,
}

#[derive(Default)]
pub struct RemoteConnectionRegistry {
    connections: BTreeMap<String, RemoteConnectionDescriptor>,
}

impl RemoteConnectionRegistry {
    pub fn ensure(&mut self, host: &SshAlias, operation_id: &str) -> RemoteConnectionDescriptor {
        if let Some(existing) = self.connections.get_mut(&host.host_id) {
            let identity = host.identity();
            if existing.host != identity {
                existing.host = identity;
                existing.state = RemoteConnectionState::Connecting;
            }
            if matches!(
                existing.state,
                RemoteConnectionState::Failed { .. }
                    | RemoteConnectionState::ActionRequired { .. }
                    | RemoteConnectionState::Stale { .. }
            ) {
                existing.state = RemoteConnectionState::Connecting;
            }
            existing.last_operation_id = Some(operation_id.to_owned());
            return existing.clone();
        }
        let descriptor = RemoteConnectionDescriptor {
            host: host.identity(),
            state: RemoteConnectionState::Connecting,
            last_operation_id: Some(operation_id.to_owned()),
        };
        self.connections
            .insert(host.host_id.clone(), descriptor.clone());
        descriptor
    }

    pub fn mark_connected(&mut self, host_id: &str, operation_id: &str) -> RemoteResult<()> {
        let connection = self.connections.get_mut(host_id).ok_or_else(|| {
            remote_error(
                "remote-connect",
                host_id,
                RemoteStage::Ssh,
                "unknown host",
                false,
                true,
            )
        })?;
        connection.state = RemoteConnectionState::Connected;
        connection.last_operation_id = Some(operation_id.to_owned());
        Ok(())
    }

    pub fn mark_reconnecting(&mut self, host_id: &str, attempt: u32) -> RemoteResult<()> {
        let connection = self.connections.get_mut(host_id).ok_or_else(|| {
            remote_error(
                "remote-reconnect",
                host_id,
                RemoteStage::Reconnect,
                "unknown host",
                false,
                true,
            )
        })?;
        connection.state = RemoteConnectionState::Reconnecting { attempt };
        Ok(())
    }

    pub fn mark_stale(&mut self, host_id: &str, reason: impl Into<String>) -> RemoteResult<()> {
        let connection = self.connections.get_mut(host_id).ok_or_else(|| {
            remote_error(
                "remote-reconnect",
                host_id,
                RemoteStage::Reconnect,
                "unknown host",
                false,
                true,
            )
        })?;
        connection.state = RemoteConnectionState::Stale {
            reason: reason.into(),
        };
        Ok(())
    }

    pub fn mark_failed(&mut self, host_id: &str, reason: impl Into<String>) -> RemoteResult<()> {
        let connection = self.connections.get_mut(host_id).ok_or_else(|| {
            remote_error(
                "remote-reconnect",
                host_id,
                RemoteStage::Reconnect,
                "unknown host",
                false,
                true,
            )
        })?;
        connection.state = RemoteConnectionState::Failed {
            reason: reason.into(),
        };
        Ok(())
    }

    pub fn get(&self, host_id: &str) -> Option<&RemoteConnectionDescriptor> {
        self.connections.get(host_id)
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn redact_output(value: &str) -> String {
    format!(
        "redacted_output bytes={} lines={}",
        value.len(),
        value.lines().count()
    )
}

fn combine_cleanup<T>(
    operation_id: &str,
    target: &str,
    result: RemoteResult<T>,
    cleanup: RemoteResult<()>,
) -> RemoteResult<T> {
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(primary), Err(cleanup)) => Err(remote_error(
            operation_id,
            target,
            primary.stage(),
            format!(
                "{}; cleanup failed: {}",
                primary.diagnostic().reason,
                cleanup.diagnostic().reason
            ),
            primary.diagnostic().retryable,
            primary.diagnostic().action_required,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        AgentPhase, AgentProjection, DomainEventKind, LayoutNode, PaneProjection, TabProjection,
        WorkspaceProjection,
    };

    fn host() -> SshAlias {
        SshAlias::from_config_contents(
            "mini",
            "Host mini\n  HostName mini.example.test\n  User example\n  Port 2200\n  IdentityFile ~/.ssh/id_ed25519\n",
            "/tmp/known_hosts",
        )
        .unwrap()
    }

    #[test]
    fn remote_terminal_command_uses_official_structured_session_contract() {
        let command = remote_terminal_command(
            "/Users/example/.config/herdr/herdr.sock",
            "w1:pane with ' quote",
            "control",
            42,
            120,
        )
        .expect("valid terminal command");
        assert_eq!(
            command,
            "env HERDR_SOCKET_PATH='/Users/example/.config/herdr/herdr.sock' PATH=\"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin\" herdr terminal session 'control' 'w1:pane with '\\'' quote' --cols 120 --rows 42"
        );
        assert!(!command.contains("pane attach"));
        assert!(!command.contains("ssh "));
    }

    #[test]
    fn remote_terminal_command_rejects_untrusted_contract_values() {
        assert!(remote_terminal_command("relative.sock", "w1:p1", "control", 24, 80).is_err());
        assert!(
            remote_terminal_command("/tmp/herdr.sock", "w1:p1\nwhoami", "control", 24, 80).is_err()
        );
        assert!(remote_terminal_command("/tmp/herdr.sock", "w1:p1", "takeover", 24, 80).is_err());
        assert!(remote_terminal_command("/tmp/herdr.sock", "w1:p1", "observe", 0, 80).is_err());
    }

    #[test]
    #[ignore = "requires HERDR_TEST_SSH_ALIAS and HERDR_TEST_SOCKET_PATH"]
    fn official_remote_socket_snapshot_probe() {
        let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS")
            .expect("HERDR_TEST_SSH_ALIAS names a configured SSH host");
        let socket_path = std::env::var("HERDR_TEST_SOCKET_PATH")
            .expect("HERDR_TEST_SOCKET_PATH is the absolute remote Unix socket path");
        let home = std::env::var_os("HOME").expect("HOME is configured");
        let alias =
            SshAlias::from_config_file(&PathBuf::from(home).join(".ssh/config"), &alias_name)
                .expect("SSH alias resolves");
        let client = RusshRemoteClient::new(alias).expect("remote client initializes");
        let connector = client
            .herdr_api_connector(&socket_path)
            .expect("remote Socket API connector initializes");
        let result = crate::herdr_api::request_with_connector(
            &connector,
            "session.snapshot",
            json!({}),
            SSH_OPERATION_TIMEOUT,
        )
        .expect("official remote Socket API snapshot responds");
        let snapshot = decode_remote_snapshot(&result, "remote-herdr-snapshot")
            .expect("remote snapshot matches the official protocol");
        let agents = crate::herdr_api::request_with_connector(
            &connector,
            "agent.list",
            json!({}),
            SSH_OPERATION_TIMEOUT,
        )
        .expect("a second channel reuses the authenticated SSH connection");
        assert_eq!(agents["type"], "agent_list");

        let subscription = crate::herdr_api::subscribe_with_connector(
            &connector,
            snapshot.event_sequence,
            &["pane.updated"],
            SSH_OPERATION_TIMEOUT,
        )
        .expect("official remote event subscription starts from the snapshot cursor");
        assert_eq!(subscription.ack.host.host_id, snapshot.host.host_id);
        assert_eq!(subscription.ack.host.session_id, snapshot.host.session_id);
        let (reader, shutdown) = subscription.into_parts();
        shutdown.shutdown();
        drop(reader);

        assert_eq!(snapshot.protocol, REMOTE_PROTOCOL_REVISION);
        assert!(!snapshot.host.host_id.is_empty());
        assert!(!snapshot.host.session_id.is_empty());
    }

    #[test]
    #[ignore = "requires an owned remote fixture and HERDR_TEST_REMOTE_TERMINAL_* variables"]
    fn official_remote_terminal_session_fixture_probe() {
        use std::io::{BufRead, BufReader};
        use std::sync::mpsc::channel;

        let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS")
            .expect("HERDR_TEST_SSH_ALIAS names a configured SSH host");
        let socket_path = std::env::var("HERDR_TEST_SOCKET_PATH")
            .expect("HERDR_TEST_SOCKET_PATH is the absolute remote Unix socket path");
        let workspace_id = std::env::var("HERDR_TEST_REMOTE_TERMINAL_WORKSPACE_ID")
            .expect("HERDR_TEST_REMOTE_TERMINAL_WORKSPACE_ID names the owned fixture workspace");
        let pane_id = std::env::var("HERDR_TEST_REMOTE_TERMINAL_PANE_ID")
            .expect("HERDR_TEST_REMOTE_TERMINAL_PANE_ID names the owned fixture pane");
        let cwd = std::env::var("HERDR_TEST_REMOTE_TERMINAL_CWD")
            .expect("HERDR_TEST_REMOTE_TERMINAL_CWD names the owned fixture directory");
        assert!(
            cwd.starts_with("/tmp/herdr-ide-verify-"),
            "remote terminal fixture must use the owned fixture namespace"
        );

        let home = std::env::var_os("HOME").expect("HOME is configured");
        let alias =
            SshAlias::from_config_file(&PathBuf::from(home).join(".ssh/config"), &alias_name)
                .expect("SSH alias resolves");
        let client = RusshRemoteClient::new(alias).expect("remote client initializes");
        let snapshot = client
            .fetch_herdr_snapshot_value(&socket_path)
            .expect("fixture session snapshot");
        let snapshot = snapshot["snapshot"]
            .as_object()
            .map(|_| &snapshot["snapshot"])
            .expect("session.snapshot response contains a snapshot");
        let owned_workspace = snapshot["workspaces"]
            .as_array()
            .and_then(|workspaces| {
                workspaces.iter().find(|workspace| {
                    workspace["workspace_id"].as_str() == Some(workspace_id.as_str())
                })
            })
            .expect("owned fixture workspace is present");
        assert!(
            owned_workspace["label"]
                .as_str()
                .is_some_and(|label| label.starts_with("herdr-ide-verify-")),
            "remote terminal refused a workspace outside the owned fixture namespace"
        );
        assert!(snapshot["panes"].as_array().is_some_and(|panes| {
            let canonical_cwd = cwd
                .strip_prefix("/tmp/")
                .map(|suffix| format!("/private/tmp/{suffix}"));
            panes.iter().any(|pane| {
                pane["pane_id"].as_str() == Some(pane_id.as_str())
                    && pane["workspace_id"].as_str() == Some(workspace_id.as_str())
                    && (pane["cwd"].as_str() == Some(cwd.as_str())
                        || pane["cwd"].as_str() == canonical_cwd.as_deref())
            })
        }));

        let process = client
            .open_terminal_session(&socket_path, &pane_id, "control", 30, 100)
            .expect("official remote terminal control session opens");
        let (reader, writer, shutdown) = process.into_parts();
        let mut writer = writer.expect("control session exposes a writer");
        let (progress_sender, progress_receiver) = channel();
        let (history_sender, history_receiver) = channel();
        let (closed_sender, closed_receiver) = channel();
        let tail_marker = "HERDR_IDE_SCROLL_080";
        let history_marker = "HERDR_IDE_SCROLL_001";
        let scroll_requested = Arc::new(AtomicBool::new(false));
        let reader_scroll_requested = Arc::clone(&scroll_requested);
        let reader_thread = std::thread::spawn(move || {
            let mut tail_marker_seen = false;
            let mut history_marker_seen_after_scroll = false;
            let mut resized_frame_seen = false;
            for line in BufReader::new(reader).lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(error) => {
                        let _ = closed_sender.send(Err(error.to_string()));
                        return;
                    }
                };
                match crate::live::parse_terminal_session_line(&line) {
                    Ok(crate::live::TerminalSessionEvent::Frame {
                        width,
                        height,
                        bytes,
                        ..
                    }) => {
                        resized_frame_seen |= width == 100 && height == 30;
                        let frame = String::from_utf8_lossy(&bytes);
                        tail_marker_seen |= frame.contains(tail_marker);
                        history_marker_seen_after_scroll |= reader_scroll_requested
                            .load(Ordering::Acquire)
                            && frame.contains(history_marker);
                        if tail_marker_seen && resized_frame_seen {
                            let _ = progress_sender.send(());
                        }
                        if history_marker_seen_after_scroll {
                            let _ = history_sender.send(());
                        }
                    }
                    Ok(crate::live::TerminalSessionEvent::Closed { .. }) => {
                        let _ = closed_sender.send(Ok((
                            tail_marker_seen,
                            resized_frame_seen,
                            history_marker_seen_after_scroll,
                        )));
                        return;
                    }
                    Err(error) => {
                        let _ = closed_sender.send(Err(error));
                        return;
                    }
                }
            }
            let _ = closed_sender.send(Ok((
                tail_marker_seen,
                resized_frame_seen,
                history_marker_seen_after_scroll,
            )));
        });

        writer
            .write_all(
                crate::live::terminal_resize_line(30, 100)
                    .unwrap()
                    .as_bytes(),
            )
            .expect("resize request writes");
        writer
            .write_all(
                crate::live::terminal_input_line(
                    b"for i in {1..80}; do printf 'HERDR_IDE_SCROLL_%03d\\n' $i; done\r",
                )
                .unwrap()
                .as_bytes(),
            )
            .expect("terminal input writes");
        writer.flush().expect("structured requests flush");
        progress_receiver
            .recv_timeout(Duration::from_secs(15))
            .expect("remote terminal frame reports the tail marker and resized grid");
        scroll_requested.store(true, Ordering::Release);
        writer
            .write_all(
                b"{\"type\":\"terminal.scroll\",\"direction\":\"up\",\"lines\":1000,\"source\":\"wheel\"}\n",
            )
            .expect("scroll repaint request writes");
        writer.flush().expect("scroll repaint requests flush");
        history_receiver
            .recv_timeout(Duration::from_secs(15))
            .expect("remote terminal scroll returns a frame from Herdr-owned history");
        writer
            .write_all(crate::live::terminal_release_line().as_bytes())
            .expect("release request writes");
        writer.flush().expect("release request flushes");
        drop(writer);
        let observed = closed_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("remote terminal stream closes after release")
            .expect("remote terminal reader remains valid");
        shutdown();
        reader_thread.join().expect("reader thread joins");
        assert_eq!(observed, (true, true, true));
    }

    #[test]
    #[ignore = "requires an owned remote fixture and HERDR_TEST_REMOTE_FILES_* variables"]
    fn remote_sftp_fixture_probe() {
        let alias_name = std::env::var("HERDR_TEST_REMOTE_FILES_SSH_ALIAS")
            .expect("HERDR_TEST_REMOTE_FILES_SSH_ALIAS");
        let root =
            std::env::var("HERDR_TEST_REMOTE_FILES_ROOT").expect("HERDR_TEST_REMOTE_FILES_ROOT");
        assert!(
            root.starts_with("/private/tmp/herdr-ide-verify-remote-files-")
                || root.starts_with("/tmp/herdr-ide-verify-remote-files-"),
            "remote SFTP probe refused a root outside the owned fixture namespace"
        );
        let home = std::env::var_os("HOME").expect("HOME");
        let alias =
            SshAlias::from_config_file(&PathBuf::from(home).join(".ssh/config"), &alias_name)
                .expect("fixture alias");
        let client = Arc::new(RusshRemoteClient::new(alias).expect("remote client"));
        let service = RemoteFileService::new(root.clone(), RusshSftpTransport::new(client))
            .expect("remote file service");

        let entries = service.list("").expect("SFTP directory list");
        let marker = entries
            .iter()
            .find(|entry| entry.name == "marker.txt")
            .expect("fixture marker");
        assert_eq!(marker.kind, FileKind::File);
        assert_eq!(marker.path, format!("{root}/marker.txt"));
        assert_eq!(
            service.open("marker.txt").expect("SFTP read").content,
            "ok\n"
        );
    }

    #[test]
    fn alias_import_ignores_wildcard_and_negated_entries() {
        let aliases = import_ssh_aliases_from_str(
            "Host *\n  User default\nHost mini staging\n  HostName 127.0.0.1\nHost !staging\n  User nobody\nHost *.internal\n  HostName ignored\n",
            "/tmp/known_hosts",
        )
        .unwrap();
        assert_eq!(
            aliases
                .iter()
                .map(|alias| alias.alias.as_str())
                .collect::<Vec<_>>(),
            ["mini", "staging"]
        );
        assert_eq!(aliases[0].host_id, "ssh:mini");
        assert_eq!(aliases[0].port, 22);
    }

    #[test]
    fn alias_import_preserves_paths_but_never_reads_key_contents() {
        let alias = SshAlias::from_config_contents(
            "mini",
            "Host mini\n  HostName mini.example.test\n  User example\n  Port 2200\n  IdentityFile /private/tmp/hide-remote-home/.ssh/id_ed25519\n",
            "/tmp/known_hosts",
        )
        .unwrap();
        assert_eq!(
            alias.identity_file,
            Some(PathBuf::from(
                "/private/tmp/hide-remote-home/.ssh/id_ed25519",
            ))
        );
        let encoded = serde_json::to_string(&alias).unwrap();
        assert!(!encoded.contains("PRIVATE KEY"));
    }

    #[test]
    fn wildcard_identity_agent_reaches_the_alias_it_covers() {
        let alias = SshAlias::from_config_contents(
            "mini",
            "Host *\n  IdentityAgent /private/tmp/hide-remote-home/agent.sock\n\nHost mini\n  HostName mini.example.test\n  User example\n",
            "/tmp/known_hosts",
        )
        .unwrap();
        assert_eq!(
            alias.agent_socket,
            AgentSocket::Path(PathBuf::from("/private/tmp/hide-remote-home/agent.sock"))
        );
    }

    #[test]
    fn the_first_matching_identity_agent_wins() {
        let alias = SshAlias::from_config_contents(
            "mini",
            "Host mini\n  HostName mini.example.test\n  User example\n  IdentityAgent /private/tmp/hide-remote-home/first.sock\n\nHost *\n  IdentityAgent /private/tmp/hide-remote-home/second.sock\n",
            "/tmp/known_hosts",
        )
        .unwrap();
        assert_eq!(
            alias.agent_socket,
            AgentSocket::Path(PathBuf::from("/private/tmp/hide-remote-home/first.sock"))
        );
    }

    #[test]
    fn an_identity_agent_for_another_host_does_not_reach_this_alias() {
        let alias = SshAlias::from_config_contents(
            "mini",
            "Host github.com\n  IdentityAgent /private/tmp/hide-remote-home/github.sock\n\nHost mini\n  HostName mini.example.test\n  User example\n",
            "/tmp/known_hosts",
        )
        .unwrap();
        assert_eq!(alias.agent_socket, AgentSocket::Environment);
    }

    #[test]
    fn identity_agent_none_and_the_environment_spelling_are_distinct() {
        let disabled = SshAlias::from_config_contents(
            "mini",
            "Host mini\n  HostName mini.example.test\n  User example\n  IdentityAgent none\n",
            "/tmp/known_hosts",
        )
        .unwrap();
        assert_eq!(disabled.agent_socket, AgentSocket::Disabled);
        let environment = SshAlias::from_config_contents(
            "mini",
            "Host mini\n  HostName mini.example.test\n  User example\n  IdentityAgent SSH_AUTH_SOCK\n",
            "/tmp/known_hosts",
        )
        .unwrap();
        assert_eq!(environment.agent_socket, AgentSocket::Environment);
    }

    #[test]
    fn an_identity_agent_this_shell_cannot_resolve_fails_visibly() {
        let error = SshAlias::from_config_contents(
            "mini",
            "Host mini\n  HostName mini.example.test\n  User example\n  IdentityAgent $HIDE_AGENT_SOCK\n",
            "/tmp/known_hosts",
        )
        .unwrap_err();
        assert_eq!(error.stage(), RemoteStage::Alias);
        let diagnostic = error.to_string();
        assert!(diagnostic.contains("action_required=true"), "{diagnostic}");
        assert!(
            diagnostic.contains("outside the declared remote environment contract"),
            "{diagnostic}"
        );
    }

    #[test]
    fn a_negated_host_pattern_keeps_its_identity_agent_away() {
        let alias = SshAlias::from_config_contents(
            "mini",
            "Host * !mini\n  IdentityAgent /private/tmp/hide-remote-home/agent.sock\n\nHost mini\n  HostName mini.example.test\n  User example\n",
            "/tmp/known_hosts",
        )
        .unwrap();
        assert_eq!(alias.agent_socket, AgentSocket::Environment);
    }

    #[test]
    fn capability_report_has_staged_failure_and_visible_recovery() {
        let mut report = CapabilityReport::new("op-1", host().identity());
        report.fail(RemoteStage::Tunnel, "forward denied", true, true);
        assert!(matches!(
            report.connection,
            RemoteConnectionState::ActionRequired { .. }
        ));
        assert!(matches!(
            report.stage(RemoteStage::Tunnel).unwrap().state,
            CapabilityState::Failed {
                retryable: true,
                action_required: true
            }
        ));
        report.pass(RemoteStage::Tunnel, "retried after approval");
        report.connected();
        assert_eq!(report.connection, RemoteConnectionState::Connected);
    }

    #[test]
    fn capability_report_requires_every_stage_before_connected() {
        let mut report = CapabilityReport::new("op-1", host().identity());
        for stage in [
            RemoteStage::Ssh,
            RemoteStage::Auth,
            RemoteStage::Herdr,
            RemoteStage::Protocol,
            RemoteStage::Pty,
            RemoteStage::Sftp,
            RemoteStage::Git,
        ] {
            report.pass(stage, "ok");
        }
        assert!(!report.all_required_passed());
        report.pass(RemoteStage::Tunnel, "ok");
        assert!(report.all_required_passed());
    }

    #[test]
    fn remote_wire_snapshot_is_typed_and_protocol_checked() {
        let value = serde_json::json!({
            "result": {"snapshot": {
                "protocol": REMOTE_PROTOCOL_REVISION,
                "version": "fixture", "tabs": [], "layouts": [], "lineage": [],
                "host": {"host_id": "ssh:mini", "session_id": "s1"},
                "event_sequence": 9,
                "workspaces": [{"workspace_id": "w1", "number": 1, "label": "fixture", "focused": false, "pane_count": 1, "tab_count": 1, "active_tab_id": "w1:t1", "agent_status": "idle"}],
                "panes": [{"pane_id": "p1", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}],
                "agents": [
                    {"agent_instance_id": "a1", "pane_id": "p1", "terminal_id": "fixture", "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1},
                    {"agent_instance_id": null, "pane_id": "p1", "terminal_id": "fixture", "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1},
                    {"pane_id": "p2", "terminal_id": "fixture2", "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}
                ]
            }}
        });
        let envelope = decode_remote_snapshot(&value, "op").unwrap();
        assert_eq!(envelope.host.host_id, "ssh:mini");
        assert_eq!(envelope.workspace_ids, ["w1"]);
        assert_eq!(envelope.pane_ids, ["p1"]);
        assert_eq!(envelope.agent_ids, ["a1"]);
    }

    #[test]
    fn remote_wire_snapshot_rejects_protocol_mismatch() {
        let value = serde_json::json!({"snapshot": {"protocol": REMOTE_PROTOCOL_REVISION - 1, "host": {"host_id": "h", "session_id": "s"}, "event_sequence": 1}});
        let error = decode_remote_snapshot(&value, "op").unwrap_err();
        assert_eq!(error.stage(), RemoteStage::Protocol);
        assert!(error.diagnostic().action_required);
    }

    #[test]
    fn remote_wire_snapshot_rejects_duplicate_ids_and_protocol_wraparound() {
        let duplicate = serde_json::json!({
            "snapshot": {
                "protocol": REMOTE_PROTOCOL_REVISION,
                "version": "fixture", "tabs": [], "layouts": [], "lineage": [],
                "host": {"host_id": "h", "session_id": "s"},
                "event_sequence": 1,
                "workspaces": [], "agents": [],
                "panes": [{"pane_id": "p1", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}, {"pane_id": "p1", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}]
            }
        });
        let error = decode_remote_snapshot(&duplicate, "op").unwrap_err();
        assert!(error.diagnostic().reason.contains("duplicate pane_id"));

        let wrapped = serde_json::json!({
            "snapshot": {
                "protocol": u64::from(u32::MAX) + 22,
                "host": {"host_id": "h", "session_id": "s"},
                "event_sequence": 1
            }
        });
        assert_eq!(
            decode_remote_snapshot(&wrapped, "op").unwrap_err().stage(),
            RemoteStage::Protocol
        );
    }

    #[test]
    fn remote_projection_marks_gap_stale_and_host_scope_is_preserved() {
        let alias = host();
        let scope = HostScope {
            host_id: alias.host_id.clone(),
            session_id: "s1".to_owned(),
        };
        let mut projection = RemoteHerdrProjection::new(alias.identity(), "s1");
        projection
            .apply_snapshot(sample_snapshot(scope.clone()))
            .unwrap();
        let error = projection
            .apply_event(DomainEvent {
                sequence: 3,
                kind: DomainEventKind::WorkspaceRenamed {
                    workspace_id: "w1".to_owned(),
                    name: "new".to_owned(),
                },
            })
            .unwrap_err();
        assert_eq!(error.stage(), RemoteStage::Protocol);
        assert!(matches!(
            projection.state(),
            RemoteConnectionState::Stale { .. }
        ));
        projection.apply_snapshot(sample_snapshot(scope)).unwrap();
        assert_eq!(projection.state(), &RemoteConnectionState::Connected);
    }

    #[test]
    fn remote_projection_preserves_domain_and_records_disconnect_reason() {
        let alias = host();
        let scope = HostScope {
            host_id: alias.host_id.clone(),
            session_id: "s1".to_owned(),
        };
        let mut projection = RemoteHerdrProjection::new(alias.identity(), "s1");
        projection.apply_snapshot(sample_snapshot(scope)).unwrap();
        projection.disconnected("transport reset");
        assert_eq!(projection.disconnect_reason(), Some("transport reset"));
        assert_eq!(projection.domain().sequence(), 1);
        assert!(matches!(
            projection.state(),
            RemoteConnectionState::Reconnecting { attempt: 0 }
        ));
    }

    #[test]
    fn sparse_remote_wire_snapshot_requires_typed_bootstrap_for_events() {
        let alias = host();
        let mut projection = RemoteHerdrProjection::new(alias.identity(), "s1");
        let value = serde_json::json!({
            "snapshot": {
                "protocol": REMOTE_PROTOCOL_REVISION,
                "version": "fixture", "tabs": [], "layouts": [], "lineage": [],
                "host": {"host_id": "ssh:mini", "session_id": "s1"},
                "event_sequence": 9,
                "workspaces": [{"workspace_id": "w1", "number": 1, "label": "fixture", "focused": false, "pane_count": 1, "tab_count": 1, "active_tab_id": "w1:t1", "agent_status": "idle"}],
                "panes": [{"pane_id": "p1", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}],
                "agents": [{"agent_instance_id": "a1", "pane_id": "p1", "terminal_id": "fixture", "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}]
            }
        });
        projection.apply_wire_snapshot(&value, "op").unwrap();
        let error = projection
            .apply_event(DomainEvent {
                sequence: 10,
                kind: DomainEventKind::WorkspaceRenamed {
                    workspace_id: "w1".to_owned(),
                    name: "next".to_owned(),
                },
            })
            .unwrap_err();
        assert!(error.diagnostic().reason.contains("typed domain snapshot"));
        assert!(matches!(
            projection.state(),
            RemoteConnectionState::Stale { .. }
        ));
    }

    #[test]
    fn remote_wire_snapshot_same_identity_is_idempotent() {
        let alias = host();
        let mut projection = RemoteHerdrProjection::new(alias.identity(), "s1");
        let value = serde_json::json!({
            "snapshot": {
                "protocol": REMOTE_PROTOCOL_REVISION,
                "version": "fixture", "tabs": [], "layouts": [], "lineage": [],
                "host": {"host_id": "ssh:mini", "session_id": "s1"},
                "event_sequence": 9,
                "workspaces": [{"workspace_id": "w1", "number": 1, "label": "fixture", "focused": false, "pane_count": 1, "tab_count": 1, "active_tab_id": "w1:t1", "agent_status": "idle"}],
                "panes": [{"pane_id": "p1", "surface": {"kind": "terminal", "attach": {"host": {"host_id": "fixture", "session_id": "s1"}, "transport": "herdr_client", "protocol": 21, "terminal_id": "fixture"}}, "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}],
                "agents": [{"agent_instance_id": "a1", "pane_id": "p1", "terminal_id": "fixture", "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1}]
            }
        });
        projection.apply_wire_snapshot(&value, "op-1").unwrap();
        projection.apply_wire_snapshot(&value, "op-2").unwrap();
        assert_eq!(projection.last_snapshot().unwrap().event_sequence, 9);
        assert_eq!(projection.state(), &RemoteConnectionState::Connected);
    }

    #[test]
    fn malformed_wire_snapshot_marks_projection_stale() {
        let alias = host();
        let mut projection = RemoteHerdrProjection::new(alias.identity(), "s1");
        let error = projection
            .apply_wire_snapshot(
                &serde_json::json!({"snapshot": {"protocol": REMOTE_PROTOCOL_REVISION}}),
                "op",
            )
            .unwrap_err();
        assert!(matches!(
            projection.state(),
            RemoteConnectionState::Stale { .. }
        ));
        assert_eq!(error.stage(), RemoteStage::Herdr);
    }

    #[test]
    fn remote_projection_rejects_foreign_workspace() {
        let alias = host();
        let foreign = HostScope {
            host_id: "ssh:other".to_owned(),
            session_id: "s1".to_owned(),
        };
        let mut projection = RemoteHerdrProjection::new(alias.identity(), "s1");
        let error = projection
            .apply_snapshot(sample_snapshot(foreign))
            .unwrap_err();
        assert_eq!(error.stage(), RemoteStage::Protocol);
        assert!(matches!(
            projection.state(),
            RemoteConnectionState::Failed { .. }
        ));
    }

    #[test]
    fn terminal_route_rejects_editor_and_accepts_typed_endpoint() {
        let endpoint = RemotePtyEndpoint {
            host: HostScope {
                host_id: "ssh:mini".to_owned(),
                session_id: "s1".to_owned(),
            },
            pane_id: "p1".to_owned(),
            agent_instance_id: Some("a1".to_owned()),
            terminal_id: "t1".to_owned(),
            protocol: REMOTE_PROTOCOL_REVISION,
            cols: 120,
            rows: 40,
            term: DEFAULT_REMOTE_TERM.to_owned(),
        };
        assert!(require_terminal_surface(&RemoteSurface::Terminal(endpoint.clone())).is_ok());
        assert!(
            require_terminal_surface(&RemoteSurface::Editor {
                editor_id: "e1".to_owned()
            })
            .is_err()
        );
        let mut invalid = endpoint;
        invalid.host.host_id.clear();
        assert!(require_terminal_surface(&RemoteSurface::Terminal(invalid)).is_err());
    }

    #[test]
    fn browser_bridge_requires_loopback_and_capability() {
        let mut spec = ReverseBrowserBridgeSpec::loopback_probe();
        assert!(spec.validate().is_ok());
        spec.remote_bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 0);
        assert!(spec.validate().is_err());
        spec.remote_bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        spec.capability.clear();
        assert!(spec.validate().is_err());
    }

    #[test]
    fn tunnel_registry_is_idempotent_and_owner_scoped() {
        let mut registry = RemoteTunnelRegistry::default();
        let spec = ReverseBrowserBridgeSpec::loopback_probe();
        let first = registry.claim(&spec, "ssh:mini", "op-1").unwrap();
        let second = registry.claim(&spec, "ssh:mini", "op-1").unwrap();
        assert_eq!(first, second);
        assert!(registry.claim(&spec, "ssh:mini", "op-2").is_err());
        registry
            .mark_ready(&first.descriptor.tunnel_id, "op-1")
            .unwrap();
        registry
            .mark_stale(&first.descriptor.tunnel_id, "op-1")
            .unwrap();
        assert!(
            registry
                .release(&first.descriptor.tunnel_id, "op-2")
                .is_err()
        );
        let released = registry
            .release(&first.descriptor.tunnel_id, "op-1")
            .unwrap();
        assert_eq!(released.descriptor.state, RemoteTunnelState::Closed);
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }

    #[test]
    fn connection_registry_converges_on_repeated_connect_and_surfaces_stale() {
        let mut registry = RemoteConnectionRegistry::default();
        let first = registry.ensure(&host(), "op-1");
        let second = registry.ensure(&host(), "op-2");
        assert_eq!(first.host, second.host);
        assert_eq!(second.last_operation_id.as_deref(), Some("op-2"));
        assert_eq!(second.state, RemoteConnectionState::Connecting);
        registry.mark_connected("ssh:mini", "op-2").unwrap();
        registry.mark_reconnecting("ssh:mini", 1).unwrap();
        registry.mark_stale("ssh:mini", "tunnel expired").unwrap();
        assert!(matches!(
            registry.get("ssh:mini").unwrap().state,
            RemoteConnectionState::Stale { .. }
        ));
    }

    #[test]
    fn connection_registry_does_not_reuse_changed_alias_target() {
        let mut registry = RemoteConnectionRegistry::default();
        let first = registry.ensure(&host(), "op-1");
        let changed = SshAlias::from_config_contents(
            "mini",
            "Host mini\n  HostName another.example.test\n  User example\n  Port 2201\n",
            "/tmp/known_hosts",
        )
        .unwrap();
        let second = registry.ensure(&changed, "op-2");
        assert_ne!(first.host, second.host);
        assert_eq!(second.host.hostname, "another.example.test");
        assert_eq!(second.host.port, 2201);
        assert_eq!(second.state, RemoteConnectionState::Connecting);
    }

    #[test]
    fn read_only_git_command_quotes_absolute_root() {
        let command = RemoteReadCommand::GitStatus {
            root: "/tmp/a b".to_owned(),
        }
        .command_line()
        .unwrap();
        assert_eq!(command, "git -C '/tmp/a b' status --short --porcelain=v1");
        assert!(
            RemoteReadCommand::GitStatus {
                root: "relative".to_owned()
            }
            .command_line()
            .is_err()
        );
        assert!(
            RemoteReadCommand::GitStatus {
                root: "/tmp/bad\0path".to_owned()
            }
            .command_line()
            .is_err()
        );
    }

    fn sample_snapshot(host: HostScope) -> DomainSnapshot {
        DomainSnapshot {
            protocol_revision: REMOTE_PROTOCOL_REVISION,
            sequence: 1,
            active_workspace_id: "w1".to_owned(),
            workspaces: vec![WorkspaceProjection {
                host,
                workspace_id: "w1".to_owned(),
                name: "Workspace".to_owned(),
                remote: true,
                active_tab_id: "tab1".to_owned(),
                tabs: vec![TabProjection {
                    tab_id: "tab1".to_owned(),
                    name: "Tab".to_owned(),
                    focused_pane_id: "p1".to_owned(),
                    panes: vec![PaneProjection {
                        pane_id: "p1".to_owned(),
                        title: "Terminal".to_owned(),
                        surface: crate::domain::SurfaceKind::Terminal,
                        agent_instance_id: Some("a1".to_owned()),
                    }],
                    layout: LayoutNode::Pane {
                        pane_id: "p1".to_owned(),
                    },
                }],
                worktree: None,
            }],
            agents: vec![AgentProjection {
                agent_instance_id: "a1".to_owned(),
                parent_agent_instance_id: None,
                host: HostScope {
                    host_id: "ssh:mini".to_owned(),
                    session_id: "s1".to_owned(),
                },
                workspace_id: "w1".to_owned(),
                tab_id: "tab1".to_owned(),
                pane_id: "p1".to_owned(),
                name: "Agent".to_owned(),
                kind: "codex".to_owned(),
                phase: AgentPhase::Working,
                summary: None,
                elapsed_seconds: 1,
            }],
        }
    }
}
