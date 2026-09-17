use super::*;

impl Runtime {
    pub(super) fn advance_remote_file_generation(&mut self) -> u64 {
        self.next_remote_file_generation = self.next_remote_file_generation.saturating_add(1);
        self.next_remote_file_generation
    }

    pub(super) fn mark_remote_files_unavailable(
        &mut self,
        status_index: usize,
        root_path: String,
        message: String,
        generation: u64,
    ) {
        self.snapshot.status.remote[status_index].files = RemoteFileListSnapshot {
            root_path: Some(root_path),
            state: "unavailable".to_owned(),
            entries: Vec::new(),
            message: Some(message),
            generation,
        };
    }

    pub(super) fn request_remote_file_list(&mut self, payload: RemoteFileListPayload) -> bool {
        let target_id = payload.target_id;
        let root_path = payload.root_path;
        let Some(status_index) = self
            .snapshot
            .status
            .remote
            .iter()
            .position(|status| status.target_id == target_id)
        else {
            self.set_error(
                "remote.files.unknown_target",
                format!("Remote file listing requested an unconfigured target {target_id}"),
                false,
            );
            return true;
        };
        if !Path::new(&root_path).is_absolute()
            || root_path.bytes().any(|byte| byte.is_ascii_control())
        {
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(
                status_index,
                root_path.clone(),
                "Remote file root must be an absolute single-line path".to_owned(),
                generation,
            );
            self.set_error(
                "remote.files.invalid_root",
                format!("Remote file root is invalid for target {target_id}"),
                false,
            );
            return true;
        }
        let status = &self.snapshot.status.remote[status_index];
        if status.state != "connected" {
            let message = status
                .message
                .clone()
                .unwrap_or_else(|| format!("Remote target {target_id} is not connected"));
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(status_index, root_path, message, generation);
            return true;
        }
        let root_is_authoritative = status.session.as_ref().is_some_and(|session| {
            session
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .any(|checkout| checkout.path == root_path)
        });
        if !root_is_authoritative {
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(
                status_index,
                root_path.clone(),
                "Remote file root is not part of the authoritative Herdr session".to_owned(),
                generation,
            );
            self.set_error(
                "remote.files.unknown_root",
                format!("Remote file root {root_path} is not open on target {target_id}"),
                false,
            );
            return true;
        }
        if status.files.root_path.as_deref() == Some(root_path.as_str())
            && matches!(status.files.state.as_str(), "loading" | "ready")
        {
            return false;
        }
        let Some(transport) = self.remote_file_transports.get(&target_id).cloned() else {
            let message = format!("Remote target {target_id} has no configured SFTP transport");
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(
                status_index,
                root_path,
                message.clone(),
                generation,
            );
            self.set_error("remote.files.transport_unavailable", message, true);
            return true;
        };
        let Some(context) = self.worker_context.clone() else {
            let message = "The remote file worker is unavailable".to_owned();
            let generation = self.advance_remote_file_generation();
            self.mark_remote_files_unavailable(
                status_index,
                root_path,
                message.clone(),
                generation,
            );
            self.set_error("remote.files.worker_unavailable", message, true);
            return true;
        };

        let generation = self.advance_remote_file_generation();
        self.snapshot.status.remote[status_index].files = RemoteFileListSnapshot {
            root_path: Some(root_path.clone()),
            state: "loading".to_owned(),
            entries: Vec::new(),
            message: None,
            generation,
        };
        self.push_diagnostic(
            "remote.files.requested",
            format!("Listing remote files for {target_id} at {root_path}"),
        );
        let worker_target_id = target_id.clone();
        let worker_root_path = root_path.clone();
        match thread::Builder::new()
            .name(format!("herdr-core-remote-files-{target_id}"))
            .spawn(move || {
                let result = RemoteFileService::new(worker_root_path.clone(), transport)
                    .and_then(|service| service.list(""))
                    .map_err(|error| error.to_string());
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_remote_file_list_result(
                        &worker_target_id,
                        &worker_root_path,
                        generation,
                        result,
                    ),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            }) {
            Ok(_) => true,
            Err(error) => self.ingest_remote_file_list_result(
                &target_id,
                &root_path,
                generation,
                Err(format!("Remote file worker could not be started: {error}")),
            ),
        }
    }

