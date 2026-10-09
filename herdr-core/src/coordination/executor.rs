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
// Creating something is single-flight: finite side effects serialize without
// occupying the durable store or Runtime, and a concurrent spawn is refused
// instead of accumulating blocked workers. The device checks before it only
// read, so they are not: each is bounded by `TARGET_CHECK_TIMEOUT` and runs
// beside another spawn's.
pub(crate) static SPAWN: Mutex<()> = Mutex::new(());
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

/// A Factory worker whose agent woke in a fresh pane, registered under the
/// Factory again (#857): the same name and worktree, a new registration for
/// the new pane and the conversation it holds, with the Factory as parent.
/// The pane is the one the core confirmed for the saved conversation, never
/// one found by its label, folder or name. Converges on the registration it
/// already made.
pub(crate) struct WokenWorker<'a> {
    pub pane: &'a str,
    pub kind: &'a str,
    pub name: &'a str,
    pub worktree: &'a str,
}

pub(crate) fn register_woken(
    client: &Client,
    authority: &Authority,
    actor: &Actor,
    parent: &str,
    worker: &WokenWorker<'_>,
) -> Result<String, String> {
    if !actor.code_owned() {
        return Err("reserved_name".into());
    }
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
            native_matches(agent, worker.pane, None, worker.kind)
                && agent.lineage_session.is_some()
                && agent.agent_session.is_some()
        })
        .ok_or("native_identity_unavailable")?;
    let record = record(
        native,
        HostIdentity {
            machine: &actor.device_id,
            scope: &host_scope,
            native_machine: &native_machine,
            on_node,
        },
        worker.pane.into(),
        worker.name.into(),
        Some(parent.into()),
        Some(worker.worktree.into()),
    )?;
    let registered = mutate(
        client,
        authority,
        actor,
        Mutation::Register {
            record,
            check: false,
        },
    )?;
    let id = registered["id"]
        .as_str()
        .ok_or("agent_unavailable")?
        .to_owned();
    publish_tokens(client, connector.as_ref(), &id)?;
    Ok(id)
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
    intent: &str,
    repository: Option<&str>,
    agent: bool,
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
    let unavailable = |check: &str, error: LinkError| {
        // The caller is told what to do next; what the node said stays here.
        crate::diagnostic!(json!({
            "component": "spawn", "kind": "device_check_failed", "device": device,
            "intent": intent, "check": check, "cause": error.to_string()
        }));
        match error {
            LinkError::Refused(_) => None,
            _ => Some("machine_unavailable".to_owned()),
        }
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
            Err(error) => {
                return Err(
                    unavailable("repository", error).unwrap_or("repository_unavailable".into())
                );
            }
        }
    }
    if !agent {
        return Ok(());
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
        Err(error) => {
            Err(unavailable("agent_installed", error).unwrap_or("agent_not_installed".into()))
        }
    }
}

