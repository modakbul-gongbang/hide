//! Persistent participant identities and resumable child creation. The delivery
//! store is the sole writer; native I/O is performed by request workers.

mod executor;
pub(crate) mod lineage;

use crate::delivery::{Actor, ledger::Ledger, watch};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) const AGENT_LIMIT: usize = 2048;
pub(crate) const SPAWN_LIMIT: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Register {
        check: bool,
        /// The caller's own machine when given; absent means the machine
        /// the caller's pane capability already names.
        machine: Option<String>,
        host_scope: String,
        session: String,
        instance: String,
        name: String,
        pane: String,
        parent: Option<String>,
        project: Option<String>,
    },
    List,
    Show {
        id: String,
    },
    End {
        id: String,
        actor: Option<String>,
    },
    Spawn {
        parent: String,
        name: String,
        intent: String,
        kind: String,
        repo: String,
        branch: String,
        path: Option<String>,
        no_watch: bool,
        args: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: String,
    pub name: String,
    pub machine: String,
    pub host_scope: String,
    pub native_machine: String,
    pub session: String,
    pub instance: String,
    pub pane: String,
    pub parent: Option<String>,
    pub project: Option<String>,
    pub actor: Actor,
    pub ended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnRecord {
    pub id: String,
    pub parent: String,
    pub intent: String,
    pub name: String,
    pub kind: String,
    pub repo: String,
    pub branch: String,
    pub requested_path: Option<String>,
    pub no_watch: bool,
    pub args: Vec<String>,
    pub path: Option<String>,
    pub pane: Option<String>,
    pub child: Option<String>,
    // Completion and the initial watch are one durable store transaction.
    // Later report/end/watch-stop events never invalidate this receipt.
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub auto_watch_id: Option<String>,
}

pub(crate) enum Mutation {
    Register {
        record: AgentRecord,
        check: bool,
    },
    End {
        id: String,
        actor: Option<String>,
    },
    Reserve {
        parent: String,
        command: Command,
    },
    Advance {
        id: String,
        path: Option<String>,
        pane: Option<String>,
        child: Option<String>,
    },
    BindChild {
        id: String,
        record: AgentRecord,
    },
    Complete {
        id: String,
    },
}

pub fn resolve_actor<'a>(ledger: &'a Ledger, key: &str) -> Option<&'a Actor> {
    let mut found = ledger.agents.iter().filter(|record| {
        !record.ended
            && (record.id == key
                || record.name == key
                || record.actor.pane_id == key
                || record.pane == key)
    });
    let first = found.next()?;
    found.next().is_none().then_some(&first.actor)
}

pub(crate) fn view(record: &AgentRecord, ledger: &Ledger) -> Value {
    json!({"id":record.id,"name":record.name,"machine":record.machine,
        "hostScope":record.host_scope,"session":record.session,"instance":record.instance,
        "pane":record.pane,"parent":record.parent,"project":record.project,
        "runtime":if record.ended { "ended" } else { "running" },
        "connection":if record.ended { "disconnected" } else { "connected" },
        "registered":!record.ended,
        "watch":ledger.watches.iter().find(|watch| watch.target.same_identity(&record.actor))})
}

fn key(value: &str) -> bool {
    crate::delivery::valid_key(value)
}

