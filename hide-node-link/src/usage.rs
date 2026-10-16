//! One bounded subscription read on the core's own node. No credential
//! type crosses this boundary; devices refuse the call.
pub use hide_agent_adapter::usage::LimitSource;
pub use hide_ai::UsageError;
use serde::{Deserialize, Serialize};
pub const WEEKLY_WINDOW_MINUTES: u64 = 10_080;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LimitFetch {
    pub result: Result<String, LimitFailure>,
    pub fallback: Option<SessionLimits>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionLimits {
    /// Only rate-limit fields, never the conversation or account.
    pub body: String,
    pub source_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureDisposition {
    Offline,
    Authentication,
    Schema,
    NotInstalled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LimitFailure {
    pub disposition: FailureDisposition,
    /// A fixed diagnostic code, never provider text.
    pub kind: String,
    pub status: Option<u16>,
    /// A bounded HTTP header; interpreted by the core's retry scheduler.
    pub retry_after: Option<String>,
}
impl LimitFailure {
    pub fn new(disposition: FailureDisposition, kind: &str) -> Self {
        Self {
            disposition,
            kind: kind.into(),
            status: None,
            retry_after: None,
        }
    }
    pub fn cli(error: UsageError) -> Self {
        let (disposition, kind) = match error {
            UsageError::NotInstalled => (FailureDisposition::NotInstalled, "not_installed"),
            UsageError::Timeout => (FailureDisposition::Offline, "timeout"),
            UsageError::Cancelled => (FailureDisposition::Offline, "cancelled"),
            UsageError::Failed(_) => (FailureDisposition::Offline, "cli_failed"),
            UsageError::NoResultFrame => (FailureDisposition::Schema, "no_result_frame"),
            UsageError::NotAuthenticated => {
                (FailureDisposition::Authentication, "not_authenticated")
            }
        };
        Self::new(disposition, kind)
    }
}

/// The own node fetches one public dataset; the core parses and caches it.
pub const PRICE_BODY_LIMIT: usize = 16 * 1024 * 1024;
pub type PriceFetch = Result<String, PriceFailure>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceFailure {
    Fetch,
    Body,
}
