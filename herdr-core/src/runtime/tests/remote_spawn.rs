//! `hide agent spawn --machine` (PRD agent-spawn-machine B1-B10): a caller on
//! this machine makes its child on a connected device, the caller keeping the
//! parent record, the authority and the watch.
//!
//! The device is a Herdr socket fake and a node double; this machine has no
//! Herdr at all, so any child-side call that fell back to the caller's own
//! machine would fail the spawn instead of passing unseen.

use super::*;
use crate::coordination::{AgentRecord, Command, split_refusal};
use crate::delivery::{Actor, ledger, worker::Worker};
use crate::fake_herdr::FakeHerdr;
use crate::handle::ChangeNotifier;
use crate::node_access::{LinkAnswer, LinkError, NodeLink};
use crate::runtime::delivery::tests::{authority, fixture};
use hide_node_link::protocol::Call;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const DEVICE: &str = "mini";
const WORKTREE: &str = "/srv/repo.worktrees/topic";

/// A device node that answers the two checks a spawn makes before it creates
/// anything.
struct Node {
    repository: AtomicBool,
    installed: AtomicBool,
    /// How many checks the spawn has asked this node.
    calls: AtomicUsize,
}

impl NodeLink for Node {
    fn call(&self, call: Call, _timeout: Duration) -> Result<LinkAnswer, LinkError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(LinkAnswer::Parsed(match call {
            Call::Repository { .. } if self.repository.load(Ordering::SeqCst) => {
                json!({"root": "/srv/repo", "git_dir": "/srv/repo/.git",
                       "common_dir": "/srv/repo/.git"})
            }
            Call::Repository { .. } => Value::Null,
            Call::AgentInstalled { name } => {
                assert_eq!(name, "codex");
                json!(self.installed.load(Ordering::SeqCst))
            }
            other => panic!("unexpected call {other:?}"),
        }))
    }
}

fn agent_row(session: bool) -> Value {
    let mut row = json!({"pane_id": "w2:p1", "workspace_id": "w2", "tab_id": "w2:t1",
        "terminal_id": "device-terminal", "revision": 1, "focused": false,
        "agent_status": "idle", "agent": "codex", "name": "worker"});
    if session {
        row["agent_session"] = json!({"source": "herdr:codex", "agent": "codex",
            "kind": "id", "value": "device-child-session"});
    }
    row
}

/// The device's Herdr: no worktree yet, one created on request, an agent that
/// exists once it is started.
fn device_herdr(started: Arc<AtomicBool>, identified: Arc<AtomicBool>) -> FakeHerdr {
    FakeHerdr::start("remote-spawn", move |method, _| match method {
        "worktree.list" => json!({"type": "worktree_list", "source": {
            "repo_key": "/srv/repo/.git", "repo_name": "repo",
            "repo_root": "/srv/repo", "source_checkout_path": "/srv/repo",
            "source_workspace_id": "w1"}, "worktrees": []}),
        "worktree.create" => json!({
            "type": "worktree_created",
            "workspace": {"workspace_id": "w2", "label": "repo", "number": 2, "focused": false,
                "pane_count": 1, "tab_count": 1, "active_tab_id": "w2:t1", "agent_status": "idle"},
            "tab": {"workspace_id": "w2", "tab_id": "w2:t1", "label": "hide", "number": 1,
                "focused": false, "pane_count": 1, "agent_status": "idle"},
            "root_pane": {"workspace_id": "w2", "tab_id": "w2:t1", "pane_id": "w2:p1",
                "terminal_id": "device-terminal", "cwd": WORKTREE, "focused": false,
                "agent_status": "idle", "revision": 0},
            "worktree": {"branch": "topic", "is_bare": false, "is_detached": false,
                "is_linked_worktree": true, "is_prunable": false, "label": "repo",
                "open_workspace_id": "w2", "path": WORKTREE}
        }),
        "pane.process_info" => json!({"type": "pane_process_info", "process_info": {
            "pane_id": "w2:p1", "shell_pid": 4100, "foreground_process_group_id": 4100,
            "foreground_processes": [{"pid": 4100, "name": "zsh"}]}}),
        "agent.start" => {
            started.store(true, Ordering::SeqCst);
            json!({"type": "agent_started", "argv": [], "agent": agent_row(false)})
        }
        "agent.list" => json!({"type": "agent_list", "agents": if started.load(Ordering::SeqCst) {
            vec![agent_row(identified.load(Ordering::SeqCst))]
        } else {
            Vec::new()
        }}),
        "pane.report_metadata" => json!({"type": "ok"}),
        other => panic!("unexpected {other}"),
    })
}

