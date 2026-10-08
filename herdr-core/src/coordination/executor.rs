use super::{AgentRecord, Command, Mutation, SpawnRecord, resolve_actor, view};
use crate::delivery::{
    Actor,
    worker::{Authority, Client, Effect},
};
use crate::runtime::delivery::CoordinationContext;
use crate::session_sync::ProjectedAgent;
use hide_herdr_client::{ApiConnector, request_with_connector};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const STORE_TIMEOUT: Duration = Duration::from_secs(5);
// Finite side effects serialize without occupying the durable store or Runtime.
// A concurrent spawn is refused instead of accumulating blocked workers.
static SPAWN: Mutex<()> = Mutex::new(());
/// Tests share `SPAWN` through the process, so those that spawn take turns
/// when a runner puts them on threads of one process.
#[cfg(test)]
pub(crate) static SPAWN_TURN: Mutex<()> = Mutex::new(());
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
            mutation: Box::new(mutation),
        },
        STORE_TIMEOUT,
    )
}
fn context(client: &Client, device: &str) -> Result<CoordinationContext, String> {
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
fn native_matches(agent: &ProjectedAgent, pane: &str, name: Option<&str>, kind: &str) -> bool {
    agent.pane_id == pane
        && name.is_none_or(|name| agent.name.as_deref() == Some(name))
        && agent.agent.as_deref().is_some_and(|observed| {
            hide_agent_adapter::canonical_kind(observed) == hide_agent_adapter::canonical_kind(kind)
        })
}
#[allow(clippy::disallowed_methods)] // a production wait, not test code
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
            native_matches(agent, pane, name, kind)
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
/// The actor of `agent` on `device`; `on_node` when that device is the core's
/// own node, whose panes the core names without a device prefix.
fn actor_for(agent: &ProjectedAgent, device: &str, on_node: bool) -> Result<Actor, String> {
    let actor = Actor {
        pane_id: if on_node {
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
    on_node: bool,
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
        origin: None,
        project,
        actor: actor_for(agent, host.machine, host.on_node)?,
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
            Ok(json!(crate::delivery::answer::AgentList {
                items: ledger
                    .agents
                    .iter()
                    .map(|record| super::agent_view(record, &ledger))
                    .collect(),
            }))
        }
        Command::Show { id } if id == super::HERE => {
            let ledger = state(&client)?;
            Ok(view(super::here(&ledger, &actor)?, &ledger))
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
            if ![&host_scope, &session, &instance, &name, &pane]
                .into_iter()
                .all(|s| super::key(s))
            {
                return Err("invalid_registration".into());
            }
            let machine = registration_machine(machine, &actor.device_id)?;
            let CoordinationContext {
                connector,
                host_scope: actual_scope,
                machine: native_machine,
                on_node,
                ..
            } = context(&client, &machine)?;
            if on_node && host_scope != actual_scope {
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
                    if id == super::HERE {
                        super::live_self(&ledger, &actor)
                            .next()
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
                    on_node,
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
/// A registration is always on the caller's own machine, which its pane
/// capability already names; a machine the caller restates must agree with it.
fn registration_machine(requested: Option<String>, caller: &str) -> Result<String, String> {
    match requested {
        None => Ok(caller.to_owned()),
        Some(machine) if !super::key(&machine) => Err("invalid_registration".into()),
        Some(machine) if machine != caller => Err("machine_identity_conflict".into()),
        Some(machine) => Ok(machine),
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
/// Registers a Factory's code-owned identity as the parent its workers are
/// spawned under (D-14); converges on the existing record.
pub(crate) fn register_code_owned(
    client: &Client,
    authority: &Authority,
    actor: &Actor,
) -> Result<String, String> {
    if !actor.code_owned() {
        return Err("reserved_name".into());
    }
    let record = AgentRecord {
        id: String::new(),
        name: actor.name.clone(),
        machine: actor.device_id.clone(),
        host_scope: crate::delivery::FACTORY_KIND.into(),
        native_machine: crate::delivery::FACTORY_KIND.into(),
        session: actor.name.clone(),
        instance: actor.pane_id.clone(),
        pane: actor.pane_id.clone(),
        parent: None,
        origin: None,
        project: None,
        actor: actor.clone(),
        ended: false,
    };
    let registered = mutate(
        client,
        authority,
        actor,
        Mutation::Register {
            record,
            check: false,
        },
    )?;
    registered["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "parent_unavailable".into())
}

/// How long the device's node may take to answer one of the spawn's checks.
const TARGET_CHECK_TIMEOUT: Duration = Duration::from_secs(8);

/// Judges a device that is not the caller's before anything is made there:
/// the id names a device, the device takes work now, `repo` (when this
/// attempt still has to make the checkout) is a repository there, and the
/// agent CLI is installed there. The node link is taken under the runtime
/// lock and called after it is released.
fn check_target(
    client: &Client,
    actor: &Actor,
    device: &str,
    kind: &str,
    repository: Option<&str>,
) -> Result<(), String> {
    use crate::node_access::{LinkError, call_as};
    use hide_node_link::{cleanup::RepositoryDirs, protocol::Call};
    let link = client
        .runtime
        .upgrade()
        .ok_or("delivery_unavailable")?
        .lock()
        .map_err(|_| "delivery_unavailable")?
        .spawn_target(&actor.device_id, device)?;
    // A node that did not answer says nothing about the repository or the
    // CLI, so it is the device that is unavailable; only its own answer can
    // refuse them.
    let unavailable = |error: LinkError| match error {
        LinkError::Refused(_) => None,
        _ => Some("machine_unavailable".to_owned()),
    };
    if let Some(repository) = repository {
        match call_as::<Option<RepositoryDirs>>(
            link.as_ref(),
            Call::Repository {
                path: repository.to_owned(),
            },
            TARGET_CHECK_TIMEOUT,
        ) {
            Ok(Some(_)) => {}
            Ok(None) => return Err("repository_unavailable".into()),
            Err(error) => return Err(unavailable(error).unwrap_or("repository_unavailable".into())),
        }
    }
    match call_as::<bool>(
        link.as_ref(),
        Call::AgentInstalled {
            name: hide_agent_adapter::canonical_kind(kind).to_owned(),
        },
        TARGET_CHECK_TIMEOUT,
    ) {
        Ok(true) => Ok(()),
        Ok(false) => Err("agent_not_installed".into()),
        Err(error) => Err(unavailable(error).unwrap_or("agent_not_installed".into())),
    }
}

fn spawn(
    client: &Client,
    authority: &Authority,
    actor: &Actor,
    mut command: Command,
) -> Result<Value, String> {
    let _single = SPAWN.try_lock().map_err(|_| "spawn_busy")?;
    // The caller's own device is the absence of a device: one intent, one
    // spelling.
    if let Command::Spawn { machine, .. } = &mut command
        && machine.as_deref() == Some(actor.device_id.as_str())
    {
        *machine = None;
    }
    let Command::Spawn {
        parent,
        machine,
        name,
        intent,
        kind,
        repo,
        branch,
        path,
        args,
    } = &command
    else {
        return Err("invalid_spawn".into());
    };
    if parent.as_ref().is_some_and(|parent| !super::key(parent))
        || machine.as_ref().is_some_and(|machine| !super::key(machine))
        || ![name, intent, kind, repo, branch]
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
    let mut ledger = state(client)?;
    if let Some(device) = machine {
        // Nothing is created for a device that cannot take the spawn, so the
        // judgment comes before the parent is registered or the intent
        // reserved, and a retry of an intent that already progressed or
        // completed is judged by what it already did.
        let known_parent = if parent.as_deref().is_none_or(|parent| parent == super::HERE) {
            super::live_self(&ledger, actor)
                .next()
                .map(|p| p.id.clone())
        } else {
            parent.clone()
        };
        let earlier = known_parent.and_then(|id| {
            ledger
                .spawns
                .iter()
                .find(|record| record.parent == id && record.intent == *intent)
        });
        if earlier.is_some_and(|record| record.machine != *machine) {
            return Err("intent_conflict".into());
        }
        if !earlier.is_some_and(|record| record.completed) {
            check_target(
                client,
                actor,
                device,
                kind,
                (!earlier.is_some_and(|record| record.pane.is_some())).then_some(repo.as_str()),
            )?;
        }
    }
    let parent_id = if parent.as_deref().is_none_or(|parent| parent == super::HERE) {
        if let Some(parent) = super::live_self(&ledger, actor).next() {
            parent.id.clone()
        } else {
            let CoordinationContext {
                connector,
                host_scope,
                machine: native_machine,
                on_node,
                ..
            } = context(client, &actor.device_id)?;
            let observed = agents(connector.as_ref())?;
            let native = observed
                .iter()
                .find(|agent| {
                    actor_for(agent, &actor.device_id, on_node)
                        .is_ok_and(|current| current.same_identity(actor))
                })
                .ok_or("parent_unavailable")?;
            let record = record(
                native,
                HostIdentity {
                    machine: &actor.device_id,
                    scope: &host_scope,
                    native_machine: &native_machine,
                    on_node,
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
        parent.clone().ok_or("parent_unavailable")?
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
    if reserved.completed {
        let ledger = state(client)?;
        return Ok(view(
            ledger
                .agents
                .iter()
                .find(|record| Some(&record.id) == reserved.child.as_ref())
                .ok_or("child_unavailable")?,
            &ledger,
        ));
    }
    // Everything the child needs happens on its own device; only the parent's
    // authority and watch stay with the caller.
    let child_device = reserved
        .machine
        .clone()
        .unwrap_or_else(|| actor.device_id.clone());
    let CoordinationContext {
        connector,
        host_scope,
        machine: native_machine,
        codex: codex_daemon,
        on_node,
    } = context(client, &child_device).map_err(|reason| {
        if reserved.machine.is_some() {
            "machine_unavailable".to_owned()
        } else {
            reason
        }
    })?;
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
                        false,
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
    let live = observed
        .iter()
        .find(|agent| native_matches(agent, pane, Some(name), kind));
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
    let mut child = record(
        &native,
        HostIdentity {
            machine: &child_device,
            scope: &host_scope,
            native_machine: &native_machine,
            on_node,
        },
        pane.into(),
        name.clone(),
        reserved.mode.responsibility(&parent_id),
        reserved.path.clone(),
    )?;
    child.origin = reserved.mode.origin(&parent_id);
    reserved = serde_json::from_value(mutate(
        client,
        authority,
        actor,
        Mutation::BindChild {
            id: reserved.id.clone(),
            record: child,
        },
    )?)
    .map_err(|_| "spawn_unavailable")?;
    let child_id = reserved.child.clone().ok_or("child_unavailable")?;
    publish_tokens(client, connector.as_ref(), &child_id)?;
    mutate(
        client,
        authority,
        actor,
        Mutation::Complete { id: reserved.id },
    )?;
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
    let CoordinationContext {
        connector,
        host_scope,
        machine: native_machine,
        on_node,
        ..
    } = self::context(&client, &actor.device_id)?;
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
                on_node,
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
            on_node,
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
    fn a_registration_lands_on_the_callers_own_machine() {
        assert_eq!(
            registration_machine(None, crate::node::TEST_NODE).as_deref(),
            Ok(crate::node::TEST_NODE)
        );
        assert_eq!(registration_machine(None, "mini").as_deref(), Ok("mini"));
        assert_eq!(
            registration_machine(Some("mini".into()), "mini").as_deref(),
            Ok("mini")
        );
        assert_eq!(
            registration_machine(Some(crate::node::TEST_NODE.into()), "mini"),
            Err("machine_identity_conflict".into())
        );
        assert_eq!(
            registration_machine(Some(String::new()), crate::node::TEST_NODE),
            Err("invalid_registration".into())
        );
    }

    #[test]
    fn native_identity_accepts_known_agent_spellings() {
        // B5/D-08: spelling does not change the native agent's identity.
        for (requested, reported, session_agent) in [
            ("CODEX", "codex", "codex"),
            ("codex", " Codex", "codex"),
            ("claude_code", "claude", "claude"),
        ] {
            let herdr = FakeHerdr::start("coordination-native-alias", move |method, _| {
                assert_eq!(method, "agent.list");
                json!({"type":"agent_list","agents":[{
                    "pane_id":"w2:p1","workspace_id":"w2","tab_id":"w2:t1",
                    "terminal_id":"fixture-child-terminal","revision":1,
                    "focused":false,"agent_status":"idle","agent":reported,"name":"child",
                    "agent_session":{"source":format!("herdr:{session_agent}"),
                        "agent":session_agent,"kind":"id","value":"fixture-child-session"}
                }]})
            });
            let child = wait_native_identity(&herdr.connector(), "w2:p1", Some("child"), requested)
                .unwrap();
            assert_eq!(child.agent.as_deref(), Some(reported));
            assert_eq!(
                child.lineage_session,
                crate::wire::session_digest("fixture-child-session")
            );
        }
    }

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
    #[test]
    fn completed_spawn_replay_after_restart_preserves_report_stop_end_and_explicit_watch() {
        use crate::delivery::{ledger, mailbox, watch, worker::Worker};
        use crate::handle::ChangeNotifier;
        use crate::runtime::delivery::tests::{authority, fixture};
        use crate::sidebar::SessionSnapshotPayload;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("agents/runs/hcoord-retire/replay-tests");
        hide_platform::fs::private::create_dir_all(&base).unwrap();
        // The same public command is exercised through the resident store
        // after reload. No native connector exists: replay must return its
        // durable result before attempting another pane or worktree action.
        let _turn = SPAWN_TURN.lock().unwrap_or_else(|e| e.into_inner());
        for ending in ["report", "stop", "end", "explicit"] {
            let root = tempfile::Builder::new()
                .prefix("replay-")
                .tempdir_in(&base)
                .unwrap();
            let (runtime, _, _, path) = fixture(root.path());
            let parent = AgentRecord {
                id: String::new(),
                name: "sender".into(),
                machine: crate::node::TEST_NODE.into(),
                host_scope: "fixture".into(),
                native_machine: "fixture-machine".into(),
                session: "native-parent".into(),
                instance: "parent-terminal".into(),
                pane: "sender".into(),
                parent: None,
                origin: None,
                project: None,
                ended: false,
                actor: Actor {
                    pane_id: "sender".into(),
                    name: "sender".into(),
                    kind: "codex".into(),
                    device_id: crate::node::TEST_NODE.into(),
                    session: crate::wire::session_digest("native-parent"),
                },
            };
            let actor = parent.actor.clone();
            let child_actor = Actor {
                pane_id: "recipient".into(),
                name: "recipient".into(),
                kind: "codex".into(),
                device_id: crate::node::TEST_NODE.into(),
                session: crate::wire::session_digest("native-child"),
            };
            let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
                {"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working",
                    "state_change_seq":1,"lineage_session":actor.session},
                {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"working",
                    "state_change_seq":1,"lineage_session":child_actor.session},
            ]}))
            .unwrap();
            runtime
                .lock()
                .unwrap()
                .observe_delivery(crate::node::TEST_NODE, &payload, None, None);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let mut state = ledger::Ledger::default();
            let parent_id = super::super::apply(
                &mut state,
                &actor,
                &Mutation::Register {
                    record: parent,
                    check: false,
                },
                now,
            )
            .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned();
            let command = Command::Spawn {
                parent: Some("here".into()),
                machine: None,
                name: "recipient".into(),
                intent: "one-child".into(),
                kind: "codex".into(),
                repo: "/fixture".into(),
                branch: "topic".into(),
                path: None,
                args: Vec::new(),
            };
            let spawn = super::super::apply(
                &mut state,
                &actor,
                &Mutation::Reserve {
                    parent: parent_id.clone(),
                    command: command.clone(),
                },
                now,
            )
            .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned();
            super::super::apply(
                &mut state,
                &actor,
                &Mutation::Advance {
                    id: spawn.clone(),
                    path: Some("/fixture/topic".into()),
                    pane: Some("recipient".into()),
                    child: None,
                },
                now,
            )
            .unwrap();
            let child = AgentRecord {
                id: String::new(),
                name: "recipient".into(),
                machine: crate::node::TEST_NODE.into(),
                host_scope: "fixture".into(),
                native_machine: "fixture-machine".into(),
                session: "native-child".into(),
                instance: "child-terminal".into(),
                pane: "recipient".into(),
                parent: Some(parent_id.clone()),
                origin: None,
                project: Some("/fixture/topic".into()),
                actor: child_actor.clone(),
                ended: false,
            };
            let child_id = super::super::apply(
                &mut state,
                &actor,
                &Mutation::BindChild {
                    id: spawn.clone(),
                    record: child,
                },
                now,
            )
            .unwrap()["child"]
                .as_str()
                .unwrap()
                .to_owned();
            let receipt =
                super::super::apply(&mut state, &actor, &Mutation::Complete { id: spawn }, now)
                    .unwrap();
            let initial_watch = receipt["auto_watch_id"].as_str().unwrap();
            match ending {
                "end" => {
                    super::super::apply(
                        &mut state,
                        &actor,
                        &Mutation::End {
                            id: child_id.clone(),
                            actor: Some(parent_id),
                        },
                        now,
                    )
                    .unwrap();
                }
                "stop" => watch::stop(&mut state, &actor, initial_watch).unwrap(),
                _ => {
                    let report = mailbox::send(
                        &mut state,
                        &child_actor,
                        &actor,
                        "done",
                        "completed",
                        "report",
                        None,
                        now,
                    )
                    .unwrap();
                    mailbox::apply(
                        &mut state,
                        &actor,
                        None,
                        &mailbox::Command::Confirm {
                            ids: vec![report.id],
                        },
                        now,
                    )
                    .unwrap();
                    if ending == "explicit" {
                        let watch = watch::start(&mut state, &actor, &child_actor, now).unwrap();
                        assert_ne!(watch.id, initial_watch);
                    }
                }
            }
            ledger::save(&path, &state).unwrap();
            let restored = ledger::load(&path).unwrap();
            let expected_watch = restored.watches.first().map(|watch| watch.id.clone());
            let next_id = restored.next_id;
            runtime
                .lock()
                .unwrap()
                .publish_delivery(Arc::new(restored), false);
            let (worker, client) = Worker::spawn(
                Arc::downgrade(&runtime),
                ChangeNotifier::noop(),
                path.clone(),
            )
            .unwrap();
            let result = run(client, authority(&actor), actor, command).unwrap();
            assert_eq!(result["id"], child_id, "{ending}");
            assert_eq!(result["registered"], ending != "end", "{ending}");
            assert_eq!(
                result["watch"]["id"].as_str(),
                expected_watch.as_deref(),
                "{ending}"
            );
            drop(worker);
            let after = ledger::load(&path).unwrap();
            assert_eq!(after.agents.len(), 2, "{ending}");
            assert_eq!(after.next_id, next_id, "{ending}");
            assert!(after.spawns[0].completed);
            assert_eq!(
                after.spawns[0].auto_watch_id.as_deref(),
                Some(initial_watch)
            );
            assert_eq!(
                after
                    .agents
                    .iter()
                    .find(|record| record.id == child_id)
                    .unwrap()
                    .ended,
                ending == "end",
                "{ending}"
            );
            assert_eq!(
                after.watches.first().map(|watch| &watch.id),
                expected_watch.as_ref(),
                "{ending}"
            );
        }
    }
}