pub(crate) fn validate_records(ledger: &Ledger) -> Result<(), String> {
    if ledger.agents.len() > AGENT_LIMIT || ledger.spawns.len() > SPAWN_LIMIT {
        return Err("capacity".into());
    }
    let mut ids = std::collections::HashSet::new();
    for record in &ledger.agents {
        if !ids.insert(record.id.as_str())
            || !key(&record.id)
            || !record.id.starts_with("agent-")
            || ![
                &record.name,
                &record.machine,
                &record.host_scope,
                &record.native_machine,
                &record.session,
                &record.instance,
                &record.pane,
            ]
            .into_iter()
            .all(|s| key(s))
            || !record.actor.valid()
            || record.actor.device_id != record.machine
            || record.actor.session != crate::wire::session_digest(&record.session)
            || record.parent.as_ref().is_some_and(|parent| {
                parent == &record.id || !ledger.agents.iter().any(|record| &record.id == parent)
            })
        {
            return Err("ledger_unavailable".into());
        }
    }
    for record in &ledger.spawns {
        if !ids.insert(record.id.as_str())
            || !key(&record.id)
            || !record.id.starts_with("spawn-")
            || ![
                &record.parent,
                &record.intent,
                &record.name,
                &record.kind,
                &record.repo,
                &record.branch,
            ]
            .into_iter()
            .all(|s| key(s))
            || !ledger
                .agents
                .iter()
                .any(|parent| parent.id == record.parent)
            || record.child.as_ref().is_some_and(|id| {
                !ledger.agents.iter().any(|child| {
                    &child.id == id
                        && child.parent.as_ref() == Some(&record.parent)
                        && record.pane.as_ref() == Some(&child.pane)
                })
            })
            || (record.completed
                && (record.path.is_none() || record.pane.is_none() || record.child.is_none()))
            || record.auto_watch_id.as_ref().is_some_and(|id| {
                !record.completed || record.no_watch || !key(id) || !id.starts_with("watch-")
            })
            || record.args.len() > 128
            || record.args.iter().map(String::len).sum::<usize>() > 8192
        {
            return Err("ledger_unavailable".into());
        }
    }
    Ok(())
}

