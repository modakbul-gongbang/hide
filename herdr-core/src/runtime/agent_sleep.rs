//! Agent sleep inside the runtime: the minute decision, the visit that wakes,
//! the three events, and the results the Herdr workers hand back (PRD
//! agent-sleep). What is decided and remembered lives in
//! `crate::agent_sleep`; what touches Herdr and processes lives in
//! `crate::agent_sleep_herdr`. Nothing here blocks: every end and every wake
//! runs on a worker and comes back through `ingest_agent_sleep_*`.

use serde::Deserialize;

use super::*;
use crate::agent_sleep::{
    DECISION_INTERVAL_MS, SLEEP_RETRY_BACKOFF_MS, Settled, SleepPhase, SleepRecord, WakeMode,
    valid_after_hours,
};
use crate::agent_sleep_herdr::{self, WakeOutcome, WakeRequest};

/// Ends in flight at once. A first enable over a long day's panes would
/// otherwise start one worker per idle agent; the rest wait for the next
/// minute's decision.
const MAX_ENDS_IN_FLIGHT: usize = 4;

/// A pane whose agent would not end: left alone until `until`, unless its
/// state changes first, which `changed_at_unix_ms` tells (B8).
#[derive(Clone, Copy, Debug)]
pub(super) struct Backoff {
    until_unix_ms: u64,
    changed_at_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentSleepSetPayload {
    pub(super) after_hours: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentWakePayload {
    pub(super) pane_id: String,
    /// Start new session rather than resuming the conversation (B14).
    #[serde(default)]
    pub(super) fresh: bool,
}

impl Runtime {
    /// The minute decision (B4-B6), run by the local coordinator's tick. A
    /// tick inside the minute is one integer comparison.
    pub(crate) fn tick_agent_sleep(&mut self, now_unix_ms: u64) -> bool {
        if now_unix_ms < self.agent_sleep_next_decision_unix_ms || self.live.is_none() {
            return false;
        }
        self.agent_sleep_next_decision_unix_ms = now_unix_ms.saturating_add(DECISION_INTERVAL_MS);
        let on_screen = self.agent_sleep_on_screen();
        let store = &mut self.snapshot.ui_state.agent_sleep;
        let seen = store.mark_seen(on_screen.iter().map(String::as_str), now_unix_ms);
        self.agent_sleep_backoff.retain(|pane_id, backoff| {
            backoff.until_unix_ms > now_unix_ms
                && store
                    .stamps
                    .get(pane_id)
                    .is_some_and(|stamps| stamps.changed_at_unix_ms == backoff.changed_at_unix_ms)
        });
        let backoff_until = self
            .agent_sleep_backoff
            .iter()
            .map(|(pane_id, backoff)| (pane_id.clone(), backoff.until_unix_ms))
            .collect::<HashMap<_, _>>();
        let due = store.due(
            &self.snapshot.navigator.agents,
            &on_screen,
            self.snapshot.ui_state.agent_sleep_after_hours,
            &backoff_until,
            now_unix_ms,
        );
        let in_flight = store
            .records
            .values()
            .filter(|record| record.phase == SleepPhase::Ending)
            .count();
        let mut changed = false;
        for pane_id in due
            .into_iter()
            .take(MAX_ENDS_IN_FLIGHT.saturating_sub(in_flight))
        {
            changed |= self.begin_agent_sleep(&pane_id, "idle", now_unix_ms);
        }
        if seen && !changed {
            self.persist_ui_state();
        }
        changed
    }

    /// Reconciles the records with a fresh local session before it is
    /// projected, and adds the rows of sleeping agents Herdr stopped listing.
    pub(super) fn settle_agent_sleep(&mut self, payload: &mut SessionSnapshotPayload) {
        let settled = self.snapshot.ui_state.agent_sleep.settle_payload(payload);
        if settled.is_empty() {
            return;
        }
        for event in &settled {
            let (pane_id, kind, phase) = match event {
                Settled::Woke { pane_id, phase } => (pane_id, "agent_sleep.awake", Some(*phase)),
                Settled::Closed { pane_id } => (pane_id, "agent_sleep.closed", None),
            };
            crate::diagnostic!(serde_json::json!({
                "component": "agent_sleep",
                "kind": kind,
                "pane_id": pane_id,
                "phase": phase,
            }));
            self.agent_sleep_dropped_input.remove(pane_id);
            self.agent_sleep_backoff.remove(pane_id);
        }
        self.persist_ui_state();
    }

    /// Marks the projected rows that sleep and moves each awake agent's
    /// change stamp. The stamps are persisted, never published.
    pub(super) fn stamp_agent_sleep(&mut self, agents: &mut [SidebarAgentSnapshot]) {
        let store = &mut self.snapshot.ui_state.agent_sleep;
        store.annotate(agents);
        if store.stamp(agents, unix_milliseconds()) {
            self.persist_ui_state();
        }
    }

    /// A committed visit that changed the tab on screen wakes that tab's
    /// sleeping agents (B12, D-12) and stamps the last look of both tabs.
    /// A preview sends no event, so it never gets here.
    pub(super) fn agent_sleep_visit(&mut self, left_tab_id: Option<&str>) -> bool {
        let Some(entered_tab_id) = self.focused_visible_tab_id() else {
            return false;
        };
        if left_tab_id == Some(entered_tab_id.as_str())
            || !self.device_in_front(self.node.as_str())
        {
            return false;
        }
        let now = unix_milliseconds();
        let entered = self.agent_sleep_tab_panes(&entered_tab_id);
        let mut looked = entered.clone();
        if let Some(left) = left_tab_id {
            looked.extend(self.agent_sleep_tab_panes(left));
        }
        let mut persist = self
            .snapshot
            .ui_state
            .agent_sleep
            .mark_seen(looked.iter().map(String::as_str), now);
        let mut changed = false;
        for pane_id in entered {
            match self.snapshot.ui_state.agent_sleep.records.get_mut(&pane_id) {
                Some(record) if record.phase == SleepPhase::Ending => {
                    record.pending_wake = Some(WakeMode::Resume);
                    persist = true;
                }
                Some(record) if record.phase == SleepPhase::Sleeping => {
                    changed |= self.begin_agent_wake(&pane_id, WakeMode::Resume, "visit");
                }
                _ => {}
            }
        }
        if persist && !changed {
            self.persist_ui_state();
        }
        changed
    }

    pub(super) fn set_agent_sleep_after(&mut self, payload: AgentSleepSetPayload) -> bool {
        if !valid_after_hours(payload.after_hours) {
            self.set_error(
                "agent_sleep.invalid_setting",
                format!(
                    "Sleep idle agents after {:?} hours is not one of the choices",
                    payload.after_hours
                ),
                false,
            );
            return true;
        }
        if self.snapshot.ui_state.agent_sleep_after_hours == payload.after_hours {
            return false;
        }
        self.snapshot.ui_state.agent_sleep_after_hours = payload.after_hours;
        // A new setting is decided on at the next tick, not a minute later.
        self.agent_sleep_next_decision_unix_ms = 0;
        crate::diagnostic!(serde_json::json!({
            "component": "agent_sleep",
            "kind": "agent_sleep.setting",
            "after_hours": payload.after_hours,
        }));
        self.persist_ui_state();
        true
    }

    /// Sleep agent from the pane menu (B15): the same end, without the time
    /// or the screen, because the person asking has already looked.
    pub(super) fn request_agent_sleep(&mut self, pane_id: &str) -> bool {
        let refusal = match self
            .snapshot
            .navigator
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
        {
            None => Some("This pane has no agent"),
            Some(_)
                if self
                    .snapshot
                    .ui_state
                    .agent_sleep
                    .records
                    .contains_key(pane_id) =>
            {
                Some("This agent is already asleep")
            }
            Some(agent) => crate::agent_sleep::sleep_refusal(agent),
        };
        if let Some(reason) = refusal {
            self.set_error("agent_sleep.refused", reason, false);
            return true;
        }
        if self.live.is_none() {
            self.set_error(
                "pane.control_unavailable",
                "Putting an agent to sleep requires a live Herdr connection",
                true,
            );
            return true;
        }
        self.begin_agent_sleep(pane_id, "manual", unix_milliseconds())
    }

    /// Wake agent, Retry, and Start new session (B12-B14). A wake asked for
    /// while the end is in flight runs when it lands; a second one while a
    /// wake is running is the same intent and starts nothing.
    pub(super) fn request_agent_wake(&mut self, payload: AgentWakePayload) -> bool {
        let mode = if payload.fresh {
            WakeMode::Fresh
        } else {
            WakeMode::Resume
        };
        let Some(record) = self
            .snapshot
            .ui_state
            .agent_sleep
            .records
            .get_mut(&payload.pane_id)
        else {
            self.set_error(
                "agent_sleep.not_asleep",
                format!("Pane {} has no sleeping agent", payload.pane_id),
                false,
            );
            return true;
        };
        match record.phase {
            SleepPhase::Waking => false,
            SleepPhase::Ending => {
                record.pending_wake = Some(mode);
                self.persist_ui_state();
                false
            }
            SleepPhase::Sleeping | SleepPhase::Failed => {
                self.begin_agent_wake(&payload.pane_id, mode, "request")
            }
        }
    }

    /// Typed input to a pane whose agent sleeps goes nowhere: the pane shows
    /// no terminal, and the shell under it is not what the operator was
    /// typing to (B12). Logged once per pane. `None` lets the input through.
    pub(super) fn drop_input_to_sleeping_pane(&mut self, pane_id: &str) -> Option<bool> {
        let record = self.snapshot.ui_state.agent_sleep.records.get(pane_id)?;
        if record.phase == SleepPhase::Ending {
            return None;
        }
        if self.agent_sleep_dropped_input.insert(pane_id.to_owned()) {
            crate::diagnostic!(serde_json::json!({
                "component": "agent_sleep",
                "kind": "agent_sleep.input_dropped",
                "pane_id": pane_id,
                "phase": record.phase,
            }));
        }
        Some(false)
    }

    /// The end worker's answer. A refusal or a timeout leaves the agent awake
    /// and the pane alone for an hour (B8).
    pub(crate) fn ingest_agent_sleep_end(
        &mut self,
        pane_id: &str,
        result: Result<u64, String>,
    ) -> bool {
        let now = unix_milliseconds();
        let store = &mut self.snapshot.ui_state.agent_sleep;
        let Some(record) = store.records.get_mut(pane_id) else {
            // The pane closed while its agent was ending.
            crate::diagnostic!(serde_json::json!({
                "component": "agent_sleep",
                "kind": "agent_sleep.slept",
                "pane_id": pane_id,
                "outcome": "pane_gone",
            }));
            return false;
        };
        let provider = record.kind.clone();
        match result {
            Ok(state_change_seq) => {
                record.phase = SleepPhase::Sleeping;
                record.state_change_seq = Some(state_change_seq);
                record.since_unix_ms = now;
                let pending_wake = record.pending_wake.take();
                crate::diagnostic!(serde_json::json!({
                    "component": "agent_sleep",
                    "kind": "agent_sleep.slept",
                    "pane_id": pane_id,
                    "provider": provider,
                    "outcome": "slept",
                }));
                if let Some(mode) = pending_wake {
                    self.begin_agent_wake(pane_id, mode, "visit_during_end");
                } else {
                    self.persist_ui_state();
                    self.refresh_agent_sleep_rows();
                }
                true
            }
            Err(reason) => {
                store.records.remove(pane_id);
                let changed_at_unix_ms = store
                    .stamps
                    .get(pane_id)
                    .map_or(0, |stamps| stamps.changed_at_unix_ms);
                self.agent_sleep_backoff.insert(
                    pane_id.to_owned(),
                    Backoff {
                        until_unix_ms: now.saturating_add(SLEEP_RETRY_BACKOFF_MS),
                        changed_at_unix_ms,
                    },
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "agent_sleep",
                    "kind": "agent_sleep.sleep_failed",
                    "pane_id": pane_id,
                    "provider": provider,
                    "outcome": "left_awake",
                    "reason": reason,
                }));
                self.persist_ui_state();
                false
            }
        }
    }