struct Spawner {
    _turn: std::sync::MutexGuard<'static, ()>,
    _root: tempfile::TempDir,
    runtime: Arc<Mutex<Runtime>>,
    actor: Actor,
    node: Arc<Node>,
    /// Whether the device's agent has reported its native session yet.
    identified: Arc<AtomicBool>,
    herdr: FakeHerdr,
    path: PathBuf,
    _worker: Worker,
    client: crate::delivery::worker::Client,
}

/// A caller registered on this machine and a connected device `mini`; a
/// second registered device, `studio`, is not connected.
fn spawner() -> Spawner {
    let turn = crate::coordination::SPAWN_TURN
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (runtime, mut actor, _, path) = fixture(root.path());
    // The ledger keeps the digest of the native session, which is what the
    // pane's observation must carry for the caller to be that participant.
    actor.session = crate::wire::session_digest("native-parent");
    let payload: crate::sidebar::SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
        {"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working",
            "state_change_seq":1,"lineage_session":actor.session},
    ]}))
    .unwrap();
    runtime
        .lock()
        .unwrap()
        .observe_delivery(crate::node::TEST_NODE, &payload, None, None);
    let started = Arc::new(AtomicBool::new(false));
    let identified = Arc::new(AtomicBool::new(true));
    let herdr = device_herdr(started, identified.clone());
    let node = Arc::new(Node {
        repository: AtomicBool::new(true),
        installed: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    });
    {
        let mut guard = runtime.lock().unwrap();
        for id in [DEVICE, "studio"] {
            guard
                .snapshot
                .ui_state
                .device_registrations
                .push(crate::model::DeviceRegistration {
                    id: id.to_owned(),
                    label: id.to_owned(),
                    ..Default::default()
                });
        }
        guard.snapshot.status.remote.push(RemoteStatusSnapshot {
            target_id: DEVICE.to_owned(),
            state: "connected".to_owned(),
            message: None,
            herdr_version: None,
            session: None,
            files: RemoteFileListSnapshot::idle(),
            catalog: Default::default(),
        });
        guard
            .device_machine_ids
            .insert(DEVICE.to_owned(), "machine-mini".to_owned());
        guard.ingest_kit_report(
            DEVICE,
            &hide_kit::KitReport {
                codex_daemon: Some(true),
                ..Default::default()
            },
        );
        guard.device_hosts.insert(
            DEVICE.to_owned(),
            hosts::DeviceHost {
                phase: hosts::HostPhase::Ready {
                    host: node.clone(),
                    platform: "macos aarch64".to_owned(),
                    helper_path: "/fake/hided".to_owned(),
                },
                generation: 1,
            },
        );
        guard.install_remote_control(RemoteControlContext::new(
            DEVICE,
            Arc::new(herdr.connector()),
            Arc::downgrade(&runtime),
            ChangeNotifier::noop(),
        ));
    }
    // The caller is already a registered participant, so nothing of this
    // machine's Herdr is needed to name it as the parent.
    let mut state = ledger::Ledger::default();
    crate::coordination::apply(
        &mut state,
        &actor,
        &crate::coordination::Mutation::Register {
            record: AgentRecord {
                id: String::new(),
                name: "sender".into(),
                machine: crate::node::TEST_NODE.into(),
                host_scope: "fixture".into(),
                native_machine: "machine-local".into(),
                session: "native-parent".into(),
                instance: "sender".into(),
                pane: "sender".into(),
                parent: None,
                origin: None,
                project: None,
                actor: actor.clone(),
                ended: false,
            },
            check: false,
        },
        1,
    )
    .unwrap();
    ledger::save(&path, &state).unwrap();
    runtime
        .lock()
        .unwrap()
        .publish_delivery(Arc::new(ledger::load(&path).unwrap()), false);
    let (worker, client) = Worker::spawn(
        Arc::downgrade(&runtime),
        ChangeNotifier::noop(),
        path.clone(),
    )
    .unwrap();
    Spawner {
        _turn: turn,
        _root: root,
        runtime,
        actor,
        node,
        identified,
        herdr,
        path,
        _worker: worker,
        client,
    }
}

