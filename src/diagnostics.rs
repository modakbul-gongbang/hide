//! Cross-process diagnostics and exact operation ownership.
//!
//! The native shell has several independent transports, but failures must be
//! rendered through one bounded, redacted record instead of being left in a
//! process-local stderr stream.  This module deliberately has no logging or
//! subprocess dependency.  Callers supply the operation identity and this
//! module owns the stable schema, redaction boundary, recovery state, and
//! exact cleanup manifest.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::domain::ConnectionState;

pub const DIAGNOSTIC_SCHEMA: &str = "herdr.ide.diagnostic.v1";
pub const OPERATION_MANIFEST_SCHEMA: &str = "herdr.ide.operation-manifest.v1";
pub const DEFAULT_DIAGNOSTIC_LIMIT: usize = 128;
const MAX_REASON_CHARS: usize = 512;
const MAX_CONTEXT_CHARS: usize = 160;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStage {
    App,
    Herdr,
    Pty,
    Cef,
    Cdp,
    Ssh,
    Sftp,
    File,
    Keychain,
    Openrouter,
    Shortcut,
    Browser,
    Reconnect,
    Bundle,
    Unknown,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticContext {
    pub host_id: Option<String>,
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
    pub pane_id: Option<String>,
    pub agent_instance_id: Option<String>,
    pub view_id: Option<String>,
}

impl DiagnosticContext {
    pub fn for_target(target: &str) -> Self {
        Self {
            pane_id: Some(sanitize_identity(target)),
            ..Self::default()
        }
    }

    pub fn sanitized(mut self) -> Self {
        for value in [
            &mut self.host_id,
            &mut self.workspace_id,
            &mut self.tab_id,
            &mut self.pane_id,
            &mut self.agent_instance_id,
            &mut self.view_id,
        ] {
            if let Some(value) = value.as_mut() {
                *value = sanitize_identity(value);
            }
        }
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StructuredDiagnostic {
    pub schema: String,
    pub timestamp_ms: u64,
    pub operation_id: String,
    pub stage: DiagnosticStage,
    pub target: String,
    pub reason: String,
    pub retryable: bool,
    pub action_required: bool,
    pub context: DiagnosticContext,
}

impl StructuredDiagnostic {
    pub fn new(
        operation_id: impl Into<String>,
        stage: DiagnosticStage,
        target: impl Into<String>,
        reason: impl AsRef<str>,
        retryable: bool,
        action_required: bool,
    ) -> Self {
        let target = sanitize_identity(&target.into());
        Self {
            schema: DIAGNOSTIC_SCHEMA.to_owned(),
            timestamp_ms: unix_millis(),
            operation_id: sanitize_identity(&operation_id.into()),
            stage,
            target: target.clone(),
            reason: redact_text(reason.as_ref()),
            retryable,
            action_required,
            context: DiagnosticContext::for_target(&target),
        }
    }

    pub fn with_context(mut self, context: DiagnosticContext) -> Self {
        self.context = context.sanitized();
        self
    }

    pub fn recovery_action(&self) -> RecoveryAction {
        if self.action_required {
            RecoveryAction::UserAction
        } else if self.retryable {
            RecoveryAction::Retry
        } else {
            RecoveryAction::Inspect
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Retry,
    Reconnect,
    UserAction,
    Inspect,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RecoveryState {
    Connected,
    Reconnecting { target: String, attempt: u32 },
    Stale { expected: u64, received: u64 },
    Failed { reason: String },
    ActionRequired { reason: String },
}

impl RecoveryState {
    pub fn from_connection(connection: &ConnectionState) -> Self {
        match connection {
            ConnectionState::Connected => Self::Connected,
            ConnectionState::Reconnecting { target } => Self::Reconnecting {
                target: sanitize_identity(target),
                attempt: 0,
            },
            ConnectionState::Stale { expected, received } => Self::Stale {
                expected: *expected,
                received: *received,
            },
            ConnectionState::Failed { reason } => Self::Failed {
                reason: redact_text(reason),
            },
            ConnectionState::ActionRequired { reason } => Self::ActionRequired {
                reason: redact_text(reason),
            },
        }
    }

    pub fn action(&self) -> RecoveryAction {
        match self {
            Self::Connected => RecoveryAction::Inspect,
            Self::Reconnecting { .. } => RecoveryAction::Reconnect,
            Self::Stale { .. } => RecoveryAction::Reconnect,
            Self::Failed { .. } => RecoveryAction::Inspect,
            Self::ActionRequired { .. } => RecoveryAction::UserAction,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecoverySurface {
    pub state: RecoveryState,
    pub title: String,
    pub detail: String,
    pub action: RecoveryAction,
    pub operation_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DiagnosticsStore {
    entries: VecDeque<StructuredDiagnostic>,
    limit: usize,
}

impl Default for DiagnosticsStore {
    fn default() -> Self {
        Self::new(DEFAULT_DIAGNOSTIC_LIMIT)
    }
}

impl DiagnosticsStore {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(limit.max(1)),
            limit: limit.max(1),
        }
    }

    pub fn record(&mut self, diagnostic: StructuredDiagnostic) {
        if self.entries.len() == self.limit {
            self.entries.pop_front();
        }
        self.entries.push_back(diagnostic);
    }

    pub fn latest(&self) -> Option<&StructuredDiagnostic> {
        self.entries.back()
    }

    pub fn entries(&self) -> impl Iterator<Item = &StructuredDiagnostic> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn recovery_surface(&self, connection: &ConnectionState) -> RecoverySurface {
        let state = RecoveryState::from_connection(connection);
        let latest = self.latest();
        let (title, detail, action, operation_id) = match (&state, latest) {
            (RecoveryState::Connected, Some(diagnostic)) if diagnostic.action_required => (
                "Action required".to_owned(),
                diagnostic.reason.clone(),
                diagnostic.recovery_action(),
                Some(diagnostic.operation_id.clone()),
            ),
            (RecoveryState::Connected, Some(diagnostic)) => (
                format!("{} failure", stage_label(diagnostic.stage)),
                diagnostic.reason.clone(),
                diagnostic.recovery_action(),
                Some(diagnostic.operation_id.clone()),
            ),
            (RecoveryState::Connected, None) => (
                "Connected".to_owned(),
                "Herdr state is current".to_owned(),
                RecoveryAction::Inspect,
                None,
            ),
            (RecoveryState::Reconnecting { target, attempt }, _) => (
                "Reconnecting".to_owned(),
                format!("target={target} attempt={attempt}"),
                RecoveryAction::Reconnect,
                latest.map(|item| item.operation_id.clone()),
            ),
            (RecoveryState::Stale { expected, received }, _) => (
                "Resync required".to_owned(),
                format!("event sequence gap expected={expected} received={received}"),
                RecoveryAction::Reconnect,
                latest.map(|item| item.operation_id.clone()),
            ),
            (RecoveryState::Failed { reason }, _) => (
                "Connection failed".to_owned(),
                reason.clone(),
                RecoveryAction::Inspect,
                latest.map(|item| item.operation_id.clone()),
            ),
            (RecoveryState::ActionRequired { reason }, _) => (
                "Action required".to_owned(),
                reason.clone(),
                RecoveryAction::UserAction,
                latest.map(|item| item.operation_id.clone()),
            ),
        };
        RecoverySurface {
            state,
            title,
            detail: redact_text(&detail),
            action,
            operation_id,
        }
    }

    pub fn jsonl(&self) -> Result<String, DiagnosticsError> {
        self.entries
            .iter()
            .map(|entry| serde_json::to_string(entry).map_err(DiagnosticsError::Serialize))
            .collect::<Result<Vec<_>, _>>()
            .map(|lines| {
                if lines.is_empty() {
                    String::new()
                } else {
                    format!("{}\n", lines.join("\n"))
                }
            })
    }

    pub fn write_jsonl_atomic(&self, path: &Path) -> Result<(), DiagnosticsError> {
        let bytes = self.jsonl()?.into_bytes();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| DiagnosticsError::Io {
                operation: "mkdir",
                path: parent.display().to_string(),
                reason: error.to_string(),
            })?;
        }
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        std::fs::write(&temporary, bytes).map_err(|error| DiagnosticsError::Io {
            operation: "write",
            path: temporary.display().to_string(),
            reason: error.to_string(),
        })?;
        std::fs::rename(&temporary, path).map_err(|error| DiagnosticsError::Io {
            operation: "publish",
            path: path.display().to_string(),
            reason: error.to_string(),
        })
    }
}

#[derive(Debug)]
pub enum DiagnosticsError {
    Serialize(serde_json::Error),
    Io {
        operation: &'static str,
        path: String,
        reason: String,
    },
}

impl fmt::Display for DiagnosticsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(error) => write!(formatter, "diagnostic serialization failed: {error}"),
            Self::Io {
                operation,
                path,
                reason,
            } => write!(
                formatter,
                "diagnostic {operation} failed for {path:?}: {reason}"
            ),
        }
    }
}

impl std::error::Error for DiagnosticsError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OwnedResource {
    pub kind: String,
    pub identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationManifest {
    pub schema: String,
    pub operation_id: String,
    pub owner_id: String,
    pub resources: BTreeMap<String, OwnedResource>,
}

impl OperationManifest {
    pub fn new(operation_id: impl Into<String>, owner_id: impl Into<String>) -> Self {
        Self {
            schema: OPERATION_MANIFEST_SCHEMA.to_owned(),
            operation_id: sanitize_identity(&operation_id.into()),
            owner_id: sanitize_identity(&owner_id.into()),
            resources: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, kind: &str, identity: &str) -> Result<(), ManifestError> {
        let identity = sanitize_identity(identity);
        let kind = sanitize_identity(kind);
        if identity.is_empty() || kind.is_empty() {
            return Err(ManifestError::InvalidIdentity);
        }
        if let Some(existing) = self.resources.get(&identity) {
            if existing.kind == kind {
                return Ok(());
            }
            return Err(ManifestError::IdentityConflict {
                identity,
                existing_kind: existing.kind.clone(),
                requested_kind: kind,
            });
        }
        self.resources
            .insert(identity.clone(), OwnedResource { kind, identity });
        Ok(())
    }

    pub fn is_owned(&self, kind: &str, identity: &str) -> bool {
        self.resources
            .get(identity)
            .is_some_and(|resource| resource.kind == kind && resource.identity == identity)
    }

    pub fn release(&mut self, kind: &str, identity: &str) -> Result<bool, ManifestError> {
        let Some(resource) = self.resources.get(identity) else {
            return Ok(false);
        };
        if resource.kind != kind {
            return Err(ManifestError::OwnerMismatch {
                identity: identity.to_owned(),
                expected_kind: resource.kind.clone(),
                actual_kind: kind.to_owned(),
            });
        }
        self.resources.remove(identity);
        Ok(true)
    }

    pub fn cleanup_plan(&self) -> Vec<OwnedResource> {
        self.resources.values().cloned().collect()
    }

    pub fn to_json(&self) -> Result<String, DiagnosticsError> {
        serde_json::to_string_pretty(self).map_err(DiagnosticsError::Serialize)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ManifestError {
    InvalidIdentity,
    IdentityConflict {
        identity: String,
        existing_kind: String,
        requested_kind: String,
    },
    OwnerMismatch {
        identity: String,
        expected_kind: String,
        actual_kind: String,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity => write!(formatter, "operation manifest identity is empty"),
            Self::IdentityConflict {
                identity,
                existing_kind,
                requested_kind,
            } => write!(
                formatter,
                "operation manifest identity {identity:?} already has kind {existing_kind:?}, requested {requested_kind:?}"
            ),
            Self::OwnerMismatch {
                identity,
                expected_kind,
                actual_kind,
            } => write!(
                formatter,
                "operation manifest identity {identity:?} requires kind {expected_kind:?}, got {actual_kind:?}"
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

pub fn redact_text(value: &str) -> String {
    let home = std::env::var_os("HOME")
        .map(|home| home.to_string_lossy().into_owned())
        .filter(|home| !home.is_empty());
    let mut value = value.replace('\n', " ").replace('\r', " ");
    if let Some(home) = home {
        value = value.replace(&home, "$HOME");
    }
    for key in [
        "api_key",
        "apikey",
        "authorization",
        "cap",
        "capability",
        "cookie",
        "x-herdr-capability",
        "password",
        "prompt",
        "secret",
        "token",
        "transcript",
        "user_keystroke",
    ] {
        value = redact_assignment(&value, key);
    }
    value
        .chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .take(MAX_REASON_CHARS)
        .collect()
}

fn redact_assignment(value: &str, key: &str) -> String {
    let mut output = value.to_owned();
    let mut search_from = 0;
    loop {
        let lower = output.to_ascii_lowercase();
        if search_from >= lower.len() {
            break;
        }
        let Some(relative) = lower[search_from..].find(key) else {
            break;
        };
        let start = search_from + relative;
        let before_ok = start == 0
            || !lower
                .as_bytes()
                .get(start.saturating_sub(1))
                .is_some_and(u8::is_ascii_alphanumeric);
        if !before_ok {
            search_from = start + key.len();
            continue;
        }
        let mut cursor = start + key.len();
        while output
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        if !matches!(output.as_bytes().get(cursor), Some(b'=') | Some(b':')) {
            search_from = cursor;
            continue;
        }
        cursor += 1;
        while output
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        let quote = matches!(output.as_bytes().get(cursor), Some(b'"') | Some(b'\''));
        if quote {
            cursor += 1;
        }
        let value_start = cursor;
        while let Some(byte) = output.as_bytes().get(cursor) {
            let delimiter = if quote {
                *byte == b'"' || *byte == b'\''
            } else {
                byte.is_ascii_whitespace() || matches!(*byte, b',' | b'}' | b']' | b';')
            };
            if delimiter {
                break;
            }
            cursor += 1;
        }
        if cursor == value_start {
            search_from = cursor.saturating_add(1);
            continue;
        }
        output.replace_range(value_start..cursor, "[REDACTED]");
        search_from = value_start + "[REDACTED]".len();
    }
    output
}

fn sanitize_identity(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .chars()
        .take(MAX_CONTEXT_CHARS)
        .collect()
}

fn stage_label(stage: DiagnosticStage) -> &'static str {
    match stage {
        DiagnosticStage::App => "App",
        DiagnosticStage::Herdr => "Herdr",
        DiagnosticStage::Pty => "PTY",
        DiagnosticStage::Cef => "CEF",
        DiagnosticStage::Cdp => "CDP",
        DiagnosticStage::Ssh => "SSH",
        DiagnosticStage::Sftp => "SFTP",
        DiagnosticStage::File => "File",
        DiagnosticStage::Keychain => "Keychain",
        DiagnosticStage::Openrouter => "OpenRouter",
        DiagnosticStage::Shortcut => "Shortcut",
        DiagnosticStage::Browser => "Browser",
        DiagnosticStage::Reconnect => "Reconnect",
        DiagnosticStage::Bundle => "Bundle",
        DiagnosticStage::Unknown => "Operation",
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ConnectionState;

    #[test]
    fn diagnostics_are_bounded_and_redacted_before_serialization() {
        let mut store = DiagnosticsStore::new(2);
        store.record(StructuredDiagnostic::new(
            "op-1",
            DiagnosticStage::Ssh,
            "mini",
            "token=secret-value transcript=private prompt=hidden",
            true,
            false,
        ));
        store.record(StructuredDiagnostic::new(
            "op-2",
            DiagnosticStage::Herdr,
            "session",
            "sequence gap",
            true,
            false,
        ));
        store.record(StructuredDiagnostic::new(
            "op-3",
            DiagnosticStage::Sftp,
            "remote-file",
            "permission denied",
            false,
            true,
        ));
        assert_eq!(store.len(), 2);
        let jsonl = store.jsonl().unwrap();
        assert!(!jsonl.contains("secret-value"));
        assert!(!jsonl.contains("private"));
        assert!(jsonl.contains("action_required"));
    }

    #[test]
    fn recovery_surface_exposes_stale_and_action_required_states() {
        let store = DiagnosticsStore::default();
        let stale = store.recovery_surface(&ConnectionState::Stale {
            expected: 8,
            received: 10,
        });
        assert_eq!(stale.title, "Resync required");
        assert_eq!(stale.action, RecoveryAction::Reconnect);
        let required = store.recovery_surface(&ConnectionState::ActionRequired {
            reason: "host key confirmation".to_owned(),
        });
        assert_eq!(required.action, RecoveryAction::UserAction);
    }

    #[test]
    fn operation_manifest_is_exact_idempotent_and_rejects_kind_conflicts() {
        let mut manifest = OperationManifest::new("op-1", "owner-1");
        manifest.register("socket", "sock-1").unwrap();
        manifest.register("socket", "sock-1").unwrap();
        assert!(manifest.is_owned("socket", "sock-1"));
        assert!(matches!(
            manifest.register("pid", "sock-1"),
            Err(ManifestError::IdentityConflict { .. })
        ));
        assert_eq!(manifest.cleanup_plan().len(), 1);
        assert!(manifest.release("socket", "sock-1").unwrap());
        assert!(manifest.cleanup_plan().is_empty());
    }

    #[test]
    fn redact_assignment_handles_json_and_key_value_forms() {
        let text = redact_text(
            r#"token: "abc", api_key=xyz authorization: Bearer value cap=secret x-herdr-capability: secret2"#,
        );
        assert!(!text.contains("abc"));
        assert!(!text.contains("xyz"));
        assert!(!text.contains("Bearer"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("secret2"));
        assert!(text.contains("[REDACTED]"));
    }
}