fn spawn(
    client: &Client,
    authority: &Authority,
    actor: &Actor,
    mut command: Command,
) -> Result<Value, String> {
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
    // The device probe takes the kind as a program name, so only a kind Hide
    // can start reaches it.
    if machine.is_some() && hide_agent_adapter::start_kind(kind).is_none() {
        return Err("invalid_spawn".into());
    }
    let mut ledger = state(client)?;
    if let Some(device) = machine {
        // Nothing is created for a device that cannot take the spawn, so the
        // judgment comes before the parent is registered or the intent
        // reserved, and a retry of an intent that already progressed or
        // completed is judged by what it already did.
        // Another agent's id is judged before any device is probed, so a
        // device's answer never reaches a caller with no authority over it.
        if let Some(explicit) = parent.as_deref().filter(|parent| *parent != super::HERE)
            && !resolve_actor(&ledger, explicit).is_some_and(|parent| parent.same_identity(actor))
        {
            return Err("parent_authority_required".into());
        }
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
        let started = earlier.is_some_and(|record| record.pane.is_some());
        if !earlier.is_some_and(|record| record.completed) {
            check_target(
                client,
                actor,
                device,
                kind,
                intent,
                // A retry whose pane exists needs the device only to be
                // reachable: the repository and the agent were already used.
                (!started).then_some(repo.as_str()),
                !started,
            )?;
        }
    }
    // The device checks above only read, so a slow device never holds the
    // process-wide lock that local and Factory spawns need; everything that
    // creates something runs under it.
    let single = SPAWN.try_lock().map_err(|_| "spawn_busy")?;
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
                // A device's path names no project of the caller's machine.
                machine.is_none().then(|| repo.clone()),
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
        node,
    } = context(client, &child_device).map_err(|reason| {
        if reserved.machine.is_some() {
            "machine_unavailable".to_owned()
        } else {
            reason
        }
    })?;
    // A device that stops answering partway leaves the spawn reserved and
    // resumable: the caller is told the device is unavailable, and the same
    // command continues once it answers again.
    let (spawn_id, on_device) = (reserved.id.clone(), reserved.machine.is_some());
    let placed = (|| -> Result<Value, String> {
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
            let (checkout, pane) = if let Some(existing) = existing {
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
                (existing, pane)
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
                (created.path, created.pane_id)
            };
            // The terminal is recorded with the pane, so a later attempt can
            // tell this pane from one Herdr gives the same id afterwards.
            let terminal = pane_terminal(connector.as_ref(), &pane)?;
            reserved = serde_json::from_value(mutate(
                client,
                authority,
                actor,
                Mutation::Advance {
                    id: reserved.id.clone(),
                    path: Some(checkout),
                    pane: Some(pane),
                    terminal: Some(terminal),
                    child: None,
                },
            )?)
            .map_err(|_| "spawn_unavailable")?;
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
            // This spawn already had its start typed here, and Herdr still
            // holds the name with no agent while the shell is back at its
            // prompt: the start ended without the agent. Asking the same
            // intent again answers that, rather than typing the start again
            // each time; a new attempt closes this pane and starts afresh.
            if observed.iter().any(|agent| {
                agent.pane_id == pane
                    && agent.name.as_deref() == Some(name)
                    && agent.agent.is_none()
            }) && shell_alone(connector.as_ref(), pane) == Ok(true)
            {
                return Err("agent_not_started".into());
            }
            close_unstarted_attempts(connector.as_ref(), &*state(client)?, &reserved, &observed);
            crate::live::start_agent(
                connector.as_ref(),
                node.as_deref(),
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
    })();
    // Nothing after this creates anything, so a device that has gone quiet
    // never holds the lock for the probe below.
    drop(single);
    placed.map_err(|reason| {
        if on_device {
            device_unreachable(connector.as_ref(), &child_device, &spawn_id, reason)
        } else {
            reason
        }
    })
}

/// Whether `pane`'s shell alone holds its terminal: no program it started
/// runs in the foreground.
fn shell_alone(connector: &dyn ApiConnector, pane: &str) -> Result<bool, String> {
    crate::wire::pane_process_info_params(pane)
        .and_then(|params| {
            request_with_connector(
                connector,
                "pane.process_info",
                params,
                Duration::from_secs(2),
            )
            .map_err(|error| error.to_string())
        })
        .and_then(crate::wire::pane_process_group)
        .map(|group| group.shell_holds_terminal())
}

/// The terminal Herdr runs in `pane` now.
fn pane_terminal(connector: &dyn ApiConnector, pane: &str) -> Result<String, String> {
    crate::wire::pane_target_params(pane)
        .and_then(|params| {
            request_with_connector(connector, "pane.get", params, Duration::from_secs(5))
                .map_err(|error| error.to_string())
        })
        .and_then(crate::wire::pane_terminal)
}