impl Spawner {
    fn spawn(
        &self,
        parent: Option<&str>,
        machine: Option<&str>,
        intent: &str,
    ) -> Result<Value, String> {
        crate::coordination::run(
            self.client.clone(),
            authority(&self.actor),
            self.actor.clone(),
            Command::Spawn {
                parent: parent.map(str::to_owned),
                machine: machine.map(str::to_owned),
                name: "worker".into(),
                intent: intent.into(),
                kind: "codex".into(),
                repo: "/srv/repo".into(),
                branch: "topic".into(),
                path: None,
                args: Vec::new(),
            },
        )
    }

    fn ledger(&self) -> ledger::Ledger {
        ledger::load(&self.path).unwrap()
    }

    fn count(&self, method: &str) -> usize {
        self.herdr
            .methods()
            .iter()
            .filter(|seen| seen.as_str() == method)
            .count()
    }
}

/// B1, B2, B3, B7 (the token's `parent_machine`): a delegated child is made on
/// the device, belongs to the caller and is watched; the lineage tokens are
/// written on the device's pane and name this machine as the parent's.
#[test]
fn a_delegated_spawn_makes_the_child_on_the_device_under_the_caller() {
    let spawner = spawner();
    let child = spawner.spawn(Some("here"), Some(DEVICE), "one").unwrap();
    assert_eq!(child["machine"], DEVICE);
    assert_eq!(child["pane"], "w2:p1");
    assert_eq!(child["project"], WORKTREE);
    assert_eq!(child["parent"], "agent-1");
    assert_eq!(child["watch"]["parent"]["pane_id"], "sender");
    assert_eq!(spawner.count("worktree.create"), 1);
    assert_eq!(spawner.count("agent.start"), 1);
    let ledger = spawner.ledger();
    let record = ledger
        .agents
        .iter()
        .find(|r| Some(r.id.as_str()) == child["id"].as_str())
        .unwrap();
    assert_eq!(record.actor.device_id, DEVICE);
    assert_eq!(record.actor.pane_id, "remote:mini:pane:w2:p1");
    assert_eq!(ledger.spawns[0].machine.as_deref(), Some(DEVICE));
    // The token goes to the device's pane and names the caller's machine.
    let tokens = spawner
        .herdr
        .calls()
        .into_iter()
        .find(|(method, _)| method == "pane.report_metadata")
        .expect("lineage tokens written on the device")
        .1["tokens"]
        .clone();
    assert_eq!(tokens["parent_pane"], "sender");
    assert_eq!(tokens["parent_machine"], "machine-local");
}

/// B4: without `--parent` the device's agent is the operator's, unwatched,
/// and records the caller as its origin.
#[test]
fn a_handoff_to_a_device_is_the_operators_root_with_the_caller_as_origin() {
    let spawner = spawner();
    let child = spawner.spawn(None, Some(DEVICE), "handoff").unwrap();
    assert_eq!(child["machine"], DEVICE);
    assert_eq!(child["parent"], Value::Null);
    assert_eq!(child["origin"], "agent-1");
    assert_eq!(child["watch"], Value::Null);
}

/// B5: the same intent returns the same child and makes nothing twice, and
/// the same intent on another machine, or none, is refused.
#[test]
fn the_machine_is_part_of_the_intent() {
    let spawner = spawner();
    let first = spawner.spawn(Some("here"), Some(DEVICE), "one").unwrap();
    let again = spawner.spawn(Some("here"), Some(DEVICE), "one").unwrap();
    assert_eq!(first["id"], again["id"]);
    assert_eq!(spawner.count("worktree.create"), 1);
    assert_eq!(spawner.count("agent.start"), 1);
    // Even with the device gone, the finished spawn answers from its receipt.
    spawner.runtime.lock().unwrap().snapshot.status.remote[0].state = "not_connected".into();
    assert_eq!(
        spawner.spawn(Some("here"), Some(DEVICE), "one").unwrap()["id"],
        first["id"]
    );
    for machine in [Some("studio"), Some("ghost"), None] {
        assert_eq!(
            spawner.spawn(Some("here"), machine, "one"),
            Err("intent_conflict".into()),
            "{machine:?}"
        );
    }
    assert_eq!(spawner.ledger().spawns.len(), 1);
}