pub(crate) fn apply(
    ledger: &mut Ledger,
    caller: &Actor,
    mutation: &Mutation,
    now: u64,
) -> Result<Value, String> {
    match mutation {
        Mutation::Register { record, check } => {
            // Only a Factory registers under its own reserved name (D-14).
            if (record.actor.code_owned() && !record.actor.same_identity(caller))
                || (!record.actor.code_owned()
                    && (crate::delivery::reserved_name(&record.name)
                        || crate::delivery::reserved_name(&record.pane)))
            {
                return Err("reserved_name".into());
            }
            if record.parent.as_ref().is_some_and(|id| {
                !ledger
                    .agents
                    .iter()
                    .any(|p| &p.id == id && !p.ended && p.actor.same_identity(caller))
            }) || (record.parent.is_none() && !record.actor.same_identity(caller))
            {
                return Err("parent_authority_required".into());
            }
            if let Some(existing) = ledger
                .agents
                .iter()
                .find(|existing| !existing.ended && existing.actor.same_identity(&record.actor))
            {
                if existing.parent != record.parent || existing.name != record.name {
                    return Err("registration_conflict".into());
                }
                return Ok(view(existing, ledger));
            }
            if ledger.agents.iter().any(|existing| {
                !existing.ended
                    && existing.machine == record.machine
                    && existing.host_scope == record.host_scope
                    && existing.name == record.name
            }) {
                return Err("name_in_use".into());
            }
            if *check {
                return Ok(json!({"registered":false,"name":record.name,"pane":record.pane}));
            }
            if ledger.agents.len() >= AGENT_LIMIT {
                return Err("capacity".into());
            }
            let mut record = record.clone();
            record.id = allocate(ledger, "agent")?;
            ledger.agents.push(record.clone());
            Ok(view(&record, ledger))
        }
        Mutation::End { id, actor } => {
            let record = ledger
                .agents
                .iter()
                .find(|record| &record.id == id)
                .ok_or("agent_unavailable")?;
            let parent = record
                .parent
                .as_ref()
                .and_then(|id| resolve_actor(ledger, id));
            if !record.actor.same_identity(caller)
                && !parent.is_some_and(|parent| parent.same_identity(caller))
            {
                return Err("agent_authority_required".into());
            }
            if actor.as_ref().is_some_and(|id| {
                !resolve_actor(ledger, id).is_some_and(|actor| actor.same_identity(caller))
            }) {
                return Err("actor_identity_conflict".into());
            }
            let target = record.actor.clone();
            ledger
                .agents
                .iter_mut()
                .find(|record| &record.id == id)
                .ok_or("agent_unavailable")?
                .ended = true;
            ledger
                .watches
                .retain(|watch| !watch.target.same_identity(&target));
            Ok(view(
                ledger
                    .agents
                    .iter()
                    .find(|record| &record.id == id)
                    .ok_or("agent_unavailable")?,
                ledger,
            ))
        }
        Mutation::Reserve { parent, command } => {
            let Command::Spawn {
                name,
                intent,
                kind,
                repo,
                branch,
                path,
                no_watch,
                args,
                ..
            } = command
            else {
                return Err("invalid_spawn".into());
            };
            if !resolve_actor(ledger, parent).is_some_and(|parent| parent.same_identity(caller)) {
                return Err("parent_authority_required".into());
            }
            if let Some(record) = ledger
                .spawns
                .iter()
                .find(|record| &record.parent == parent && &record.intent == intent)
            {
                if record.name != *name
                    || record.kind != *kind
                    || record.repo != *repo
                    || record.branch != *branch
                    || record.requested_path != *path
                    || record.no_watch != *no_watch
                    || record.args != *args
                {
                    return Err("intent_conflict".into());
                }
                return Ok(json!(record));
            }
            if ledger.spawns.len() >= SPAWN_LIMIT {
                return Err("capacity".into());
            }
            let record = SpawnRecord {
                id: allocate(ledger, "spawn")?,
                parent: parent.clone(),
                intent: intent.clone(),
                name: name.clone(),
                kind: kind.clone(),
                repo: repo.clone(),
                branch: branch.clone(),
                requested_path: path.clone(),
                no_watch: *no_watch,
                args: args.clone(),
                path: None,
                pane: None,
                child: None,
                completed: false,
                auto_watch_id: None,
            };
            ledger.spawns.push(record.clone());
            Ok(json!(record))
        }
        Mutation::Advance {
            id,
            path,
            pane,
            child,
        } => {
            let parent = ledger
                .spawns
                .iter()
                .find(|record| &record.id == id)
                .ok_or("spawn_unavailable")?
                .parent
                .clone();
            if !resolve_actor(ledger, &parent).is_some_and(|parent| parent.same_identity(caller)) {
                return Err("parent_authority_required".into());
            }
            let record = ledger
                .spawns
                .iter_mut()
                .find(|record| &record.id == id)
                .ok_or("spawn_unavailable")?;
            if [
                (record.path.as_ref(), path.as_ref()),
                (record.pane.as_ref(), pane.as_ref()),
                (record.child.as_ref(), child.as_ref()),
            ]
            .into_iter()
            .any(|(stored, next)| {
                stored
                    .zip(next)
                    .is_some_and(|(stored, next)| stored != next)
            }) {
                return Err("intent_conflict".into());
            }
            if path.is_some() {
                record.path = path.clone()
            };
            if pane.is_some() {
                record.pane = pane.clone()
            };
            if child.is_some() {
                record.child = child.clone()
            };
            Ok(json!(record))
        }
        Mutation::BindChild { id, record } => {
            let spawn = ledger
                .spawns
                .iter()
                .find(|spawn| &spawn.id == id)
                .ok_or("spawn_unavailable")?
                .clone();
            if !resolve_actor(ledger, &spawn.parent)
                .is_some_and(|parent| parent.same_identity(caller))
            {
                return Err("parent_authority_required".into());
            }
            if record.parent.as_ref() != Some(&spawn.parent)
                || spawn.pane.as_ref() != Some(&record.pane)
                || record.name != spawn.name
                || record.actor.kind != spawn.kind
                || record.project != spawn.path
            {
                return Err("child_identity_changed".into());
            }
            let existing = if let Some(child) = &spawn.child {
                Some(
                    ledger
                        .agents
                        .iter()
                        .find(|record| &record.id == child)
                        .ok_or("child_unavailable")?,
                )
            } else {
                // Recovery may find a registration installed before an older
                // interrupted binding. Its end state remains authoritative.
                ledger
                    .agents
                    .iter()
                    .find(|old| old.actor.same_identity(&record.actor))
            };
            let child = if let Some(existing) = existing {
                if !existing.actor.same_identity(&record.actor)
                    || existing.parent != record.parent
                    || existing.name != record.name
                {
                    return Err("child_identity_changed".into());
                }
                existing.id.clone()
            } else {
                apply(
                    ledger,
                    caller,
                    &Mutation::Register {
                        record: record.clone(),
                        check: false,
                    },
                    now,
                )?["id"]
                    .as_str()
                    .ok_or("child_unavailable")?
                    .to_owned()
            };
            // Registering and binding the child share the worker's one commit,
            // so a retry cannot lose the ended child's identity between them.
            let spawn = ledger
                .spawns
                .iter_mut()
                .find(|spawn| &spawn.id == id)
                .ok_or("spawn_unavailable")?;
            spawn.child = Some(child);
            Ok(json!(spawn))
        }
        Mutation::Complete { id } => {
            let spawn = ledger
                .spawns
                .iter()
                .find(|spawn| &spawn.id == id)
                .ok_or("spawn_unavailable")?
                .clone();
            let parent = resolve_actor(ledger, &spawn.parent)
                .ok_or("parent_unavailable")?
                .clone();
            if !parent.same_identity(caller) {
                return Err("parent_authority_required".into());
            }
            if spawn.completed {
                return Ok(json!(spawn));
            }
            if spawn.path.is_none() || spawn.pane.is_none() {
                return Err("spawn_incomplete".into());
            }
            let child = spawn
                .child
                .as_ref()
                .and_then(|id| ledger.agents.iter().find(|child| &child.id == id))
                .ok_or("child_unavailable")?
                .clone();
            // A child can finish while its immediate token write is still
            // returning. Confirmed completion remains authoritative even
            // when the initial watch has not been installed yet.
            let reported_complete = ledger.letters.iter().any(|letter| {
                letter.kind == "report"
                    && letter.sender.same_identity(&child.actor)
                    && letter.recipient.same_identity(&parent)
                    && letter.intake_confirmed()
            });
            let auto_watch_id = if !spawn.no_watch && !child.ended && !reported_complete {
                Some(watch::start(ledger, &parent, &child.actor, now)?.id)
            } else {
                None
            };
            let spawn = ledger
                .spawns
                .iter_mut()
                .find(|spawn| &spawn.id == id)
                .ok_or("spawn_unavailable")?;
            spawn.completed = true;
            spawn.auto_watch_id = auto_watch_id;
            Ok(json!(spawn))
        }
    }
}

