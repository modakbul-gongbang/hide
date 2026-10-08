//! Close-pane sleeping sessions use the existing writer and close owner.
use super::*;
use crate::agent_sleep::{AgentSleepStore, DormantPhase, DormantRecord, SleepId};

impl Runtime {
    pub(super) fn begin_dormant_sleep(&mut self, pane_id: &str, now: u64) -> bool {
        let Some(agent) = self
            .snapshot
            .navigator
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
            .cloned()
        else {
            return false;
        };
        // Row facts are laid only by the current reference-proven overlay.
        // A path is never treated as a native session id.
        let native_id = agent.row_facts.as_ref().and(agent.session_id.clone());
        let Some(native_session_id) = native_id else {
            self.set_error(
                "agent_sleep.identity_unconfirmed",
                "This session has no confirmed native identity",
                true,
            );
            return true;
        };
        if agent.state_change_seq.is_none() {
            self.set_error(
                "agent_sleep.execution_unconfirmed",
                "This session has no confirmed execution identity",
                true,
            );
            return true;
        }
        let Some(label_owner) =
            hide_session::label_reference_token(&agent.agent_kind, "id", &native_session_id)
        else {
            self.set_error(
                "agent_sleep.identity_unconfirmed",
                "This session's reader cannot confirm its native identity",
                true,
            );
            return true;
        };
        if self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .values()
            .any(|record| {
                record.old_pane_id == pane_id
                    && record.connection_generation == self.live_generation
                    && record.native_session_id == native_session_id
                    && !record.closed
            })
        {
            return false;
        }
        if let Some(reason) = crate::agent_state::rest_refusal(&agent) {
            self.set_error("agent_sleep.refused", reason, false);
            return true;
        }
        let Some(tab) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.device_id == self.node.as_str())
            .flat_map(|workspace| &workspace.checkouts)
            .flat_map(|checkout| &checkout.tabs)
            .find(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
        else {
            self.set_error(
                "agent_sleep.context_unavailable",
                "This session has no current local tab",
                true,
            );
            return true;
        };
        let Some(context) = self.close_context(tab) else {
            self.set_error(
                "agent_sleep.context_unavailable",
                "This session's checkout context is incomplete",
                true,
            );
            return true;
        };
        let Some(cwd) = self.agent_sleep_pane_cwd(pane_id) else {
            self.set_error(
                "agent_sleep.context_unavailable",
                "This session has no working folder",
                true,
            );
            return true;
        };
        let record = DormantRecord {
            phase: DormantPhase::SavingClose,
            revision: 1,
            node_id: self.node.as_str().into(),
            connection_generation: self.live_generation,
            old_pane_id: pane_id.into(),
            old_state_change_seq: agent.state_change_seq,
            kind: agent.agent_kind,
            native_session_id,
            label_owner,
            identity_label: agent.identity_label,
            cwd,
            context,
            close_key: None,
            closed: false,
            wake_pane_id: None,
            wake_tab_id: None,
            since_unix_ms: now,
            transition_started_unix_ms: now,
            reason: None,
        };
        if let Err(reason) = self.snapshot.ui_state.agent_sleep.admit_dormant(record) {
            self.set_error("agent_sleep.admission_refused", reason, false);
            return true;
        }
        self.persist_ui_state();
        true
    }

    fn dormant_execution_matches(&self, record: &DormantRecord) -> bool {
        record.old_state_change_seq.is_some()
            && record.node_id == self.node.as_str()
            && record.connection_generation == self.live_generation
            && self.live.is_some()
            && self
                .close_operations
                .values()
                .filter(|operation| operation.pane_ids.contains(&record.old_pane_id))
                .all(|operation| {
                    record.close_key.as_deref() == Some(operation.request.key.as_str())
                })
            && self.snapshot.navigator.agents.iter().any(|agent| {
                agent.pane_id == record.old_pane_id
                    && hide_agent_adapter::canonical_kind(&agent.agent_kind)
                        == hide_agent_adapter::canonical_kind(&record.kind)
                    && agent.session_id.as_deref() == Some(record.native_session_id.as_str())
                    && agent.state_change_seq == record.old_state_change_seq
                    && agent.row_facts.is_some()
                    && crate::agent_state::rest_refusal(agent).is_none()
            })
    }

