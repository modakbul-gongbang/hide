//! What the Factory host reads from and asks of the runtime. Each call takes
//! the lock for owned data only; no subprocess, file or network work happens
//! here (docs/ARCHITECTURE.md).

use crate::delivery::worker::{Authority, Prepared};
use crate::delivery::{Actor, Command};
use crate::runtime::delivery::Observation;
use crate::workspace_control::Query;

use super::Runtime;
use crate::factory::screen::{
    ActionAnswer, FactorySection, FactoryTaskSection, REQUEST_ID_LIMIT, ScreenRequest,
};
use crate::model::Edited;

/// `factory_action`: one person's action on a Factory screen, the stage-1
/// command as the CLI sends it (PRD software-factory-ui).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FactoryActionPayload {
    request_id: String,
    command: hide_factory::Command,
}

/// `factory_task_open`: the Task page a screen shows.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FactoryTaskOpenPayload {
    factory: String,
    task: String,
}

/// A worker pane as the runtime sees it now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkerProbe {
    /// Herdr shows an agent in the pane.
    pub present: bool,
    /// A close Hide asked for is in flight for the pane, so its agent
    /// vanishing is that close, not the worker going away.
    pub closing: bool,
    /// The agent sleeps (or its end is in flight).
    pub asleep: bool,
    /// What the agent is doing, from the status model's axes; work it
    /// delegated counts as working.
    pub activity: crate::agent_state::AgentUse,
    /// When the core saw the agent's state last change (D-52); `None`
    /// before the core has observed it.
    pub changed_at_unix_ms: Option<u64>,
}

/// What a diagnosis may read about a worker (D-37, D-52): the user turn
/// and last answer its adapter declares, from the agent's row, and where
/// to read its screen off the lock.
pub(crate) struct WorkerTextSources {
    pub user_turn: Option<String>,
    pub last_answer: Option<String>,
    pub raw_pane: Option<String>,
    pub connector: Option<std::sync::Arc<dyn hide_herdr_client::ApiConnector>>,
}

/// How far up the spawn lineage a caller is followed to its worker.
const LINEAGE_LIMIT: usize = 16;

impl Runtime {
    pub(crate) fn set_factory_screen_port(&mut self, port: crate::factory::ScreenPort) {
        self.factory_screen = Some(port);
    }

    /// The Factory host's changed values; it builds and compares them off
    /// the lock, so this only swaps them in under a new edit number.
    pub(crate) fn set_factory_screen(
        &mut self,
        summary: Option<std::sync::Arc<hide_factory::FactorySummary>>,
        task: Option<Option<FactoryTaskSection>>,
    ) {
        if let Some(summary) = summary {
            self.snapshot
                .factory
                .get_or_insert_with(Edited::default)
                .edit()
                .summary = Some(summary);
            self.refresh_agent_scopes();
        }
        if let Some(task) = task {
            self.snapshot.factory_task = task.map(Edited::new);
        }
    }

    pub(crate) fn factory_answered(&mut self, answer: ActionAnswer) {
        self.snapshot
            .factory
            .get_or_insert_with(Edited::<FactorySection>::default)
            .edit()
            .push_answer(answer);
    }

