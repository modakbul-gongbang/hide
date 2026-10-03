//! Durable agent delivery, owned by the core independently of hcoord.
pub(crate) mod doorbell;
pub mod ledger;
pub mod mailbox;
pub mod watch;
pub mod worker;

pub use mailbox::Command;

pub const DELIVERY_EXPIRY_MS: u64 = 10 * 60 * 1_000;
pub const INPUT_QUIET_MS: u64 = 30_000;
pub const TICK_MS: u64 = 60_000;
pub const INACTIVITY_MS: u64 = 20 * 60_000;
pub const SECOND_WARNING_MS: u64 = 60 * 60_000;
pub const RETENTION_MS: u64 = 30 * 24 * 60 * 60_000;
pub const OPEN_LIMIT: usize = 1_024;
pub const LETTER_LIMIT: usize = 5_000;
pub const WATCH_LIMIT: usize = 32;
pub const BODY_LIMIT: usize = 16 * 1_024;
pub const FILE_LIMIT: usize = 16 * 1_024 * 1_024;
pub const HOOK_LIMIT: usize = 8 * 1_024;
pub const HOOK_LETTERS: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Actor {
    pub pane_id: String,
    pub name: String,
    pub kind: String,
    pub device_id: String,
    pub session: Option<String>,
}

impl Actor {
    pub(crate) fn valid(&self) -> bool {
        valid_key(&self.pane_id)
            && valid_key(&self.name)
            && valid_key(&self.kind)
            && valid_key(&self.device_id)
            && self.session.as_deref().is_none_or(valid_key)
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        self.pane_id == other.pane_id
            && self.device_id == other.device_id
            && self.session == other.session
    }
}

pub(crate) fn valid_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
