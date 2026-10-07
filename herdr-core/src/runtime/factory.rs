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

/// How far up the spawn lineage a caller is followed to its worker.
const LINEAGE_LIMIT: usize = 16;

impl Runtime {
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
        if self.kit_states.contains_key(self.node.as_str()) {
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
}
