//! The answers `hide agent`, `hide request`, `hide inbox` and `hide watch`
//! return, as types, so the CLI's exported contract describes each answer
//! from the type that builds it (`answer_schemas`) and a changed field
//! changes the contract.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use super::ledger::Letter;
use super::mailbox::Intake;
use super::watch::Watch;

/// One registered agent, as `hide agent show`, `register`, `end` and `spawn`
/// answer it.
#[derive(Serialize, JsonSchema)]
pub struct AgentView {
    pub id: String,
    pub name: String,
    pub machine: String,
    #[serde(rename = "hostScope")]
    pub host_scope: String,
    pub session: String,
    pub instance: String,
    pub pane: String,
    pub parent: Option<String>,
    pub project: Option<String>,
    /// `running` or `ended`.
    pub runtime: &'static str,
    /// `connected` or `disconnected`.
    pub connection: &'static str,
    pub registered: bool,
    /// The watch on this agent, if one is running.
    pub watch: Option<Watch>,
}

/// `hide agent list`.
#[derive(Serialize, JsonSchema)]
pub struct AgentList {
    pub items: Vec<AgentView>,
}

/// `hide agent register --check` for a caller not yet registered.
#[derive(Serialize, JsonSchema)]
pub struct RegisterCheck {
    pub registered: bool,
    pub name: String,
    pub pane: String,
}

/// One letter of `hide inbox`.
#[derive(Serialize, JsonSchema)]
pub struct InboxEntry {
    #[serde(flatten)]
    pub letter: Letter,
    /// The command that acknowledges the letter, for an agent with no
    /// prompt hook to confirm it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ack_command: Option<String>,
}

/// `hide inbox --confirm`.
#[derive(Serialize, JsonSchema)]
pub struct Confirmed {
    pub confirmed: Vec<String>,
}

/// `hide watch stop`.
#[derive(Serialize, JsonSchema)]
pub struct Stopped {
    pub stopped: String,
}

/// The JSON Schema of every answer, by the name the CLI contract gives it.
pub fn answer_schemas() -> Value {
    json!({
        "agent": schemars::schema_for!(AgentView),
        "agent_list": schemars::schema_for!(AgentList),
        "agent_register_check": schemars::schema_for!(RegisterCheck),
        "letter": schemars::schema_for!(Letter),
        "inbox": schemars::schema_for!(Vec<InboxEntry>),
        "intake": schemars::schema_for!(Intake),
        "confirmed": schemars::schema_for!(Confirmed),
        "watch": schemars::schema_for!(Watch),
        "watch_list": schemars::schema_for!(Vec<Watch>),
        "stopped": schemars::schema_for!(Stopped),
    })
}