    pub(in crate::runtime) fn dormant_close_precondition(
        &self,
        operation: &PendingClose,
    ) -> Option<String> {
        self.snapshot
            .ui_state
            .agent_sleep
            .dormant
            .values()
            .find(|record| record.close_key.as_deref() == Some(operation.request.key.as_str()))
            .filter(|record| !self.dormant_execution_matches(record))
            .map(|_| "The sleeping session changed before its pane could close".into())
    }

    pub(crate) fn dormant_close_effect_is_current(
        &self,
        effect: &live::CloseEffectRequest,
    ) -> bool {
        effect.connection_generation == self.live_generation
            && self
                .close_operations
                .get(&effect.key)
                .is_some_and(|operation| {
                    operation.phase == "transmitting"
                        && operation.connection_generation == effect.connection_generation
                })
            && self
                .snapshot
                .ui_state
                .agent_sleep
                .dormant
                .values()
                .any(|record| {
                    record.close_key.as_deref() == Some(effect.key.as_str())
                        && record.phase == DormantPhase::Closing
                        && self.dormant_execution_matches(record)
                })
    }

    pub(in crate::runtime) fn bind_dormant_close(&mut self, pane_id: &str, key: &str) {
        if let Some(record) = self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .values_mut()
            .find(|record| {
                record.old_pane_id == pane_id
                    && record.connection_generation == self.live_generation
                    && record.phase == DormantPhase::Closing
                    && record.close_key.is_none()
            })
        {
            record.close_key = Some(key.into());
        }
    }

    pub(in crate::runtime) fn defer_dormant_close(&mut self, key: &str) -> bool {
        let Some(record) = self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .values_mut()
            .find(|record| record.close_key.as_deref() == Some(key))
        else {
            return false;
        };
        if let Err(reason) = record.transition(DormantPhase::SavingCloseReady) {
            record.phase = DormantPhase::Failed;
            record.reason = Some(reason.into());
        }
        true
    }

    pub(in crate::runtime) fn confirm_dormant_close(&mut self, operation: &PendingClose) {
        // An external/manual disappearance during capture is not proof of
        // an intentional sleep close. Only an issued effect can settle it.
        if !matches!(
            operation.phase.as_str(),
            "transmitting" | "awaiting_topology" | "unknown"
        ) {
            return;
        }
        if let Some(record) = self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .values_mut()
            .find(|record| {
                record.close_key.as_deref() == Some(operation.request.key.as_str())
                    && record.connection_generation == operation.connection_generation
                    && record.connection_generation == self.live_generation
                    && matches!(
                        record.phase,
                        DormantPhase::Closing | DormantPhase::CloseUnknown
                    )
            })
        {
            record.closed = true;
            if let Err(reason) = record.transition(DormantPhase::Sleeping) {
                record.phase = DormantPhase::Failed;
                record.reason = Some(reason.into());
            }
            self.refresh_dormant_rows();
            self.persist_ui_state();
        }
    }

    fn fail_dormant(&mut self, id: &SleepId, reason: &str) {
        crate::diagnostic!(serde_json::json!({
            "component": "agent_sleep", "kind": "agent_sleep.failed", "sleep_id": id.as_str(),
        }));
        let path = self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .get(id)
            .map(|record| record.context.checkout_path.clone());
        if self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .get(id)
            .is_some_and(|record| !record.closed)
        {
            self.snapshot.ui_state.agent_sleep.dormant.remove(id);
        } else if let Some(record) = self.snapshot.ui_state.agent_sleep.dormant.get_mut(id) {
            record.phase = DormantPhase::Failed;
            record.reason = Some(reason.into());
        }
        if let Some(path) = path {
            self.finish_agent_effect(&path, &format!("sleep:{}", id.as_str()));
        }
        self.set_error("agent_sleep.failed", reason, true);
        self.refresh_dormant_rows();
        self.persist_ui_state();
    }