    /// Hands a screen action to the engine thread; a queue that cannot take
    /// it is answered at once on the same request id.
    pub(super) fn factory_action(&mut self, payload: FactoryActionPayload) -> bool {
        if payload.request_id.is_empty() || payload.request_id.len() > REQUEST_ID_LIMIT {
            crate::diagnostic!(serde_json::json!({
                "component": "factory",
                "kind": "screen.request_id_invalid",
                "length": payload.request_id.len(),
            }));
            return false;
        }
        let request_id = payload.request_id.clone();
        let sent = match &self.factory_screen {
            Some(port) => port.send(ScreenRequest::Action {
                request_id: payload.request_id,
                command: payload.command,
            }),
            None => Err("factory_unavailable"),
        };
        match sent {
            Ok(()) => false,
            Err(reason) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "factory",
                    "kind": "screen.action_refused",
                    "request_id": request_id,
                    "reason": reason,
                }));
                // Spelled out so the screen's refusal labels can be held to them.
                let refusal = if reason == "factory_busy" {
                    hide_factory::Refusal::new("factory_busy", "Try again in a moment")
                } else {
                    hide_factory::Refusal::new("factory_unavailable", "See the diagnostic log")
                };
                self.factory_answered(ActionAnswer {
                    request_id,
                    answer: refusal.to_json(),
                });
                true
            }
        }
    }

    pub(super) fn factory_task_open(&mut self, payload: FactoryTaskOpenPayload) -> bool {
        self.factory_screen_send(ScreenRequest::OpenTask {
            factory: payload.factory,
            task: payload.task,
        })
    }

    pub(super) fn factory_task_close(&mut self) -> bool {
        self.factory_screen_send(ScreenRequest::CloseTask)
    }

    /// Remembers the Home agent pane started as the Factory secretary (B23).
    /// A pane no tab holds is refused and logged; the screen starts again.
    pub(super) fn factory_secretary_set(&mut self, pane_id: String) -> bool {
        if !self.pane_listed(&pane_id) {
            crate::diagnostic!(serde_json::json!({
                "component": "factory",
                "kind": "screen.secretary_unlisted",
                "pane_id": pane_id,
            }));
            return false;
        }
        if self.snapshot.ui_state.factory_secretary_pane.as_deref() == Some(pane_id.as_str()) {
            return false;
        }
        self.snapshot.ui_state.factory_secretary_pane = Some(pane_id);
        self.persist_ui_state();
        true
    }

    /// A page the queue could not take is logged; the screen keeps its
    /// skeleton and the next open asks again (design #13).
    fn factory_screen_send(&mut self, request: ScreenRequest) -> bool {
        let sent = match &self.factory_screen {
            Some(port) => port.send(request),
            None => Err("factory_unavailable"),
        };
        if let Err(reason) = sent {
            crate::diagnostic!(serde_json::json!({
                "component": "factory",
                "kind": "screen.page_refused",
                "reason": reason,
            }));
        }
        false
    }

    /// Checks a local caller like a delivery request does and names the pane
    /// and checkout it speaks from. A checkout-bound caller is the operator
    /// acting without a pane: its hint is never identity or lineage.
    pub(crate) fn factory_caller(
        &self,
        caller: &str,
        expected: &crate::workspace_control::Context,
        hint: Option<&str>,
    ) -> Result<crate::factory::FactoryCaller, String> {
        let context = self
            .workspace_control_query(self.node.as_str(), caller, Query::Info)
            .map_err(|refusal| refusal.reason.to_owned())?
            .context;
        if context != *expected {
            return Err("caller_context_changed".into());
        }
        let pane = match crate::workspace_control::Caller::parse(caller) {
            crate::workspace_control::Caller::Pane(pane) => Some(pane.to_owned()),
            crate::workspace_control::Caller::Checkout { .. } => None,
        };
        // A hint naming another pane than a pane caller's own can only make
        // the caller a worker, never an operator.
        let claimed = pane
            .as_deref()
            .and(hint)
            .filter(|hint| pane.as_deref() != Some(*hint))
            .map(str::to_owned);
        let ancestors = pane
            .as_deref()
            .map_or_else(crate::factory::Lineage::none, |pane| {
                self.factory_lineage(pane)
            });
        Ok(crate::factory::FactoryCaller {
            pane,
            cwd: Some(context.checkout_path.clone()).filter(|path| !path.is_empty()),
            claimed,
            ancestors,
        })
    }

    pub(crate) fn factory_question_caller(
        &self,
        device: &str,
        caller: &str,
        expected: &crate::workspace_control::Context,
        session: &str,
        runtime: &str,
        terminal_id: &str,
    ) -> Result<crate::factory::QuestionCaller, String> {
        // Factory starts use factory_delivery's core-node authority. A
        // foreign pane cannot be one of those accepted workers, and must
        // not open an SSH API channel under this short question budget.
        if device != self.node.as_str() {
            return Err("factory_guard_local_only".into());
        }
        let crate::workspace_control::Caller::Pane(pane) =
            crate::workspace_control::Caller::parse(caller)
        else {
            return Err("agent_pane_required".into());
        };
        let context = self
            .workspace_control_query(device, caller, Query::Info)
            .map_err(|_| "factory_guard_context_changed")?
            .context;
        if context != *expected
            || !crate::delivery::valid_key(session)
            || !crate::delivery::valid_key(terminal_id)
        {
            return Err("factory_guard_context_changed".into());
        }
        let observation = self
            .delivery_observations
            .get(pane)
            .ok_or("factory_guard_native_unavailable")?;
        let kind = match runtime {
            "claude-code" => "claude",
            "codex" => "codex",
            _ => return Err("factory_guard_runtime_invalid".into()),
        };
        let actor = &observation.actor;
        actor.require_native_identity()?;
        if actor.device_id != device
            || actor.kind != kind
            || crate::wire::session_digest(session) != actor.session
        {
            return Err("factory_guard_native_changed".into());
        }
        Ok(crate::factory::QuestionCaller {
            actor: actor.clone(),
            context,
            raw_pane: observation.raw_pane_id.clone(),
            terminal_id: terminal_id.to_owned(),
            connector: self
                .delivery_connector(device)
                .ok_or("factory_guard_disconnected")?,
        })
    }

    /// Join the accepted spawn to the independently re-read execution.
    /// An ended ledger record can still be the current cancelled/revived
    /// worker; its native identity, not its ended bit, decides that join.
    pub(crate) fn factory_question_current(
        &self,
        caller: &crate::factory::QuestionCaller,
        worker: &hide_factory::model::WorkerRef,
    ) -> Result<(), String> {
        let actor = &caller.actor;
        // Context and connection are re-read without invoking a transport.
        let context = self
            .workspace_control_query(&actor.device_id, &actor.pane_id, Query::Info)
            .map_err(|_| "factory_guard_context_changed")?
            .context;
        let native = self
            .delivery_observations
            .get(&actor.pane_id)
            .ok_or("factory_guard_native_changed")?;
        let connector = self
            .delivery_connector(&actor.device_id)
            .ok_or("factory_guard_disconnected")?;
        if context != caller.context
            || !native.actor.same_identity(actor)
            || native.actor.kind != actor.kind
            || native.actor.name != actor.name
            || native.raw_pane_id != caller.raw_pane
            || !std::sync::Arc::ptr_eq(&connector, &caller.connector)
        {
            return Err("factory_guard_native_changed".into());
        }
        let ledger = self.delivery_state()?;
        let record = worker
            .agent
            .as_deref()
            .and_then(|id| ledger.agents.iter().find(|a| a.id == id))
            .ok_or("factory_guard_spawn_unavailable")?;
        let parent = record
            .parent
            .as_deref()
            .and_then(|id| ledger.agents.iter().find(|a| a.id == id))
            .ok_or("factory_guard_spawn_unavailable")?;
        let native_context = self.coordination_context(&actor.device_id)?;
        if worker.pane.as_deref() != Some(actor.pane_id.as_str())
            || worker.runtime.as_str() != actor.kind
            || !record.actor.same_identity(actor)
            || record.actor.kind != actor.kind
            || record.pane != actor.pane_id
            || record.native_machine != native_context.machine
            || record.host_scope != native_context.host_scope
            || !parent.actor.same_identity(&crate::delivery::Actor::factory(
                &worker.factory,
                &actor.device_id,
            ))
            || !parent.actor.code_owned()
        {
            return Err("factory_guard_spawn_changed".into());
        }
        Ok(())
    }

    /// The agents above `pane` in the spawn lineage, nearest first, from
    /// the ledger in memory; at most [`LINEAGE_LIMIT`] steps.
    fn factory_lineage(&self, pane: &str) -> crate::factory::Lineage {
        let mut lineage = crate::factory::Lineage {
            complete: false,
            ..crate::factory::Lineage::none()
        };
        let Ok(ledger) = self.delivery_state() else {
            return lineage;
        };
        // The newest record on the pane, ended or not: an agent can end its
        // own record and keep running, so ending it never makes an operator.
        // Nor does registering again without a parent: the same agent's
        // newest record that names one still decides where it sits.
        let newest = ledger.agents.iter().rev().find(|agent| agent.pane == pane);
        let start = newest.and_then(|newest| {
            ledger
                .agents
                .iter()
                .rev()
                .filter(|agent| agent.pane == pane && agent.actor.same_identity(&newest.actor))
                .find(|agent| agent.parent.is_some())
                .or(Some(newest))
        });
        let mut next = match start {
            Some(agent) => {
                lineage.factory_spawned |= agent.actor.code_owned();
                agent.parent.clone()
            }
            None => None,
        };
        while let Some(id) = next {
            if lineage.agents.len() >= LINEAGE_LIMIT || lineage.agents.contains(&id) {
                return lineage;
            }
            let Some(agent) = ledger.agents.iter().find(|agent| agent.id == id) else {
                lineage.agents.push(id);
                return lineage;
            };
            lineage.agents.push(id);
            lineage.factory_spawned |= agent.actor.code_owned();
            // An ended agent's pane may be someone else's now.
            if !agent.ended {
                lineage.panes.push(agent.pane.clone());
            }
            next = agent.parent.clone();
        }
        lineage.complete = true;
        lineage
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
        let recipient = Actor::factory(factory, self.node.as_str());
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
        use crate::agent_state::AgentUse;
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
            closing: self.panes_closing.contains(pane) || self.factory_closes_sent.contains(pane),
            asleep,
            activity: match agent {
                None => AgentUse::Unknown,
                Some(agent) if crate::agent_state::has_work_in_progress(agent) => AgentUse::Working,
                Some(agent) => AgentUse::of(&agent.demand, agent.blocked, &agent.activity),
            },
            changed_at_unix_ms: agent.and_then(|agent| agent.changed_at_unix_ms),
        }
    }

    /// Owned copies of what a worker's adapter declares it reports, and
    /// the connection its screen is read through; nothing is read here.
    pub(crate) fn factory_worker_texts(&self, pane: &str) -> WorkerTextSources {
        use hide_agent_adapter::Capability;
        let agent = self
            .snapshot
            .navigator
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane);
        let factory = agent
            .and_then(|agent| hide_agent_adapter::adapter(&agent.agent_kind))
            .map(|row| row.factory);
        let user_turn = factory
            .filter(|f| matches!(f.user_turn, Capability::Available(_)))
            .and(agent)
            .and_then(|agent| agent.user_turn.as_ref())
            .and_then(|turn| turn.content.as_ref())
            .map(|content| {
                let mut text = content.text().to_owned();
                for choice in content.choices() {
                    text.push_str("\n- ");
                    text.push_str(choice);
                }
                text
            });
        let last_answer = factory
            .filter(|f| matches!(f.turn_end_and_answer, Capability::Available(_)))
            .and(agent)
            .and_then(|agent| agent.request.as_ref())
            .and_then(|request| request.reply.as_ref())
            .map(|reply| reply.text.clone());
        WorkerTextSources {
            user_turn,
            last_answer,
            raw_pane: self
                .delivery_observations
                .get(pane)
                .map(|observation| observation.raw_pane_id.clone()),
            connector: self.delivery_connector(self.node.as_str()),
        }
    }

    /// Panes the operator closed in Hide: a Factory worker among them pauses
    /// its Task instead of being started again (D-26).
    pub(crate) fn factory_panes_closed(&mut self, panes: &[String]) {
        if panes.is_empty() {
            return;
        }
        let sent = match &self.factory_screen {
            Some(port) => port.panes_closed(panes.to_vec()),
            None => return,
        };
        match sent {
            // Marked under the same lock as the send, so a tick that reads a
            // worker after the close guard cleared still sees it closing.
            Ok(()) => self.factory_closes_sent.extend(panes.iter().cloned()),
            Err(reason) => crate::diagnostic!(serde_json::json!({
                "component": "factory",
                "kind": "worker.close_unsent",
                "panes": panes,
                "reason": reason,
            })),
        }
    }

    /// The engine has taken these closes: its Tasks are paused, so the
    /// panes need no closing mark any more.
    pub(crate) fn factory_closes_taken(&mut self, panes: &[String]) {
        for pane in panes {
            self.factory_closes_sent.remove(pane);
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

    /// Whether a start of `kind` may go: this Mac's kit has answered since
    /// launch, or the start does not need it (`codex_launch`). When not,
    /// asks the kit to read the machine; the Factory starts workers with no
    /// Settings on screen.
    pub(crate) fn factory_kit_read(&mut self, kind: &str) -> bool {
        if !crate::codex_launch::needs_kit_answer(kind)
            || self.kit_states.contains_key(self.node.as_str())
        {
            return true;
        }
        self.request_kit_check();
        false
    }

    /// When `provider`'s usage limit resets, while the usage rows show a
    /// window used up (B58).
    pub(crate) fn factory_usage_limit(&self, provider: &str, now_unix_ms: u64) -> Option<u64> {
        usage_limit(
            &self.snapshot.navigator.provider_usage,
            provider,
            now_unix_ms,
        )
    }

    pub(crate) fn factory_ai_settings(&self) -> Option<hide_ai::AiSettings> {
        self.ai_settings.clone()
    }
}

/// The latest reset of a used-up window of `provider` still ahead of now,
/// in milliseconds: the main row or any bucket at 100 percent.
fn usage_limit(
    rows: &[crate::model::ProviderUsageSnapshot],
    provider: &str,
    now_unix_ms: u64,
) -> Option<u64> {
    let row = rows.iter().find(|row| row.provider == provider)?;
    std::iter::once((row.used_percent, row.resets_at_unix_seconds))
        .chain(
            row.buckets
                .iter()
                .map(|bucket| (bucket.used_percent, bucket.resets_at_unix_seconds)),
        )
        .filter_map(|(used, resets)| match (used, resets) {
            (Some(used), Some(resets)) if used >= 100.0 => Some(resets.saturating_mul(1_000)),
            _ => None,
        })
        .filter(|resets| *resets > now_unix_ms)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordination::AgentRecord;
    use crate::delivery::ledger::Ledger;
    use crate::model::SidebarAgentSnapshot;
    use crate::model::{ProviderUsageBucketSnapshot, ProviderUsageSnapshot};
    use std::sync::Arc;

    fn agent(id: &str, pane: &str, parent: Option<&str>) -> AgentRecord {
        AgentRecord {
            id: id.into(),
            name: id.into(),
            machine: crate::node::TEST_NODE.into(),
            host_scope: "fixture".into(),
            native_machine: "fixture-machine".into(),
            session: id.into(),
            instance: pane.into(),
            pane: pane.into(),
            parent: parent.map(str::to_owned),
            origin: None,
            project: None,
            actor: match pane.strip_prefix("factory:") {
                Some(factory) => Actor::factory(factory, crate::node::TEST_NODE),
                None => Actor {
                    pane_id: pane.into(),
                    name: id.into(),
                    kind: "claude".into(),
                    device_id: crate::node::TEST_NODE.into(),
                    session: crate::wire::session_digest(id),
                },
            },
            ended: false,
        }
    }

    #[test]
    fn a_question_requires_the_current_spawn_session_even_when_its_ledger_record_ended() {
        let root = tempfile::tempdir().unwrap();
        let herdr = crate::fake_herdr::FakeHerdr::start("question-worker", |method, _| {
            panic!("in-memory authority checks must not call {method}")
        });
        let (runtime, _, _, _) = crate::runtime::delivery::tests::fixture(root.path());
        let mut runtime = runtime.lock().unwrap();
        let observed =
            crate::runtime::delivery::tests::recipient_at_rest(&mut runtime, "native-1", &herdr);
        let context = crate::runtime::delivery::tests::authority(&observed.actor).context;
        let native = runtime
            .coordination_context(crate::node::TEST_NODE)
            .unwrap();
        let mut accepted = agent("accepted-worker", "recipient", Some("factory-parent"));
        accepted.actor = observed.actor.clone();
        accepted.host_scope = native.host_scope;
        accepted.native_machine = native.machine;
        accepted.ended = true;
        runtime.delivery_ledger = Ok(Arc::new(Ledger {
            agents: vec![agent("factory-parent", "factory:f-1", None), accepted],
            ..Ledger::default()
        }));
        let worker = hide_factory::model::WorkerRef {
            factory: "f-1".into(),
            agent: Some("accepted-worker".into()),
            name: "worker".into(),
            pane: Some("recipient".into()),
            runtime: hide_factory::model::Runtime::CODEX,
            worktree: "/checkouts/fixture".into(),
            branch: "task".into(),
            started_at: 1,
            asleep: false,
            model: None,
            effort: None,
        };
        let caller = runtime
            .factory_question_caller(
                crate::node::TEST_NODE,
                "recipient",
                &context,
                "native-1",
                "codex",
                "terminal-1",
            )
            .unwrap();
        assert_eq!(
            runtime.factory_question_current(&caller, &worker),
            Ok(()),
            "cancel/revive may retain this accepted native execution with an ended registration"
        );
        let descendant = hide_factory::model::WorkerRef {
            pane: Some("sender".into()),
            ..worker.clone()
        };
        assert!(
            runtime
                .factory_question_current(&caller, &descendant)
                .is_err()
        );
        let missing = hide_factory::model::WorkerRef {
            agent: Some("not-registered".into()),
            ..worker.clone()
        };
        assert!(runtime.factory_question_current(&caller, &missing).is_err());

        crate::runtime::delivery::tests::recipient_at_rest(&mut runtime, "native-2", &herdr);
        assert!(
            runtime.factory_question_current(&caller, &worker).is_err(),
            "reused pane is a different execution"
        );
        assert!(
            runtime
                .factory_question_caller(
                    crate::node::TEST_NODE,
                    "recipient",
                    &context,
                    "native-1",
                    "codex",
                    "terminal-1",
                )
                .is_err(),
            "the old hook's native generation cannot attest the new execution"
        );
        let replaced = runtime
            .factory_question_caller(
                crate::node::TEST_NODE,
                "recipient",
                &context,
                "native-2",
                "codex",
                "terminal-1",
            )
            .unwrap();
        assert!(
            runtime
                .factory_question_current(&replaced, &worker)
                .is_err(),
            "an ended ledger by itself proves nothing about the replacement"
        );
    }

    #[test]
    fn foreign_checkout_and_changed_connection_question_callers_are_unproven() {
        let root = tempfile::tempdir().unwrap();
        let herdr = crate::fake_herdr::FakeHerdr::start("question-authority", |method, _| {
            panic!("refused callers must not open a native request: {method}")
        });
        let (runtime, _, _, _) = crate::runtime::delivery::tests::fixture(root.path());
        let mut runtime = runtime.lock().unwrap();
        let observed =
            crate::runtime::delivery::tests::recipient_at_rest(&mut runtime, "native-1", &herdr);
        let context = crate::runtime::delivery::tests::authority(&observed.actor).context;
        assert!(matches!(runtime.factory_question_caller(
            "foreign-device", "recipient", &context, "native-1", "codex", "terminal-1",
        ), Err(reason) if reason == "factory_guard_local_only"));
        let checkout = crate::workspace_control::checkout_caller_id("cap", "/checkouts/fixture");
        assert!(matches!(runtime.factory_question_caller(
            crate::node::TEST_NODE, &checkout, &context, "native-1", "codex", "terminal-1",
        ), Err(reason) if reason == "agent_pane_required"));
        let caller = runtime
            .factory_question_caller(
                crate::node::TEST_NODE,
                "recipient",
                &context,
                "native-1",
                "codex",
                "terminal-1",
            )
            .unwrap();
        crate::runtime::delivery::tests::recipient_at_rest(&mut runtime, "native-1", &herdr);
        let worker = hide_factory::model::WorkerRef {
            factory: "f-1".into(),
            agent: Some("accepted-worker".into()),
            name: "worker".into(),
            pane: Some("recipient".into()),
            runtime: hide_factory::model::Runtime::CODEX,
            worktree: "/checkouts/fixture".into(),
            branch: "task".into(),
            started_at: 1,
            asleep: false,
            model: None,
            effort: None,
        };
        assert_eq!(
            runtime.factory_question_current(&caller, &worker),
            Err("factory_guard_native_changed".into()),
            "the same native id on a replaced connection cannot reuse its proof"
        );
    }

    #[test]
    fn a_caller_s_lineage_names_every_agent_above_it_and_stops_at_a_loop() {
        let mut runtime = crate::runtime::tests::runtime();
        runtime.delivery_ledger = Ok(Arc::new(Ledger {
            agents: vec![
                agent("agent-factory", "factory:f-1", None),
                agent("agent-worker", "w1:p1", Some("agent-factory")),
                agent("agent-child", "w2:p1", Some("agent-worker")),
                agent("agent-grandchild", "w3:p1", Some("agent-child")),
                agent("agent-a", "w9:p1", Some("agent-b")),
                agent("agent-b", "w9:p2", Some("agent-a")),
            ],
            ..Ledger::default()
        }));
        let full = runtime.factory_lineage("w3:p1");
        assert_eq!(
            full.agents,
            vec!["agent-child", "agent-worker", "agent-factory"]
        );
        assert_eq!(full.panes, vec!["w2:p1", "w1:p1", "factory:f-1"]);
        assert!(full.complete && full.factory_spawned);
        let none = runtime.factory_lineage("w0:p1");
        assert!(none.agents.is_empty() && none.complete);
        let looped = runtime.factory_lineage("w9:p1");
        assert_eq!(
            looped.agents,
            vec!["agent-b", "agent-a"],
            "a loop ends the walk"
        );
        assert!(!looped.complete, "a loop cannot rule out a worker above");
        assert!(!looped.factory_spawned);
    }

    #[test]
    fn a_checkout_bound_caller_is_the_operator_and_its_hint_is_ignored() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, actor, _, _) = crate::runtime::delivery::tests::fixture(root.path());
        let context = crate::runtime::delivery::tests::authority(&actor).context;
        let mut runtime = runtime.lock().unwrap();
        // The hinted pane is a worker's child with a lineage of its own; a
        // checkout caller's hint is not read, so none of it applies.
        runtime.delivery_ledger = Ok(Arc::new(Ledger {
            agents: vec![
                agent("agent-worker", "w1:p1", None),
                agent("agent-sender", "sender", Some("agent-worker")),
            ],
            ..Ledger::default()
        }));
        let checkout = crate::workspace_control::checkout_caller_id("cap", "/checkouts/fixture");
        for hint in [None, Some("sender"), Some("w1:p1")] {
            let caller = runtime.factory_caller(&checkout, &context, hint).unwrap();
            assert_eq!(caller.pane, None, "hint {hint:?}");
            assert_eq!(caller.claimed, None, "hint {hint:?}");
            assert_eq!(
                caller.ancestors,
                crate::factory::Lineage::none(),
                "hint {hint:?}"
            );
            assert_eq!(caller.cwd.as_deref(), Some("/checkouts/fixture"));
        }
        // A pane caller keeps its own pane and lineage; a hint naming another
        // pane can only make it that pane's worker.
        let own = runtime
            .factory_caller("sender", &context, Some("sender"))
            .unwrap();
        assert_eq!(own.pane.as_deref(), Some("sender"));
        assert_eq!(own.claimed, None);
        assert_eq!(own.ancestors.agents, vec!["agent-worker"]);
        let other = runtime
            .factory_caller("sender", &context, Some("w1:p1"))
            .unwrap();
        assert_eq!(other.pane.as_deref(), Some("sender"));
        assert_eq!(other.claimed.as_deref(), Some("w1:p1"));
    }

    #[test]
    fn an_ended_agent_still_walks_up_but_lends_no_pane() {
        let mut runtime = crate::runtime::tests::runtime();
        let ended = |mut record: AgentRecord| {
            record.ended = true;
            record
        };
        runtime.delivery_ledger = Ok(Arc::new(Ledger {
            agents: vec![
                agent("agent-factory", "factory:f-1", None),
                ended(agent("agent-worker", "w1:p1", Some("agent-factory"))),
                ended(agent("agent-child", "w2:p1", Some("agent-worker"))),
                agent("agent-grandchild", "w3:p1", Some("agent-child")),
            ],
            ..Ledger::default()
        }));
        // An agent that ended its own record still walks up to its worker.
        let ended_child = runtime.factory_lineage("w2:p1");
        assert_eq!(ended_child.agents, vec!["agent-worker", "agent-factory"]);
        assert!(ended_child.complete && ended_child.factory_spawned);
        // Ended ancestors stay in the walk but lend no pane to bind by.
        let live = runtime.factory_lineage("w3:p1");
        assert_eq!(
            live.agents,
            vec!["agent-child", "agent-worker", "agent-factory"]
        );
        assert_eq!(live.panes, vec!["factory:f-1"]);
    }

    #[test]
    fn registering_again_without_a_parent_keeps_an_agent_below_its_worker() {
        let mut runtime = crate::runtime::tests::runtime();
        let mut child = agent("agent-child", "w2:p1", Some("agent-worker"));
        child.ended = true;
        // The same agent, on the same pane and session, registered again
        // with no parent after ending its first record.
        let again = AgentRecord {
            id: "agent-child-again".into(),
            parent: None,
            ..child.clone()
        };
        // A different agent later on a reused pane id is its own.
        let mut reused = agent("agent-other", "w3:p1", None);
        let mut gone = agent("agent-gone", "w3:p1", Some("agent-worker"));
        gone.ended = true;
        reused.ended = false;
        runtime.delivery_ledger = Ok(Arc::new(Ledger {
            agents: vec![
                agent("agent-factory", "factory:f-1", None),
                agent("agent-worker", "w1:p1", Some("agent-factory")),
                child,
                again,
                gone,
                reused,
            ],
            ..Ledger::default()
        }));
        let walked = runtime.factory_lineage("w2:p1");
        assert_eq!(walked.agents, vec!["agent-worker", "agent-factory"]);
        assert!(walked.factory_spawned);
        let other = runtime.factory_lineage("w3:p1");
        assert!(other.agents.is_empty(), "{:?}", other.agents);
        assert!(!other.factory_spawned);
    }

    fn row(provider: &str, used: f64, bucket: Option<(f64, u64)>) -> ProviderUsageSnapshot {
        ProviderUsageSnapshot {
            provider: provider.into(),
            label: provider.into(),
            window_minutes: 10_080,
            state: "available".into(),
            used_percent: Some(used),
            resets_at_unix_seconds: Some(5_000),
            message: None,
            last_checked_at_unix_ms: None,
            last_success_at_unix_ms: None,
            last_error_kind: None,
            buckets: bucket
                .map(|(used, resets)| ProviderUsageBucketSnapshot {
                    label: "Current session".into(),
                    state: "available".into(),
                    used_percent: Some(used),
                    resets_at_unix_seconds: Some(resets),
                    message: None,
                })
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn a_used_up_window_limits_its_runtime_until_its_reset() {
        let rows = [
            row("claude", 40.0, Some((100.0, 2_000))),
            row("codex", 99.0, None),
        ];
        assert_eq!(usage_limit(&rows, "claude", 1_000_000), Some(2_000_000));
        assert_eq!(
            usage_limit(&rows, "claude", 2_000_000),
            None,
            "the reset passed"
        );
        assert_eq!(
            usage_limit(&rows, "codex", 1_000_000),
            None,
            "99 percent still runs"
        );
        assert_eq!(usage_limit(&[], "claude", 0), None);
    }

    /// A worker row with the status model's axes set directly.
    fn worker_row(pane: &str, kind: &str, demand: &str, activity: &str) -> SidebarAgentSnapshot {
        let mut row = crate::sidebar::project_agents(
            crate::sidebar::owned_label_fixture(serde_json::json!({"agents": [
                {"id": pane, "pane_id": pane, "agent_status": "idle", "state_change_seq": 1}
            ]}))
            .unwrap(),
        )
        .agents
        .remove(0);
        row.agent_kind = kind.into();
        row.demand = demand.into();
        row.activity = activity.into();
        row.blocked = false;
        row.group = "seen".into();
        row.changed_at_unix_ms = Some(5_000);
        row
    }

    #[test]
    fn a_worker_rests_only_when_its_row_says_stopped() {
        use crate::agent_state::AgentUse;
        let mut runtime = crate::runtime::tests::runtime();
        runtime.snapshot.navigator.agents = vec![
            worker_row("quiet", "claude", "none", "stopped"),
            worker_row("unsure", "claude", "none", "unknown"),
            worker_row("asking", "claude", "question", "stopped"),
            worker_row("busy", "claude", "none", "working"),
        ];
        let probe = runtime.factory_worker_probe("quiet");
        assert_eq!(
            (probe.present, probe.activity, probe.changed_at_unix_ms),
            (true, AgentUse::Quiet, Some(5_000))
        );
        assert_eq!(
            runtime.factory_worker_probe("unsure").activity,
            AgentUse::Unknown,
            "what Hide cannot tell is never rest (D-52)"
        );
        assert_eq!(
            runtime.factory_worker_probe("asking").activity,
            AgentUse::Waiting
        );
        assert_eq!(
            runtime.factory_worker_probe("busy").activity,
            AgentUse::Working
        );
        let gone = runtime.factory_worker_probe("gone");
        assert_eq!((gone.present, gone.activity), (false, AgentUse::Unknown));
        assert!(!gone.closing);
        runtime.panes_closing.insert("quiet".into());
        assert!(runtime.factory_worker_probe("quiet").closing);
    }

    #[test]
    fn a_diagnosis_reads_only_what_the_worker_s_adapter_declares() {
        let mut runtime = crate::runtime::tests::runtime();
        let mut claude = worker_row("claude-pane", "claude", "question", "stopped");
        claude.user_turn = Some(hide_session::turns::UserTurnFact {
            kind: hide_session::turns::UserTurnKind::Question,
            content: Some(hide_session::turns::UserTurnContent::new(
                "Blue or green?",
                ["blue", "green"],
            )),
        });
        let mut unknown = claude.clone();
        unknown.pane_id = "other-pane".into();
        unknown.agent_kind = "some-new-agent".into();
        runtime.snapshot.navigator.agents = vec![claude, unknown];
        let texts = runtime.factory_worker_texts("claude-pane");
        assert_eq!(
            texts.user_turn.as_deref(),
            Some("Blue or green?\n- blue\n- green")
        );
        assert_eq!(
            runtime.factory_worker_texts("other-pane").user_turn,
            None,
            "an agent with no adapter declares nothing"
        );
    }

    #[test]
    fn only_a_start_that_takes_the_codex_flag_waits_for_the_kit() {
        assert!(crate::codex_launch::needs_kit_answer("codex"));
        assert!(!crate::codex_launch::needs_kit_answer("claude"));
        assert!(!crate::codex_launch::needs_kit_answer("unknown-agent"));
    }
}