    /// The wake worker's answer. A started agent is confirmed by the session
    /// update that lists it, which is what clears the record (B16).
    pub(crate) fn ingest_agent_wake(
        &mut self,
        pane_id: &str,
        mode: WakeMode,
        outcome: WakeOutcome,
    ) -> bool {
        let Some(record) = self.snapshot.ui_state.agent_sleep.records.get_mut(pane_id) else {
            crate::diagnostic!(serde_json::json!({
                "component": "agent_sleep",
                "kind": "agent_sleep.woke",
                "pane_id": pane_id,
                "mode": mode,
                "outcome": "confirmed_before_answer",
            }));
            return false;
        };
        let provider = record.kind.clone();
        match outcome {
            WakeOutcome::Started => {
                crate::diagnostic!(serde_json::json!({
                    "component": "agent_sleep",
                    "kind": "agent_sleep.woke",
                    "pane_id": pane_id,
                    "provider": provider,
                    "mode": mode,
                    "outcome": "started",
                }));
                false
            }
            WakeOutcome::Failed { reason, detail } => {
                record.phase = SleepPhase::Failed;
                record.reason = Some(reason.clone());
                crate::diagnostic!(serde_json::json!({
                    "component": "agent_sleep",
                    "kind": "agent_sleep.wake_failed",
                    "pane_id": pane_id,
                    "provider": provider,
                    "mode": mode,
                    "reason": reason,
                    "detail": detail,
                }));
                self.persist_ui_state();
                self.refresh_agent_sleep_rows();
                true
            }
        }
    }

