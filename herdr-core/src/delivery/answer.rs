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
    pub runtime: Runtime,
    pub connection: Connection,
    pub registered: bool,
    /// The watch on this agent, if one is running.
    pub watch: Option<Watch>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Running,
    Ended,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Connected,
    Disconnected,
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
///
/// An answer is written, never read back, so every field it serializes is
/// present, a `None` as `null`; the schemas say so (`present`), where
/// schemars, describing what deserializing would accept, would leave an
/// optional or defaulted field out of `required`.
pub fn answer_schemas() -> Value {
    let mut schemas = json!({
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
    });
    present(&mut schemas);
    schemas
}

/// The fields an answer leaves out when they have no value
/// (`skip_serializing_if`).
const OMITTED: &[&str] = &["ack_command"];

/// Marks every property of every object schema in `schema` required, but
/// those in `OMITTED`.
fn present(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::Object(properties)) = map.get("properties") {
                let required: Vec<Value> = properties
                    .keys()
                    .filter(|key| !OMITTED.contains(&key.as_str()))
                    .map(|key| Value::String(key.clone()))
                    .collect();
                map.insert("required".to_owned(), Value::Array(required));
            }
            map.values_mut().for_each(present);
        }
        Value::Array(items) => items.iter_mut().for_each(present),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field the answer always writes is required in its schema, a `null`
    /// one included, and the one it can leave out is not.
    #[test]
    fn every_written_field_is_required() {
        let schemas = answer_schemas();
        let agent = schemas["agent"]["required"].as_array().unwrap();
        for field in [
            "parent",
            "project",
            "watch",
            "runtime",
            "connection",
            "hostScope",
        ] {
            assert!(agent.contains(&json!(field)), "agent {field}");
        }
        let watch = &schemas["agent"]["definitions"]["Watch"]["required"];
        assert!(watch.as_array().unwrap().contains(&json!("generation")));
        let entry = &schemas["inbox"]["items"]["required"];
        let entry = match entry {
            Value::Array(entry) => entry.clone(),
            _ => schemas["inbox"]["definitions"]["InboxEntry"]["required"]
                .as_array()
                .unwrap()
                .clone(),
        };
        assert!(entry.contains(&json!("reply_to")));
        assert!(!entry.contains(&json!("ack_command")));
        assert_eq!(
            schemas["agent"]["definitions"]["Runtime"]["enum"],
            json!(["running", "ended"])
        );
    }
}
