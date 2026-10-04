use super::{AgentRecord, Command, Mutation, SpawnRecord, resolve_actor, view};
use crate::delivery::{
    Actor,
    worker::{Authority, Client, Effect},
};
use crate::session_sync::ProjectedAgent;
use hide_herdr_client::{ApiConnector, request_with_connector};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    let result = request_with_connector(
        connector,
        "agent.list",
        crate::wire::empty_params(),
        Duration::from_secs(2),
    )
    .map_err(|error| format!("{error}"))?;
    crate::wire::agents_response(result).map_err(|_| "native_identity_unavailable".into())
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
fn record(
    agent: &ProjectedAgent,
    machine: &str,
    host_scope: &str,
    native_machine: &str,
    instance: String,
    name: String,
    parent: Option<String>,
    project: Option<String>,
) -> Result<AgentRecord, String> {
    Ok(AgentRecord {
        id: String::new(),
        name,
        machine: machine.into(),
        host_scope: host_scope.into(),
        native_machine: native_machine.into(),
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
        actor: actor_for(agent, machine)?,
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
                &machine,
                &host_scope,
                &native_machine,
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
                &actor.device_id,
                &host_scope,
                &native_machine,
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
    let observed = agents(connector.as_ref())?;
    let native = observed
        .iter()
        .find(|agent| agent.pane_id == pane && agent.name.as_deref() == Some(name))
        .ok_or("child_unavailable")?;
    let child = record(
        native,
        &actor.device_id,
        &host_scope,
        &native_machine,
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
            &actor.device_id,
            &host_scope,
            &native_machine,
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
    let native_child = observed
        .iter()
        .find(|agent| agent.pane_id == child)
        .ok_or("child_unavailable")?;
    let child_record = record(
        native_child,
        &actor.device_id,
        &host_scope,
        &native_machine,
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
