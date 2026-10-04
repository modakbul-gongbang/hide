use super::{AgentRecord, Command, Mutation, SpawnRecord, resolve_actor, view};
use crate::delivery::{
    Actor,
    worker::{Authority, Client, Effect},
};
use crate::session_sync::ProjectedAgent;
use hide_herdr_client::{ApiConnector, request_with_connector};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const STORE_TIMEOUT: Duration = Duration::from_secs(5);
// Finite side effects serialize without occupying the durable store or Runtime.
// A concurrent spawn is refused instead of accumulating blocked workers.
static SPAWN: Mutex<()> = Mutex::new(());
fn state(client: &Client) -> Result<Arc<crate::delivery::ledger::Ledger>, String> {
    client
        .runtime
        .upgrade()
        .ok_or("delivery_unavailable")?
        .lock()
        .map_err(|_| "delivery_unavailable")?
        .delivery_state()
}
fn mutate(
    client: &Client,
    authority: &Authority,
    actor: &Actor,
    mutation: Mutation,
) -> Result<Value, String> {
    client.submit(
        Effect::Agents {
            authority: Authority {
                caller: authority.caller.clone(),
                context: authority.context.clone(),
            },
            actor: actor.clone(),
            mutation,
        },
        STORE_TIMEOUT,
    )
}
fn context(
    client: &Client,
    device: &str,
) -> Result<
    (
        Arc<dyn ApiConnector>,
        String,
        String,
        crate::codex_launch::CodexDaemon,
    ),
    String,
> {
    client
        .runtime
        .upgrade()
        .ok_or("delivery_unavailable")?
        .lock()
        .map_err(|_| "delivery_unavailable")?
        .coordination_context(device)
}
fn agents(connector: &dyn ApiConnector) -> Result<Vec<ProjectedAgent>, String> {
    agents_with_timeout(connector, Duration::from_secs(2))
}
fn agents_with_timeout(
    connector: &dyn ApiConnector,
    timeout: Duration,
) -> Result<Vec<ProjectedAgent>, String> {
    let result = request_with_connector(
        connector,
        "agent.list",
        crate::wire::empty_params(),
        timeout,
    )
    .map_err(|error| format!("{error}"))?;
    crate::wire::agents_response(result).map_err(|_| "native_identity_unavailable".into())
}
fn wait_native_identity(
    connector: &dyn ApiConnector,
    pane: &str,
    name: Option<&str>,
    kind: &str,
) -> Result<ProjectedAgent, String> {
    // The public socket start acknowledges the typed command before the
    // process and its native session are detected. Wait for that observation,
    // never send another start, and keep every read within one finite deadline.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("native_identity_unavailable".into());
        }
        let observed = agents_with_timeout(connector, remaining.min(Duration::from_secs(2)))?;
        if let Some(agent) = observed.into_iter().find(|agent| {
            agent.pane_id == pane
                && name.is_none_or(|name| agent.name.as_deref() == Some(name))
                && agent.agent.as_deref() == Some(kind)
                && agent.lineage_session.is_some()
                && agent.agent_session.is_some()
        }) {
            return Ok(agent);
        }
        std::thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100)),
        );
    }
}
fn actor_for(agent: &ProjectedAgent, device: &str) -> Result<Actor, String> {
    let actor = Actor {
        pane_id: if device == "local" {
            agent.pane_id.clone()
        } else {
            format!("remote:{device}:pane:{}", agent.pane_id)
        },
        name: agent.name.clone().unwrap_or_else(|| agent.pane_id.clone()),
        kind: agent.agent.clone().ok_or("native_identity_unavailable")?,
        device_id: device.into(),
        session: agent.lineage_session.clone(),
    };
    actor.require_native_identity()?;
    Ok(actor)
}
struct HostIdentity<'a> {
    machine: &'a str,
    scope: &'a str,
    native_machine: &'a str,
}
fn record(
    agent: &ProjectedAgent,
    host: HostIdentity<'_>,
    instance: String,
    name: String,
    parent: Option<String>,
    project: Option<String>,
) -> Result<AgentRecord, String> {
    Ok(AgentRecord {
        id: String::new(),
        name,
        machine: host.machine.into(),
        host_scope: host.scope.into(),
        native_machine: host.native_machine.into(),
        session: agent
            .agent_session
            .as_ref()
            .ok_or("native_identity_unavailable")?
            .value
            .clone(),
        instance,
        pane: agent.pane_id.clone(),
        parent,
        project,
        actor: actor_for(agent, host.machine)?,
        ended: false,
    })
}