fn allocate(ledger: &mut Ledger, prefix: &str) -> Result<String, String> {
    let id = ledger.next_id;
    ledger.next_id = id.checked_add(1).ok_or("capacity")?;
    Ok(format!("{prefix}-{id}"))
}

pub(crate) use executor::{link_fork, register_code_owned, run};

#[cfg(test)]
mod tests {
    use super::*;
    fn record(pane: &str, session: &str, parent: Option<String>) -> AgentRecord {
        AgentRecord {
            id: String::new(),
            name: pane.into(),
            machine: "local".into(),
            host_scope: "fixture-scope".into(),
            native_machine: "fixture-machine".into(),
            session: session.into(),
            instance: format!("terminal-{pane}"),
            pane: pane.into(),
            parent,
            project: None,
            actor: Actor {
                pane_id: pane.into(),
                name: pane.into(),
                kind: "codex".into(),
                device_id: "local".into(),
                session: crate::wire::session_digest(session),
            },
            ended: false,
        }
    }
    fn register(ledger: &mut Ledger, record: AgentRecord, caller: &Actor) -> String {
        apply(
            ledger,
            caller,
            &Mutation::Register {
                record,
                check: false,
            },
            1,
        )
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn command(intent: &str) -> Command {
        Command::Spawn {
            parent: "here".into(),
            name: "worker".into(),
            intent: intent.into(),
            kind: "codex".into(),
            repo: "/fixture".into(),
            branch: "topic".into(),
            path: None,
            no_watch: false,
            args: vec!["--model".into(), "fixture model".into()],
        }
    }
    fn register_as(
        ledger: &mut Ledger,
        record: AgentRecord,
        caller: &Actor,
    ) -> Result<Value, String> {
        apply(
            ledger,
            caller,
            &Mutation::Register {
                record,
                check: false,
            },
            1,
        )
    }

    #[test]
    fn only_a_factory_registers_under_a_factory_name() {
        let mut ledger = Ledger::default();
        // A pane named or placed like a Factory is refused.
        let named = record("factory:f-1", "native-impostor", None);
        let caller = named.actor.clone();
        assert_eq!(
            register_as(&mut ledger, named, &caller).unwrap_err(),
            "reserved_name"
        );
        let mut placed = record("pane-1", "native-pane", None);
        placed.name = "factory:f-1".into();
        let caller = placed.actor.clone();
        assert_eq!(
            register_as(&mut ledger, placed, &caller).unwrap_err(),
            "reserved_name"
        );
        // A pane cannot register the Factory's own record either.
        let factory = Actor::factory("f-1");
        let mut owned = record("factory:f-1", "factory:f-1", None);
        owned.actor = factory.clone();
        let pane = record("pane-2", "native-pane-2", None).actor;
        assert_eq!(
            register_as(&mut ledger, owned.clone(), &pane).unwrap_err(),
            "reserved_name"
        );
        // The Factory registers itself, and its worker as its child.
        let id = register(&mut ledger, owned, &factory);
        let worker = record("worker-pane", "native-worker", Some(id.clone()));
        let child = register(&mut ledger, worker, &factory);
        assert_eq!(
            ledger.agents.iter().find(|r| r.id == child).unwrap().parent,
            Some(id)
        );
    }

    #[test]
    fn registration_check_is_read_only_and_execution_identity_replays_after_reload() {
        let mut ledger = Ledger::default();
        let parent = record("parent", "native-parent", None);
        let actor = parent.actor.clone();
        let before = ledger.clone();
        apply(
            &mut ledger,
            &actor,
            &Mutation::Register {
                record: parent.clone(),
                check: true,
            },
            1,
        )
        .unwrap();
        assert_eq!(ledger, before);
        let id = register(&mut ledger, parent.clone(), &actor);
        assert_eq!(resolve_actor(&ledger, &id), Some(&actor));
        let bytes = ledger.bytes().unwrap();
        let mut restored: Ledger = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(register(&mut restored, parent, &actor), id);
        assert_eq!(restored.agents.len(), 1);
        let child = record("child", "native-child", Some(id.clone()));
        let child_id = register(&mut restored, child, &actor);
        assert_eq!(
            restored
                .agents
                .iter()
                .find(|record| record.id == child_id)
                .unwrap()
                .parent,
            Some(id)
        );
    }
    #[test]
    fn a_parent_intent_has_one_durable_child_and_other_parents_have_their_own() {
        let mut ledger = Ledger::default();
        let one = record("one", "native-one", None);
        let caller_one = one.actor.clone();
        let id_one = register(&mut ledger, one, &caller_one);
        let two = record("two", "native-two", None);
        let caller_two = two.actor.clone();
        let id_two = register(&mut ledger, two, &caller_two);
        let reserve = Mutation::Reserve {
            parent: id_one.clone(),
            command: command("same"),
        };
        let first = apply(&mut ledger, &caller_one, &reserve, 2).unwrap();
        let bytes = ledger.bytes().unwrap();
        let mut restored: Ledger = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            apply(&mut restored, &caller_one, &reserve, 3).unwrap(),
            first
        );
        let second = apply(
            &mut restored,
            &caller_two,
            &Mutation::Reserve {
                parent: id_two,
                command: command("same"),
            },
            3,
        )
        .unwrap();
        assert_ne!(first["id"], second["id"]);
        assert_eq!(restored.spawns.len(), 2);
        let id = first["id"].as_str().unwrap().to_owned();
        apply(
            &mut restored,
            &caller_one,
            &Mutation::Advance {
                id: id.clone(),
                path: Some("/fixture/topic".into()),
                pane: Some("child-pane".into()),
                child: None,
            },
            4,
        )
        .unwrap();
        assert_eq!(
            apply(&mut restored, &caller_one, &reserve, 5).unwrap()["pane"],
            "child-pane"
        );
        assert!(
            apply(
                &mut restored,
                &caller_two,
                &Mutation::Advance {
                    id,
                    path: None,
                    pane: Some("wrong".into()),
                    child: None
                },
                5
            )
            .is_err()
        );
    }
    #[test]
    fn ending_a_child_requires_its_parent_and_stops_the_matching_watch() {
        let mut ledger = Ledger::default();
        let parent = record("parent", "native-parent", None);
        let actor = parent.actor.clone();
        let id = register(&mut ledger, parent, &actor);
        let child = record("child", "native-child", Some(id.clone()));
        let child_id = register(&mut ledger, child, &actor);
        let target = resolve_actor(&ledger, &child_id).unwrap().clone();
        watch::start(&mut ledger, &actor, &target, 2).unwrap();
        assert_eq!(ledger.watches.len(), 1);
        let stranger = record("stranger", "native-stranger", None).actor;
        assert!(
            apply(
                &mut ledger,
                &stranger,
                &Mutation::End {
                    id: child_id.clone(),
                    actor: None
                },
                3
            )
            .is_err()
        );
        apply(
            &mut ledger,
            &actor,
            &Mutation::End {
                id: child_id.clone(),
                actor: Some(id),
            },
            3,
        )
        .unwrap();
        assert!(ledger.watches.is_empty());
        assert!(resolve_actor(&ledger, &child_id).is_none());
        assert!(
            ledger
                .agents
                .iter()
                .find(|record| record.id == child_id)
                .unwrap()
                .ended
        );
    }
    #[test]
    fn changed_intent_payload_or_invalid_persisted_session_is_refused() {
        let mut ledger = Ledger::default();
        let parent = record("parent", "session", None);
        let actor = parent.actor.clone();
        let id = register(&mut ledger, parent, &actor);
        apply(
            &mut ledger,
            &actor,
            &Mutation::Reserve {
                parent: id.clone(),
                command: command("key"),
            },
            1,
        )
        .unwrap();
        let mut changed = command("key");
        if let Command::Spawn { no_watch, .. } = &mut changed {
            *no_watch = true;
        }
        assert_eq!(
            apply(
                &mut ledger,
                &actor,
                &Mutation::Reserve {
                    parent: id,
                    command: changed
                },
                1
            )
            .unwrap_err(),
            "intent_conflict"
        );
        ledger.agents[0].session = "replaced".into();
        assert!(ledger.bytes().is_err());
    }
    fn pending_spawn() -> (Ledger, Actor, String, String, AgentRecord) {
        let mut ledger = Ledger::default();
        let parent = record("parent", "native-parent", None);
        let actor = parent.actor.clone();
        let parent_id = register(&mut ledger, parent, &actor);
        let reserved = apply(
            &mut ledger,
            &actor,
            &Mutation::Reserve {
                parent: parent_id.clone(),
                command: command("once"),
            },
            1,
        )
        .unwrap();
        let id = reserved["id"].as_str().unwrap().to_owned();
        apply(
            &mut ledger,
            &actor,
            &Mutation::Advance {
                id: id.clone(),
                path: Some("/fixture/topic".into()),
                pane: Some("worker".into()),
                child: None,
            },
            2,
        )
        .unwrap();
        let mut child = record("worker", "native-child", Some(parent_id.clone()));
        child.project = Some("/fixture/topic".into());
        (ledger, actor, parent_id, id, child)
    }

