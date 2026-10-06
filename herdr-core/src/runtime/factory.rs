//! What the Factory host reads from and asks of the runtime. Each call takes
//! the lock for owned data only; no subprocess, file or network work happens
//! here (docs/ARCHITECTURE.md).

use crate::delivery::worker::{Authority, Prepared};
use crate::delivery::{Actor, Command};
use crate::runtime::delivery::Observation;
use crate::workspace_control::Query;

use super::Runtime;

/// A worker pane as the runtime sees it now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkerProbe {
    /// Herdr shows an agent in the pane.
    pub present: bool,
    /// The agent sleeps (or its end is in flight).
    pub asleep: bool,
    pub working: bool,
    /// The agent waits for the person in its own pane.
    pub waiting: bool,
    pub status_changed_at_unix_ms: u64,
}

impl Runtime {
    /// Checks a local caller like a delivery request does and names the pane
    /// and checkout it speaks from.
    pub(crate) fn factory_caller(
        &self,
        caller: &str,
        expected: &crate::workspace_control::Context,
        hint: Option<&str>,
    ) -> Result<crate::factory::FactoryCaller, String> {
        let context = self
            .workspace_control_query("local", caller, Query::Info)
            .map_err(|refusal| refusal.reason.to_owned())?
            .context;
        if context != *expected {
            return Err("caller_context_changed".into());
        }
        let pane = match crate::workspace_control::Caller::parse(caller) {
            crate::workspace_control::Caller::Pane(pane) => Some(pane.to_owned()),
            crate::workspace_control::Caller::Checkout { .. } => hint
                .filter(|pane| {
                    self.workspace_control_query("local", pane, Query::Info)
                        .is_ok_and(|answer| answer.context == context)
                })
                .map(str::to_owned),
        };
        Ok(crate::factory::FactoryCaller {
            pane,
            cwd: Some(context.checkout_path.clone()).filter(|path| !path.is_empty()),
        })
    }

    /// A delivery command the Factory sends as its own recipient, to the pane
    /// `target` when the command has one.
    pub(crate) fn factory_prepare(
        &self,
        id: &str,
        target: Option<&str>,
        command: Command,
    ) -> Result<Prepared, String> {
        let (client, authority, actor) = self.factory_delivery(id)?;
        let target = match target {
            Some(pane) => Some(
                self.delivery_observations
                    .get(pane)
                    .cloned()
                    .ok_or("target_unavailable")?,
            ),
            None => None,
        };
        Ok(Prepared::new(client, authority, actor, target, command))
    }

    /// A worker's report to its Factory as a ledger letter from the worker's
    /// own pane (PRD software-factory: ask/block/propose/done use delivery).
    pub(crate) fn factory_worker_letter(
        &self,
        pane: &str,
        factory: &str,
        command: Command,
    ) -> Result<Prepared, String> {
        let observation = self
            .delivery_observations
            .get(pane)
            .ok_or("agent_pane_required")?;
        let actor = observation.actor.clone();
        actor.require_native_identity()?;
        let context = self
            .workspace_control_query(&actor.device_id, pane, Query::Info)
            .map_err(|_| "agent_pane_required")?
            .context;
        let recipient = Actor::factory(factory);
        if !self.factory_recipient_current(&recipient) {
            return Err("target_unavailable".into());
        }
        Ok(Prepared::new(
            self.delivery_client.clone().ok_or("delivery_unavailable")?,
            Authority {
                caller: pane.to_owned(),
                context,
            },
            actor,
            Some(Observation::code_owned(recipient)),
            command,
        ))
    }

    pub(crate) fn factory_worker_probe(&self, pane: &str) -> WorkerProbe {
        let asleep = self
            .snapshot
            .ui_state
            .agent_sleep
            .records
            .contains_key(pane);
        let agent = self
            .snapshot
            .navigator
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane);
        WorkerProbe {
            present: agent.is_some(),
            asleep,
            working: agent
                .is_some_and(|agent| agent.activity == "working" || agent.group == "working"),
            waiting: agent.is_some_and(|agent| agent.demand != "none" || agent.blocked),
            status_changed_at_unix_ms: self
                .delivery_observations
                .get(pane)
                .map_or(0, |observation| observation.status_changed_at_unix_ms),
        }
    }

    /// Puts a worker to sleep through the agent sleep path (D-14, B27).
    /// `Ok(false)`: not yet, the agent is still in its turn; ask again.
    pub(crate) fn factory_sleep(&mut self, pane: &str) -> Result<bool, &'static str> {
        if self
            .snapshot
            .ui_state
            .agent_sleep
            .records
            .contains_key(pane)
        {
            return Ok(true);
        }
        let Some(agent) = self
            .snapshot
            .navigator
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane)
        else {
            return Err("This pane has no agent");
        };
        match crate::agent_sleep::sleep_refusal(agent) {
            None => {}
            Some("This agent is working") | Some("Hide cannot tell what this agent is doing") => {
                return Ok(false);
            }
            Some(reason) => return Err(reason),
        }
        if self.live.is_none() {
            return Err("Putting an agent to sleep requires a live Herdr connection");
        }
        self.request_agent_sleep(pane);
        Ok(true)
    }

    /// Wakes a sleeping worker in the same pane and session (B27, B54).
    pub(crate) fn factory_wake(&mut self, pane: &str) -> bool {
        if !self
            .snapshot
            .ui_state
            .agent_sleep
            .records
            .contains_key(pane)
        {
            return false;
        }
        self.request_agent_wake(super::agent_sleep::AgentWakePayload {
            pane_id: pane.to_owned(),
            fresh: false,
        })
    }

    /// A local issue for a Factory Task, or the one already carrying its
    /// marker (B13, B73).
    pub(crate) fn factory_local_issue(
        &mut self,
        project: &str,
        title: &str,
        body: &str,
        marker: &str,
    ) -> Result<u32, String> {
        if let Ok(store) = &self.local_issues
            && let Some(existing) = store.project(project).and_then(|project| {
                project
                    .issues
                    .iter()
                    .find(|issue| issue.body.contains(marker))
            })
        {
            return Ok(existing.number);
        }
        self.create_local_issue(project, title, body)
    }

    pub(crate) fn factory_local_issue_read(
        &self,
        project: &str,
        number: u32,
    ) -> Option<crate::local_issues::LocalIssue> {
        self.local_issues
            .as_ref()
            .ok()
            .and_then(|store| store.issue(project, number))
            .cloned()
    }

    /// Whether this Mac's kit has answered since launch; when not, asks it to
    /// read the machine. A Codex start needs that answer (`codex_launch`),
    /// and the Factory starts workers with no Settings on screen.
    pub(crate) fn factory_kit_read(&mut self) -> bool {
        if self
            .kit_states
            .contains_key(crate::workspace::LOCAL_DEVICE_ID)
        {
            return true;
        }
        self.request_kit_check();
        false
    }

    pub(crate) fn factory_ai_settings(&self) -> Option<hide_ai::AiSettings> {
        self.ai_settings.clone()
    }
}
