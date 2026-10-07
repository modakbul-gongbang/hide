//! Durable agent delivery owned by the core.
pub(crate) mod doorbell;
mod human;
pub mod ledger;
pub mod mailbox;
pub mod watch;
pub mod worker;

pub mod answer;

pub use mailbox::Command;

pub const DELIVERY_EXPIRY_MS: u64 = 60 * 60 * 1_000;
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

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct Actor {
    pub pane_id: String,
    pub name: String,
    pub kind: String,
    pub device_id: String,
    pub session: Option<String>,
}

/// The kind of a code-owned recipient: a Software Factory's engine, which
/// reads its own letters from the ledger with no pane, composer or doorbell
/// (D-14). Only the core's Factory host constructs one.
pub const FACTORY_KIND: &str = "factory";
pub const FACTORY_PREFIX: &str = "factory:";
/// A watch whose observer is a Factory warns after this long (PRD
/// software-factory, Technical structure: the window is per watch).
pub const FACTORY_INACTIVITY_MS: u64 = 30 * 60_000;

impl Actor {
    /// The code-owned recipient `factory:<id>` on `node`, the core's own.
    pub fn factory(id: &str, node: &str) -> Self {
        let name = format!("{FACTORY_PREFIX}{id}");
        Self {
            pane_id: name.clone(),
            name: name.clone(),
            kind: FACTORY_KIND.into(),
            device_id: node.into(),
            session: crate::wire::session_digest(&name),
        }
    }

    /// Whether this is a Factory's code-owned identity rather than a pane's.
    pub fn code_owned(&self) -> bool {
        self.kind == FACTORY_KIND && self.pane_id.starts_with(FACTORY_PREFIX)
    }

    /// Mailbox authority requires a positively observed native session. Watch
    /// targets may lack it and use status-only activity, without mailbox access.
    pub(crate) fn require_native_identity(&self) -> Result<(), String> {
        if self.valid() && self.session.is_some() {
            Ok(())
        } else {
            Err("native_identity_required".into())
        }
    }

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

/// A name or pane a pane may never claim: it would read as a Factory.
pub(crate) fn reserved_name(value: &str) -> bool {
    value.starts_with(FACTORY_PREFIX)
}

pub(crate) fn valid_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