    #[test]
    fn interrupted_child_binding_preserves_an_end_and_completion_receipt_after_reload() {
        let (mut ledger, actor, parent, spawn, child) = pending_spawn();
        let bind = Mutation::BindChild {
            id: spawn.clone(),
            record: child,
        };
        let bound = apply(&mut ledger, &actor, &bind, 3).unwrap();
        let child = bound["child"].as_str().unwrap().to_owned();
        apply(
            &mut ledger,
            &actor,
            &Mutation::End {
                id: child.clone(),
                actor: Some(parent),
            },
            4,
        )
        .unwrap();
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        let next_id = restored.next_id;
        assert_eq!(
            apply(&mut restored, &actor, &bind, 5).unwrap()["child"],
            child
        );
        let complete = Mutation::Complete { id: spawn };
        let receipt = apply(&mut restored, &actor, &complete, 6).unwrap();
        assert_eq!(receipt["completed"], true);
        assert!(receipt["auto_watch_id"].is_null());
        assert!(resolve_actor(&restored, &child).is_none());
        assert!(restored.watches.is_empty());
        assert_eq!(restored.agents.len(), 2);
        assert_eq!(restored.next_id, next_id);
        let mut restored: Ledger = serde_json::from_slice(&restored.bytes().unwrap()).unwrap();
        let before = restored.clone();
        assert_eq!(apply(&mut restored, &actor, &complete, 7).unwrap(), receipt);
        assert_eq!(restored, before);
    }

