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
use std::sync::atomic::{AtomicBool, Ordering};

const DEVICE: &str = "mini";
const WORKTREE: &str = "/srv/repo.worktrees/topic";

/// A device node that answers the two checks a spawn makes before it creates
/// anything.
struct Node {
    repository: AtomicBool,
    installed: AtomicBool,
}

impl NodeLink for Node {
    fn call(&self, call: Call, _timeout: Duration) -> Result<LinkAnswer, LinkError> {
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
fn device_herdr(started: Arc<AtomicBool>) -> FakeHerdr {
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
            vec![agent_row(true)]
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
    let herdr = device_herdr(started);
    let node = Arc::new(Node {
        repository: AtomicBool::new(true),
        installed: AtomicBool::new(true),
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