/// B6-B9: a device that cannot take the spawn is refused with a stable code
/// and nothing is created: no spawn record, no call to the device's Herdr.
#[test]
fn a_device_that_cannot_take_the_spawn_is_refused_before_anything_is_made() {
    let spawner = spawner();
    let refused = |spawner: &Spawner, machine: &str, intent: &str| {
        let error = spawner
            .spawn(Some("here"), Some(machine), intent)
            .expect_err("refused");
        let (code, detail) = split_refusal(&error);
        (code.to_owned(), detail.map(str::to_owned))
    };
    // Only a device the caller could have named is listed: the caller's own
    // machine is not, and neither is one that is not connected.
    assert_eq!(
        refused(&spawner, "ghost", "unknown"),
        ("machine_unknown".into(), Some(DEVICE.into()))
    );
    assert_eq!(
        refused(&spawner, "studio", "offline"),
        ("machine_unavailable".into(), None)
    );
    spawner.node.repository.store(false, Ordering::SeqCst);
    assert_eq!(
        refused(&spawner, DEVICE, "no-repository"),
        ("repository_unavailable".into(), None)
    );
    spawner.node.repository.store(true, Ordering::SeqCst);
    spawner.node.installed.store(false, Ordering::SeqCst);
    assert_eq!(
        refused(&spawner, DEVICE, "no-agent"),
        ("agent_not_installed".into(), None)
    );
    assert!(spawner.ledger().spawns.is_empty());
    assert!(spawner.herdr.methods().is_empty());
    // The refusals reserved nothing, so the same intent works once the device
    // can take it.
    spawner.node.installed.store(true, Ordering::SeqCst);
    assert!(
        spawner
            .spawn(Some("here"), Some(DEVICE), "no-agent")
            .is_ok()
    );
}

/// B10: this machine's own id is the same as no `--machine`, so it never
/// reaches the device checks and the intent reads the same either way.
#[test]
fn the_callers_own_machine_is_no_machine() {
    let spawner = spawner();
    // This machine has no Herdr here, so the spawn stops at its own context;
    // what matters is that it was judged as a local spawn and not as a device.
    let own = spawner.spawn(Some("here"), Some(crate::node::TEST_NODE), "local");
    let none = spawner.spawn(Some("here"), None, "local");
    assert_eq!(own, none);
    assert!(own.is_err());
    assert_eq!(
        spawner.ledger().spawns[0].machine,
        None,
        "the reservation is the local one"
    );
}

/// B5: a spawn that stopped after it created the pane resumes on the same
/// intent, needing only the device's Herdr, and makes nothing twice.
#[test]
fn a_spawn_that_stopped_after_its_pane_resumes_without_the_device_checks() {
    let spawner = spawner();
    spawner.identified.store(false, Ordering::SeqCst);
    let error = spawner
        .spawn(Some("here"), Some(DEVICE), "resume")
        .expect_err("the agent never reported its session");
    assert_eq!(error, "native_identity_unavailable");
    let before = spawner.herdr.methods();
    assert_eq!(count(&before, "worktree.create"), 1);
    assert_eq!(count(&before, "agent.start"), 1);
    // The repository and the agent were used by the first attempt; the retry
    // does not ask the node about them again.
    spawner.node.repository.store(false, Ordering::SeqCst);
    spawner.node.installed.store(false, Ordering::SeqCst);
    spawner.identified.store(true, Ordering::SeqCst);
    let answer = spawner
        .spawn(Some("here"), Some(DEVICE), "resume")
        .expect("the retry finishes the same spawn");
    let after = spawner.herdr.methods();
    assert_eq!(count(&after, "worktree.create"), 1, "{after:?}");
    assert_eq!(count(&after, "agent.start"), 1, "{after:?}");
    assert_eq!(spawner.ledger().spawns.len(), 1);
    assert_eq!(
        spawner.ledger().spawns[0].child.as_deref(),
        answer["id"].as_str()
    );
}

fn count(methods: &[String], name: &str) -> usize {
    methods.iter().filter(|method| *method == name).count()
}

/// Another agent's id as the parent is judged before any device is probed, so
/// a device's answer never reaches a caller with no authority over it.
#[test]
fn an_unowned_parent_is_refused_before_the_device_is_asked() {
    let spawner = spawner();
    spawner.node.repository.store(false, Ordering::SeqCst);
    let error = spawner
        .spawn(Some("agent-999"), Some("ghost"), "not-mine")
        .expect_err("refused");
    assert_eq!(error, "parent_authority_required");
    assert!(spawner.ledger().spawns.is_empty());
    assert!(spawner.herdr.methods().is_empty());
}