    #[test]
    fn completion_and_initial_watch_commit_together_and_failure_is_resumable() {
        let (mut ledger, actor, _, spawn, child) = pending_spawn();
        apply(
            &mut ledger,
            &actor,
            &Mutation::BindChild {
                id: spawn.clone(),
                record: child,
            },
            3,
        )
        .unwrap();
        for index in 0..crate::delivery::WATCH_LIMIT {
            let target = record(&format!("other-{index}"), "session", None).actor;
            watch::start(&mut ledger, &actor, &target, 4).unwrap();
        }
        let complete = Mutation::Complete { id: spawn };
        let before = ledger.clone();
        assert_eq!(
            apply(&mut ledger, &actor, &complete, 5).unwrap_err(),
            "capacity"
        );
        assert_eq!(ledger, before);
        let released = ledger.watches[0].id.clone();
        watch::stop(&mut ledger, &actor, &released).unwrap();
        let receipt = apply(&mut ledger, &actor, &complete, 6).unwrap();
        let id = receipt["auto_watch_id"].as_str().unwrap();
        assert_eq!(receipt["completed"], true);
        assert!(ledger.watches.iter().any(|watch| watch.id == id));
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        watch::stop(&mut restored, &actor, id).unwrap();
        let before = restored.clone();
        assert_eq!(apply(&mut restored, &actor, &complete, 7).unwrap(), receipt);
        assert_eq!(restored, before);
    }
    #[test]
    fn report_confirmed_before_spawn_completion_prevents_initial_watch_installation() {
        use crate::delivery::mailbox;
        for acknowledged in [false, true] {
            let (mut ledger, actor, _, spawn, child) = pending_spawn();
            let child_actor = child.actor.clone();
            apply(
                &mut ledger,
                &actor,
                &Mutation::BindChild {
                    id: spawn.clone(),
                    record: child,
                },
                3,
            )
            .unwrap();
            let report = mailbox::send(
                &mut ledger,
                &child_actor,
                &actor,
                "done",
                "completed",
                "report",
                None,
                4,
            )
            .unwrap();
            mailbox::apply(
                &mut ledger,
                &actor,
                None,
                &mailbox::Command::Confirm {
                    ids: vec![report.id.clone()],
                },
                5,
            )
            .unwrap();
            if acknowledged {
                mailbox::apply(
                    &mut ledger,
                    &actor,
                    None,
                    &mailbox::Command::Ack { id: report.id },
                    6,
                )
                .unwrap();
            }
            let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
            let complete = Mutation::Complete { id: spawn };
            let receipt = apply(&mut restored, &actor, &complete, 7).unwrap();
            assert!(restored.watches.is_empty());
            assert_eq!(receipt["completed"], true);
            assert!(receipt["auto_watch_id"].is_null());
            let before = restored.clone();
            assert_eq!(apply(&mut restored, &actor, &complete, 8).unwrap(), receipt);
            assert_eq!(restored, before);
            // Only an explicit watch start opens a fresh observation after a
            // confirmed report; completing this intent does not remove it.
            let watch = watch::start(&mut restored, &actor, &child_actor, 9).unwrap();
            apply(&mut restored, &actor, &complete, 10).unwrap();
            assert_eq!(restored.watches[0].id, watch.id);
        }
    }