    fn begin_agent_sleep(&mut self, pane_id: &str, trigger: &str, now_unix_ms: u64) -> bool {
        let Some(context) = self.live.clone() else {
            return false;
        };
        let Some(agent) = self
            .snapshot
            .navigator
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
        else {
            return false;
        };
        let cwd = self.agent_sleep_pane_cwd(pane_id);
        let Some(record) = SleepRecord::ending(agent, cwd, now_unix_ms) else {
            return false;
        };
        let kind = record.kind.clone();
        crate::diagnostic!(serde_json::json!({
            "component": "agent_sleep",
            "kind": "agent_sleep.ending",
            "pane_id": pane_id,
            "provider": kind,
            "trigger": trigger,
        }));
        self.snapshot
            .ui_state
            .agent_sleep
            .records
            .insert(pane_id.to_owned(), record);
        if let Err(message) = agent_sleep_herdr::spawn_end(context, pane_id.to_owned(), kind) {
            return self.ingest_agent_sleep_end(pane_id, Err(message));
        }
        self.persist_ui_state();
        // The pane is still awake and drawn so until the end lands (B8).
        false
    }

    fn begin_agent_wake(&mut self, pane_id: &str, mode: WakeMode, trigger: &str) -> bool {
        let Some(context) = self.live.clone() else {
            return false;
        };
        let codex_daemon = self.codex_daemon_for_pane(pane_id);
        let Some(record) = self.snapshot.ui_state.agent_sleep.records.get_mut(pane_id) else {
            return false;
        };
        record.phase = SleepPhase::Waking;
        record.reason = None;
        record.pending_wake = None;
        let args = match mode {
            WakeMode::Resume => {
                let Some(args) = crate::recent_closed::resume_arguments(&ClosedAgent {
                    kind: record.kind.clone(),
                    session_id: Some(record.session_id.clone()),
                }) else {
                    // Only Claude and Codex sleep, so this is a record this
                    // build cannot resume; starting it bare would lose the
                    // conversation without saying so.
                    let detail = format!("{} has no resume arguments", record.kind);
                    return self.ingest_agent_wake(
                        pane_id,
                        mode,
                        WakeOutcome::Failed {
                            reason: "The conversation couldn\u{2019}t be resumed.".to_owned(),
                            detail,
                        },
                    );
                };
                args
            }
            WakeMode::Fresh => Vec::new(),
        };
        let request = WakeRequest {
            pane_id: pane_id.to_owned(),
            kind: record.kind.clone(),
            name: record
                .agent_name
                .clone()
                .unwrap_or_else(|| crate::fork::wake_name(pane_id)),
            mode,
            args,
            cwd: record.cwd.clone(),
            codex_daemon,
        };
        crate::diagnostic!(serde_json::json!({
            "component": "agent_sleep",
            "kind": "agent_sleep.waking",
            "pane_id": pane_id,
            "provider": request.kind,
            "mode": mode,
            "trigger": trigger,
        }));
        if let Err(message) = agent_sleep_herdr::spawn_wake(context, request) {
            return self.ingest_agent_wake(
                pane_id,
                mode,
                WakeOutcome::Failed {
                    reason: "The agent did not become ready in time.".to_owned(),
                    detail: message,
                },
            );
        }
        self.persist_ui_state();
        self.refresh_agent_sleep_rows();
        true
    }

