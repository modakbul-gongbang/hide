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
use std::time::{Duration, Instant};

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

#[derive(Debug)]
struct Batch {
    generation: u64,
    patches: Vec<Patch>,
}
#[derive(Debug)]
struct Completion {
    signature: String,
    generation: u64,
    succeeded: bool,
    finished_at: Instant,
}

pub(crate) struct Writer {
    sender: Option<mpsc::SyncSender<Batch>>,
    worker: Option<JoinHandle<()>>,
    device: String,
    // Only signatures still missing from the latest snapshot are retained.
    // Generation checks keep obsolete batch completions from changing a retry.
    in_flight: HashMap<String, u64>,
    acknowledged: HashMap<String, Instant>,
    generation: u64,
    completed: mpsc::Receiver<Completion>,
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
        let (sender, receiver) = mpsc::sync_channel::<Batch>(1);
        let (done, completed) = mpsc::sync_channel(super::AGENT_LIMIT * 2);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let worker = thread::Builder::new()
            .name("hide-lineage".into())
            .spawn(move || {
                while let Ok(batch) = receiver.recv() {
                    for patch in batch.patches {
                        if stop.load(Ordering::Acquire) || runtime.upgrade().is_none() {
                            return;
                        }
                        let result = write(connector.as_ref(), &patch);
                        let _ = done.try_send(Completion {
                            signature: signature(&patch),
                            generation: batch.generation,
                            succeeded: result.is_ok(),
                            finished_at: Instant::now(),
                        });
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
            in_flight: HashMap::new(),
            acknowledged: HashMap::new(),
            generation: 0,
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
        snapshot_started_at: Instant,
    ) {
        let mut failed = false;
        while let Ok(completion) = self.completed.try_recv() {
            if self.in_flight.get(&completion.signature) != Some(&completion.generation) {
                continue;
            }
            self.in_flight.remove(&completion.signature);
            if completion.succeeded {
                self.acknowledged
                    .insert(completion.signature, completion.finished_at);
            } else {
                failed = true;
            }
        }
        if !changed
            && !failed
            && !self.pending_retry
            && self.acknowledged.is_empty()
            && self.observed_registrations == Some(ledger.agents.len())
        {
            return;
        }
        self.pending_retry = false;
        self.observed_registrations = Some(ledger.agents.len());
        let patches = plan(ledger, &self.device, agents);
        let current = patches.iter().map(signature).collect::<HashSet<_>>();
        self.in_flight
            .retain(|signature, _| current.contains(signature));
        // An ACK does not prove that a subsequent snapshot still has the
        // tokens. Suppress only reads that began before that ACK; any newer
        // snapshot (including reconnect bootstrap) may repair the same patch.
        self.acknowledged.retain(|signature, finished_at| {
            current.contains(signature) && snapshot_started_at < *finished_at
        });
        let fresh = patches
            .into_iter()
            .filter(|patch| {
                let signature = signature(patch);
                !self.in_flight.contains_key(&signature)
                    && !self.acknowledged.contains_key(&signature)
            })
            .collect::<Vec<_>>();
        if fresh.is_empty() {
            return;
        }
        let signatures = fresh.iter().map(signature).collect::<Vec<_>>();
        let Some(generation) = self.generation.checked_add(1) else {
            return;
        };
        if self.sender.as_ref().is_some_and(|sender| {
            sender
                .try_send(Batch {
                    generation,
                    patches: fresh,
                })
                .is_ok()
        }) {
            self.generation = generation;
            self.in_flight.extend(
                signatures
                    .into_iter()
                    .map(|signature| (signature, generation)),
            );
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
    let mut records: HashMap<&str, Vec<&AgentRecord>> = HashMap::new();
    for record in ledger
        .agents
        .iter()
        .filter(|record| record.machine == device)
    {
        records
            .entry(record.pane.as_str())
            .or_default()
            .push(record);
    }
    let mut patches = Vec::new();
    // The registry bounds retained targets, not their native-list position.
    // Consuming each match also bounds duplicate observations to one patch.
    for agent in agents {
        let Some(current_session) = agent.lineage_session.as_deref() else {
            continue;
        };
        let Some(registrations) = records.remove(agent.pane_id.as_str()) else {
            continue;
        };
        if !registrations.iter().any(|record| record.parent.is_some()) {
            continue;
        };
        // Each pane group is consumed once, so identity selection is linear
        // in retained registrations across the pass, independent of order.
        let Some(record) = registrations
            .iter()
            .rev()
            .copied()
            .find(|record| record.actor.session.as_deref() == Some(current_session))
            .or_else(|| registrations.last().copied())
        else {
            continue;
        };
        let parent = record
            .parent
            .as_ref()
            .and_then(|id| parents.get(id.as_str()).copied());
        let tokens = desired(record, parent, Some(current_session));
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
            origin: None,
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
    type WriterFixture = (
        Writer,
        mpsc::Receiver<Batch>,
        mpsc::SyncSender<Completion>,
        Arc<Ledger>,
        Vec<ProjectedAgent>,
    );
    fn writer_fixture() -> WriterFixture {
        let (sender, batches) = mpsc::sync_channel(1);
        let (done, completed) = mpsc::sync_channel(super::super::AGENT_LIMIT * 2);
        let parent = record("agent-1", "w1:p1", "parent", None);
        let child = record("agent-2", "w2:p1", "child", Some(&parent.id));
        let agents = vec![agent(&child, BTreeMap::new())];
        let ledger = Arc::new(Ledger {
            next_id: 3,
            agents: vec![parent, child],
            ..Default::default()
        });
        (
            Writer {
                sender: Some(sender),
                worker: None,
                device: "local".into(),
                in_flight: HashMap::new(),
                acknowledged: HashMap::new(),
                generation: 0,
                completed,
                observed_registrations: None,
                stopping: Arc::new(AtomicBool::new(false)),
                pending_retry: false,
            },
            batches,
            done,
            ledger,
            agents,
        )
    }

    fn completion(batch: &Batch, succeeded: bool, finished_at: Instant) -> Completion {
        Completion {
            signature: signature(&batch.patches[0]),
            generation: batch.generation,
            succeeded,
            finished_at,
        }
    }

    #[test]
    fn successful_unseen_tokens_are_repaired_by_a_fresh_snapshot_or_reconnect() {
        for reconnect in [false, true] {
            let (mut writer, batches, done, ledger, missing) = writer_fixture();
            let before = Instant::now();
            writer.observe(&ledger, &missing, true, before);
            let first = batches.try_recv().unwrap();
            let finished = before + Duration::from_millis(1);
            done.try_send(completion(&first, true, finished)).unwrap();
            // This read began before the ACK. Its missing values cannot prove
            // a reset, so the successful write is not duplicated yet.
            writer.observe(&ledger, &missing, false, before);
            assert!(batches.try_recv().is_err());
            assert!(writer.in_flight.is_empty());
            assert_eq!(writer.acknowledged.len(), 1);
            // Tokens were reset before we ever observed a matched snapshot.
            // Both the next regular read and a reconnect repair that state.
            writer.observe(&ledger, &missing, reconnect, finished);
            let repair = batches.try_recv().unwrap();
            assert_eq!(repair.patches, first.patches);
            assert!(repair.generation > first.generation);
            done.try_send(completion(&repair, true, finished)).unwrap();
            let mut matched = missing.clone();
            matched[0].tokens = repair.patches[0].tokens.clone();
            writer.observe(&ledger, &matched, true, finished);
            assert!(writer.in_flight.is_empty());
            assert!(writer.acknowledged.is_empty());
            writer.observe(&ledger, &matched, false, finished);
            assert!(batches.try_recv().is_err());
        }
    }

    #[test]
    fn obsolete_completion_cannot_cancel_a_new_write_of_the_same_signature() {
        let (mut writer, batches, done, ledger, missing) = writer_fixture();
        let observed = Instant::now();
        writer.observe(&ledger, &missing, true, observed);
        let old = batches.try_recv().unwrap();
        let mut matched = missing.clone();
        matched[0].tokens = old.patches[0].tokens.clone();
        writer.observe(&ledger, &matched, true, observed);
        writer.observe(&ledger, &missing, true, observed);
        let current = batches.try_recv().unwrap();
        done.try_send(completion(&old, true, observed)).unwrap();
        writer.observe(&ledger, &missing, false, observed);
        assert!(batches.try_recv().is_err());
        assert!(writer.acknowledged.is_empty());
        assert_eq!(
            writer.in_flight.get(&signature(&current.patches[0])),
            Some(&current.generation)
        );
        done.try_send(completion(&current, false, observed))
            .unwrap();
        writer.observe(&ledger, &missing, false, observed);
        let retry = batches.try_recv().unwrap();
        assert!(retry.generation > current.generation);
        assert_eq!(writer.in_flight.len(), 1);
    }

    #[test]
    fn a_full_pending_batch_is_retried_without_duplicating_in_flight_patches() {
        let (mut writer, batches, _, ledger, missing) = writer_fixture();
        let observed = Instant::now();
        writer
            .sender
            .as_ref()
            .unwrap()
            .try_send(Batch {
                generation: 0,
                patches: Vec::new(),
            })
            .unwrap();
        writer.observe(&ledger, &missing, true, observed);
        assert!(writer.pending_retry);
        assert!(writer.in_flight.is_empty());
        batches.try_recv().unwrap();
        writer.observe(&ledger, &missing, false, observed);
        let submitted = batches.try_recv().unwrap();
        assert_eq!(submitted.patches.len(), 1);
        assert!(!writer.pending_retry);
        writer.observe(&ledger, &missing, false, observed);
        assert!(batches.try_recv().is_err());
        assert_eq!(writer.in_flight.len(), 1);
    }
    #[test]
    fn a_registered_child_after_unregistered_native_prefix_receives_all_four_tokens() {
        let parent = record("agent-1", "w1:p1", "parent", None);
        let child = record("agent-2", "w2:p1", "child", Some(&parent.id));
        let ledger = Ledger {
            next_id: 3,
            agents: vec![parent.clone(), child.clone()],
            ..Default::default()
        };
        let mut observations = (0..super::super::AGENT_LIMIT)
            .map(|index| {
                let mut native = agent(&child, BTreeMap::new());
                native.pane_id = format!("unregistered-{index}");
                native
            })
            .collect::<Vec<_>>();
        observations.push(agent(&child, BTreeMap::new()));
        let patches = plan(&ledger, "local", &observations);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].pane, child.pane);
        assert_eq!(
            patches[0].tokens,
            BTreeMap::from([
                ("parent_pane".into(), Value::String(parent.pane)),
                ("parent_machine".into(), Value::Null),
                (
                    "child_session".into(),
                    Value::String(child.actor.session.unwrap())
                ),
                (
                    "parent_session".into(),
                    Value::String(parent.actor.session.unwrap())
                ),
            ])
        );
    }

    #[test]
    fn duplicate_native_observations_cannot_exceed_registered_patch_count() {
        let parent = record("agent-1", "w1:p1", "parent", None);
        let child = record("agent-2", "w2:p1", "child", Some(&parent.id));
        let observed = agent(&child, BTreeMap::new());
        let ledger = Ledger {
            next_id: 3,
            agents: vec![parent, child.clone()],
            ..Default::default()
        };
        let observations = vec![observed; super::super::AGENT_LIMIT + 1];
        let patches = plan(&ledger, "local", &observations);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].pane, child.pane);
    }
    struct ReplacementFixture {
        ledger: Ledger,
        native: ProjectedAgent,
        expected: BTreeMap<String, Value>,
    }
    fn replacement_fixture(stale_last: bool) -> ReplacementFixture {
        let previous_parent = record("agent-1", "w1:p1", "old-parent", None);
        let parent = record("agent-2", "w2:p1", "current-parent", None);
        let current = record("agent-3", "w3:p1", "current-child", Some(&parent.id));
        let stale = record("agent-4", "w3:p1", "old-child", Some(&previous_parent.id));
        let native = agent(&current, BTreeMap::new());
        let expected = BTreeMap::from([
            ("parent_pane".into(), Value::String(parent.pane.clone())),
            ("parent_machine".into(), Value::Null),
            (
                "child_session".into(),
                Value::String(current.actor.session.clone().unwrap()),
            ),
            (
                "parent_session".into(),
                Value::String(parent.actor.session.clone().unwrap()),
            ),
        ]);
        let mut ledger = Ledger {
            next_id: 5,
            agents: vec![previous_parent, parent],
            ..Default::default()
        };
        if stale_last {
            ledger.agents.extend([current, stale]);
        } else {
            ledger.agents.extend([stale, current]);
        }
        ledger.validate().unwrap();
        ReplacementFixture {
            ledger,
            native,
            expected,
        }
    }

    #[test]
    fn current_session_registration_wins_in_both_append_orders() {
        for stale_last in [false, true] {
            let mut fixture = replacement_fixture(stale_last);
            let patches = plan(&fixture.ledger, "local", &[fixture.native.clone()]);
            assert_eq!(patches.len(), 1);
            assert_eq!(patches[0].pane, fixture.native.pane_id);
            assert_eq!(patches[0].tokens, fixture.expected);
            // A settled replacement must never be cleared by a late stale
            // registration, including a stale registration that has ended.
            fixture.native.tokens = fixture.expected;
            assert!(plan(&fixture.ledger, "local", &[fixture.native.clone()]).is_empty());
            fixture
                .ledger
                .agents
                .iter_mut()
                .find(|record| record.id == "agent-4")
                .unwrap()
                .ended = true;
            assert!(plan(&fixture.ledger, "local", &[fixture.native]).is_empty());
        }
    }

    #[test]
    fn ended_current_session_registration_retains_its_own_lineage() {
        let mut fixture = replacement_fixture(true);
        fixture
            .ledger
            .agents
            .iter_mut()
            .find(|record| record.id == "agent-3")
            .unwrap()
            .ended = true;
        fixture
            .ledger
            .agents
            .iter_mut()
            .find(|record| record.id == "agent-2")
            .unwrap()
            .ended = true;
        fixture.ledger.validate().unwrap();
        let patches = plan(&fixture.ledger, "local", &[fixture.native.clone()]);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].tokens, fixture.expected);
        fixture.native.tokens = fixture.expected;
        assert!(plan(&fixture.ledger, "local", &[fixture.native]).is_empty());
    }