    #[test]
    fn pending_report_installs_initial_watch_then_confirmation_closes_it() {
        use crate::delivery::mailbox;
        let (mut ledger, actor, _, spawn, child) = pending_spawn();
        let child_actor = child.actor.clone();
        apply(
            &mut ledger,
            &actor,
            &Mutation::BindChild {
                id: spawn.clone(),
                record: child,
            },
            3,
        )
        .unwrap();
        let report = mailbox::send(
            &mut ledger,
            &child_actor,
            &actor,
            "done",
            "completed",
            "report",
            None,
            4,
        )
        .unwrap();
        let complete = Mutation::Complete { id: spawn };
        let receipt = apply(&mut ledger, &actor, &complete, 5).unwrap();
        assert_eq!(ledger.watches.len(), 1);
        assert_eq!(receipt["auto_watch_id"], ledger.watches[0].id);
        mailbox::apply(
            &mut ledger,
            &actor,
            None,
            &mailbox::Command::Confirm {
                ids: vec![report.id],
            },
            6,
        )
        .unwrap();
        assert!(ledger.watches.is_empty());
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        let before = restored.clone();
        assert_eq!(apply(&mut restored, &actor, &complete, 7).unwrap(), receipt);
        assert_eq!(restored, before);
    }

    #[test]
    fn ack_only_report_installs_initial_watch_until_actual_confirmation_and_replays_once() {
        use crate::delivery::mailbox;
        for legacy in [false, true] {
            let (mut ledger, parent, _, spawn, child) = pending_spawn();
            let child_actor = child.actor.clone();
            apply(
                &mut ledger,
                &parent,
                &Mutation::BindChild {
                    id: spawn.clone(),
                    record: child,
                },
                3,
            )
            .unwrap();
            let report = mailbox::send(
                &mut ledger,
                &child_actor,
                &parent,
                "done",
                "completed",
                "report",
                None,
                4,
            )
            .unwrap();
            let intake = mailbox::pull(&ledger, &parent).unwrap();
            assert_eq!(intake.ids.as_slice(), std::slice::from_ref(&report.id));
            assert_eq!(
                mailbox::apply(
                    &mut ledger,
                    &parent,
                    None,
                    &mailbox::Command::Ack {
                        id: report.id.clone()
                    },
                    5,
                )
                .unwrap()["state"],
                "acknowledged"
            );
            if legacy {
                ledger.letters[0].hook_confirmed = None;
                ledger.letters[0].finished_at_unix_ms = Some(5);
            }
            let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
            let complete = Mutation::Complete { id: spawn };
            let receipt = apply(&mut restored, &parent, &complete, 6).unwrap();
            assert_eq!(restored.watches.len(), 1);
            assert_eq!(receipt["auto_watch_id"], restored.watches[0].id);
            let confirm = mailbox::Command::Confirm { ids: intake.ids };
            mailbox::apply(&mut restored, &parent, None, &confirm, 7).unwrap();
            assert!(restored.watches.is_empty());
            let rearmed = watch::start(&mut restored, &parent, &child_actor, 8).unwrap();
            let mut restored: Ledger = serde_json::from_slice(&restored.bytes().unwrap()).unwrap();
            let before = restored.clone();
            assert_eq!(
                apply(&mut restored, &parent, &complete, 9).unwrap(),
                receipt
            );
            mailbox::apply(&mut restored, &parent, None, &confirm, 10).unwrap();
            assert_eq!(restored, before);
            assert_eq!(restored.watches[0].id, rearmed.id);
        }
    }
}
