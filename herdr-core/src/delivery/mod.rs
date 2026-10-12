//! Durable agent delivery owned by the core.
mod bell;
pub(crate) mod doorbell;
mod human;
pub mod ledger;
pub mod mailbox;
pub mod watch;
pub mod worker;

pub mod answer;

pub use mailbox::Command;

pub const DELIVERY_EXPIRY_MS: u64 = 60 * 60 * 1_000;
/// How long a `request` or `block` letter waits for its answer, from the time
/// it was sent, before the wait ends as `deadline`.
pub const ANSWER_WAIT_MS: u64 = 24 * 60 * 60 * 1_000;
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
/// The kind of Hide's own code-owned identity: the origin of the agents
/// Hide starts for the operator rather than for a pane, such as a Memory
/// card's promotion into the repository (PRD project-memory-v2 D-25). It has
/// no pane and reads no letters; only the core constructs one.
pub const HIDE_KIND: &str = "hide";
pub const HIDE_PREFIX: &str = "hide:";
/// The one role Hide starts agents as today.
pub const HIDE_MEMORY_ROLE: &str = "memory";
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

    /// Hide's own code-owned identity for `role` on `node`, the core's own.
    pub fn hide(role: &str, node: &str) -> Self {
        let name = format!("{HIDE_PREFIX}{role}");
        Self {
            pane_id: name.clone(),
            name: name.clone(),
            kind: HIDE_KIND.into(),
            device_id: node.into(),
            session: crate::wire::session_digest(&name),
        }
    }

    /// Whether this is a code-owned identity (a Factory's or Hide's own)
    /// rather than a pane's.
    pub fn code_owned(&self) -> bool {
        (self.kind == FACTORY_KIND && self.pane_id.starts_with(FACTORY_PREFIX))
            || (self.kind == HIDE_KIND && self.pane_id.starts_with(HIDE_PREFIX))
    }

    /// Whether this is Hide's own code-owned identity, the origin of the work
    /// Hide starts itself, as opposed to a Factory's or a pane's.
    pub fn hide_owned(&self) -> bool {
        self.kind == HIDE_KIND && self.pane_id.starts_with(HIDE_PREFIX)
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

/// A name or pane a pane may never claim: it would read as a Factory or as
/// Hide itself.
pub(crate) fn reserved_name(value: &str) -> bool {
    value.starts_with(FACTORY_PREFIX) || value.starts_with(HIDE_PREFIX)
}

/// A pane's native session as a letter pull or a Factory question guard names
/// it: the helper's own rule, so the CLI, the daemon and the hook agree.
pub use hide_agent_hooks::delivery::{
    SESSION_LIMIT, prompt_digest, valid_prompt_digest, valid_session,
};

pub(crate) fn valid_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