    pub(super) fn ingest_remote_file_list_result(
        &mut self,
        target_id: &str,
        root_path: &str,
        generation: u64,
        result: Result<Vec<FileEntry>, String>,
    ) -> bool {
        let Some(status_index) = self
            .snapshot
            .status
            .remote
            .iter()
            .position(|status| status.target_id == target_id)
        else {
            self.set_error(
                "remote.files.unknown_target",
                format!("Remote file result named an unconfigured target {target_id}"),
                false,
            );
            return true;
        };
        let files = &self.snapshot.status.remote[status_index].files;
        if files.generation != generation || files.root_path.as_deref() != Some(root_path) {
            self.push_diagnostic(
                "remote.files.stale",
                format!(
                    "Ignored stale remote file result for {target_id} at {root_path} generation {generation}"
                ),
            );
            return true;
        }
        match result {
            Ok(mut entries) => {
                entries.sort_by(|left, right| {
                    let left_is_directory = left.kind == FileKind::Directory;
                    let right_is_directory = right.kind == FileKind::Directory;
                    right_is_directory
                        .cmp(&left_is_directory)
                        .then_with(|| {
                            left.name
                                .to_ascii_lowercase()
                                .cmp(&right.name.to_ascii_lowercase())
                        })
                        .then_with(|| left.path.cmp(&right.path))
                });
                let entry_count = entries.len();
                self.snapshot.status.remote[status_index].files = RemoteFileListSnapshot {
                    root_path: Some(root_path.to_owned()),
                    state: "ready".to_owned(),
                    entries: entries
                        .into_iter()
                        .map(|entry| RemoteFileEntrySnapshot {
                            path: entry.path,
                            name: entry.name,
                            is_directory: entry.kind == FileKind::Directory,
                            size_bytes: entry.size_bytes,
                        })
                        .collect(),
                    message: None,
                    generation,
                };
                self.push_diagnostic(
                    "remote.files.ready",
                    format!("Listed {entry_count} remote files for {target_id} at {root_path}"),
                );
            }
            Err(message) => {
                self.mark_remote_files_unavailable(
                    status_index,
                    root_path.to_owned(),
                    message.clone(),
                    generation,
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_files",
                    "kind": "remote.files_failed",
                    "target": target_id,
                    "root_path": root_path,
                    "generation": generation,
                    "message": message,
                }));
            }
        }
        true
    }

    pub(super) fn request_remote_control(&mut self, payload: RemoteControlPayload) -> bool {
        let target_id = payload.target_id;
        let request_id = payload.request_id;
        let pane_focus_target = match (&payload.request, payload.report_pane_focus_outcome) {
            (RemoteControlRequest::FocusPane { pane_id }, true) => Some(pane_id.clone()),
            _ => None,
        };
        macro_rules! fail_request {
            ($kind:expr, $message:expr, $retryable:expr $(,)?) => {{
                let message = $message;
                if pane_focus_target.is_some() {
                    self.finish_pane_focus_request_by_id(
                        &request_id,
                        "failed",
                        Some(message.clone()),
                        $retryable,
                    );
                }
                self.set_error($kind, message, $retryable);
                return true;
            }};
        }
        if target_id.trim().is_empty() || request_id.trim().is_empty() {
            fail_request!(
                "remote.control.invalid_request",
                "Remote control requires non-empty target_id and request_id".to_owned(),
                false,
            );
        }
        if self
            .remote_control_requests
            .iter()
            .any(|known| known == &(target_id.clone(), request_id.clone()))
        {
            self.push_diagnostic(
                "remote.control.duplicate_ignored",
                format!("Ignored duplicate remote request {request_id} for {target_id}"),
            );
            return true;
        }
        if let Some(pane_id) = pane_focus_target.as_deref() {
            self.snapshot.status.pane_focus_request = Some(PaneFocusRequestSnapshot {
                request_id: request_id.clone(),
                target_pane_id: pane_id.to_owned(),
                phase: "pending".to_owned(),
                message: None,
                retryable: false,
            });
        }
        let Some(context) = self.remote_controls.get(&target_id).cloned() else {
            fail_request!(
                "remote.control.unavailable",
                format!("Remote control is unavailable for target {target_id}"),
                true,
            );
        };
        let Some(remote) = self
            .snapshot
            .status
            .remote
            .iter()
            .find(|remote| remote.target_id == target_id)
        else {
            fail_request!(
                "remote.control.unknown_target",
                format!("Remote target {target_id} is not configured"),
                false,
            );
        };
        if remote.state != "connected" {
            fail_request!(
                "remote.control.not_connected",
                format!(
                    "Remote target {target_id} is {}; no command was sent",
                    remote.state
                ),
                true,
            );
        }
        let Some(session) = remote.session.clone() else {
            fail_request!(
                "remote.control.session_missing",
                format!("Remote target {target_id} has no authoritative session projection"),
                true,
            );
        };
        let connection_generation = *self
            .remote_connection_generations
            .entry(target_id.clone())
            .or_insert(0);
        let remote_mutation = remote_mutation_descriptor(&session, &payload.request);
        let mut source_pane_id = None;
        if let Some(pane_id) = payload.request.pane_id().map(str::to_owned) {
            if pane_id.trim().is_empty() {
                fail_request!(
                    "remote.control.invalid_pane",
                    "Remote pane control requires a non-empty pane_id".to_owned(),
                    false,
                );
            }
            let pane_exists = session.workspaces.iter().any(|workspace| {
                workspace.checkouts.iter().any(|checkout| {
                    checkout
                        .tabs
                        .iter()
                        .any(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
                })
            });
            if !pane_exists {
                fail_request!(
                    "remote.control.pane_not_found",
                    format!("Pane {pane_id} does not belong to remote target {target_id}"),
                    false,
                );
            }
            source_pane_id = remote_pane_source_id(&target_id, &pane_id).map(str::to_owned);
            if source_pane_id.is_none() {
                fail_request!(
                    "remote.control.invalid_pane_scope",
                    format!("Pane {pane_id} is not scoped to remote target {target_id}"),
                    false,
                );
            }
            let status_unknown = matches!(&payload.request, RemoteControlRequest::ClosePane { .. })
                && session
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == pane_id && agent.requires_close_status_check);
            if status_unknown {
                self.set_error(
                    "remote.control.close_status_unknown",
                    format!(
                        "Pane {pane_id} on {target_id} has an unknown activity status; refresh status before closing"
                    ),
                    true,
                );
                return true;
            }
            let needs_confirmation =
                matches!(&payload.request, RemoteControlRequest::ClosePane { .. })
                    && session
                        .agents
                        .iter()
                        .any(|agent| agent.pane_id == pane_id && agent.requires_close_confirmation);
            if needs_confirmation && !payload.request.confirmed() {
                self.set_error(
                    "remote.control.close_confirmation_required",
                    format!(
                        "Pane {pane_id} on {target_id} is working or needs attention; remote close requires confirmed=true"
                    ),
                    false,
                );
                return true;
            }
        }

        let action = match payload.request {
            RemoteControlRequest::FocusPane { .. } => {
                RemoteControlAction::Pane(PaneControlAction::Focus {
                    pane_id: source_pane_id.expect("remote pane source id was validated"),
                })
            }
            RemoteControlRequest::SplitPane { direction, cwd, .. } => {
                RemoteControlAction::Pane(PaneControlAction::Split {
                    pane_id: source_pane_id.expect("remote pane source id was validated"),
                    direction,
                    cwd,
                })
            }
            RemoteControlRequest::TogglePaneZoom { .. } => {
                RemoteControlAction::Pane(PaneControlAction::ToggleZoom {
                    pane_id: source_pane_id.expect("remote pane source id was validated"),
                })
            }
            RemoteControlRequest::ClosePane { .. } => {
                RemoteControlAction::Pane(PaneControlAction::Close {
                    pane_id: source_pane_id.expect("remote pane source id was validated"),
                })
            }
            RemoteControlRequest::FocusWorkspace { workspace_id } => {
                let Some(source_id) = session
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.id == workspace_id)
                    .then(|| remote_workspace_source_id(&target_id, &workspace_id))
                    .flatten()
                else {
                    self.set_error(
                        "remote.control.workspace_not_found",
                        format!(
                            "Workspace {workspace_id} does not belong to remote target {target_id}"
                        ),
                        false,
                    );
                    return true;
                };
                RemoteControlAction::FocusWorkspace {
                    workspace_id: source_id.to_owned(),
                }
            }
            RemoteControlRequest::FocusTab { tab_id } => {
                let exists = !tab_id.trim().is_empty()
                    && session.workspaces.iter().any(|workspace| {
                        workspace.checkouts.iter().any(|checkout| {
                            checkout
                                .tabs
                                .iter()
                                .any(|tab| tab.id.as_deref() == Some(tab_id.as_str()))
                        })
                    });
                if !exists {
                    self.set_error(
                        "remote.control.tab_not_found",
                        format!("Tab {tab_id} does not belong to remote target {target_id}"),
                        false,
                    );
                    return true;
                }
                let Some(source_id) = remote_tab_source_id(&target_id, &tab_id) else {
                    self.set_error(
                        "remote.control.invalid_tab_scope",
                        format!("Tab {tab_id} is not scoped to remote target {target_id}"),
                        false,
                    );
                    return true;
                };
                RemoteControlAction::FocusTab {
                    tab_id: source_id.to_owned(),
                }
            }
            RemoteControlRequest::CreateTab {
                workspace_id,
                cwd,
                label,
            } => {
                let Some(source_id) = session
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.id == workspace_id)
                    .then(|| remote_workspace_source_id(&target_id, &workspace_id))
                    .flatten()
                else {
                    self.set_error(
                        "remote.control.workspace_not_found",
                        format!(
                            "Workspace {workspace_id} does not belong to remote target {target_id}"
                        ),
                        false,
                    );
                    return true;
                };
                if cwd.trim().is_empty() || label.trim().is_empty() {
                    self.set_error(
                        "remote.control.invalid_tab",
                        "Remote tab creation requires non-empty cwd and label",
                        false,
                    );
                    return true;
                }
                RemoteControlAction::CreateTab {
                    workspace_id: source_id.to_owned(),
                    cwd,
                    label,
                }
            }
            RemoteControlRequest::CloseTab { tab_id, confirmed } => {
                let tab = session
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .find(|tab| tab.id.as_deref() == Some(tab_id.as_str()));
                let Some(tab) = tab else {
                    self.set_error(
                        "remote.control.tab_not_found",
                        format!("Tab {tab_id} does not belong to remote target {target_id}"),
                        false,
                    );
                    return true;
                };
                let pane_ids = tab
                    .panes
                    .iter()
                    .map(|pane| pane.id.as_str())
                    .collect::<HashSet<_>>();
                let status_unknown = session.agents.iter().any(|agent| {
                    pane_ids.contains(agent.pane_id.as_str()) && agent.requires_close_status_check
                });
                if status_unknown {
                    self.set_error(
                        "remote.control.close_status_unknown",
                        format!(
                            "Tab {tab_id} on {target_id} has a pane whose activity status is unknown; refresh status before closing"
                        ),
                        true,
                    );
                    return true;
                }
                let needs_confirmation = session.agents.iter().any(|agent| {
                    pane_ids.contains(agent.pane_id.as_str()) && agent.requires_close_confirmation
                });
                if needs_confirmation && !confirmed {
                    self.set_error(
                        "remote.control.close_confirmation_required",
                        format!(
                            "Tab {tab_id} on {target_id} contains an agent that is working or needs attention; remote close requires confirmed=true"
                        ),
                        false,
                    );
                    return true;
                }
                let Some(source_id) = remote_tab_source_id(&target_id, &tab_id) else {
                    self.set_error(
                        "remote.control.invalid_tab_scope",
                        format!("Tab {tab_id} is not scoped to remote target {target_id}"),
                        false,
                    );
                    return true;
                };
                RemoteControlAction::CloseTab {
                    tab_id: source_id.to_owned(),
                }
            }
        };

        let creation_key = remote_tab_creation_key(&target_id, &action);
        if let Some(key) = creation_key.as_ref()
            && !self.remote_tab_creations_in_flight.insert(key.clone())
        {
            self.push_diagnostic(
                "remote.control.duplicate_tab_ignored",
                format!(
                    "Ignored duplicate in-flight tab.create for workspace {} on {target_id}",
                    key.1
                ),
            );
            return true;
        }
        let remote_operation_key = if let Some(descriptor) = remote_mutation {
            self.remote_operations.retain(|_, operation| {
                !(operation.scope_id == descriptor.scope_id
                    && matches!(operation.phase.as_str(), "failed" | "refused"))
            });
            if self.remote_operations.values().any(|operation| {
                operation.scope_id == descriptor.scope_id
                    && matches!(
                        operation.phase.as_str(),
                        "transmitting" | "awaiting_topology" | "unknown"
                    )
            }) {
                fail_request!(
                    "remote.control.in_progress",
                    format!(
                        "{} is already running for {} on remote target {}",
                        descriptor.kind, descriptor.scope_id, target_id
                    ),
                    false,
                );
            }
            let operation_key = (target_id.clone(), request_id.clone());
            self.insert_remote_operation(
                &operation_key,
                descriptor,
                unix_milliseconds(),
                connection_generation,
            );
            Some(operation_key)
        } else {
            None
        };
        if self.remote_control_requests.len() == 128 {
            self.remote_control_requests.pop_front();
        }
        self.remote_control_requests
            .push_back((target_id.clone(), request_id.clone()));
        self.push_diagnostic(
            "remote.control.requested",
            format!("Sending {} to {target_id}", action.kind()),
        );
        let dispatched_request_id = request_id.clone();
        if let Err(message) =
            live::spawn_remote_control(context, request_id, action, connection_generation)
        {
            if let Some((operation_target, operation_request)) = remote_operation_key.as_ref() {
                self.fail_remote_operation(operation_target, operation_request, message.clone());
            }
            if let Some(key) = creation_key {
                self.remote_tab_creations_in_flight.remove(&key);
            }
            if pane_focus_target.is_some() {
                self.finish_pane_focus_request_by_id(
                    &dispatched_request_id,
                    "failed",
                    Some(message.clone()),
                    true,
                );
            }
            if remote_operation_key.is_none() {
                self.set_error("remote.control.worker_failed", message, true);
            }
        }
        true
    }

    pub fn ingest_provider_usage(
        &mut self,
        provider_usage: Vec<crate::model::ProviderUsageSnapshot>,
    ) -> bool {
        if self.snapshot.navigator.provider_usage == provider_usage {
            return false;
        }
        self.snapshot.navigator.provider_usage = provider_usage;
        true
    }

    pub(crate) fn usage_activity(&self) -> crate::usage::UsageActivity {
        crate::usage::UsageActivity {
            window_visible: self.usage_window_visible,
            popover_open_generation: self.usage_popover_open_generation,
        }
    }

    /// Recomputes the pet's pose, badge row, and attention queue from the
    /// current agent list and connection state. Idempotent: the same inputs
    /// produce the same snapshot and report no change.
    pub(super) fn refresh_pet(&mut self) -> bool {
        let now = unix_milliseconds();
        let connected = self.snapshot.status.herdr.state == "connected";
        let agents = &self.snapshot.navigator.agents;
        let summary = pet::summarize(agents, connected);
        if summary.needs_you + summary.working > 0 {
            self.pet_active_at_unix_ms = now;
        }
        let idle_ms = now.saturating_sub(self.pet_active_at_unix_ms);
        let waking = now < self.pet_waking_until_unix_ms;
        let ambient = pet::ambient_totals(agents, connected);
        let attention_pane_ids = if connected {
            pet::observe_unseen(&mut self.pet_unseen_observed, agents, now);
            pet::attention_order(&self.snapshot.navigator.agents, &self.pet_unseen_observed)
        } else {
            Vec::new()
        };

        let next = PetSnapshot {
            visible: self.snapshot.ui_state.pet_visible,
            connection: self.snapshot.status.herdr.state.clone(),
            connection_message: self.snapshot.status.herdr.message.clone(),
            pose: pet::pose(summary, idle_ms, waking, connected).to_owned(),
            sleep_phase: pet::sleep_phase_for_idle_ms(idle_ms).as_str().to_owned(),
            roam_allowed: connected && pet::is_roam_allowed(summary, idle_ms, self.pet_dragging),
            badges: PetBadgesSnapshot {
                needs_you: summary.needs_you,
                done: summary.done,
                working: summary.working,
                seen: summary.seen,
                disconnected: summary.disconnected,
                subagents_active: ambient.subagents_active,
                background_running: ambient.background_running,
                background_failed: ambient.background_failed,
            },
            attention_pane_ids,
            origin: self.snapshot.ui_state.pet_origin,
            shortcut: self.snapshot.ui_state.pet_shortcut.clone(),
            shortcut_error: self.snapshot.pet.shortcut_error.clone(),
            theme_id: self.snapshot.pet.theme_id.clone(),
        };
        if self.snapshot.pet == next {
            return false;
        }
        self.snapshot.pet = next;
        true
    }

    /// Applying the same visibility twice converges instead of flapping, so
    /// four surfaces sharing one state can all set it freely.
    pub(super) fn set_pet_visible(&mut self, visible: bool) -> bool {
        if self.snapshot.ui_state.pet_visible == visible {
            return false;
        }
        self.snapshot.ui_state.pet_visible = visible;
        self.persist_ui_state();
        self.note_pet_activity();
        self.refresh_pet();
        true
    }

    /// Pointer or toggle activity wakes a sleeping pet before the normal
    /// priority resumes.
    pub(super) fn note_pet_activity(&mut self) {
        let now = unix_milliseconds();
        let idle_ms = now.saturating_sub(self.pet_active_at_unix_ms);
        if pet::sleep_phase_for_idle_ms(idle_ms) != pet::SleepPhase::Awake {
            self.pet_waking_until_unix_ms = now.saturating_add(PET_WAKING_MS);
        }
        self.pet_active_at_unix_ms = now;
    }

    pub(super) fn apply_persisted_pet_state(&mut self) {
        self.snapshot.pet.visible = self.snapshot.ui_state.pet_visible;
        self.snapshot.pet.origin = self.snapshot.ui_state.pet_origin;
        self.snapshot.pet.shortcut = self.snapshot.ui_state.pet_shortcut.clone();
    }

    /// Raises the operator-focused pane's read record and sets the read axis
    /// on every row, then persists the record when it actually moved.
    ///
    /// This is the only place the read axis is decided, and the pane it reads
    /// is the one the operator chose, not the one Herdr reports focused.
    /// Herdr marks every pane in a tab seen the moment the tab is focused, so
    /// three finished agents side by side would clear together; Hide keeps its
    /// own pane-level record instead and never derives unread from Herdr's
    /// `done` or `idle`, nor from a focus it merely inherited.
    ///
    /// The record moves on a real state change or an operator focus, not on
    /// every tick, so the save this triggers is not a per-tick disk write.
    pub(super) fn apply_pane_read_state(
        &mut self,
        agents: &mut [SidebarAgentSnapshot],
        scope: ReadRecordScope<'_>,
    ) -> bool {
        let focused = self.operator_focused_pane_id.clone();
        let changes = crate::sidebar::apply_read_state(
            agents,
            &mut self.snapshot.ui_state.pane_read_records,
            focused.as_deref(),
            scope,
        );
        let synced = self.sync_pane_status_from_agents(agents);
        let pruned = prune_pane_text_scales(
            &mut self.snapshot.ui_state.pane_text_scales,
            &self.snapshot.navigator.workspaces,
            agents,
            scope,
        );
        if changes.is_empty() && !pruned {
            return synced;
        }
        self.record_read_record_changes(&changes);
        true
    }

    /// Re-runs the read axis over the agents already in the snapshot, for the
    /// moment the operator picks a pane without a new agent list arriving.
    pub(super) fn refresh_pane_read_state(&mut self) -> bool {
        let before = self.snapshot.navigator.agents.clone();
        let mut agents = std::mem::take(&mut self.snapshot.navigator.agents);
        self.apply_pane_read_state(&mut agents, ReadRecordScope::Retain);
        let changed = before != agents;
        self.snapshot.navigator.agents = agents;
        changed | self.refresh_inactive_groups()
    }

    /// Applies the read axis to one remote target's agent rows.
    ///
    /// A pane is a pane: a remote row earns its read record the same way a
    /// local one does, from the focus its own server reports, because Hide
    /// never focuses a remote pane itself. Eviction is scoped to this target's
    /// pane id prefix, so a local sync cannot drop what this pass wrote and
    /// this pass cannot drop another target's records.
    ///
    /// The remote pane tree arrives freshly projected on every sync, with no
    /// read axis applied, so it is synced whether or not the ledger moved.
    pub(super) fn apply_remote_read_state(
        &mut self,
        target_id: &str,
        session: &mut RemoteSessionSnapshot,
    ) -> bool {
        let focused = session.focused_pane_id.clone();
        let prefix = remote_pane_id_prefix(target_id);
        let changes = crate::sidebar::apply_read_state(
            &mut session.agents,
            &mut self.snapshot.ui_state.pane_read_records,
            focused.as_deref(),
            ReadRecordScope::Remote(&prefix),
        );
        let lineage_pruned = crate::sidebar::prune_lineage_collapse(
            &mut self.snapshot.ui_state.collapsed_agent_pane_ids,
            &session.agents,
            ReadRecordScope::Remote(&prefix),
        );
        crate::sidebar::apply_lineage(
            &mut session.agents,
            &session.workspaces,
            &self.snapshot.ui_state.collapsed_agent_pane_ids,
        );
        if lineage_pruned {
            self.persist_ui_state();
        }
        let synced = sync_pane_status(&mut session.workspaces, &session.agents);
        let pruned = prune_pane_text_scales(
            &mut self.snapshot.ui_state.pane_text_scales,
            &session.workspaces,
            &session.agents,
            ReadRecordScope::Remote(&prefix),
        );
        if changes.is_empty() && !pruned && !lineage_pruned {
            return synced;
        }
        self.record_read_record_changes(&changes);
        true
    }

    /// Logs each read record move and saves the ledger.
    ///
    /// The record moves on a real state change, a focus move, or a pane going
    /// away, not on every tick, so the save this triggers is not a per-tick
    /// disk write.
    pub(super) fn record_read_record_changes(
        &mut self,
        changes: &[crate::sidebar::ReadRecordChange],
    ) {
        for change in changes {
            crate::diagnostic!(serde_json::json!({
                "component": "session",
                "kind": "pane.read_record",
                "pane_id": change.pane_id,
                "evicted": change.evicted,
                "state_change_seq": change.record.state_change_seq,
                "demand": change.record.demand,
                "activity": change.record.activity,
            }));
        }
        self.persist_ui_state();
    }

    /// Copies each local pane's status word and close-confirmation answer from
    /// the agent rows that just had the read axis applied.
    pub(super) fn sync_pane_status_from_agents(&mut self, agents: &[SidebarAgentSnapshot]) -> bool {
        sync_pane_status(&mut self.snapshot.navigator.workspaces, agents)
    }

    /// Recomputes the two inactive folds from current core facts. No row is
    /// moved or copied: the full collections stay authoritative for search,
    /// focus, and non-sidebar consumers.
    pub(super) fn refresh_inactive_groups(&mut self) -> bool {
        crate::project_context::refresh_inactive_groups(
            &mut self.snapshot.navigator,
            &self.snapshot.ui_state,
            unix_milliseconds(),
        )
    }

    /// Drops the conversation choice of every pane that is no longer an
    /// eligible agent pane. Nothing is added here: a pane shows its terminal
    /// until the operator asks for the conversation, so the set only ever
    /// grows through `toggle_conversation`.
    pub(super) fn sync_conversation_modes(&mut self, agents: &[SidebarAgentSnapshot]) -> bool {
        let live: BTreeSet<String> = agents
            .iter()
            .filter(|agent| conversation_agent_kind(&agent.agent_kind))
            .map(|agent| agent.pane_id.clone())
            .collect();
        let conversation = &mut self.snapshot.ui_state.conversation_pane_ids;
        let before = conversation.len();
        conversation.retain(|pane_id| live.contains(pane_id));
        before != conversation.len()
    }

    /// Refills every pane's child summary and breadcrumb from the final agent
    /// list.
    ///
    /// It runs after the read axis and the lineage, not while the panes are
    /// built: a chip's mark and emphasis come from the group, the group comes
    /// from the read axis, and the children come from the lineage, so a pass
    /// that ran earlier would publish a chip row describing a state the
    /// sidebar had already moved past.
    pub(super) fn sync_pane_lineage(&mut self) -> bool {
        let agents = std::mem::take(&mut self.snapshot.navigator.agents);
        let diagnosis = self.hook_diagnosis.clone();
        let status_of = |runtime: hide_agent_hooks::AgentRuntime| {
            diagnosis
                .as_ref()
                .and_then(|diagnosis| diagnosis.status_of(runtime))
                .cloned()
        };
        let mut changed = false;
        let mut delegated_tabs_changed = false;
        // Collected on the same walk as the pane children, so the Settings
        // diagnosis and the pane's own mark can never disagree about which
        // sessions predate the install (PRD B27, D-61).
        let mut predating: Vec<crate::model::AgentHookPaneSnapshot> = Vec::new();
        // A tab is the operator's whenever it holds an agent they own. One
        // holding only delegated children is the pile this change exists to
        // take off the strip (PRD B1).
        for tab in self
            .snapshot
            .navigator
            .workspaces
            .iter_mut()
            .flat_map(|workspace| workspace.checkouts.iter_mut())
            .flat_map(|checkout| checkout.tabs.iter_mut())
        {
            let mut holds_an_agent = false;
            let mut all_delegated = true;
            for pane in &tab.panes {
                let Some(agent) = agents.iter().find(|agent| agent.pane_id == pane.id) else {
                    continue;
                };
                holds_an_agent = true;
                all_delegated &= agent.delegated;
            }
            let delegated = holds_an_agent && all_delegated;
            if tab.delegated != delegated {
                tab.delegated = delegated;
                delegated_tabs_changed = true;
            }
        }
        for pane in self
            .snapshot
            .navigator
            .workspaces
            .iter_mut()
            .flat_map(|workspace| workspace.checkouts.iter_mut())
            .flat_map(|checkout| checkout.tabs.iter_mut())
            .flat_map(|tab| tab.panes.iter_mut())
            .chain(
                self.snapshot
                    .navigator
                    .scratch
                    .tabs
                    .iter_mut()
                    .flat_map(|tab| tab.panes.iter_mut()),
            )
        {
            // A remote pane's answer is fixed and was decided where it was
            // projected; the local hook state says nothing about it.
            if crate::agent_hooks::is_remote_pane(&pane.id) {
                continue;
            }
            let tokens = self
                .pane_hook_tokens
                .get(&pane.id)
                .copied()
                .unwrap_or_default();
            let children =
                crate::sidebar::project_pane_children(&agents, &pane.id, tokens, &status_of);
            if let Some(children) = children.as_ref()
                && children.uninstrumented_code.as_deref()
                    == Some(
                        hide_agent_hooks::diagnosis::UninstrumentedReason::SessionPredatesInstall
                            .code(),
                    )
            {
                predating.push(crate::model::AgentHookPaneSnapshot {
                    pane_id: pane.id.clone(),
                    label: agents
                        .iter()
                        .find(|agent| agent.pane_id == pane.id)
                        .map(|agent| agent.chat_title.clone().unwrap_or_else(|| agent.id.clone()))
                        .unwrap_or_else(|| pane.id.clone()),
                    message: children.uninstrumented_reason.clone().unwrap_or_default(),
                });
            }
            let lineage_path = crate::sidebar::project_lineage_path(&agents, &pane.id);
            if pane.children != children {
                pane.children = children;
                changed = true;
            }
            if pane.lineage_path != lineage_path {
                pane.lineage_path = lineage_path;
                changed = true;
            }
        }
        self.snapshot.navigator.agents = agents;
        let hooks = crate::model::AgentHooksSnapshot {
            runtimes: self
                .hook_diagnosis
                .iter()
                .flat_map(|diagnosis| diagnosis.runtimes.iter())
                .map(|row| crate::model::AgentHookRuntimeSnapshot {
                    id: row.runtime.id().to_owned(),
                    label: row.label.clone(),
                    path: row.path.clone(),
                    headline: row.headline(),
                    installed: matches!(row.status, hide_agent_hooks::HookStatus::Installed { .. }),
                    offers_install: row.offers_install(),
                })
                .collect(),
            sessions_predating_install: predating,
        };
        if self.snapshot.status.agent_hooks != hooks {
            self.snapshot.status.agent_hooks = hooks;
            changed = true;
        }
        if delegated_tabs_changed {
            self.rebuild_tab_strips();
        }
        changed | delegated_tabs_changed | self.refresh_inactive_groups()
    }

    /// Whether a delegated child's clock should be running at all.
    ///
    /// A finished child is not stuck, a released pane has no session to be
    /// stuck in, an unknown activity gives nothing to measure, and a remote
    /// pane is uninstrumented by decision (PRD B19, D-51).
    pub(super) fn stall_eligible(&self, agent: &SidebarAgentSnapshot) -> bool {
        if !agent.delegated || crate::agent_hooks::is_remote_pane(&agent.pane_id) {
            return false;
        }
        if agent.activity == "unknown" {
            return false;
        }
        if self
            .terminal_session_lifecycles
            .get(&agent.pane_id)
            .is_some_and(|lifecycle| lifecycle.state == "released")
        {
            return false;
        }
        // Waiting on the operator, or running with nothing to show for it.
        // A stopped child with no demand has finished, which is not waiting.
        agent.demand != "none" || agent.blocked || agent.activity == "working"
    }

    /// Advances every eligible child's clock and drops the rest.
    ///
    /// While the server is away the clocks hold their reading rather than
    /// counting: a disconnection is Hide's blindness, not the agent being
    /// stuck (PRD B20, D-54).
    pub(super) fn advance_stall_clocks(&mut self, agents: &[SidebarAgentSnapshot], now: u64) {
        let connected = self.snapshot.status.herdr.state == "connected";
        let mut live = BTreeSet::new();
        for agent in agents {
            if !self.stall_eligible(agent) {
                continue;
            }
            live.insert(agent.pane_id.clone());
            let fingerprint = (
                agent.state_change_seq,
                agent.demand.clone(),
                agent.activity.clone(),
            );
            match self.stall_clocks.get_mut(&agent.pane_id) {
                Some(clock) if clock.fingerprint == fingerprint => {
                    if connected {
                        clock.stalled_ms = clock.elapsed(now);
                    }
                    clock.last_sample_unix_ms = now;
                }
                _ => {
                    self.stall_clocks.insert(
                        agent.pane_id.clone(),
                        StallClock {
                            fingerprint,
                            stalled_ms: 0,
                            last_sample_unix_ms: now,
                        },
                    );
                }
            }
        }
        self.stall_clocks
            .retain(|pane_id, _| live.contains(pane_id));
    }

    /// What each lineage root should be told about its descendants, as of
    /// `now`. A pure read, so the coordinator can ask whether a threshold is
    /// about to be crossed without changing anything.
    ///
    /// The notice lands on the root rather than climbing one level at a time:
    /// at depth three, one level per threshold would keep the operator
    /// waiting forty-five minutes for news of something stuck for fifteen
    /// (PRD B18, D-62).
    pub(super) fn stall_escalations(
        &self,
        agents: &[SidebarAgentSnapshot],
        now: u64,
    ) -> BTreeMap<String, (&'static str, String, String)> {
        let mut worst: BTreeMap<String, (u64, u8, &'static str, String, String)> = BTreeMap::new();
        for agent in agents {
            let Some(clock) = self.stall_clocks.get(&agent.pane_id) else {
                continue;
            };
            if !self.stall_eligible(agent) {
                continue;
            }
            let elapsed = clock.elapsed(now);
            let level = if elapsed >= STALL_HARD_MS {
                "hard"
            } else if elapsed >= STALL_SOFT_MS {
                "soft"
            } else {
                continue;
            };
            let root = agent
                .lineage_path_pane_ids
                .first()
                .cloned()
                .unwrap_or_else(|| agent.pane_id.clone());
            let name = agent.chat_title.clone().unwrap_or_else(|| agent.id.clone());
            let notice = format!(
                "{name} has been waiting {} minutes on {}",
                elapsed / 60_000,
                waiting_on(agent)
            );
            let priority = stall_priority(agent);
            let candidate = (elapsed, priority, level, notice, agent.pane_id.clone());
            // Longest wait first, and on a tie the one asking for the most.
            // Without the second key the notice names whichever sibling the
            // row order happened to reach first, which is not an answer.
            match worst.get(&root) {
                Some(best) if best.0 > elapsed => {}
                Some(best) if best.0 == elapsed && best.1 <= priority => {}
                _ => {
                    worst.insert(root, candidate);
                }
            }
        }
        worst
            .into_iter()
            .map(|(root, (_, _, level, notice, pane_id))| (root, (level, notice, pane_id)))
            .collect()
    }

    /// Runs the clocks and writes what they say onto the rows.
    pub(super) fn apply_stall_escalation(&mut self, agents: &mut [SidebarAgentSnapshot], now: u64) {
        self.advance_stall_clocks(agents, now);
        let escalations = self.stall_escalations(agents, now);
        let hard_children = escalations
            .values()
            .filter(|(level, _, _)| *level == "hard")
            .map(|(_, _, pane_id)| pane_id.clone())
            .collect::<BTreeSet<_>>();
        for agent in agents.iter_mut() {
            let (level, notice) = match escalations.get(&agent.pane_id) {
                Some((level, notice, _)) => ((*level).to_owned(), Some(notice.clone())),
                None => (String::new(), None),
            };
            agent.stall_level = level;
            agent.stall_notice = notice;
            // The child that ran out of time stops being drawn as somebody
            // else's work, because from here on it is the operator's.
            if hard_children.contains(&agent.pane_id) {
                agent.delegated = false;
            }
        }
        crate::sidebar::rederive_ownership(agents);
    }

    /// Whether a stall threshold has been crossed since the last publish.
    ///
    /// The coordinator asks this on the agent tick it already runs, so a
    /// stalled session - which by definition reports nothing new - still
    /// reaches the operator without a timer of Hide's own (PRD B36).
    pub fn stall_publish_due(&self) -> bool {
        self.stall_publish_due_at(unix_milliseconds())
    }

    pub(super) fn stall_publish_due_at(&self, now: u64) -> bool {
        if self.snapshot.status.herdr.state != "connected" {
            return false;
        }
        let agents = &self.snapshot.navigator.agents;
        let escalations = self.stall_escalations(agents, now);
        agents.iter().any(|agent| {
            let level = escalations
                .get(&agent.pane_id)
                .map(|(level, _, _)| *level)
                .unwrap_or("");
            agent.stall_level != level
        })
    }

    /// Marks the tabs that exist only to hold delegated children, and asks
    /// Herdr to move any child still sharing its parent's tab into one.
    ///
    /// Detection is the same on every pass, so a child that arrives while
    /// Hide is running and a child already split when Hide started are the
    /// same case and take the same path (PRD B1, B3, D-44). Herdr keeps
    /// owning split geometry and the PTY size, so the pane is really moved
    /// rather than merely left undrawn (PRD D-15).
    pub(super) fn relocate_delegated_child_panes(&mut self) -> bool {
        // Where Herdr currently holds each pane. The layout is the only place
        // that carries Herdr's own workspace and tab ids for a pane.
        let placement = self
            .snapshot
            .pane_layouts
            .iter()
            .flat_map(|layout| {
                layout
                    .pane_ids()
                    .into_iter()
                    .map(|pane_id| {
                        (
                            pane_id.to_owned(),
                            (layout.workspace_id.clone(), layout.tab_id.clone()),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<BTreeMap<_, _>>();
        let now = unix_milliseconds();
        let mut requests = Vec::new();
        for agent in &self.snapshot.navigator.agents {
            if !agent.delegated || crate::agent_hooks::is_remote_pane(&agent.pane_id) {
                continue;
            }
            let Some(parent_pane_id) = agent.lineage_parent_pane_id.as_deref() else {
                continue;
            };
            let (Some((workspace_id, tab_id)), Some((_, parent_tab_id))) =
                (placement.get(&agent.pane_id), placement.get(parent_pane_id))
            else {
                continue;
            };
            if tab_id != parent_tab_id {
                continue;
            }
            if self
                .pane_relocations_in_flight
                .get(&agent.pane_id)
                .is_some_and(|asked| now.saturating_sub(*asked) < RELOCATION_RETRY_INTERVAL_MS)
            {
                continue;
            }
            requests.push((
                agent.pane_id.clone(),
                workspace_id.clone(),
                agent.chat_title.clone().unwrap_or_else(|| agent.id.clone()),
            ));
        }
        // A pane Herdr no longer reports can never answer, so its record is
        // dropped rather than held forever.
        self.pane_relocations_in_flight
            .retain(|pane_id, _| placement.contains_key(pane_id));
        if requests.is_empty() {
            return false;
        }
        let Some(context) = self.live.as_ref().cloned() else {
            return false;
        };
        for (pane_id, workspace_id, label) in requests {
            self.pane_relocations_in_flight.insert(pane_id.clone(), now);
            if let Err(message) = live::spawn_pane_control(
                context.clone(),
                PaneControlAction::MoveToNewTab {
                    pane_id: pane_id.clone(),
                    workspace_id,
                    label,
                },
            ) {
                self.pane_relocations_in_flight.remove(&pane_id);
                self.push_diagnostic(
                    "lineage.relocate_failed",
                    format!("Could not move delegated pane {pane_id}: {message}"),
                );
            }
        }
        true
    }

    /// Takes the hook-install judgement the coordinator read off the lock.
    /// Queues an install the operator approved, or says why it cannot.
    ///
    /// The write itself happens on the coordinator thread: it is file I/O,
    /// and nothing that touches the disk runs under this mutex.
    pub(super) fn request_agent_hook_install(&mut self, runtime_id: &str) -> bool {
        let Some(runtime) = hide_agent_hooks::AgentRuntime::from_id(runtime_id) else {
            self.set_error(
                "agent_hooks.unknown_runtime",
                format!("Hide has no agent hook adapter for {runtime_id}"),
                false,
            );
            return true;
        };
        // Approving twice is one install: the request is a set, and the
        // install itself rewrites the same hook group either way.
        self.pending_hook_installs.insert(runtime);
        true
    }

    /// Applies one Background AI settings event.
    ///
    /// The choice takes effect on the snapshot at once, so the control moves
    /// under the operator's hand rather than after a file write; the write
    /// itself is queued for the coordinator, because nothing that touches the
    /// disk runs under this mutex.
    pub(super) fn apply_ai_settings(&mut self, payload: AiSettingsPayload) -> bool {
        let mut changed = false;
        if let Some(observing) = payload.observing
            && self.ai_observing != observing
        {
            self.ai_observing = observing;
            changed = true;
        }

        // A model without the provider it belongs to is not applied to
        // whichever provider happens to be selected: the event is refused and
        // says so.
        if payload.provider.is_none() && payload.model.is_some() {
            self.set_error(
                "ai_settings.model_without_provider",
                "A background AI model must name the provider it belongs to",
                false,
            );
            return true;
        }

        if let Some(id) = payload.provider.as_deref() {
            let Some(provider) = hide_ai::ProviderId::from_id(id) else {
                self.set_error(
                    "ai_settings.unknown_provider",
                    format!("Hide has no background AI provider called {id}"),
                    false,
                );
                return true;
            };
            let mut settings = self.ai_settings.clone().unwrap_or_default();
            // Naming a model keeps the current selection; naming only a
            // provider selects it. Choosing a model for the provider that is
            // already selected does both, which is the same thing.
            match payload.model {
                Some(model) => settings.set_model(provider, model),
                None => settings.provider = provider,
            }
            if self.ai_settings.as_ref() != Some(&settings) {
                self.ai_settings = Some(settings.clone());
                self.pending_ai_settings_save = Some(settings);
                changed = true;
            }
        }

        if changed {
            self.refresh_background_ai();
        }
        true
    }

    /// What the provider probe should ask, and whether it should ask at all.
    ///
    /// An empty answer while the group is off screen is intentional, the same
    /// way an empty disk request is: an idle Hide must never start a provider
    /// process.
    pub fn ai_request(&self) -> crate::ai::AiRequest {
        let settings = self.ai_settings.clone().unwrap_or_default();
        crate::ai::AiRequest {
            observing: self.ai_observing,
            models: hide_ai::PROVIDERS
                .iter()
                .map(|provider| (*provider, settings.model(*provider).to_owned()))
                .collect(),
        }
    }

    /// Hands a queued settings write to the caller that can perform it.
    pub(crate) fn take_ai_settings_save(&mut self) -> Option<hide_ai::AiSettings> {
        self.pending_ai_settings_save.take()
    }

    /// Stores the choice the coordinator read from the settings file, and
    /// whether reading it failed.
    ///
    /// A failed read is not taken as the defaults in silence: the defaults
    /// are used and the reason travels to the screen with them.
    pub(crate) fn ingest_ai_settings(
        &mut self,
        settings: hide_ai::AiSettings,
        chosen: bool,
        unavailable_reason: Option<String>,
    ) -> bool {
        let same = self.ai_settings.as_ref() == Some(&settings)
            && self.snapshot.status.background_ai.chosen == chosen
            && self.snapshot.status.background_ai.unavailable_reason == unavailable_reason;
        if same {
            return false;
        }
        self.ai_settings = Some(settings);
        self.snapshot.status.background_ai.chosen = chosen;
        self.snapshot.status.background_ai.unavailable_reason = unavailable_reason;
        self.refresh_background_ai();
        true
    }

    /// Stores what the providers answered.
    pub(crate) fn ingest_background_ai(
        &mut self,
        read: crate::model::BackgroundAiSnapshot,
    ) -> bool {
        if self.background_ai_providers == read.providers {
            return false;
        }
        self.background_ai_providers = read.providers;
        self.refresh_background_ai();
        true
    }

    /// Reports a settings write that did not happen, so a choice the operator
    /// made and the file on disk cannot silently disagree.
    pub(crate) fn report_ai_settings_failure(&mut self, reason: String) -> bool {
        if self
            .snapshot
            .status
            .background_ai
            .unavailable_reason
            .as_deref()
            == Some(reason.as_str())
        {
            return false;
        }
        self.snapshot.status.background_ai.unavailable_reason = Some(reason);
        true
    }

    /// Rebuilds the Background AI section from the choice and the last
    /// provider answers. The model each row reports is the configured one,
    /// which is what the probe was run with.
    pub(super) fn refresh_background_ai(&mut self) {
        let settings = self.ai_settings.clone().unwrap_or_default();
        let mut providers = if self.background_ai_providers.is_empty() {
            crate::model::BackgroundAiSnapshot::unread().providers
        } else {
            self.background_ai_providers.clone()
        };
        for row in &mut providers {
            if let Some(provider) = hide_ai::ProviderId::from_id(&row.id) {
                row.model = settings.model(provider).to_owned();
            }
        }
        self.snapshot.status.background_ai.provider = settings.provider.as_str().to_owned();
        self.snapshot.status.background_ai.providers = providers;
    }

    /// Hands the queued installs to the caller that can perform them.
    pub(crate) fn take_agent_hook_installs(&mut self) -> Vec<hide_agent_hooks::AgentRuntime> {
        std::mem::take(&mut self.pending_hook_installs)
            .into_iter()
            .collect()
    }

    pub(crate) fn ingest_hook_diagnosis(&mut self, diagnosis: hide_agent_hooks::Diagnosis) -> bool {
        if self.hook_diagnosis.as_ref() == Some(&diagnosis) {
            return false;
        }
        self.hook_diagnosis = Some(diagnosis);
        self.sync_pane_lineage();
        true
    }

    /// Steps one text scale by a direction the shell sent, or names the
    /// direction it could not read and answers `None`.
    pub(super) fn stepped_text_scale(&mut self, current: f32, direction: &str) -> Option<f32> {
        match direction {
            "in" => Some(clamp_pane_text_scale(current + PANE_TEXT_SCALE_STEP)),
            "out" => Some(clamp_pane_text_scale(current - PANE_TEXT_SCALE_STEP)),
            "reset" => Some(DEFAULT_PANE_TEXT_SCALE),
            other => {
                self.set_error(
                    "pane.text_scale_unknown_direction",
                    format!("{other} is not a text scale direction; expected in, out, or reset"),
                    true,
                );
                None
            }
        }
    }

    /// Saves the current UI state and surfaces a write failure instead of
    /// dropping it.
    /// Writes the operator's UI state and the pane sizes the next launch
    /// attaches with. The sizes are not part of the UI state the shell draws,
    /// so they are collected here rather than carried on the snapshot.
    pub(super) fn write_ui_state(&mut self) -> Result<(), String> {
        let Some(context) = self.worker_context.clone() else {
            // Standalone runtimes have no shared mutex or worker context.
            return persistence::save(
                &self.state_path,
                &self.snapshot.ui_state,
                &self
                    .terminal_sizes
                    .iter()
                    .map(|(id, size)| (id.clone(), *size))
                    .collect(),
            );
        };
        self.state_save_pending = true;
        if self.state_save_active {
            return Ok(());
        }
        self.state_save_active = true;
        match thread::Builder::new()
            .name("hide-state-save".into())
            .spawn(move || {
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                loop {
                    let (path, state, sizes) = {
                        let mut guard = runtime.lock().unwrap_or_else(|e| e.into_inner());
                        if !guard.state_save_pending {
                            guard.state_save_active = false;
                            return;
                        }
                        guard.state_save_pending = false;
                        (
                            guard.state_path.clone(),
                            guard.snapshot.ui_state.clone(),
                            guard
                                .terminal_sizes
                                .iter()
                                .map(|(id, size)| (id.clone(), *size))
                                .collect(),
                        )
                    };
                    // The existing save function serializes and writes outside the
                    // runtime mutex. One pending flag coalesces newer UI state.
                    if let Err(message) = persistence::save(&path, &state, &sizes) {
                        runtime.lock().unwrap_or_else(|e| e.into_inner()).set_error(
                            "ui_state.save_failed",
                            message,
                            true,
                        );
                        context.notifier.notify();
                    }
                }
            }) {
            Ok(worker) => {
                self.state_save_worker = Some(worker);
                Ok(())
            }
            Err(error) => {
                self.state_save_active = false;
                Err(format!("UI state save worker could not start: {error}"))
            }
        }
    }

    pub(super) fn persist_ui_state(&mut self) {
        if let Err(message) = self.write_ui_state() {
            self.set_error("ui_state.save_failed", message, true);
        }
    }

    pub fn ingest_fork_result(
        &mut self,
        parent_pane_id: &str,
        result: Result<String, String>,
        elapsed_ms: u128,
    ) -> bool {
        self.forks_in_flight.remove(parent_pane_id);
        // Either outcome leaves the process, because a fork that produced no
        // pane and no message is the report the operator brought: a modal
        // appeared and nothing else happened.
        match result {
            Ok(forked_pane_id) => {
                self.push_diagnostic(
                    "pane.fork.created",
                    format!("Forked pane {parent_pane_id} into {forked_pane_id} in {elapsed_ms}ms"),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_fork",
                    "kind": "pane.fork.created",
                    "pane_id": parent_pane_id,
                    "forked_pane_id": forked_pane_id,
                    "duration_ms": elapsed_ms,
                }));
                true
            }
            Err(message) => {
                self.set_error(
                    "pane.fork_failed",
                    format!("Pane {parent_pane_id} could not be forked: {message}"),
                    true,
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_fork",
                    "kind": "pane.fork_failed",
                    "pane_id": parent_pane_id,
                    "message": message,
                    "duration_ms": elapsed_ms,
                }));
                true
            }
        }
    }

    pub fn ingest_remote_control_result(
        &mut self,
        target_id: &str,
        request_id: &str,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        self.ingest_remote_control_result_with_generation(
            target_id, request_id, action, result, elapsed_ms, None,
        )
    }

    pub(crate) fn ingest_remote_control_result_with_generation(
        &mut self,
        target_id: &str,
        request_id: &str,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, String>,
        elapsed_ms: u128,
        connection_generation: Option<u64>,
    ) -> bool {
        self.ingest_remote_control_result_with_failure(
            target_id,
            request_id,
            action,
            result.map_err(live::ControlFailure::Definite),
            elapsed_ms,
            connection_generation,
        )
    }

    pub(crate) fn ingest_remote_control_failure_with_generation(
        &mut self,
        target_id: &str,
        request_id: &str,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, live::ControlFailure>,
        elapsed_ms: u128,
        connection_generation: Option<u64>,
    ) -> bool {
        self.ingest_remote_control_result_with_failure(
            target_id,
            request_id,
            action,
            result,
            elapsed_ms,
            connection_generation,
        )
    }

    fn ingest_remote_control_result_with_failure(
        &mut self,
        target_id: &str,
        request_id: &str,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, live::ControlFailure>,
        elapsed_ms: u128,
        connection_generation: Option<u64>,
    ) -> bool {
        let action_kind = action.kind();
        let remote_operation_key = (target_id.to_owned(), request_id.to_owned());
        if let Some(generation) = connection_generation {
            if self
                .remote_connection_generations
                .get(target_id)
                .copied()
                .unwrap_or(0)
                != generation
            {
                self.push_diagnostic(
                    "remote.control.stale_result",
                    format!(
                        "Ignored remote {action_kind} result for {target_id} from connection generation {generation}"
                    ),
                );
                return false;
            }
            if self
                .remote_operations
                .get(&remote_operation_key)
                .is_some_and(|operation| operation.connection_generation != generation)
            {
                return false;
            }
        }
        let tracked_remote_operation = self.remote_operations.contains_key(&remote_operation_key);
        let is_pane_focus = matches!(
            &action,
            RemoteControlAction::Pane(PaneControlAction::Focus { .. })
        );
        if self
            .remote_operations
            .get(&remote_operation_key)
            .and_then(|operation| operation.deadline_at_unix_ms)
            .is_some_and(|deadline| deadline <= unix_milliseconds())
        {
            let message =
                "Remote result arrived after its deadline; no mutation was resent".to_owned();
            self.mark_remote_operation_unknown(&remote_operation_key, message.clone());
            if is_pane_focus {
                self.finish_pane_focus_request_by_id(request_id, "failed", Some(message), true);
            }
            return true;
        }
        if let Some(key) = remote_tab_creation_key(target_id, &action) {
            self.remote_tab_creations_in_flight.remove(&key);
        }
        match result {
            Ok(RemoteControlOutcome::Acknowledged {
                created_tab_id,
                created_pane_id,
            }) => {
                if tracked_remote_operation {
                    self.acknowledge_remote_operation(
                        target_id,
                        request_id,
                        created_pane_id.clone(),
                    );
                }
                if is_pane_focus {
                    self.finish_pane_focus_request_by_id(request_id, "succeeded", None, false);
                }
                let mut receipt = String::new();
                if let Some(tab_id) = created_tab_id.as_deref() {
                    receipt.push_str(&format!("; created tab {tab_id}"));
                }
                if let Some(pane_id) = created_pane_id.as_deref() {
                    receipt.push_str(&format!("; created pane {pane_id}"));
                }
                self.push_diagnostic(
                    "remote.control.ready",
                    format!(
                        "{action_kind} for {target_id} acknowledged in {elapsed_ms} ms{receipt}; awaiting authoritative event"
                    ),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_control",
                    "kind": "remote.control.ready",
                    "target": target_id,
                    "request_id": request_id,
                    "action": action_kind,
                    "created_tab_id": created_tab_id,
                    "created_pane_id": created_pane_id,
                    "duration_ms": elapsed_ms,
                }));
            }
            // A remote target owns its tab order; `spawn_remote_control`
            // refuses the only action that reports one back.
            Ok(RemoteControlOutcome::TabsOrdered { .. }) => {
                if tracked_remote_operation {
                    self.fail_remote_operation(
                        target_id,
                        request_id,
                        format!("{action_kind} returned a tab-order outcome"),
                    );
                }
                if is_pane_focus {
                    self.finish_pane_focus_request_by_id(
                        request_id,
                        "failed",
                        Some(format!(
                            "{action_kind} for {target_id} returned an invalid outcome"
                        )),
                        true,
                    );
                }
                if !tracked_remote_operation {
                    self.set_error(
                        "remote.control.failed",
                        format!("{action_kind} for {target_id} returned a tab order remotely"),
                        false,
                    );
                }
            }
            Err(error) => {
                let message = error.message().to_owned();
                if tracked_remote_operation {
                    if error.is_ambiguous() {
                        self.mark_remote_operation_unknown(
                            &remote_operation_key,
                            format!(
                                "{message}; no mutation was resent and a fresh topology is required"
                            ),
                        );
                    } else {
                        self.fail_remote_operation(target_id, request_id, message.clone());
                    }
                }
                if is_pane_focus {
                    self.finish_pane_focus_request_by_id(
                        request_id,
                        "failed",
                        Some(message.clone()),
                        true,
                    );
                }
                if !tracked_remote_operation {
                    self.set_error(
                        "remote.control.failed",
                        format!("{action_kind} for {target_id} failed: {message}"),
                        true,
                    );
                }
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_control",
                    "kind": "remote.control.failed",
                    "target": target_id,
                    "request_id": request_id,
                    "action": action_kind,
                    "message": message,
                    "duration_ms": elapsed_ms,
                }));
            }
        }
        true
    }

    /// Names each agent by the project and checkout its pane sits in, as the
    /// sidebar tree shows them. Herdr's own workspace label ("hide main") is
    /// a launcher artifact that can span two repositories, so it is kept only
    /// for a pane the navigator has not placed.
    pub(super) fn place_agents_in_navigator(&self, agents: &mut [SidebarAgentSnapshot]) {
        for agent in agents {
            let placed = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .find_map(|workspace| {
                    workspace.checkouts.iter().find_map(|checkout| {
                        checkout
                            .tabs
                            .iter()
                            .flat_map(|tab| tab.panes.iter())
                            .any(|pane| pane.id == agent.pane_id)
                            .then(|| (workspace.label.clone(), checkout.label.clone()))
                    })
                });
            if let Some((workspace_label, checkout_label)) = placed {
                agent.workspace_label = workspace_label;
                agent.checkout_label = Some(checkout_label);
            }
        }
    }

    pub(super) fn refresh_agent_lineage(&mut self) {
        crate::sidebar::apply_lineage(
            &mut self.snapshot.navigator.agents,
            &self.snapshot.navigator.workspaces,
            &self.snapshot.ui_state.collapsed_agent_pane_ids,
        );
        for remote in &mut self.snapshot.status.remote {
            if let Some(session) = &mut remote.session {
                crate::sidebar::apply_lineage(
                    &mut session.agents,
                    &session.workspaces,
                    &self.snapshot.ui_state.collapsed_agent_pane_ids,
                );
            }
        }
    }
}