    pub(in crate::runtime) fn settle_dormant_close_result(&mut self, key: &str) {
        let Some(operation) = self.close_operations.get(key) else {
            return;
        };
        let phase = operation.phase.as_str();
        let Some(id) = self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .iter()
            .find(|(_, record)| record.close_key.as_deref() == Some(key) && !record.closed)
            .map(|(id, _)| id.clone())
        else {
            return;
        };
        match phase {
            "failed" | "refused" => self.fail_dormant(
                &id,
                "The session's pane was not closed. Check its current state before trying again.",
            ),
            "unknown" => {
                let record = self
                    .snapshot
                    .ui_state
                    .agent_sleep
                    .dormant
                    .get_mut(&id)
                    .unwrap();
                record.phase = DormantPhase::CloseUnknown;
                record.reason =
                    Some("The pane's close is unconfirmed; check status before retrying".into());
                self.refresh_dormant_rows();
                self.persist_ui_state();
            }
            _ => {}
        }
    }

    pub(in crate::runtime) fn refresh_dormant_rows(&mut self) -> bool {
        let mut rows = self.snapshot.ui_state.agent_sleep.dormant_snapshots();
        for row in &mut rows {
            row.checking = self
                .dormant_status_check
                .as_ref()
                .is_some_and(|(id, _)| id == &row.sleep_id);
        }
        if self.snapshot.navigator.sleeping_sessions == rows {
            return false;
        }
        self.snapshot.navigator.sleeping_sessions = rows;
        true
    }

    /// Called by the one state writer after its actual durability result.
    pub(in crate::runtime) fn ingest_dormant_saved(
        &mut self,
        saved: &AgentSleepStore,
        succeeded: bool,
    ) -> bool {
        self.with_close_geometry(|runtime| runtime.apply_dormant_saved(saved, succeeded))
    }

    fn apply_dormant_saved(&mut self, saved: &AgentSleepStore, succeeded: bool) -> bool {
        let receipts = self.snapshot.ui_state.agent_sleep.dormant_saved(
            saved,
            self.node.as_str(),
            self.live_generation,
        );
        if receipts.is_empty() {
            return false;
        }
        for (id, phase) in receipts {
            if !succeeded {
                if let Some(key) = self
                    .snapshot
                    .ui_state
                    .agent_sleep
                    .dormant
                    .get(&id)
                    .and_then(|record| record.close_key.clone())
                {
                    self.cancel_close_before_effect(
                        &key,
                        "Sleeping-session persistence was not confirmed".into(),
                    );
                }
                self.fail_dormant(
                    &id,
                    "The sleeping session could not be saved; no new close or start was sent",
                );
                continue;
            }
            let record = self.snapshot.ui_state.agent_sleep.dormant[&id].clone();
            match phase {
                DormantPhase::SavingClose => {
                    if !self.dormant_execution_matches(&record) {
                        self.fail_dormant(
                            &id,
                            "The session changed while it was being saved; its pane was left awake",
                        );
                        continue;
                    }
                    if self.geometry_tab_busy(&record.context.tab_id) {
                        self.fail_dormant(&id, "The tab has a pending layout operation. Wait for it to finish before sleeping this session.");
                        continue;
                    }
                    if let Err(reason) = self
                        .snapshot
                        .ui_state
                        .agent_sleep
                        .dormant
                        .get_mut(&id)
                        .unwrap()
                        .transition(DormantPhase::Closing)
                    {
                        self.fail_dormant(&id, reason);
                        continue;
                    }
                    // Never leave an untagged geometry close queued after an
                    // intent becomes unknown. Admission and capture are atomic
                    // under the same runtime lock.
                    self.close_local_pane(record.old_pane_id.clone(), false, false);
                    let admitted = self
                        .snapshot
                        .ui_state
                        .agent_sleep
                        .dormant
                        .get(&id)
                        .is_some_and(|record| record.close_key.is_some());
                    if !admitted {
                        self.fail_dormant(
                            &id,
                            "The pane close was not admitted; the session was left awake",
                        );
                    }
                }
                DormantPhase::SavingCloseReady => {
                    let Some(key) = record.close_key.as_deref() else {
                        self.fail_dormant(&id, "The saved sleeping session has no close operation");
                        continue;
                    };
                    let Some(operation) = self.close_operations.get(key).cloned() else {
                        self.fail_dormant(
                            &id,
                            "The sleeping session's close operation is no longer current",
                        );
                        continue;
                    };
                    if operation.phase != "preparing"
                        || operation.stage != "durable_sleep"
                        || !self.dormant_execution_matches(&record)
                    {
                        self.cancel_close_before_effect(
                            key,
                            "The session changed while its close was being saved".into(),
                        );
                        self.fail_dormant(&id, "The session changed; no sleeping pane was closed");
                        continue;
                    }
                    if let Err(reason) = self
                        .snapshot
                        .ui_state
                        .agent_sleep
                        .dormant
                        .get_mut(&id)
                        .unwrap()
                        .transition(DormantPhase::Closing)
                    {
                        self.fail_dormant(&id, reason);
                        continue;
                    }
                    if let Some(reason) = self.close_precondition_failure(&operation) {
                        self.cancel_close_before_effect(key, reason.clone());
                        self.fail_dormant(&id, &reason);
                        continue;
                    }
                    let Some(context) = self.live.clone() else {
                        continue;
                    };
                    if let Some(effect) = self.prepare_captured_close_effect(key)
                        && let Err(reason) = live::spawn_saved_close_effect(context, effect)
                    {
                        self.cancel_close_before_effect(key, reason.clone());
                        self.fail_dormant(&id, &reason);
                    }
                }
                DormantPhase::SavingWake => self.start_saved_dormant_tab(&id),
                DormantPhase::SavingStart => self.start_saved_dormant_agent(&id),
                _ => {}
            }
        }
        self.refresh_dormant_rows();
        true
    }

