use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[cfg(target_vendor = "apple")]
use security_framework::passwords::{
    PasswordOptions, delete_generic_password, generic_password, set_generic_password,
};

const DEFAULT_MAX_CONTEXT_CHARS: usize = 4_000;
const DEFAULT_DEBOUNCE_MS: u64 = 2_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeychainError {
    CommandFailed {
        operation: &'static str,
        code: Option<i32>,
    },
    Io {
        operation: &'static str,
        message: String,
    },
    InvalidOutput {
        operation: &'static str,
    },
}

impl std::fmt::Display for KeychainError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandFailed { operation, code } => {
                write!(
                    formatter,
                    "keychain operation={operation} failed code={code:?}"
                )
            }
            Self::Io { operation, message } => {
                write!(
                    formatter,
                    "keychain operation={operation} I/O failed: {message}"
                )
            }
            Self::InvalidOutput { operation } => {
                write!(
                    formatter,
                    "keychain operation={operation} returned invalid output"
                )
            }
        }
    }
}

impl std::error::Error for KeychainError {}

pub trait KeychainStore {
    fn set(&mut self, value: &str) -> Result<(), KeychainError>;
    fn get(&self) -> Result<Option<String>, KeychainError>;
    fn delete(&mut self) -> Result<(), KeychainError>;

    fn has_key(&self) -> Result<bool, KeychainError> {
        self.get().map(|value| value.is_some())
    }
}

/// The production store uses the macOS Security Framework without placing the
/// secret in settings, snapshots, diagnostics, or log messages.
#[derive(Clone, Debug)]
pub struct MacKeychainStore {
    service: String,
    account: String,
}

impl MacKeychainStore {
    pub fn new(
        service: impl Into<String>,
        account: impl Into<String>,
    ) -> Result<Self, KeychainError> {
        let service = service.into();
        let account = account.into();
        if service.is_empty() || account.is_empty() {
            return Err(KeychainError::InvalidOutput {
                operation: "configure",
            });
        }
        Ok(Self { service, account })
    }
}

impl KeychainStore for MacKeychainStore {
    fn set(&mut self, value: &str) -> Result<(), KeychainError> {
        #[cfg(target_vendor = "apple")]
        {
            return set_generic_password(
                self.service.as_str(),
                self.account.as_str(),
                value.as_bytes(),
            )
            .map_err(|error| security_error("set", error));
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = value;
            Err(KeychainError::Io {
                operation: "set",
                message: "macOS Security Framework is unavailable on this target".to_owned(),
            })
        }
    }

    fn get(&self) -> Result<Option<String>, KeychainError> {
        #[cfg(target_vendor = "apple")]
        {
            return match generic_password(PasswordOptions::new_generic_password(
                self.service.as_str(),
                self.account.as_str(),
            )) {
                Ok(value) => String::from_utf8(value)
                    .map(|value| (!value.is_empty()).then_some(value))
                    .map_err(|_| KeychainError::InvalidOutput { operation: "get" }),
                Err(error) if is_missing_item(error) => Ok(None),
                Err(error) => Err(security_error("get", error)),
            };
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            Err(KeychainError::Io {
                operation: "get",
                message: "macOS Security Framework is unavailable on this target".to_owned(),
            })
        }
    }

    fn delete(&mut self) -> Result<(), KeychainError> {
        #[cfg(target_vendor = "apple")]
        {
            return match delete_generic_password(self.service.as_str(), self.account.as_str()) {
                Ok(()) => Ok(()),
                Err(error) if is_missing_item(error) => Ok(()),
                Err(error) => Err(security_error("delete", error)),
            };
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            Err(KeychainError::Io {
                operation: "delete",
                message: "macOS Security Framework is unavailable on this target".to_owned(),
            })
        }
    }
}

#[cfg(target_vendor = "apple")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(target_vendor = "apple")]
fn is_missing_item(error: security_framework::base::Error) -> bool {
    error.code() == ERR_SEC_ITEM_NOT_FOUND
}

#[cfg(target_vendor = "apple")]
fn security_error(
    operation: &'static str,
    error: security_framework::base::Error,
) -> KeychainError {
    KeychainError::CommandFailed {
        operation,
        code: Some(error.code()),
    }
}

#[derive(Clone, Debug, Default)]
pub struct MemoryKeychainStore {
    values: BTreeMap<String, String>,
    slot: String,
}

impl MemoryKeychainStore {
    pub fn new(slot: impl Into<String>) -> Self {
        Self {
            values: BTreeMap::new(),
            slot: slot.into(),
        }
    }
}

impl KeychainStore for MemoryKeychainStore {
    fn set(&mut self, value: &str) -> Result<(), KeychainError> {
        self.values.insert(self.slot.clone(), value.to_owned());
        Ok(())
    }

