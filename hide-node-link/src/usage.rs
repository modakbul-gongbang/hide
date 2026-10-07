//! What a node answers for the toolbar's Weekly Usage rows: the Codex login
//! it holds, the weekly window its newest Codex session recorded, and the
//! text `claude -p /usage` printed. The core schedules the reads, asks the
//! network and words the rows.

use serde::{Deserialize, Serialize};

pub use hide_ai::UsageError;

/// The weekly window, in the minutes Codex names its windows with.
pub const WEEKLY_WINDOW_MINUTES: u64 = 10_080;

/// The token and account `auth.json` holds. The core asks only its own node,
/// which shares its process; a node across a link answers to the login that
/// reached it (PRD D-13). `Debug` leaves the token out, so a logged value
/// never carries it.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct CodexCredentials {
    pub access_token: String,
    pub account_id: String,
}

impl std::fmt::Debug for CodexCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CodexCredentials")
            .field("access_token", &"<redacted>")
            .field("account_id", &self.account_id)
            .finish()
    }
}

/// Why no credentials were read.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialsRefusal {
    /// No `auth.json`, or not JSON.
    Missing,
    /// JSON without a token and an account.
    Schema,
}

impl CredentialsRefusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing => "credentials_missing",
            Self::Schema => "credentials_schema",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum CredentialsAnswer {
    Found { credentials: CodexCredentials },
    Refused { refusal: CredentialsRefusal },
}

/// The weekly window the newest Codex session recorded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CodexWeeklyUsage {
    pub used_percent: f64,
    pub resets_at_unix_seconds: u64,
    /// When the session recorded it, if the line says.
    pub source_at_unix_ms: Option<u64>,
}

/// What `claude -p /usage` printed, or why it printed nothing usable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum UsageText {
    Text { text: String },
    Failed { error: UsageError },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_leaves_the_token_out() {
        let credentials = CodexCredentials {
            access_token: "secret-token".to_owned(),
            account_id: "account".to_owned(),
        };
        let shown = format!(
            "{credentials:?} {:?}",
            CredentialsAnswer::Found {
                credentials: credentials.clone()
            }
        );
        assert!(!shown.contains("secret-token"), "{shown}");
        assert!(shown.contains("account"));
    }
}