pub(crate) fn run(
    client: Client,
    authority: Authority,
    actor: Actor,
    command: Command,
) -> Result<Value, String> {
    match command {
        Command::List => {
            let ledger = state(&client)?;
            Ok(
                json!({"items":ledger.agents.iter().map(|record|view(record,&ledger)).collect::<Vec<_>>()}),
            )
        }
        Command::Show { id } => {
            let ledger = state(&client)?;
            let record = ledger
                .agents
                .iter()
                .find(|record| record.id == id)
                .ok_or("agent_unavailable")?;
            Ok(view(record, &ledger))
        }
        Command::End {
            id,
            actor: asserted,
        } => mutate(
            &client,
            &authority,
            &actor,
            Mutation::End {
                id,
                actor: asserted,
            },
        ),
        Command::Register {
            check,
            machine,
            host_scope,
            session,
            instance,
            name,
            pane,
            parent,
            project,
        } => {
            if ![&machine, &host_scope, &session, &instance, &name, &pane]
                .into_iter()
                .all(|s| super::key(s))
            {
                return Err("invalid_registration".into());
            }
            if machine != actor.device_id {
                return Err("machine_identity_conflict".into());
            }
            let (connector, actual_scope, native_machine, _) = context(&client, &machine)?;
            if machine == "local" && host_scope != actual_scope {
                return Err("host_scope_conflict".into());
            }
            let observed = agents(connector.as_ref())?;
            let native = observed
                .iter()
                .find(|agent| agent.pane_id == pane)
                .ok_or("target_unavailable")?;
            if native.lineage_session != crate::wire::session_digest(&session) {
                return Err("session_identity_conflict".into());
            }
            let ledger = state(&client)?;
            let parent = parent
                .map(|id| {
                    if id == "here" {
                        ledger
                            .agents
                            .iter()
                            .find(|p| !p.ended && p.actor.same_identity(&actor))
                            .map(|p| p.id.clone())
                            .ok_or("parent_unavailable".to_owned())
                    } else {
                        Ok(id)
                    }
                })
                .transpose()?;
            let record = record(
                native,
                HostIdentity {
                    machine: &machine,
                    scope: &host_scope,
                    native_machine: &native_machine,
                },
                instance,
                name,
                parent,
                project,
            )?;
            let value = mutate(
                &client,
                &authority,
                &actor,
                Mutation::Register { record, check },
            )?;
            if !check {
                publish_tokens(
                    &client,
                    connector.as_ref(),
                    value["id"].as_str().ok_or("agent_unavailable")?,
                )?;
            }
            Ok(value)
        }
        command @ Command::Spawn { .. } => spawn(&client, &authority, &actor, command),
    }
}
fn publish_tokens(client: &Client, connector: &dyn ApiConnector, id: &str) -> Result<(), String> {
    let ledger = state(client)?;
    let child = ledger
        .agents
        .iter()
        .find(|record| record.id == id)
        .ok_or("agent_unavailable")?;
    if let Some(parent) = child
        .parent
        .as_ref()
        .and_then(|id| ledger.agents.iter().find(|record| &record.id == id))
    {
        super::lineage::write_record(connector, child, parent)?;
    }
    Ok(())
}
fn spawn(
    client: &Client,
    authority: &Authority,
    actor: &Actor,
    command: Command,
) -> Result<Value, String> {
    let _single = SPAWN.try_lock().map_err(|_| "spawn_busy")?;
    let Command::Spawn {
        parent,
        name,
        intent,
        kind,
        repo,
        branch,
        path,
        no_watch,
        args,
    } = &command
    else {
        return Err("invalid_spawn".into());
    };
    if ![parent, name, intent, kind, repo, branch]
        .into_iter()
        .all(|s| super::key(s))
        || args.len() > 128
        || args.iter().map(String::len).sum::<usize>() > 8192
        || args.iter().any(|arg| arg.contains('\0'))
    {
        return Err("invalid_spawn".into());
    }
    if !crate::fork::valid_agent_name(name) {
        return Err("invalid_agent_name".into());
    }
    let (connector, host_scope, native_machine, codex_daemon) = context(client, &actor.device_id)?;
    let mut ledger = state(client)?;
    let parent_id = if parent == "here" {
        if let Some(parent) = ledger
            .agents
            .iter()
            .find(|record| !record.ended && record.actor.same_identity(actor))
        {
            parent.id.clone()
        } else {
            let observed = agents(connector.as_ref())?;
            let native = observed
                .iter()
                .find(|agent| {
                    actor_for(agent, &actor.device_id)
                        .is_ok_and(|current| current.same_identity(actor))
                })
                .ok_or("parent_unavailable")?;
            let record = record(
                native,
                HostIdentity {
                    machine: &actor.device_id,
                    scope: &host_scope,
                    native_machine: &native_machine,
                },
                native.pane_id.clone(),
                actor.name.clone(),
                None,
                Some(repo.clone()),
            )?;
            mutate(
                client,
                authority,
                actor,
                Mutation::Register {
                    record,
                    check: false,
                },
            )?["id"]
                .as_str()
                .ok_or("parent_unavailable")?
                .to_owned()
        }
    } else {
        parent.clone()
    };
    ledger = state(client)?;
    if !resolve_actor(&ledger, &parent_id).is_some_and(|parent| parent.same_identity(actor)) {
        return Err("parent_authority_required".into());
    }
    let reserved = mutate(
        client,
        authority,
        actor,
        Mutation::Reserve {
            parent: parent_id.clone(),
            command: command.clone(),
        },
    )?;
    let mut reserved: SpawnRecord =
        serde_json::from_value(reserved).map_err(|_| "spawn_unavailable")?;
    if reserved.pane.is_none() {
        // Reconcile a worktree whose creation reply was interrupted. The
        // existing local/device connector and checkout owner are shared with
        // the product's worktree/start path; no spawn-specific SSH exists.
        let listed = request_with_connector(
            connector.as_ref(),
            "worktree.list",
            crate::wire::worktree_list_params(repo)?,
            Duration::from_secs(5),
        )
        .map_err(|error| format!("{error}"))?;
        let existing = crate::wire::listed_worktree_path(listed, branch)?;
        if let Some(existing) = existing {
            if path.as_ref().is_some_and(|path| path != &existing) {
                return Err("checkout_path_conflict".into());
            }
            let owner = crate::checkout_owner::OwnerOpen::Worktree {
                path: existing.clone(),
                repository_root: repo.clone(),
                label: name.clone(),
            };
            // The persistent unique tab label makes an interrupted tab-create
            // discoverable before a retry could create another child pane.
            let snapshot = request_with_connector(
                connector.as_ref(),
                "session.snapshot",
                crate::wire::empty_params(),
                Duration::from_secs(5),
            )
            .map_err(|error| format!("{error}"))?;
            let snapshot =
                crate::wire::snapshot_response(snapshot).map_err(|_| "snapshot_unavailable")?;
            let label = format!("hide:{}", reserved.id);
            let existing_pane = snapshot
                .tabs
                .iter()
                .find(|tab| tab.label == label)
                .and_then(|tab| snapshot.panes.iter().find(|pane| pane.tab_id == tab.tab_id))
                .or_else(|| {
                    snapshot
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.label == label)
                        .and_then(|workspace| {
                            snapshot
                                .panes
                                .iter()
                                .find(|pane| pane.workspace_id == workspace.workspace_id)
                        })
                })
                .map(|pane| pane.pane_id.clone());
            let pane = match existing_pane {
                Some(pane) => pane,
                None => {
                    crate::live::open_owner_tab(
                        connector.as_ref(),
                        &owner,
                        &existing,
                        &label,
                        Default::default(),
                    )
                    .map_err(|error| format!("{error:?}"))?
                    .pane_id
                }
            };
            reserved = serde_json::from_value(mutate(
                client,
                authority,
                actor,
                Mutation::Advance {
                    id: reserved.id.clone(),
                    path: Some(existing),
                    pane: Some(pane),
                    child: None,
                },
            )?)
            .map_err(|_| "spawn_unavailable")?;
        } else {
            let mut params = crate::wire::worktree_create_params(repo, branch, None, false)?;
            params["label"] = json!(format!("hide:{}", reserved.id));
            if let Some(path) = path {
                params["path"] = json!(path);
            }
            let result = request_with_connector(
                connector.as_ref(),
                "worktree.create",
                params,
                Duration::from_secs(5),
            )
            .map_err(|error| format!("{error}"))?;
            let created = crate::wire::created_worktree(result)?;
            reserved = serde_json::from_value(mutate(
                client,
                authority,
                actor,
                Mutation::Advance {
                    id: reserved.id.clone(),
                    path: Some(created.path),
                    pane: Some(created.pane_id),
                    child: None,
                },
            )?)
            .map_err(|_| "spawn_unavailable")?;
        }
    }
    let pane = reserved.pane.as_deref().ok_or("spawn_unavailable")?;
    let observed = agents(connector.as_ref())?;
    let live = observed.iter().find(|agent| {
        agent.pane_id == pane
            && agent.agent.as_deref() == Some(kind)
            && agent.name.as_deref() == Some(name)
    });
    if live.is_none() {
        // A changed session in an already registered child is refused. A
        // retry never launches over a replacement occupant.
        if reserved.child.is_some() {
            return Err("child_identity_changed".into());
        }
        crate::live::start_agent(
            connector.as_ref(),
            &reserved.id,
            pane,
            name,
            kind,
            args.clone(),
            codex_daemon,
        )?;
    }
    let native = wait_native_identity(connector.as_ref(), pane, Some(name), kind)?;
    let child = record(
        &native,
        HostIdentity {
            machine: &actor.device_id,
            scope: &host_scope,
            native_machine: &native_machine,
        },
        pane.into(),
        name.clone(),
        Some(parent_id.clone()),
        reserved.path.clone(),
    )?;
    let value = mutate(
        client,
        authority,
        actor,
        Mutation::Register {
            record: child,
            check: false,
        },
    )?;
    let child_id = value["id"].as_str().ok_or("child_unavailable")?.to_owned();
    mutate(
        client,
        authority,
        actor,
        Mutation::Advance {
            id: reserved.id,
            path: None,
            pane: None,
            child: Some(child_id.clone()),
        },
    )?;
    publish_tokens(client, connector.as_ref(), &child_id)?;
    if !no_watch {
        mutate(
            client,
            authority,
            actor,
            Mutation::Watch {
                parent: parent_id,
                child: child_id.clone(),
            },
        )?;
    }
    let ledger = state(client)?;
    Ok(view(
        ledger
            .agents
            .iter()
            .find(|record| record.id == child_id)
            .ok_or("child_unavailable")?,
        &ledger,
    ))
}