    fn get(&self) -> Result<Option<String>, KeychainError> {
        Ok(self.values.get(&self.slot).cloned())
    }

    fn delete(&mut self) -> Result<(), KeychainError> {
        self.values.remove(&self.slot);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterSettings {
    pub enabled: bool,
    pub model: String,
    pub max_context_chars: usize,
    pub debounce_ms: u64,
}

impl Default for OpenRouterSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "openai/gpt-4o-mini".to_owned(),
            max_context_chars: DEFAULT_MAX_CONTEXT_CHARS,
            debounce_ms: DEFAULT_DEBOUNCE_MS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SummaryError {
    OptInRequired,
    MissingApiKey,
    ContextTooLarge { chars: usize, max: usize },
    RateLimited { retry_after_ms: u64 },
    Sink(String),
    Keychain(KeychainError),
}

impl std::fmt::Display for SummaryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OptInRequired => formatter.write_str("summary opt-in is required"),
            Self::MissingApiKey => {
                formatter.write_str("OpenRouter API key is missing from Keychain")
            }
            Self::ContextTooLarge { chars, max } => write!(
                formatter,
                "sanitized context exceeds {max} characters ({chars})"
            ),
            Self::RateLimited { retry_after_ms } => write!(
                formatter,
                "summary debounce active retry_after_ms={retry_after_ms}"
            ),
            Self::Sink(message) => write!(formatter, "summary sink failed: {message}"),
            Self::Keychain(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SummaryError {}

impl From<KeychainError> for SummaryError {
    fn from(error: KeychainError) -> Self {
        Self::Keychain(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SummaryConsentPreview {
    pub sanitized_context: String,
    pub chars: usize,
    pub max_chars: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SummaryRequest {
    pub model: String,
    pub sanitized_context: String,
    pub keychain_present: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SummaryMetadata {
    pub source: String,
    pub model: String,
    pub context_chars: usize,
    pub context_digest: String,
    pub created_at_ms: u64,
}

pub trait SummarySink {
    fn write(&mut self, metadata: &SummaryMetadata) -> Result<(), String>;
}

#[derive(Clone, Debug, Default)]
pub struct MemorySummarySink {
    latest: Option<SummaryMetadata>,
    writes: usize,
}

impl MemorySummarySink {
    pub fn latest(&self) -> Option<&SummaryMetadata> {
        self.latest.as_ref()
    }

    pub fn writes(&self) -> usize {
        self.writes
    }
}

impl SummarySink for MemorySummarySink {
    fn write(&mut self, metadata: &SummaryMetadata) -> Result<(), String> {
        self.latest = Some(metadata.clone());
        self.writes += 1;
        Ok(())
    }
}

pub struct SummaryWriter<S> {
    keychain: S,
    settings: OpenRouterSettings,
    last_published_at_ms: Option<u64>,
}

impl<S: KeychainStore> SummaryWriter<S> {
    pub fn new(keychain: S, settings: OpenRouterSettings) -> Self {
        Self {
            keychain,
            settings,
            last_published_at_ms: None,
        }
    }

    pub fn settings(&self) -> &OpenRouterSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut OpenRouterSettings {
        &mut self.settings
    }

    pub fn keychain(&self) -> &S {
        &self.keychain
    }

    pub fn keychain_mut(&mut self) -> &mut S {
        &mut self.keychain
    }

    pub fn consent_preview(
        &self,
        recent_context: &str,
    ) -> Result<SummaryConsentPreview, SummaryError> {
        let sanitized_context = sanitize_context(recent_context);
        let chars = sanitized_context.chars().count();
        if chars > self.settings.max_context_chars {
            return Err(SummaryError::ContextTooLarge {
                chars,
                max: self.settings.max_context_chars,
            });
        }
        Ok(SummaryConsentPreview {
            sanitized_context,
            chars,
            max_chars: self.settings.max_context_chars,
        })
    }

    pub fn prepare_request(&self, recent_context: &str) -> Result<SummaryRequest, SummaryError> {
        if !self.settings.enabled {
            return Err(SummaryError::OptInRequired);
        }
        let preview = self.consent_preview(recent_context)?;
        if !self.keychain.has_key()? {
            return Err(SummaryError::MissingApiKey);
        }
        Ok(SummaryRequest {
            model: self.settings.model.clone(),
            sanitized_context: preview.sanitized_context,
            keychain_present: true,
        })
    }

    pub fn publish<Sink: SummarySink>(
        &mut self,
        source: &str,
        recent_context: &str,
        now_ms: u64,
        sink: &mut Sink,
    ) -> Result<SummaryMetadata, SummaryError> {
        let request = self.prepare_request(recent_context)?;
        if let Some(previous) = self.last_published_at_ms {
            let elapsed = now_ms.saturating_sub(previous);
            let debounce = self.settings.debounce_ms;
            if elapsed < debounce {
                return Err(SummaryError::RateLimited {
                    retry_after_ms: debounce - elapsed,
                });
            }
        }
        let metadata = SummaryMetadata {
            source: source.to_owned(),
            model: request.model,
            context_chars: request.sanitized_context.chars().count(),
            context_digest: digest(&request.sanitized_context),
            created_at_ms: now_ms,
        };
        sink.write(&metadata).map_err(SummaryError::Sink)?;
        self.last_published_at_ms = Some(now_ms);
        Ok(metadata)
    }
}

pub fn sanitize_context(value: &str) -> String {
    let mut result = String::with_capacity(value.len().min(DEFAULT_MAX_CONTEXT_CHARS));
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            if chars.peek() == Some(&']') {
                let _ = chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                        let _ = chars.next();
                        break;
                    }
                }
            }
            continue;
        }
        if character.is_control() && character != '\n' && character != '\t' {
            continue;
        }
        result.push(character);
    }
    result
}

fn digest(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64-{hash:016x}")
}

pub fn debounce_duration(settings: &OpenRouterSettings) -> Duration {
    Duration::from_millis(settings.debounce_ms)
}

#[cfg(test)]
mod tests {
    use super::{
        KeychainStore, MemoryKeychainStore, MemorySummarySink, OpenRouterSettings, SummaryError,
        SummaryWriter, sanitize_context,
    };

    #[test]
    fn consent_preview_removes_osc_and_control_bytes_without_touching_normal_text() {
        let raw = "hello\u{1b}]8;;https://secret.example\u{7}link\u{1b}]8;;\u{7}\nnext\u{1b}[31m";
        assert_eq!(sanitize_context(raw), "hellolink\nnext[31m");
    }

    #[test]
    fn summary_requires_explicit_opt_in_and_keychain_presence() {
        let store = MemoryKeychainStore::new("openrouter");
        let mut writer = SummaryWriter::new(store, OpenRouterSettings::default());
        assert_eq!(
            writer.prepare_request("context"),
            Err(SummaryError::OptInRequired)
        );
        writer.settings_mut().enabled = true;
        assert_eq!(
            writer.prepare_request("context"),
            Err(SummaryError::MissingApiKey)
        );
        writer.keychain_mut().set("secret").unwrap();
        let request = writer.prepare_request("context").unwrap();
        assert_eq!(request.sanitized_context, "context");
        assert!(request.keychain_present);
    }

    #[test]
    fn summary_writer_is_single_debounced_metadata_owner() {
        let mut store = MemoryKeychainStore::new("openrouter");
        store.set("secret").unwrap();
        let settings = OpenRouterSettings {
            enabled: true,
            debounce_ms: 100,
            ..OpenRouterSettings::default()
        };
        let mut writer = SummaryWriter::new(store, settings);
        let mut sink = MemorySummarySink::default();
        let first = writer
            .publish("agent:a", "visible context", 1_000, &mut sink)
            .unwrap();
        assert_eq!(sink.writes(), 1);
        assert_eq!(sink.latest(), Some(&first));
        assert_eq!(
            writer.publish("agent:a", "second", 1_050, &mut sink),
            Err(SummaryError::RateLimited { retry_after_ms: 50 })
        );
        let second = writer
            .publish("agent:a", "second", 1_100, &mut sink)
            .unwrap();
        assert_ne!(first.context_digest, second.context_digest);
        assert_eq!(sink.writes(), 2);
    }

    #[test]
    fn oversized_context_is_explicitly_rejected_before_provider_work() {
        let mut store = MemoryKeychainStore::new("openrouter");
        store.set("secret").unwrap();
        let settings = OpenRouterSettings {
            enabled: true,
            max_context_chars: 4,
            ..OpenRouterSettings::default()
        };
        let writer = SummaryWriter::new(store, settings);
        assert_eq!(
            writer.prepare_request("12345"),
            Err(SummaryError::ContextTooLarge { chars: 5, max: 4 })
        );
    }

    #[cfg(target_vendor = "apple")]
    #[test]
    fn keychain_delete_preserves_non_missing_security_failures() {
        let error =
            super::security_error("delete", security_framework::base::Error::from_code(-50));
        assert_eq!(
            error,
            super::KeychainError::CommandFailed {
                operation: "delete",
                code: Some(-50),
            }
        );
        assert!(!super::is_missing_item(
            security_framework::base::Error::from_code(-50)
        ));
        assert!(super::is_missing_item(
            security_framework::base::Error::from_code(super::ERR_SEC_ITEM_NOT_FOUND)
        ));
    }
}