/// An agent on a device cannot start work on this machine or on a third one.
#[test]
fn a_caller_on_a_device_may_name_no_other_device() {
    let spawner = spawner();
    let mut runtime = spawner.runtime.lock().unwrap();
    for target in [crate::node::TEST_NODE, "studio", "ghost"] {
        assert_eq!(
            runtime.spawn_target(DEVICE, target).err().as_deref(),
            Some("machine_not_permitted"),
            "{target}"
        );
    }
    assert!(runtime.spawn_target(crate::node::TEST_NODE, DEVICE).is_ok());
}

/// A device whose Herdr stops answering after the node passed the checks is
/// unavailable, not a raw transport error, and the same command continues
/// the spawn once the device answers again.
#[test]
fn a_device_herdr_that_stops_answering_is_unavailable_and_the_retry_converges() {
    let spawner = spawner();
    let install = |connector: Arc<dyn hide_herdr_client::ApiConnector>| {
        spawner
            .runtime
            .lock()
            .unwrap()
            .install_remote_control(RemoteControlContext::new(
                DEVICE,
                connector,
                Arc::downgrade(&spawner.runtime),
                ChangeNotifier::noop(),
            ));
    };
    install(Arc::new(hide_herdr_client::LocalSocketConnector::new(
        "/tmp/hide-remote-spawn-no-herdr.sock",
    )));
    assert_eq!(
        spawner
            .spawn(Some("here"), Some(DEVICE), "down")
            .expect_err("the device Herdr is down"),
        "machine_unavailable"
    );
    let reserved = spawner.ledger().spawns;
    assert_eq!(reserved.len(), 1);
    assert!(reserved[0].pane.is_none() && reserved[0].child.is_none());
    install(Arc::new(spawner.herdr.connector()));
    let answer = spawner
        .spawn(Some("here"), Some(DEVICE), "down")
        .expect("the retry continues the same spawn");
    let ledger = spawner.ledger();
    assert_eq!(ledger.spawns.len(), 1);
    assert_eq!(ledger.spawns[0].child.as_deref(), answer["id"].as_str());
    assert_eq!(count(&spawner.herdr.methods(), "worktree.create"), 1);
}

/// An agent on a device is refused by the spawn itself, before the intent is
/// reserved and before anything is asked of any Herdr.
#[test]
fn a_spawn_by_a_device_caller_is_refused_before_anything_is_made() {
    let spawner = spawner();
    let mut actor = spawner.actor.clone();
    actor.device_id = DEVICE.into();
    for machine in ["studio", crate::node::TEST_NODE] {
        let error = crate::coordination::run(
            spawner.client.clone(),
            authority(&actor),
            actor.clone(),
            Command::Spawn {
                parent: Some("here".into()),
                machine: Some(machine.into()),
                name: "worker".into(),
                intent: "from-device".into(),
                kind: "codex".into(),
                repo: "/srv/repo".into(),
                branch: "topic".into(),
                path: None,
                args: Vec::new(),
            },
        )
        .expect_err("refused");
        assert_eq!(error, "machine_not_permitted", "{machine}");
    }
    assert!(spawner.ledger().spawns.is_empty());
    assert!(spawner.herdr.methods().is_empty());
    assert_eq!(spawner.node.calls.load(Ordering::SeqCst), 0);
}

/// The device checks only read, so they run before the process-wide lock: a
/// spawn that finds the lock held has already judged the device, creates
/// nothing, and the same command works once the lock is free.
#[test]
fn a_busy_spawn_lock_refuses_after_the_checks_and_creates_nothing() {
    let spawner = spawner();
    {
        let _held = crate::coordination::SPAWN
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            spawner
                .spawn(Some("here"), Some(DEVICE), "busy")
                .expect_err("busy"),
            "spawn_busy"
        );
    }
    assert!(spawner.node.calls.load(Ordering::SeqCst) > 0);
    assert!(spawner.ledger().spawns.is_empty());
    assert!(spawner.herdr.methods().is_empty());
    assert!(spawner.spawn(Some("here"), Some(DEVICE), "busy").is_ok());
}