pub(crate) fn link_fork(
    context: &crate::live::LiveContext,
    parent: &str,
    child: &str,
) -> Result<(), String> {
    let runtime = context.runtime.upgrade().ok_or("delivery_unavailable")?;
    let (client, authority, actor) = runtime
        .lock()
        .map_err(|_| "delivery_unavailable")?
        .coordination_fork_context(parent)?;
    let (connector, host_scope, native_machine, _) = self::context(&client, &actor.device_id)?;
    let observed = agents(connector.as_ref())?;
    let native_parent = observed
        .iter()
        .find(|agent| agent.pane_id == parent)
        .ok_or("parent_unavailable")?;
    let ledger = state(&client)?;
    let parent_id = if let Some(parent) = ledger
        .agents
        .iter()
        .find(|record| !record.ended && record.actor.same_identity(&actor))
    {
        parent.id.clone()
    } else {
        let parent_record = record(
            native_parent,
            HostIdentity {
                machine: &actor.device_id,
                scope: &host_scope,
                native_machine: &native_machine,
            },
            parent.into(),
            actor.name.clone(),
            None,
            None,
        )?;
        let parent_value = mutate(
            &client,
            &authority,
            &actor,
            Mutation::Register {
                record: parent_record,
                check: false,
            },
        )?;
        parent_value["id"]
            .as_str()
            .ok_or("parent_unavailable")?
            .to_owned()
    };
    let native_child = wait_native_identity(
        connector.as_ref(),
        child,
        None,
        native_parent.agent.as_deref().ok_or("parent_unavailable")?,
    )?;
    let child_record = record(
        &native_child,
        HostIdentity {
            machine: &actor.device_id,
            scope: &host_scope,
            native_machine: &native_machine,
        },
        child.into(),
        native_child.name.clone().unwrap_or_else(|| child.into()),
        Some(parent_id),
        native_child.cwd.clone(),
    )?;
    let value = mutate(
        &client,
        &authority,
        &actor,
        Mutation::Register {
            record: child_record,
            check: false,
        },
    )?;
    publish_tokens(
        &client,
        connector.as_ref(),
        value["id"].as_str().ok_or("child_unavailable")?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_herdr::FakeHerdr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn an_accepted_start_waits_for_native_identity_without_starting_again() {
        let observations = Arc::new(AtomicUsize::new(0));
        let reads = observations.clone();
        let herdr = FakeHerdr::start("coordination-native-ready", move |method, _| {
            assert_eq!(method, "agent.list");
            let mut agent = json!({"pane_id":"w2:p1","workspace_id":"w2",
                "tab_id":"w2:t1","terminal_id":"fixture-child-terminal",
                "revision":1,"focused":false,"agent_status":"idle",
                "agent":"claude","name":"child"});
            if reads.fetch_add(1, Ordering::SeqCst) > 0 {
                agent["agent_session"] = json!({"source":"herdr:claude",
                    "agent":"claude","kind":"id","value":"fixture-child-session"});
            }
            json!({"type":"agent_list","agents":[agent]})
        });
        let child =
            wait_native_identity(&herdr.connector(), "w2:p1", Some("child"), "claude").unwrap();
        assert_eq!(
            child.lineage_session,
            crate::wire::session_digest("fixture-child-session")
        );
        assert_eq!(observations.load(Ordering::SeqCst), 2);
        assert_eq!(herdr.methods(), ["agent.list", "agent.list"]);
    }
}
