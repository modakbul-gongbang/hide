//! Replacement shells preserve Herdr's protected primary-workspace boundary.
use super::*;

impl Runtime {
    pub(super) fn primary_needs_shell(&self, workspace: &str, tab: &str) -> bool {
        let Some(primary) = self.herdr_worktrees.get(workspace) else {
            return false;
        };
        !primary.is_linked_worktree
            && self
                .herdr_workspace_tab_order
                .get(workspace)
                .is_some_and(|tabs| tabs.as_slice() == [tab])
            && self.herdr_worktrees.iter().any(|(id, tree)| {
                id != workspace && tree.is_linked_worktree && tree.repo_key == primary.repo_key
            })
    }

    pub(crate) fn ingest_close_replacement(
        &mut self,
        request: &live::CloseEffectRequest,
        tab_id: &str,
        payload: SessionSnapshotPayload,
    ) -> Result<(), String> {
        let operation = self
            .close_operations
            .get(&request.key)
            .ok_or("close intent is no longer pending")?;
        if operation.phase != "transmitting"
            || operation.connection_generation != self.live_generation
        {
            return Err("close intent changed while preparing its shell".into());
        }
        let context = request
            .replacement
            .as_ref()
            .ok_or("replacement context is missing")?;
        self.queue_restored_agent_tab(context, tab_id)?;
        self.finish_agent_effect(&context.checkout_path, &format!("close:{}", request.key));
        self.close_operations
            .get_mut(&request.key)
            .expect("checked")
            .replacement_tab_id = Some(tab_id.to_owned());
        // The owned fresh projection includes both tabs. Arrange the shell before
        // removing the old tab so its area cannot collapse between the two effects.
        self.ingest_session(Ok(payload));
        if !self
            .agent_layout_of(&(
                workspace::LOCAL_DEVICE_ID.to_owned(),
                context.checkout_path.clone(),
            ))
            .is_some_and(|layout| layout.tree.display(tab_id).is_some())
            && self.workspace_views.is_some()
        {
            return Err(
                "The replacement shell could not be placed; the original tab was not closed".into(),
            );
        }
        let operation = self
            .close_operations
            .get(&request.key)
            .ok_or("close intent disappeared")?;
        if let Some(message) = self.close_precondition_failure(operation) {
            return Err(message);
        }
        if let Some(operation) = self.close_operations.get_mut(&request.key) {
            operation.deadline_at_unix_ms =
                Some(unix_milliseconds().saturating_add(CLOSE_STAGE_TIMEOUT_MS));
        }
        Ok(())
    }

    pub(super) fn queue_restored_agent_tab(
        &mut self,
        context: &ClosedContext,
        tab_id: &str,
    ) -> Result<(), String> {
        let Some(store) = self.workspace_views.as_mut() else {
            return Ok(());
        };
        let key = (
            workspace::LOCAL_DEVICE_ID.to_owned(),
            context.checkout_path.clone(),
        );
        let area = context.agent_area.clone().or_else(|| {
            store
                .views
                .get(&key.0, &key.1)
                .map(|view| (view.agent_layout.tree.active_area.clone(), usize::MAX))
        });
        let Some((area, index)) = area else {
            return Ok(());
        };
        if store.agent_placements.len() >= 64 && !store.agent_placements.contains_key(tab_id) {
            return Err("Agent placement queue is full".into());
        }
        store
            .agent_placements
            .insert(tab_id.to_owned(), (key, area, Some(index)));
        Ok(())
    }

    pub(super) fn retry_agent_close(&mut self, key: &str) -> bool {
        let Some(operation) = self.close_operations.get(key).cloned() else {
            return false;
        };
        if !operation.request.context.replacement_shell
            || !matches!(operation.phase.as_str(), "refused" | "failed")
        {
            return false;
        }
        let reuse = operation.replacement_tab_id.as_ref().is_some_and(|id| {
            self.herdr_workspace_tab_order
                .values()
                .any(|tabs| tabs.contains(id))
        });
        if !reuse
            && !self.reserve_agent_effect(
                &operation.request.context.checkout_path,
                &format!("close:{key}"),
            )
        {
            return true;
        }
        if let Some(message) = self.close_precondition_failure(&operation) {
            self.cancel_close_before_effect(key, message);
            return true;
        }
        let Some(context) = self.live.clone() else {
            return false;
        };
        if let Some(current) = self.close_operations.get_mut(key) {
            current.allow_replacement_create = !reuse;
            if !reuse {
                current.replacement_tab_id = None;
            }
            current.phase = "preparing".into();
            current.stage = "capture".into();
            current.message = None;
            current.deadline_at_unix_ms =
                Some(unix_milliseconds().saturating_add(CLOSE_STAGE_TIMEOUT_MS));
            current.connection_generation = self.live_generation;
            current.request.connection_generation = self.live_generation;
        }
        let request = self.close_operations[key].request.clone();
        self.panes_closing.extend(operation.pane_ids);
        if let Err(message) = live::spawn_close_capture(context, request) {
            self.fail_close_operation(key, message);
        }
        self.sync_recent_closed_snapshot();
        true
    }

    pub(super) fn dismiss_agent_close(&mut self, key: &str) -> bool {
        let Some(operation) = self.close_operations.get(key).cloned() else {
            return false;
        };
        if !matches!(operation.phase.as_str(), "failed" | "refused") {
            return false;
        }
        self.clear_close_guards(&operation);
        self.close_operations.remove(key);
        self.close_capture_order.retain(|id| id != key);
        self.promote_close_reservations();
        self.sync_recent_closed_snapshot();
        true
    }
}