/// Closes the pane of every earlier attempt of `reserved`: a spawn of the
/// same parent, name and device that made a pane and never bound a child,
/// because its agent did not start (a refused start, or one Herdr typed that
/// never showed its agent). Each new attempt is its own intent with a pane of
/// its own, so the earlier pane would stay behind as an extra tab, holding the
/// agent's name in Herdr when its start was typed. Herdr hands a pane id out
/// again (a new server, a live handoff), so a pane is the attempt's own only
/// while it runs the terminal the attempt recorded; any other pane under that
/// id, and a record from before terminals were recorded, is logged and left
/// open. A pane where an agent shows or the shell does not hold the terminal
/// is left alone, and a pane that cannot be read or closed is logged and left
/// to Herdr's own answer.
fn close_unstarted_attempts(
    connector: &dyn ApiConnector,
    ledger: &crate::delivery::ledger::Ledger,
    reserved: &SpawnRecord,
    observed: &[ProjectedAgent],
) {
    let earlier = ledger.spawns.iter().filter(|record| {
        record.id != reserved.id
            && record.parent == reserved.parent
            && record.name == reserved.name
            && record.machine == reserved.machine
            && record.child.is_none()
            && !record.completed
            && record.pane != reserved.pane
    });
    for record in earlier {
        let Some(pane) = record.pane.as_deref() else {
            continue;
        };
        if observed
            .iter()
            .any(|agent| agent.pane_id == pane && agent.agent.is_some())
        {
            continue;
        }
        let same_pane = match record.terminal.as_deref() {
            None => Err("pane_identity_unrecorded".to_owned()),
            Some(recorded) => pane_terminal(connector, pane).and_then(|terminal| {
                if terminal == recorded {
                    Ok(())
                } else {
                    Err("pane_replaced".to_owned())
                }
            }),
        };
        let closed = match same_pane.and_then(|()| shell_alone(connector, pane)) {
            Ok(false) => continue,
            Ok(true) => crate::wire::pane_target_params(pane).and_then(|params| {
                request_with_connector(connector, "pane.close", params, Duration::from_secs(5))
                    .map(drop)
                    .map_err(|error| error.to_string())
            }),
            Err(reason) => Err(reason),
        };
        crate::diagnostic!(json!({
            "component": "spawn", "kind": "unstarted_attempt.close", "spawn": reserved.id,
            "attempt": record.id, "pane_id": pane, "closed": closed.is_ok(),
            "reason": closed.err(),
        }));
    }
}