    /// Draws a record's new phase on its row and pane without waiting for
    /// the next session update.
    fn refresh_agent_sleep_rows(&mut self) -> bool {
        let mut agents = self.snapshot.navigator.agents.clone();
        self.snapshot.ui_state.agent_sleep.annotate(&mut agents);
        for agent in &mut agents {
            crate::sidebar::rederive(agent);
        }
        if agents == self.snapshot.navigator.agents {
            return false;
        }
        self.sync_pane_status_from_agents(&agents);
        self.snapshot.navigator.agents = agents;
        // A pane's child chips carry the child's mark.
        self.sync_pane_lineage();
        true
    }

    /// The panes of the tab on this machine's screen; empty while another
    /// device is in front, because none of this machine's tabs is then.
    fn agent_sleep_on_screen(&self) -> HashSet<String> {
        if !self.device_in_front(self.node.as_str()) {
            return HashSet::new();
        }
        self.shown_agent_tabs()
            .iter()
            .flat_map(|tab_id| self.agent_sleep_tab_panes(tab_id))
            .collect()
    }

    fn agent_sleep_tab_panes(&self, tab_id: &str) -> Vec<String> {
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .filter(|tab| tab.id.as_deref() == Some(tab_id))
            .flat_map(|tab| tab.panes.iter().map(|pane| pane.id.clone()))
            .collect()
    }

    fn agent_sleep_pane_cwd(&self, pane_id: &str) -> Option<String> {
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .find(|pane| pane.id == pane_id)
            .map(|pane| pane.cwd.clone())
            .filter(|cwd| !cwd.trim().is_empty())
    }
}
