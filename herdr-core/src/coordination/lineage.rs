//! Changed-only metadata writes, driven by the existing coordinator refresh.
//! One active batch and one pending batch per server, with bounded patches.
use super::AgentRecord;
use crate::delivery::ledger::Ledger;
use crate::runtime::Runtime;
use crate::session_sync::ProjectedAgent;
use hide_herdr_client::{ApiConnector, request_with_connector};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const KEYS: [&str; 4] = [
    "parent_pane",
    "parent_machine",
    "child_session",
    "parent_session",
];
#[derive(Clone, Debug, PartialEq)]
struct Patch {
    pane: String,
    tokens: BTreeMap<String, Value>,
}

pub(crate) struct Writer {
    sender: Option<mpsc::SyncSender<Vec<Patch>>>,
    worker: Option<JoinHandle<()>>,
    device: String,
    queued: HashSet<String>,
    completed: mpsc::Receiver<(String, bool)>,
    // Registration identities are append-only. Ending one changes no token
    // identity, so unrelated delivery writes cannot trigger reconciliation.
    observed_registrations: Option<usize>,
    stopping: Arc<AtomicBool>,
    pending_retry: bool,
}

impl Writer {
    pub(crate) fn new(
        device: String,
        connector: Arc<dyn ApiConnector>,
        runtime: Weak<Mutex<Runtime>>,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel::<Vec<Patch>>(1);
        let (done, completed) = mpsc::sync_channel(super::AGENT_LIMIT * 2);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let worker = thread::Builder::new()
            .name("hide-lineage".into())
            .spawn(move || {
                while let Ok(patches) = receiver.recv() {
                    for patch in patches {
                        if stop.load(Ordering::Acquire) || runtime.upgrade().is_none() {
                            return;
                        }
                        let result = write(connector.as_ref(), &patch);
                        let _ = done.try_send((signature(&patch), result.is_ok()));
                        if let Err(error) = result {
                            crate::diagnostic!(
                                json!({"component":"lineage","kind":"write_failed","message":error})
                            );
                        }
                    }
                }
            })
            .map_err(|_| "lineage_unavailable")?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            device,
            queued: HashSet::new(),
            completed,
            observed_registrations: None,
            stopping,
            pending_retry: false,
        })
    }

    pub(crate) fn observe(
        &mut self,
        ledger: &Arc<Ledger>,
        agents: &[ProjectedAgent],
        changed: bool,
    ) {
        let mut failed = false;
        while let Ok((signature, ok)) = self.completed.try_recv() {
            if !ok {
                self.queued.remove(&signature);
                failed = true;
            }
        }
        if !changed
            && !failed
            && !self.pending_retry
            && self.observed_registrations == Some(ledger.agents.len())
        {
            return;
        }
        self.pending_retry = false;
        self.observed_registrations = Some(ledger.agents.len());
        let patches = plan(ledger, &self.device, agents);
        let current = patches
            .iter()
            .map(|patch| signature(patch))
            .collect::<HashSet<_>>();
        self.queued.retain(|signature| current.contains(signature));
        let fresh = patches
            .into_iter()
            .filter(|patch| !self.queued.contains(&signature(patch)))
            .collect::<Vec<_>>();
        if fresh.is_empty() {
            return;
        }
        let signatures = fresh.iter().map(signature).collect::<Vec<_>>();
        if self
            .sender
            .as_ref()
            .is_some_and(|sender| sender.try_send(fresh).is_ok())
        {
            self.queued.extend(signatures);
        } else {
            self.pending_retry = true;
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn signature(patch: &Patch) -> String {
    format!("{}:{:?}", patch.pane, patch.tokens)
}
fn desired(
    record: &AgentRecord,
    parent: Option<&AgentRecord>,
    current_session: Option<&str>,
) -> BTreeMap<String, Value> {
    let live = (record.actor.session.as_deref() == current_session)
        .then_some(parent)
        .flatten();
    let values = if let Some(parent) = live {
        [
            Value::String(parent.pane.clone()),
            if record.host_scope == parent.host_scope && record.machine == parent.machine {
                Value::Null
            } else {
                Value::String(parent.native_machine.clone())
            },
            record
                .actor
                .session
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
            parent
                .actor
                .session
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        ]
    } else {
        [Value::Null, Value::Null, Value::Null, Value::Null]
    };
    KEYS.into_iter()
        .zip(values)
        .map(|(key, value)| (key.into(), value))
        .collect()
}
fn plan(ledger: &Ledger, device: &str, agents: &[ProjectedAgent]) -> Vec<Patch> {
    if ledger.agents.is_empty() {
        return Vec::new();
    }
    let parents = ledger
        .agents
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    let records = ledger
        .agents
        .iter()
        .filter(|record| record.machine == device && record.parent.is_some())
        .map(|record| (record.pane.as_str(), record))
        .collect::<HashMap<_, _>>();
    let mut patches = Vec::new();
    for agent in agents.iter().take(super::AGENT_LIMIT) {
        let Some(record) = records.get(agent.pane_id.as_str()) else {
            continue;
        };
        if agent.lineage_session.is_none() {
            continue;
        }
        let parent = record
            .parent
            .as_ref()
            .and_then(|id| parents.get(id.as_str()).copied());
        let tokens = desired(record, parent, agent.lineage_session.as_deref());
        if tokens.iter().any(|(key, value)| {
            if value.is_null() {
                agent.tokens.get(key).is_some_and(|old| !old.is_null())
            } else {
                agent.tokens.get(key) != Some(value)
            }
        }) {
            patches.push(Patch {
                pane: agent.pane_id.clone(),
                tokens,
            });
        }
    }
    patches
}
fn write(connector: &dyn ApiConnector, patch: &Patch) -> Result<(), String> {
    // The pinned public pane.report_metadata token patch: null clears one key.
    request_with_connector(
        connector,
        "pane.report_metadata",
        json!({"pane_id":patch.pane,"source":"hide","tokens":patch.tokens}),
        Duration::from_secs(1),
    )
    .map(|_| ())
    .map_err(|error| format!("{error}"))
}
pub(crate) fn write_record(
    connector: &dyn ApiConnector,
    record: &AgentRecord,
    parent: &AgentRecord,
) -> Result<(), String> {
    write(
        connector,
        &Patch {
            pane: record.pane.clone(),
            tokens: desired(record, Some(parent), record.actor.session.as_deref()),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delivery::Actor;
    fn record(id: &str, pane: &str, session: &str, parent: Option<&str>) -> AgentRecord {
        AgentRecord {
            id: id.into(),
            name: id.into(),
            machine: "local".into(),
            host_scope: "fixture".into(),
            native_machine: "fixture-machine".into(),
            session: session.into(),
            instance: pane.into(),
            pane: pane.into(),
            parent: parent.map(str::to_owned),
            project: None,
            actor: Actor {
                pane_id: pane.into(),
                name: id.into(),
                kind: "codex".into(),
                device_id: "local".into(),
                session: crate::wire::session_digest(session),
            },
            ended: false,
        }
    }
    #[test]
    fn tokens_keep_digest_contract_and_session_replacement_clears_every_key() {
        let parent = record("agent-1", "w1:p1", "parent", None);
        let child = record("agent-2", "w2:p1", "child", Some(&parent.id));
        let tokens = desired(&child, Some(&parent), child.actor.session.as_deref());
        assert_eq!(tokens["parent_pane"], "w1:p1");
        assert!(tokens["parent_machine"].is_null());
        assert_eq!(
            tokens["child_session"],
            child.actor.session.clone().unwrap()
        );
        assert_eq!(
            tokens["parent_session"],
            parent.actor.session.clone().unwrap()
        );
        assert!(
            desired(&child, Some(&parent), Some("replacement"))
                .values()
                .all(Value::is_null)
        );
        assert!(
            desired(&child, None, child.actor.session.as_deref())
                .values()
                .all(Value::is_null)
        );
    }
    #[test]
    fn an_ended_registration_keeps_lineage_and_same_server_omits_machine() {
        let mut parent = record("agent-1", "w1:p1", "parent", None);
        let mut child = record("agent-2", "w2:p1", "child", Some(&parent.id));
        let original = desired(&child, Some(&parent), child.actor.session.as_deref());
        child.ended = true;
        parent.ended = true;
        assert_eq!(
            desired(&child, Some(&parent), child.actor.session.as_deref()),
            original
        );
        child.host_scope = "other-server".into();
        assert_eq!(
            desired(&child, Some(&parent), child.actor.session.as_deref())["parent_machine"],
            "fixture-machine"
        );
    }
    fn agent(record: &AgentRecord, tokens: BTreeMap<String, Value>) -> ProjectedAgent {
        ProjectedAgent {
            pane_id: record.pane.clone(),
            name: Some(record.name.clone()),
            workspace_id: "fixture".into(),
            tab_id: "fixture:tab".into(),
            cwd: None,
            agent: Some("codex".into()),
            agent_status: Some("idle".into()),
            agent_session: None,
            spawned_from_pane_id: None,
            spawned_from_machine_id: None,
            declared_parent_session: None,
            lineage_session: record.actor.session.clone(),
            state_change_seq: 1,
            tokens,
        }
    }
    #[test]
    fn reconciliation_writes_only_different_panes_and_preserves_unknown_sessions() {
        let parent = record("agent-1", "w1:p1", "parent", None);
        let first = record("agent-2", "w2:p1", "first", Some(&parent.id));
        let second = record("agent-3", "w3:p1", "second", Some(&parent.id));
        let ledger = Ledger {
            next_id: 4,
            agents: vec![parent.clone(), first.clone(), second.clone()],
            ..Default::default()
        };
        let first_tokens = desired(&first, Some(&parent), first.actor.session.as_deref());
        let second_tokens = desired(&second, Some(&parent), second.actor.session.as_deref());
        let observed = vec![
            agent(&first, first_tokens.clone()),
            agent(&second, second_tokens.clone()),
        ];
        assert!(plan(&ledger, "local", &observed).is_empty());
        let mut changed = observed.clone();
        changed[1].tokens.remove("parent_session");
        let patches = plan(&ledger, "local", &changed);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].pane, second.pane);
        changed[0].lineage_session = None;
        assert_eq!(plan(&ledger, "local", &changed).len(), 1);
        changed[0].lineage_session = crate::wire::session_digest("replacement");
        let patches = plan(&ledger, "local", &changed);
        assert_eq!(patches.len(), 2);
        assert!(
            patches
                .iter()
                .find(|patch| patch.pane == first.pane)
                .unwrap()
                .tokens
                .values()
                .all(Value::is_null)
        );
    }
}