/// A failure of a spawn's work on a device, as the caller should read it: the
/// device is unavailable when its Herdr no longer answers, whatever step was
/// running; any other failure keeps its own code. The dropped cause is logged
/// with the device and the spawn it belongs to.
fn device_unreachable(
    connector: &dyn ApiConnector,
    device: &str,
    spawn: &str,
    reason: String,
) -> String {
    match request_with_connector(
        connector,
        "agent.list",
        crate::wire::empty_params(),
        Duration::from_secs(2),
    ) {
        Err(
            error @ (hide_herdr_client::ApiError::NotRunning(_)
            | hide_herdr_client::ApiError::Transport(_)),
        ) => {
            crate::diagnostic!(json!({
                "component": "spawn", "kind": "device_unreachable", "device": device,
                "spawn": spawn, "reason": reason, "cause": error.to_string()
            }));
            "machine_unavailable".to_owned()
        }
        _ => reason,
    }
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
    /// A Factory parent and the spawn records of its attempts, each made the
    /// way the executor makes them: reserved, then advanced with its pane and
    /// the terminal recorded with it (none for a record an older build wrote).
    struct Attempts {
        actor: Actor,
        ledger: crate::delivery::ledger::Ledger,
        parent: String,
    }

    impl Attempts {
        fn new() -> Self {
            let actor = Actor {
                pane_id: "factory".into(),
                name: "factory".into(),
                kind: "codex".into(),
                device_id: crate::node::TEST_NODE.into(),
                session: crate::wire::session_digest("native-factory"),
            };
            let mut ledger = crate::delivery::ledger::Ledger::default();
            let parent = super::super::apply(
                &mut ledger,
                &actor,
                &Mutation::Register {
                    record: AgentRecord {
                        id: String::new(),
                        name: "factory".into(),
                        machine: crate::node::TEST_NODE.into(),
                        host_scope: "fixture".into(),
                        native_machine: "fixture-machine".into(),
                        session: "native-factory".into(),
                        instance: "factory-terminal".into(),
                        pane: "factory".into(),
                        parent: None,
                        origin: None,
                        project: None,
                        ended: false,
                        actor: actor.clone(),
                    },
                    check: false,
                },
                1,
            )
            .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned();
            Self {
                actor,
                ledger,
                parent,
            }
        }

        fn attempt(
            &mut self,
            name: &str,
            intent: &str,
            pane: &str,
            terminal: Option<&str>,
        ) -> SpawnRecord {
            let reserved = super::super::apply(
                &mut self.ledger,
                &self.actor,
                &Mutation::Reserve {
                    parent: self.parent.clone(),
                    command: Command::Spawn {
                        parent: Some(self.parent.clone()),
                        machine: None,
                        name: name.into(),
                        intent: intent.into(),
                        kind: "claude".into(),
                        repo: "/fixture".into(),
                        branch: "factory/801".into(),
                        path: None,
                        args: Vec::new(),
                    },
                },
                1,
            )
            .unwrap();
            serde_json::from_value(
                super::super::apply(
                    &mut self.ledger,
                    &self.actor,
                    &Mutation::Advance {
                        id: reserved["id"].as_str().unwrap().into(),
                        path: Some("/fixture.worktrees/801".into()),
                        pane: Some(pane.into()),
                        terminal: terminal.map(Into::into),
                        child: None,
                    },
                    1,
                )
                .unwrap(),
            )
            .unwrap()
        }
    }

    /// A Herdr whose panes run the given terminals, each with its shell
    /// alone at the prompt unless named busy, and that closes what it is
    /// asked to.
    fn attempt_panes(
        name: &str,
        terminals: &'static [(&'static str, &'static str)],
        busy: &'static [&'static str],
    ) -> FakeHerdr {
        FakeHerdr::start(name, move |method, params| {
            let pane = params["pane_id"].as_str().unwrap().to_owned();
            match method {
                "pane.get" => {
                    let (_, terminal) = terminals
                        .iter()
                        .find(|(id, _)| *id == pane)
                        .expect("a pane the fixture runs");
                    json!({"type": "pane_info", "pane": {"pane_id": pane,
                        "terminal_id": terminal, "workspace_id": "w2", "tab_id": "w2:t1",
                        "focused": false, "agent_status": "idle", "revision": 1}})
                }
                "pane.process_info" => {
                    let foreground = if busy.contains(&pane.as_str()) {
                        4200
                    } else {
                        4100
                    };
                    json!({"type": "pane_process_info", "process_info": {
                        "pane_id": pane, "shell_pid": 4100,
                        "foreground_process_group_id": foreground,
                        "foreground_processes": [{"pid": foreground, "name": "zsh"}]}})
                }
                "pane.close" => json!({"type": "ok"}),
                other => panic!("unexpected {other}"),
            }
        })
    }

    fn closed_panes(herdr: &FakeHerdr) -> Vec<String> {
        herdr
            .calls()
            .into_iter()
            .filter(|(method, _)| method == "pane.close")
            .map(|(_, params)| params["pane_id"].as_str().unwrap().to_owned())
            .collect()
    }

    /// A Factory retry is a new attempt with its own intent and pane; the
    /// earlier attempts of the same parent and name whose agent never
    /// started are closed, so no extra tab and no held name stays behind.
    /// A pane where an agent shows or the shell is busy, another name's
    /// spawn and the attempt's own pane are left alone.
    #[test]
    fn a_new_attempt_closes_the_idle_panes_of_earlier_attempts_that_never_started() {
        let mut attempts = Attempts::new();
        attempts.attempt("worker", "factory-801", "w2:p1", Some("t1"));
        attempts.attempt("worker", "factory-801-a1", "w2:p2", Some("t2"));
        attempts.attempt("worker", "factory-801-a2", "w2:p3", Some("t3"));
        attempts.attempt("other", "factory-857", "w3:p1", Some("t31"));
        let current = attempts.attempt("worker", "factory-801-a3", "w2:p4", Some("t4"));
        // The shell runs a command in w2:p3: that attempt is busy.
        let herdr = attempt_panes(
            "coordination-earlier-attempts",
            &[
                ("w2:p1", "t1"),
                ("w2:p2", "t2"),
                ("w2:p3", "t3"),
                ("w3:p1", "t31"),
            ],
            &["w2:p3"],
        );
        // An agent shows in w2:p2, and Herdr holds the name for the start in
        // w2:p1 that never showed one.
        let observed: Vec<ProjectedAgent> =
            crate::wire::agents_response(json!({"type": "agent_list", "agents": [
                {"pane_id": "w2:p1", "workspace_id": "w2", "tab_id": "w2:t1",
                    "terminal_id": "t1", "revision": 0, "focused": false,
                    "agent_status": "unknown", "name": "worker", "launch_pending": true},
                {"pane_id": "w2:p2", "workspace_id": "w2", "tab_id": "w2:t2",
                    "terminal_id": "t2", "revision": 0, "focused": false,
                    "agent_status": "idle", "agent": "claude"},
            ]}))
            .unwrap();
        close_unstarted_attempts(&herdr.connector(), &attempts.ledger, &current, &observed);
        assert_eq!(closed_panes(&herdr), ["w2:p1"]);
    }

    /// Herdr hands a pane id out again after its server is replaced, so an
    /// earlier attempt's pane id can name an idle shell the attempt never
    /// made (issue 874). Only a pane still running the terminal the attempt
    /// recorded is its own and closed; a pane id now running another
    /// terminal, and a record an older build wrote without one, are left
    /// open and logged.
    #[test]
    fn an_earlier_attempt_pane_is_closed_only_while_it_runs_the_terminal_the_attempt_recorded() {
        let mut attempts = Attempts::new();
        let kept = attempts.attempt("worker", "factory-801", "w2:p1", Some("t1"));
        let replaced = attempts.attempt("worker", "factory-801-a1", "w2:p2", Some("t2-before"));
        let unrecorded = attempts.attempt("worker", "factory-801-a2", "w2:p3", None);
        let current = attempts.attempt("worker", "factory-801-a3", "w2:p4", Some("t4"));
        let herdr = attempt_panes(
            "coordination-earlier-attempt-identity",
            &[("w2:p1", "t1"), ("w2:p2", "t2-after"), ("w2:p3", "t3")],
            &[],
        );
        let ((), records) = crate::diagnostics::capture(|| {
            close_unstarted_attempts(&herdr.connector(), &attempts.ledger, &current, &[]);
        });
        assert_eq!(closed_panes(&herdr), ["w2:p1"]);
        let outcome = |attempt: &SpawnRecord| {
            records
                .iter()
                .find(|record| {
                    record["kind"] == "unstarted_attempt.close" && record["attempt"] == attempt.id
                })
                .map(|record| (record["closed"].clone(), record["reason"].clone()))
        };
        assert_eq!(outcome(&kept), Some((json!(true), Value::Null)));
        assert_eq!(
            outcome(&replaced),
            Some((json!(false), json!("pane_replaced")))
        );
        assert_eq!(
            outcome(&unrecorded),
            Some((json!(false), json!("pane_identity_unrecorded")))
        );
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
                    terminal: None,
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
    /// A worker that woke in a fresh pane is registered under its Factory
    /// again, keeping its name and worktree, and asking again names the same
    /// registration (#857). A pane that shows another kind of agent, or none,
    /// is no registration.
    #[test]
    fn a_worker_woken_in_a_new_pane_is_registered_under_its_factory_once() {
        use crate::delivery::worker::Worker;
        use crate::handle::ChangeNotifier;
        use crate::runtime::delivery::tests::{authority, fixture, recipient_at_rest};
        let root = tempfile::tempdir().unwrap();
        let herdr = FakeHerdr::start("coordination-woken", |method, _| match method {
            "agent.list" => json!({"type":"agent_list","agents":[
                {"pane_id":"recipient","workspace_id":"w2","tab_id":"w2:t1",
                    "terminal_id":"woken-terminal","revision":1,"focused":false,
                    "agent_status":"idle","agent":"pi","name":"worker",
                    "agent_session":{"source":"herdr:pi","agent":"pi","kind":"id",
                        "value":"woken-session"}},
                {"pane_id":"sender","workspace_id":"w2","tab_id":"w2:t2",
                    "terminal_id":"other-terminal","revision":1,"focused":false,
                    "agent_status":"idle","agent":"claude","name":"other",
                    "agent_session":{"source":"herdr:claude","agent":"claude","kind":"id",
                        "value":"other-session"}}
            ]}),
            "pane.report_metadata" => json!({"type":"ok"}),
            other => panic!("unexpected {other}"),
        });
        let (runtime, _, _, path) = fixture(root.path());
        let factory = Actor::factory("f-1", crate::node::TEST_NODE);
        {
            let mut current = runtime.lock().unwrap();
            recipient_at_rest(&mut current, "native-woken", &herdr);
            current.set_factory_recipients([("f-1".into(), 30 * 60_000)].into());
        }
        let (worker, client) = Worker::spawn(
            Arc::downgrade(&runtime),
            ChangeNotifier::noop(),
            path.clone(),
        )
        .unwrap();
        let authority = authority(&factory);
        let parent = register_code_owned(&client, &authority, &factory).unwrap();
        let woken = WokenWorker {
            pane: "recipient",
            kind: "pi",
            name: "factory-fixture-T-1",
            worktree: "/fixture/topic",
        };
        let id = register_woken(&client, &authority, &factory, &parent, &woken).unwrap();
        assert_eq!(
            register_woken(&client, &authority, &factory, &parent, &woken).as_deref(),
            Ok(id.as_str()),
            "asking again names the same registration"
        );
        // Not the pane's agent kind, and a pane that shows no agent.
        for (pane, kind) in [("sender", "pi"), ("recipient", "claude"), ("nowhere", "pi")] {
            let refused = register_woken(
                &client,
                &authority,
                &factory,
                &parent,
                &WokenWorker {
                    pane,
                    kind,
                    ..woken
                },
            );
            assert_eq!(
                refused,
                Err("native_identity_unavailable".into()),
                "{pane} {kind}"
            );
        }
        drop(worker);
        let ledger = crate::delivery::ledger::load(&path).unwrap();
        let record = ledger
            .agents
            .iter()
            .find(|record| record.id == id)
            .expect("the woken registration");
        assert_eq!(
            (
                record.pane.as_str(),
                record.name.as_str(),
                record.parent.as_deref(),
                record.project.as_deref(),
                record.ended,
            ),
            (
                "recipient",
                "factory-fixture-T-1",
                Some(parent.as_str()),
                Some("/fixture/topic"),
                false
            )
        );
        assert_eq!(ledger.agents.len(), 2, "the Factory and its one worker");
        assert!(
            herdr
                .calls()
                .iter()
                .any(|(method, params)| method == "pane.report_metadata"
                    && params["pane_id"] == "recipient")
        );
    }
}
