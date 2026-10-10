//! `hide memory --hook`: an agent hook's Project Memory, answered by the core
//! from the one store it owns, whichever machine's agent asks (PRD
//! core-host-node-move D-12, B14).
//!
//! The Project is the one holding the checkout the caller's pane is attested
//! to, read by that checkout's own node (`Call::Project`); nothing the hook
//! sends names it. The store path and the node's link are taken under the
//! lock as owned values ([`Scope`]); the node call, the read-only open, the
//! retrieval and the receipt tag run off it, inside the caller's deadline.
//! Only the core's own node and a node that dials in have Memory: a device the
//! core dials keeps none, as before (Q20).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hide_memory::hook::{HookContext, HookEvent, HookOutcome, HookRequest};
use hide_node_link::protocol::Call;
use serde_json::{Value, json};

pub use hide_memory::hook::{PROMPT_LIMIT_BYTES, prefix};

use crate::node::NodeId;
use crate::node_access::{NodeLink, call_as};

/// How long the core spends on one hook's Memory once the request is
/// admitted, inside the 1,250 ms the hook waits for it.
pub const WITHIN: Duration = Duration::from_millis(1_000);

/// What the core answers a hook's Memory read from.
pub struct Scope {
    pub(crate) store: PathBuf,
    pub(crate) node: NodeId,
    pub(crate) checkout_path: String,
    pub(crate) link: Arc<dyn NodeLink>,
}

/// What one hook asks, as the CLI sent it.
pub struct Ask {
    runtime: String,
    event: HookEvent,
    session: String,
    prompt: Option<String>,
    /// The agent's working folder, canonical on the agent's machine.
    cwd: Option<String>,
}

impl Ask {
    /// The ask a `memory` request carries: an event Memory answers, the
    /// runtime as Memory names its sessions, the agent's own session id, and
    /// the prompt and working folder when the hook has them.
    pub fn from_wire(value: &Value) -> Option<Self> {
        let event = match value["event"].as_str()? {
            "SessionStart" => HookEvent::SessionStart,
            "UserPromptSubmit" => HookEvent::UserPromptSubmit,
            _ => return None,
        };
        let runtime = value["runtime"]
            .as_str()
            .filter(|runtime| memory_runtimes().any(|known| known == *runtime))?;
        let session = value["session"]
            .as_str()
            .filter(|session| crate::delivery::valid_session(session))?;
        let text = |key: &str| match value.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(Value::String(text)) => Some(Some(text.clone())),
            Some(_) => None,
        };
        let cwd = text("cwd")?;
        if cwd
            .as_deref()
            .is_some_and(|cwd| cwd.is_empty() || cwd.chars().any(char::is_control))
        {
            return None;
        }
        Some(Self {
            runtime: runtime.to_owned(),
            event,
            session: session.to_owned(),
            prompt: text("prompt")?,
            cwd,
        })
    }
}

/// The runtimes whose adapter declares Memory, as Memory names their
/// sessions (`claude` for Claude Code).
fn memory_runtimes() -> impl Iterator<Item = String> {
    hide_agent_adapter::ADAPTERS
        .iter()
        .filter(|row| row.memory.is_some())
        .filter_map(|row| row.session)
        .filter_map(|format| {
            serde_json::to_value(hide_session::Agent::from_format(format))
                .ok()
                .and_then(|name| name.as_str().map(str::to_owned))
        })
}

/// The Memory context for `ask`, or none with why, before `deadline`.
pub fn answer(scope: &Scope, ask: &Ask, deadline: Instant) -> HookContext {
    let without = |outcome| HookContext {
        context: None,
        outcome,
    };
    // A store that does not exist yet costs no node call.
    if !scope.store.is_file() {
        return without(HookOutcome::Unavailable);
    }
    let Some(within) = deadline.checked_duration_since(Instant::now()) else {
        return without(HookOutcome::Deadline);
    };
    let project = match project_identity(
        scope.link.as_ref(),
        &scope.node,
        &scope.checkout_path,
        within,
    ) {
        Ok(project) => project,
        Err(reason) => {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "hook.project_unresolved",
                "device": scope.node.as_str(),
                "reason": reason,
            }));
            return without(if Instant::now() >= deadline {
                HookOutcome::Deadline
            } else {
                HookOutcome::ProjectUnresolved
            });
        }
    };
    let path_context = ask
        .cwd
        .as_deref()
        .map(|cwd| hide_memory::hook::path_context(&project, cwd));
    hide_memory::hook::context_for(
        &scope.store,
        &project,
        HookRequest {
            runtime_id: &ask.runtime,
            event: ask.event,
            session_id: &ask.session,
            prompt: ask.prompt.as_deref(),
            path_context: path_context.as_deref(),
        },
        move || Instant::now() >= deadline,
    )
}

/// The Project that holds `checkout_path`, as the node that holds it reads
/// it; the id names the node.
pub(crate) fn project_identity(
    link: &dyn NodeLink,
    node: &NodeId,
    checkout_path: &str,
    within: Duration,
) -> Result<hide_project::ProjectIdentity, String> {
    let facts: hide_project::ProjectFacts = call_as(
        link,
        Call::Project {
            path: checkout_path.to_owned(),
        },
        within,
    )
    .map_err(|error| error.to_string())?;
    Ok(hide_project::ProjectIdentity {
        id: hide_project::project_id(node.as_str(), &facts.root),
        root: facts.root,
        checkout_root: facts.checkout_root,
        device_id: node.as_str().to_owned(),
        kind: facts.kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_memory_wire_admits_exactly_the_runtimes_whose_adapter_declares_memory() {
        let schema: Value =
            serde_json::from_str(include_str!("../../contracts/hided-ws.schema.json")).unwrap();
        let wire: BTreeSet<String> = schema["$defs"]["memoryAsk"]["properties"]["runtime"]["enum"]
            .as_array()
            .expect("memory runtime enum")
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(wire, memory_runtimes().collect::<BTreeSet<_>>());
        for runtime in &wire {
            let ask = Ask::from_wire(&json!({
                "event": "UserPromptSubmit", "runtime": runtime, "session": "s-1",
            }));
            assert!(ask.is_some(), "{runtime}");
        }
        for refused in [
            json!({"event": "UserPromptSubmit", "runtime": "grok", "session": "s-1"}),
            json!({"event": "Stop", "runtime": "codex", "session": "s-1"}),
            json!({"event": "SessionStart", "runtime": "codex", "session": ""}),
            json!({"event": "SessionStart", "runtime": "codex", "session": "s-1", "cwd": "a\nb"}),
            json!({"event": "SessionStart", "runtime": "codex", "session": "s-1", "prompt": 7}),
        ] {
            assert!(Ask::from_wire(&refused).is_none(), "{refused}");
        }
    }
}
