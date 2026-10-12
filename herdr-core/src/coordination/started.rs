//! What the coordination ledger says Hide started itself (PRD
//! hide-ai-origin-mark D-01, D-07). A spawn is Hide's own when it is a
//! handoff whose origin is Hide's code-owned identity: a Factory's worker is
//! a delegation of a Factory identity, and a pane agent's handoff has a
//! pane's identity as its origin, so neither matches. Nothing here reads a
//! branch name or a path's spelling.

use super::{SpawnMode, SpawnRecord, SpawnTask};
use crate::delivery::ledger::Ledger;

/// One live agent Hide started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StartedAgent {
    /// The pane in the core's own ids, and the session digest it was
    /// registered with, which a later agent in a reused pane does not share.
    pub(crate) pane_id: String,
    pub(crate) session: Option<String>,
    /// The pane as the machine itself names it, which a connected device's
    /// pane is qualified from.
    pub(crate) raw_pane: String,
    pub(crate) machine: String,
    pub(crate) task: Option<SpawnTask>,
}

/// A checkout Hide made for a spawn, on the machine the child lives on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StartedCheckout {
    pub(crate) machine: String,
    pub(crate) path: String,
    pub(crate) task: Option<SpawnTask>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HideStarted {
    pub(crate) agents: Vec<StartedAgent>,
    pub(crate) checkouts: Vec<StartedCheckout>,
}

fn hide_handoff<'a>(ledger: &'a Ledger, spawn: &SpawnRecord) -> bool {
    spawn.mode == SpawnMode::Handoff
        && ledger
            .agents
            .iter()
            .any(|origin| origin.id == spawn.parent && origin.actor.hide_owned())
}