    fn dormant_owner(&self, record: &DormantRecord) -> Option<crate::checkout_owner::OwnerOpen> {
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|project| project.device_id == self.node.as_str())
            .find_map(|project| {
                project
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.path == record.context.checkout_path)
                    .map(|checkout| {
                        super::super::projects::owner_open(
                            project,
                            checkout,
                            self.node.as_str(),
                            &self.node,
                        )
                    })
            })
    }

    pub(in crate::runtime) fn request_dormant_wake(&mut self, id: &SleepId) -> bool {
        let Some(record) = self.snapshot.ui_state.agent_sleep.dormant.get(id).cloned() else {
            self.set_error(
                "agent_sleep.not_found",
                "This sleeping session is no longer available",
                false,
            );
            return true;
        };
        if record.phase.in_flight() {
            return false;
        }
        if !record.closed || !matches!(record.phase, DormantPhase::Sleeping | DormantPhase::Failed)
        {
            self.reject_dormant_action(
                id,
                "agent_sleep.wake_refused",
                "The session's close has not been confirmed",
            );
            return true;
        }
        let args = crate::recent_closed::resume_arguments(&ClosedAgent {
            kind: record.kind.clone(),
            session_id: Some(record.native_session_id.clone()),
        });
        if args.is_none()
            || record.validate().is_err()
            || self.dormant_owner(&record).is_none()
            || record.node_id != self.node.as_str()
            || self.live.is_none()
        {
            self.reject_dormant_action(
                id,
                "agent_sleep.wake_unavailable",
                "This session's reader, checkout or connection is unavailable",
            );
            return true;
        }
        if let Err(reason) = self
            .snapshot
            .ui_state
            .agent_sleep
            .admit_dormant_transition()
        {
            self.reject_dormant_action(id, "agent_sleep.admission_refused", reason);
            return true;
        }
        if !self.reserve_agent_effect(
            &record.context.checkout_path,
            &format!("sleep:{}", id.as_str()),
        ) {
            self.reject_dormant_action(id, "agent_sleep.admission_refused", "This checkout already has pending agent operations. Wait for them to finish, then try again.");
            return true;
        }
        let current = self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .get_mut(id)
            .unwrap();
        if let Err(reason) = current.transition(DormantPhase::SavingWake) {
            self.finish_agent_effect(
                &record.context.checkout_path,
                &format!("sleep:{}", id.as_str()),
            );
            self.fail_dormant(id, reason);
            return true;
        }
        current.connection_generation = self.live_generation;
        current.transition_started_unix_ms = unix_milliseconds();
        self.refresh_dormant_rows();
        self.persist_ui_state();
        true
    }

    pub(in crate::runtime) fn request_dormant_status(&mut self, id: &SleepId) -> bool {
        // Every close is bound and saved with its key before any effect is
        // sent. A recovered intent without that key therefore sent no close;
        // it can be released without replaying or inspecting a pane address.
        if self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .get(id)
            .is_some_and(|record| {
                record.node_id == self.node.as_str()
                    && !record.closed
                    && record.phase == DormantPhase::CloseUnknown
                    && record.close_key.is_none()
            })
        {
            self.fail_dormant(
                id,
                "Sleep was not sent. Check the original pane before trying again.",
            );
            return true;
        }
        if self.dormant_status_check.is_some() {
            if self
                .dormant_status_check
                .as_ref()
                .is_some_and(|(pending, _)| pending == id)
            {
                return false;
            }
            self.reject_dormant_action(id, "agent_sleep.status_busy", "Another sleeping session is being checked. Wait for it to finish, then check again.");
            return true;
        }
        let Some(record) = self.snapshot.ui_state.agent_sleep.dormant.get(id).cloned() else {
            return false;
        };
        if !matches!(
            record.phase,
            DormantPhase::CloseUnknown | DormantPhase::WakeUnknown
        ) {
            return false;
        }
        let Some(context) = self
            .live
            .clone()
            .filter(|_| record.node_id == self.node.as_str())
        else {
            self.reject_dormant_action(
                id,
                "agent_sleep.status_unavailable",
                "Reconnect this device before checking the sleeping session",
            );
            return true;
        };
        let owner = self.dormant_owner(&record);
        self.dormant_status_check = Some((id.clone(), self.live_generation));
        let work = agent_sleep_herdr::DormantWork {
            id: id.clone(),
            record,
        };
        if let Err(reason) =
            agent_sleep_herdr::spawn_dormant_status(context, work, self.live_generation, owner)
        {
            self.dormant_status_check = None;
            self.reject_dormant_action(id, "agent_sleep.status_unavailable", &reason);
        }
        self.refresh_dormant_rows();
        true
    }

    fn reject_dormant_action(&mut self, id: &SleepId, code: &str, reason: &str) {
        if let Some(record) = self.snapshot.ui_state.agent_sleep.dormant.get_mut(id) {
            record.reason = Some(reason.into());
            self.refresh_dormant_rows();
            self.persist_ui_state();
        }
        self.set_error(code, reason, true);
    }

    pub(crate) fn ingest_dormant_status(
        &mut self,
        work: &agent_sleep_herdr::DormantWork,
        generation: u64,
        result: Result<agent_sleep_herdr::DormantStatus, String>,
    ) -> bool {
        if self.dormant_status_check.as_ref() != Some(&(work.id.clone(), generation)) {
            return false;
        }
        self.dormant_status_check = None;
        if generation != self.live_generation
            || work.record.node_id != self.node.as_str()
            || self.snapshot.ui_state.agent_sleep.dormant.get(&work.id) != Some(&work.record)
        {
            return self.refresh_dormant_rows();
        }
        let mut next = work.record.clone();
        match result {
            Ok(observed) => {
                let old_present = Self::close_operation_target_present(
                    &observed.snapshot,
                    &live::CloseCaptureTarget::Pane { pane_id: next.old_pane_id.clone() },
                    &next.context.tab_id,
                );
                self.ingest_session(Ok(observed.snapshot));
                if !next.closed {
                    if !old_present && next.close_key.is_some() {
                        next.closed = true;
                        next.connection_generation = generation;
                        if let Err(reason) = next.transition(DormantPhase::Sleeping) {
                            next.phase = DormantPhase::Failed;
                            next.reason = Some(reason.into());
                        }
                    } else {
                        next.reason = Some("The saved close is still unconfirmed. No close was resent; check the original pane before checking again.".into());
                    }
                } else {
                    match observed.wake_tab {
                        Ok(Some((tab, pane))) => {
                            next.connection_generation = generation;
                            next.wake_tab_id = Some(tab);
                            next.wake_pane_id = Some(pane);
                            next.reason = Some("The wake tab is present. Its saved conversation must be confirmed before sleep is cleared; no start was resent.".into());
                        }
                        Ok(None) => next.reason = Some("No tab carrying this wake intent was confirmed. No start was resent; check the device connection before checking again.".into()),
                        Err(_) => next.reason = Some("The wake tab could not be inspected safely. Resolve its checkout or layout before checking again.".into()),
                    }
                }
            }
            Err(_) => next.reason = Some("Status could not be read. Reconnect this device, then check again; nothing was resent.".into()),
        }
        // Session ingest may have independently confirmed and removed this
        // record. Never resurrect it from the inspection's captured copy.
        if self.snapshot.ui_state.agent_sleep.dormant.get(&work.id) != Some(&work.record) {
            self.refresh_dormant_rows();
            return true;
        }
        if next != work.record {
            self.snapshot
                .ui_state
                .agent_sleep
                .dormant
                .insert(work.id.clone(), next);
            let agents = self.snapshot.navigator.agents.clone();
            self.confirm_dormant_wake(&agents);
            self.refresh_dormant_rows();
            self.persist_ui_state();
            return true;
        }
        self.refresh_dormant_rows()
    }

    fn start_saved_dormant_tab(&mut self, id: &SleepId) {
        let record = self.snapshot.ui_state.agent_sleep.dormant[id].clone();
        let Some(owner) = self.dormant_owner(&record) else {
            self.fail_dormant(id, "The sleeping session's checkout is no longer available");
            return;
        };
        let Some(context) = self.live.clone() else {
            self.fail_dormant(id, "The sleeping session's connection is unavailable");
            return;
        };
        let current = self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .get_mut(id)
            .unwrap();
        if let Err(reason) = current.transition(DormantPhase::Creating) {
            self.fail_dormant(id, reason);
            return;
        }
        let work = agent_sleep_herdr::DormantWork {
            id: id.clone(),
            record: current.clone(),
        };
        if let Err(reason) = agent_sleep_herdr::spawn_dormant_tab(context, work, owner) {
            self.fail_dormant(id, &reason);
        }
    }

    pub(crate) fn dormant_work_is_current(&self, work: &agent_sleep_herdr::DormantWork) -> bool {
        work.record.connection_generation == self.live_generation
            && work.record.node_id == self.node.as_str()
            && self.live.is_some()
            && self.snapshot.ui_state.agent_sleep.dormant.get(&work.id) == Some(&work.record)
    }

    pub(crate) fn ingest_dormant_tab(
        &mut self,
        work: &agent_sleep_herdr::DormantWork,
        result: agent_sleep_herdr::DormantTabOutcome,
    ) -> bool {
        if !self.dormant_work_is_current(work) || work.record.phase != DormantPhase::Creating {
            return false;
        }
        match result {
            agent_sleep_herdr::DormantTabOutcome::Created(tab, pane, snapshot) => {
                self.ingest_session(Ok(*snapshot));
                let current = self
                    .snapshot
                    .ui_state
                    .agent_sleep
                    .dormant
                    .get_mut(&work.id)
                    .unwrap();
                current.wake_tab_id = Some(tab);
                current.wake_pane_id = Some(pane);
                if let Err(reason) = current.transition(DormantPhase::SavingStart) {
                    self.fail_dormant(&work.id, reason);
                    return true;
                }
                self.persist_ui_state();
            }
            agent_sleep_herdr::DormantTabOutcome::NotCreated => {
                self.fail_dormant(&work.id, "The saved working folder is unavailable. Restore it before retrying; no tab was created.");
            }
            agent_sleep_herdr::DormantTabOutcome::Unknown => {
                let current = self
                    .snapshot
                    .ui_state
                    .agent_sleep
                    .dormant
                    .get_mut(&work.id)
                    .unwrap();
                current.phase = DormantPhase::WakeUnknown;
                current.reason = Some(
                    "The wake tab could not be confirmed; check its status before retrying".into(),
                );
                // Retain the bounded admission while an external create is unknown.
                self.persist_ui_state();
            }
        }
        self.refresh_dormant_rows();
        true
    }

    fn start_saved_dormant_agent(&mut self, id: &SleepId) {
        let record = self.snapshot.ui_state.agent_sleep.dormant[id].clone();
        let Some(args) = crate::recent_closed::resume_arguments(&ClosedAgent {
            kind: record.kind.clone(),
            session_id: Some(record.native_session_id.clone()),
        }) else {
            self.fail_dormant(
                id,
                "This session's provider cannot resume its saved conversation",
            );
            return;
        };
        let Some(context) = self.live.clone() else {
            self.fail_dormant(id, "The sleeping session's connection is unavailable");
            return;
        };
        let current = self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .get_mut(id)
            .unwrap();
        if let Err(reason) = current.transition(DormantPhase::Starting) {
            self.fail_dormant(id, reason);
            return;
        }
        current.transition_started_unix_ms = unix_milliseconds();
        let work = agent_sleep_herdr::DormantWork {
            id: id.clone(),
            record: current.clone(),
        };
        if let Err(reason) = agent_sleep_herdr::spawn_dormant_start(context, work, args) {
            self.fail_dormant(id, &reason);
        }
    }

    pub(crate) fn ingest_dormant_start(
        &mut self,
        work: &agent_sleep_herdr::DormantWork,
        outcome: agent_sleep_herdr::DormantStartOutcome,
    ) -> bool {
        if !self.dormant_work_is_current(work) || work.record.phase != DormantPhase::Starting {
            return false;
        }
        match outcome {
            agent_sleep_herdr::DormantStartOutcome::Started => {}
            agent_sleep_herdr::DormantStartOutcome::NotStarted => self.fail_dormant(
                &work.id,
                "The saved conversation could not be resumed. Retry uses the same wake tab.",
            ),
            agent_sleep_herdr::DormantStartOutcome::Unknown => {
                let current = self
                    .snapshot
                    .ui_state
                    .agent_sleep
                    .dormant
                    .get_mut(&work.id)
                    .unwrap();
                current.phase = DormantPhase::WakeUnknown;
                current.reason =
                    Some("The resume result is unknown; check its status before retrying".into());
                self.persist_ui_state();
            }
        }
        self.refresh_dormant_rows();
        true
    }

    pub(super) fn confirm_dormant_wake(&mut self, agents: &[SidebarAgentSnapshot]) {
        let waiting = |record: &DormantRecord| {
            record.closed
                && record.connection_generation == self.live_generation
                && record.wake_pane_id.is_some()
                && matches!(
                    record.phase,
                    DormantPhase::Starting | DormantPhase::WakeUnknown | DormantPhase::Failed
                )
        };
        if !self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .values()
            .any(waiting)
        {
            return;
        }
        // One current-pane index, only while a wake needs confirmation.
        // Archive size is bounded; never scan all agents once per record.
        let by_pane = agents
            .iter()
            .map(|agent| (agent.pane_id.as_str(), agent))
            .collect::<HashMap<_, _>>();
        let confirmed = self
            .snapshot
            .ui_state
            .agent_sleep
            .dormant
            .iter()
            .filter_map(|(id, record)| {
                (waiting(record)
                    && record
                        .wake_pane_id
                        .as_deref()
                        .and_then(|pane| by_pane.get(pane))
                        .is_some_and(|agent| {
                            hide_agent_adapter::canonical_kind(&agent.agent_kind)
                                == hide_agent_adapter::canonical_kind(&record.kind)
                                && agent.row_facts.is_some()
                                && agent.session_id.as_deref()
                                    == Some(record.native_session_id.as_str())
                        }))
                .then(|| (id.clone(), record.context.checkout_path.clone()))
            })
            .collect::<Vec<_>>();
        if confirmed.is_empty() {
            return;
        }
        for (id, path) in confirmed {
            self.snapshot.ui_state.agent_sleep.dormant.remove(&id);
            self.finish_agent_effect(&path, &format!("sleep:{}", id.as_str()));
            crate::diagnostic!(
                serde_json::json!({"component":"agent_sleep", "kind":"agent_sleep.wake_confirmed", "sleep_id":id.as_str()})
            );
        }
        self.refresh_dormant_rows();
        self.persist_ui_state();
    }

    pub(super) fn expire_dormant_confirmation(&mut self, now: u64) -> bool {
        let mut changed = false;
        for record in self.snapshot.ui_state.agent_sleep.dormant.values_mut() {
            if record.phase.in_flight()
                && !matches!(
                    record.phase,
                    DormantPhase::CloseUnknown | DormantPhase::WakeUnknown
                )
                && (record.connection_generation != self.live_generation
                    || now.saturating_sub(record.transition_started_unix_ms) > 180_000)
            {
                record.phase = if record.closed {
                    DormantPhase::WakeUnknown
                } else {
                    DormantPhase::CloseUnknown
                };
                record.reason = Some(
                    "The operation's outcome needs a current status check; it was not resent"
                        .into(),
                );
                changed = true;
            }
        }
        if changed {
            self.refresh_dormant_rows();
            self.persist_ui_state();
        }
        changed
    }
}