    #[test]
    fn a_positive_session_with_no_registration_clears_all_historical_lineage_keys() {
        let mut fixture = replacement_fixture(true);
        fixture.native.tokens = fixture.expected;
        fixture
            .native
            .tokens
            .insert("unrelated".into(), json!("keep"));
        fixture.native.lineage_session = crate::wire::session_digest("unregistered-replacement");
        let patches = plan(&fixture.ledger, "local", &[fixture.native.clone()]);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].pane, fixture.native.pane_id);
        assert_eq!(patches[0].tokens.len(), 4);
        assert!(patches[0].tokens.values().all(Value::is_null));
        assert!(!patches[0].tokens.contains_key("unrelated"));
        fixture.native.lineage_session = None;
        assert!(plan(&fixture.ledger, "local", &[fixture.native]).is_empty());
    }

    #[test]
    fn a_current_root_clears_child_history_but_unowned_root_tokens_stay_untouched() {
        let mut fixture = replacement_fixture(true);
        fixture
            .ledger
            .agents
            .iter_mut()
            .find(|record| record.id == "agent-3")
            .unwrap()
            .parent = None;
        fixture
            .ledger
            .agents
            .iter_mut()
            .find(|record| record.id == "agent-3")
            .unwrap()
            .origin = Some("agent-1".into());
        fixture.ledger.validate().unwrap();
        fixture.native.tokens = fixture.expected.clone();
        let untouched = agent(&fixture.ledger.agents[0], fixture.expected);
        let patches = plan(
            &fixture.ledger,
            "local",
            &[fixture.native.clone(), untouched],
        );
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].pane, fixture.native.pane_id);
        assert_eq!(patches[0].tokens.len(), 4);
        assert!(patches[0].tokens.values().all(Value::is_null));
    }
}