/// The agents and checkouts Hide started, in ledger order. Compared by value
/// with the previous answer, so a ledger change that moves neither (a letter,
/// a watch) publishes nothing.
pub(crate) fn hide_started(ledger: &Ledger) -> HideStarted {
    let mut started = HideStarted::default();
    for spawn in ledger
        .spawns
        .iter()
        .filter(|spawn| hide_handoff(ledger, spawn))
    {
        let machine = super::spawn_machine(ledger, spawn);
        if let (Some(path), Some(machine)) = (&spawn.path, machine) {
            started.checkouts.push(StartedCheckout {
                machine: machine.to_owned(),
                path: path.clone(),
                task: spawn.task.clone(),
            });
        }
        let Some(child) = spawn
            .child
            .as_ref()
            .and_then(|id| ledger.agents.iter().find(|agent| &agent.id == id))
            .filter(|agent| !agent.ended)
        else {
            continue;
        };
        started.agents.push(StartedAgent {
            pane_id: child.actor.pane_id.clone(),
            session: child.actor.session.clone(),
            raw_pane: child.pane.clone(),
            machine: child.machine.clone(),
            task: spawn.task.clone(),
        });
    }
    started
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::coordination::{AgentRecord, Mutation, apply};
    use crate::delivery::Actor;

    const NODE: &str = "node-1";

    fn pane_on(machine: &str, name: &str, session: &str) -> Actor {
        Actor {
            pane_id: name.into(),
            name: name.into(),
            kind: "claude".into(),
            device_id: machine.into(),
            session: crate::wire::session_digest(session),
        }
    }

    fn pane(name: &str, session: &str) -> Actor {
        pane_on(NODE, name, session)
    }

    fn record(actor: &Actor) -> AgentRecord {
        AgentRecord {
            id: String::new(),
            name: actor.name.clone(),
            machine: actor.device_id.clone(),
            host_scope: actor.kind.clone(),
            native_machine: actor.kind.clone(),
            session: actor.name.clone(),
            instance: actor.pane_id.clone(),
            pane: actor.pane_id.clone(),
            parent: None,
            origin: None,
            project: None,
            actor: actor.clone(),
            ended: false,
            forked: false,
        }
    }

    fn register(ledger: &mut Ledger, actor: &Actor) -> String {
        apply(
            ledger,
            actor,
            &Mutation::Register {
                record: record(actor),
                check: false,
            },
            1,
        )
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Where a spawn's child lives: its machine, its pane and the session
    /// its pane runs.
    pub(crate) struct Child<'a> {
        pub(crate) machine: &'a str,
        pub(crate) pane: &'a str,
        pub(crate) session: &'a str,
    }

    /// A completed spawn by `caller` (registered as `parent`): a handoff, or
    /// with `delegated` a delegation of `parent`. The child's checkout is
    /// `/repo.worktrees/<branch>`.
    fn spawn(
        ledger: &mut Ledger,
        caller: &Actor,
        parent: &str,
        delegated: bool,
        task: Option<SpawnTask>,
        branch: &str,
        child: Child,
    ) -> Result<String, String> {
        let command = crate::coordination::Command::Spawn {
            parent: delegated.then(|| parent.to_owned()),
            machine: (child.machine != caller.device_id).then(|| child.machine.to_owned()),
            name: format!("child-{branch}"),
            intent: format!("intent-{branch}"),
            kind: "claude".into(),
            repo: "/repo".into(),
            branch: branch.into(),
            path: None,
            args: Vec::new(),
            task,
        };
        let reserved = apply(
            ledger,
            caller,
            &Mutation::Reserve {
                parent: parent.to_owned(),
                command,
            },
            1,
        )?;
        let id = reserved["id"].as_str().unwrap().to_owned();
        let mut actor = pane_on(child.machine, child.pane, child.session);
        actor.name = format!("child-{branch}");
        let mut child_record = record(&actor);
        let mode = if delegated {
            SpawnMode::Delegation
        } else {
            SpawnMode::Handoff
        };
        child_record.parent = mode.responsibility(parent);
        child_record.origin = mode.origin(parent);
        child_record.project = Some(format!("/repo.worktrees/{branch}"));
        apply(
            ledger,
            caller,
            &Mutation::Advance {
                id: id.clone(),
                path: Some(format!("/repo.worktrees/{branch}")),
                pane: Some(actor.pane_id.clone()),
                terminal: None,
                child: None,
            },
            1,
        )?;
        apply(
            ledger,
            caller,
            &Mutation::BindChild {
                id: id.clone(),
                record: child_record,
            },
            1,
        )?;
        apply(ledger, caller, &Mutation::Complete { id: id.clone() }, 1)?;
        Ok(id)
    }

    pub(crate) fn memory_task(target: &str) -> Option<SpawnTask> {
        Some(SpawnTask {
            kind: crate::coordination::MEMORY_PROMOTE_TASK.into(),
            target: Some(target.into()),
        })
    }

    /// A ledger in which Hide, as `node`, started one agent for a card on
    /// `child`'s machine, in the checkout `/repo.worktrees/<branch>`.
    pub(crate) fn hide_started_ledger(
        node: &str,
        branch: &str,
        task: Option<SpawnTask>,
        child: Child,
    ) -> Ledger {
        let mut ledger = Ledger::default();
        let hide = Actor::hide("memory", node);
        let origin = register(&mut ledger, &hide);
        spawn(&mut ledger, &hide, &origin, false, task, branch, child).unwrap();
        ledger
    }

    fn here(pane: &str) -> Child<'_> {
        Child {
            machine: NODE,
            pane,
            session: "session",
        }
    }

    #[test]
    fn an_agent_and_checkout_hide_started_carry_the_task() {
        let ledger = hide_started_ledger(NODE, "hide-memory-m72", memory_task("m72"), here("p72"));
        let started = hide_started(&ledger);
        assert_eq!(started.agents.len(), 1);
        assert_eq!(started.agents[0].task, memory_task("m72"));
        assert_eq!(started.agents[0].pane_id, "p72");
        assert_eq!(
            started.checkouts,
            [StartedCheckout {
                machine: NODE.into(),
                path: "/repo.worktrees/hide-memory-m72".into(),
                task: memory_task("m72"),
            }]
        );
    }

    #[test]
    fn a_hide_start_with_no_task_is_still_hide_ais() {
        let ledger = hide_started_ledger(NODE, "any-name", None, here("p1"));
        let started = hide_started(&ledger);
        assert_eq!(started.agents.len(), 1);
        assert_eq!(started.agents[0].task, None);
    }

    #[test]
    fn a_pane_agents_handoff_a_delegation_and_a_branch_name_are_not_hide_ais() {
        let mut ledger = Ledger::default();
        let operator = pane("operator-pane", "operator");
        let operator_id = register(&mut ledger, &operator);
        // The branch spells Hide AI's naming; the record does not say it.
        spawn(
            &mut ledger,
            &operator,
            &operator_id,
            false,
            None,
            "hide-memory-m1-x",
            here("p1"),
        )
        .unwrap();
        spawn(
            &mut ledger,
            &operator,
            &operator_id,
            true,
            None,
            "delegated",
            here("p2"),
        )
        .unwrap();
        assert_eq!(hide_started(&ledger), HideStarted::default());
    }

    #[test]
    fn a_factory_workers_spawn_is_not_hide_ais() {
        let mut ledger = Ledger::default();
        let factory = Actor::factory("fixture", NODE);
        let origin = register(&mut ledger, &factory);
        spawn(
            &mut ledger,
            &factory,
            &origin,
            true,
            None,
            "factory-801",
            here("p1"),
        )
        .unwrap();
        assert_eq!(hide_started(&ledger), HideStarted::default());
    }

    #[test]
    fn an_ended_agent_leaves_its_checkout_in_place() {
        let mut ledger =
            hide_started_ledger(NODE, "hide-memory-m43", memory_task("m43"), here("p43"));
        for agent in &mut ledger.agents {
            if agent.origin.is_some() {
                agent.ended = true;
            }
        }
        let started = hide_started(&ledger);
        assert!(started.agents.is_empty());
        assert_eq!(started.checkouts.len(), 1);
    }

    #[test]
    fn only_hides_own_identity_may_give_a_task() {
        let mut ledger = Ledger::default();
        let operator = pane("operator-pane", "operator");
        let operator_id = register(&mut ledger, &operator);
        let error = spawn(
            &mut ledger,
            &operator,
            &operator_id,
            false,
            memory_task("m1"),
            "x",
            here("p1"),
        )
        .unwrap_err();
        assert_eq!(error, "task_reserved");
        let hide = Actor::hide("memory", NODE);
        let origin = register(&mut ledger, &hide);
        let error = spawn(
            &mut ledger,
            &hide,
            &origin,
            true,
            memory_task("m1"),
            "y",
            here("p2"),
        )
        .unwrap_err();
        assert_eq!(error, "task_reserved");
    }
}
