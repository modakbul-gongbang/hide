//! Provider boundary for Hide's background AI features.
//!
//! A feature owns its prompt, output schema and parser and submits an
//! [`AiRequest`]; this crate owns provider lifecycle, availability, timeouts,
//! cancellation, structured errors and duplicate suppression, and hands back
//! a schema-validated [`serde_json::Value`]. No feature type lives here.

mod claude;
mod codex;
mod log;
mod router;
mod schema;

pub use claude::ClaudeInteractiveBackend;
pub use codex::{CodexAppServerBackend, CodexConfig};
pub use log::{AiLogEvent, AiLogSink, NoopLogSink};
pub use router::{AiRouter, RouterConfig};

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable provider name; also the log field value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Codex,
    Claude,
}

impl ProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Caller-generated idempotency key. It travels to the provider as the client
/// message id and is the only identifier a log line carries for the request.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RequestId(pub String);

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a feature submits. Everything a provider needs to answer, and nothing
/// about how any provider works.
#[derive(Clone, Debug)]
pub struct AiRequest {
    pub feature_id: &'static str,
    pub request_id: RequestId,
    /// Identifies what the request is about (a pane, a checkout); together
    /// with the feature and the input hash it forms the duplicate key.
    pub subject_id: String,
    pub system: String,
    pub input: String,
    pub output_schema: Value,
    pub max_output_tokens: u32,
    pub deadline: Duration,
    /// Version of the feature's prompt and schema pair, for the log only.
    pub schema_version: &'static str,
}

/// Provider readiness as the router sees it. Every variant except `Ready`
/// keeps the provider out of selection and is reported to the caller.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Availability {
    Ready,
    NeedsLogin,
    NotInstalled,
    /// The provider exists but cannot answer right now (a broken child
    /// process, a model the account does not offer).
    Unavailable {
        reason: String,
    },
    /// No stable contract exists to drive the provider; the reason names the
    /// gap.
    Unsupported {
        reason: String,
    },
}

impl Availability {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub fn class(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NeedsLogin => "needs_login",
            Self::NotInstalled => "not_installed",
            Self::Unavailable { .. } => "unavailable",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

/// Failure classes a caller can branch on. Payloads carry diagnostics, never
/// prompt or output content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AiError {
    /// Not retried until availability changes.
    NotAuthenticated,
    /// Account-global cooldown on that provider.
    UsageLimited {
        retry_after: Option<Duration>,
    },
    ProviderUnavailable(String),
    Timeout,
    Cancelled,
    /// Schema mismatch or a stream that ended before the turn completed.
    InvalidOutput(String),
    Transient(String),
    /// The provider has no stable contract; see `Availability::Unsupported`.
    Unsupported(String),
    /// No connected provider; carries each provider's availability.
    NoProvider(Vec<(ProviderId, Availability)>),
}

impl AiError {
    pub fn class(&self) -> &'static str {
        match self {
            Self::NotAuthenticated => "not_authenticated",
            Self::UsageLimited { .. } => "usage_limited",
            Self::ProviderUnavailable(_) => "provider_unavailable",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::InvalidOutput(_) => "invalid_output",
            Self::Transient(_) => "transient",
            Self::Unsupported(_) => "unsupported",
            Self::NoProvider(_) => "no_provider",
        }
    }
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UsageLimited { retry_after } => match retry_after {
                Some(wait) => write!(f, "usage_limited;retry_after_s={}", wait.as_secs()),
                None => f.write_str("usage_limited"),
            },
            Self::ProviderUnavailable(reason)
            | Self::InvalidOutput(reason)
            | Self::Transient(reason)
            | Self::Unsupported(reason) => write!(f, "{}:{reason}", self.class()),
            Self::NoProvider(states) => {
                f.write_str("no_provider")?;
                for (provider, state) in states {
                    write!(f, ";{provider}={}", state.class())?;
                }
                Ok(())
            }
            _ => f.write_str(self.class()),
        }
    }
}

impl std::error::Error for AiError {}

/// Token accounting a provider reports for one answer, for the log only.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AiUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// One provider answer before the router validates it.
#[derive(Clone, Debug)]
pub struct AiResponse {
    pub value: Value,
    pub usage: AiUsage,
}

/// Cooperative cancellation shared between the caller and the transport.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub trait AiBackend: Send + Sync {
    fn id(&self) -> ProviderId;
    fn availability(&self) -> Availability;
    fn execute(&self, request: &AiRequest, cancel: &CancelToken) -> Result<AiResponse, AiError>;
}
