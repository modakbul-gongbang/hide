use super::*;

impl Runtime {
    pub(super) fn reconcile_remote_terminal_panes(
        &mut self,
        target_id: &str,
        live_pane_ids: &HashSet<String>,
        active_pane_ids: &HashSet<String>,
    ) -> bool {
        let target_prefix = remote_pane_id_prefix(target_id);
        let belongs_to_target = |pane_id: &str| pane_id.starts_with(&target_prefix);
        let projected_pane_ids = self
            .snapshot
            .terminal
            .panes
            .iter()
            .filter(|pane| belongs_to_target(&pane.pane_id))
            .map(|pane| pane.pane_id.clone())
            .collect::<HashSet<_>>();
        let mut changed = &projected_pane_ids != live_pane_ids;
        // A pane the remote no longer lists loses everything; a pane it lists
        // but does not run a session for keeps its sizes and loses the session.
        changed |= self.retain_terminal_pane_state(|pane_id| {
            !belongs_to_target(pane_id) || live_pane_ids.contains(pane_id)
        });
        changed |= self.retain_terminal_session_state(|pane_id| {
            !belongs_to_target(pane_id) || active_pane_ids.contains(pane_id)
        });

        self.snapshot.terminal.panes.retain(|pane| {
            !belongs_to_target(&pane.pane_id) || live_pane_ids.contains(&pane.pane_id)
        });
        let mut pane_ids = live_pane_ids.iter().cloned().collect::<Vec<_>>();
        pane_ids.sort();
        for pane_id in &pane_ids {
            self.ensure_terminal_pane(pane_id);
        }
        let idle_lifecycle = TerminalSessionLifecycle::default();
        for pane in self.snapshot.terminal.panes.iter_mut().filter(|pane| {
            belongs_to_target(&pane.pane_id) && !active_pane_ids.contains(&pane.pane_id)
        }) {
            let idle = TerminalPaneSnapshot {
                pane_id: pane.pane_id.clone(),
                closed: false,
                exit_code: None,
                transport_state: idle_lifecycle.state.to_owned(),
                transport_message: None,
                transport_generation: 0,
                transport_attempt: 0,
                transport_last_attempt_at_unix_ms: None,
                transport_exit_category: None,
                transport_retry_decision: idle_lifecycle.retry_decision.to_owned(),
            };
            if *pane != idle {
                *pane = idle;
                changed = true;
            }
        }
        let mut active_pane_ids = active_pane_ids.iter().cloned().collect::<Vec<_>>();
        active_pane_ids.sort();
        for pane_id in active_pane_ids {
            let current = self
                .terminal_session_lifecycles
                .get(&pane_id)
                .cloned()
                .unwrap_or_default();
            changed |= terminal_control_request_allowed(
                current.state,
                self.terminal_sessions.contains_key(&pane_id),
            );
            self.request_terminal_control(&pane_id);
        }
        changed
    }
    /// Removes every piece of terminal state for panes that no longer exist.
    /// Keeping this list in one place prevents a newly added pane-keyed cache
    /// from surviving retirement and being inherited if Herdr reuses an id.
    pub(super) fn retain_terminal_pane_state(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let before = self.terminal_state_len();
        self.retain_terminal_session_state(&keep);
        self.terminal_sizes.retain(|pane_id, _| keep(pane_id));
        self.terminal_view_sizes.retain(|pane_id, _| keep(pane_id));
        self.panes_closing.retain(|pane_id| keep(pane_id));
        before != self.terminal_state_len()
    }
    /// Removes the state of a pane's terminal session while the pane itself,
    /// and so its sizes, stays known.
    pub(super) fn retain_terminal_session_state(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let before = self.terminal_state_len();
        self.terminal_sessions.retain(|pane_id, _| keep(pane_id));
        self.terminal_session_generations
            .retain(|pane_id, _| keep(pane_id));
        self.terminal_session_lifecycles
            .retain(|pane_id, _| keep(pane_id));
        self.terminal_recovery.retain(|pane_id, _| keep(pane_id));
        self.terminal_frames_need_full
            .retain(|pane_id| keep(pane_id));
        self.terminal_foreign_frame_sizes
            .retain(|pane_id, _| keep(pane_id));
        self.panes_awaiting_size.retain(|pane_id| keep(pane_id));
        self.panes_scrolled_before_size
            .retain(|pane_id| keep(pane_id));
        before != self.terminal_state_len()
    }
    /// Every pane-keyed terminal map, counted together so a retain pass can
    /// report whether it removed anything.
    pub(super) fn terminal_state_len(&self) -> usize {
        self.terminal_sessions.len()
            + self.terminal_session_generations.len()
            + self.terminal_session_lifecycles.len()
            + self.terminal_recovery.len()
            + self.terminal_sizes.len()
            + self.terminal_view_sizes.len()
            + self.terminal_frames_need_full.len()
            + self.terminal_foreign_frame_sizes.len()
            + self.panes_awaiting_size.len()
            + self.panes_scrolled_before_size.len()
            + self.panes_closing.len()
    }
    pub(super) fn reconcile_remote_terminal_selection(&mut self) -> bool {
        let focused_device_id = self.snapshot.navigator.focused_device_id.clone();
        let sessions = self
            .snapshot
            .status
            .remote
            .iter()
            .filter_map(|status| {
                status
                    .session
                    .clone()
                    .map(|session| (status.target_id.clone(), session))
            })
            .collect::<Vec<_>>();
        sessions
            .into_iter()
            .fold(false, |changed, (target_id, session)| {
                let (live_pane_ids, active_pane_ids) = remote_terminal_pane_sets(
                    &session,
                    focused_device_id.as_deref() == Some(target_id.as_str()),
                );
                self.reconcile_remote_terminal_panes(&target_id, &live_pane_ids, &active_pane_ids)
                    | changed
            })
    }
    pub(super) fn ensure_terminal_pane(&mut self, pane_id: &str) {
        if self
            .snapshot
            .terminal
            .panes
            .iter()
            .any(|pane| pane.pane_id == pane_id)
        {
            return;
        }
        let pane = self.terminal_pane_snapshot(pane_id);
        self.snapshot.terminal.panes.push(pane);
    }
    pub(super) fn terminal_pane_snapshot(&self, pane_id: &str) -> TerminalPaneSnapshot {
        let lifecycle = self
            .terminal_session_lifecycles
            .get(pane_id)
            .cloned()
            .unwrap_or_default();
        TerminalPaneSnapshot {
            pane_id: pane_id.to_owned(),
            closed: false,
            exit_code: None,
            transport_state: lifecycle.state.to_owned(),
            transport_message: lifecycle.message,
            transport_generation: lifecycle.generation,
            transport_attempt: lifecycle.attempt,
            transport_last_attempt_at_unix_ms: self
                .terminal_recovery
                .get(pane_id)
                .and_then(|r| r.last_attempt_at_unix_ms),
            transport_exit_category: lifecycle.exit_category,
            transport_retry_decision: lifecycle.retry_decision.to_owned(),
        }
    }
    pub(super) fn sync_transport_projection(&mut self, pane_id: &str) {
        let Some(lifecycle) = self.terminal_session_lifecycles.get(pane_id).cloned() else {
            return;
        };
        if let Some(pane) = self
            .snapshot
            .terminal
            .panes
            .iter_mut()
            .find(|pane| pane.pane_id == pane_id)
        {
            pane.transport_state = lifecycle.state.to_owned();
            pane.transport_message = lifecycle.message;
            pane.transport_generation = lifecycle.generation;
            pane.transport_attempt = lifecycle.attempt;
            pane.transport_last_attempt_at_unix_ms = self
                .terminal_recovery
                .get(pane_id)
                .and_then(|r| r.last_attempt_at_unix_ms);
            pane.transport_exit_category = lifecycle.exit_category;
            pane.transport_retry_decision = lifecycle.retry_decision.to_owned();
        }
    }
    pub(super) fn sync_focused_terminal_projection(&mut self) {
        let Some(pane_id) = self.snapshot.terminal.pane_id.as_deref() else {
            self.snapshot.terminal.closed = false;
            self.snapshot.terminal.exit_code = None;
            return;
        };
        if let Some(pane) = self
            .snapshot
            .terminal
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
        {
            self.snapshot.terminal.closed = pane.closed;
            self.snapshot.terminal.exit_code = pane.exit_code;
        }
    }
    /// Applies a background pane-control result to the owner-thread snapshot.
    /// The child process is never waited on while the Swift caller holds the
    /// runtime lock; completion arrives through the normal change callback.
    /// Records the outcome of a fork worker.
    ///
    /// `herdr agent new` creates the pane and starts the agent in one atomic
    /// call, so a failure leaves nothing behind and there is no half-made pane
    /// to clean up. The reason it failed is reported rather than swallowed.
    /// Records the machine's listeners and re-attributes every pane to them.
    ///
    /// Panes are re-walked here because ports arrive on their own window rather
    /// than with a session snapshot, so a server that started since the last
    /// topology update would otherwise stay invisible until the topology moved.
    pub fn ingest_listening_ports(&mut self, ports: crate::model::ListeningPortsSnapshot) -> bool {
        if self.listening_ports == ports {
            return false;
        }
        self.listening_ports = ports;
        let entries = self.listening_ports.entries.clone();
        let mut changed = false;
        for workspace in self.snapshot.navigator.workspaces.iter_mut() {
            for checkout in workspace.checkouts.iter_mut() {
                for tab in checkout.tabs.iter_mut() {
                    for pane in tab.panes.iter_mut() {
                        let attributed = crate::ports::attributed_ports(&pane.cwd, &entries);
                        if pane.ports != attributed {
                            pane.ports = attributed;
                            changed = true;
                        }
                    }
                }
            }
        }
        changed
    }
    /// Stores what a pane search found and moves the viewport to the match.
    ///
    /// The scroll goes through the same terminal-control write the wheel uses,
    /// because Herdr owns the pane's history and answers a viewport move with a
    /// fresh frame either way.
    pub fn ingest_pane_find(
        &mut self,
        pane_id: &str,
        result: Result<live::PaneFindOutcome, String>,
    ) -> bool {
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(message) => {
                self.snapshot.find = PaneFindSnapshot {
                    pane_id: Some(pane_id.to_owned()),
                    unavailable_reason: Some(message),
                    ..PaneFindSnapshot::default()
                };
                return true;
            }
        };
        let next = PaneFindSnapshot {
            pane_id: Some(pane_id.to_owned()),
            term: outcome.term,
            index: outcome.index,
            total: outcome.total,
            truncated: outcome.truncated,
            unavailable_reason: None,
        };
        let changed = self.snapshot.find != next;
        self.snapshot.find = next;
        if let Some((direction, lines)) = outcome.scroll {
            if let Some(said) = self.scroll_withheld_for_missing_size(pane_id) {
                return said || changed;
            }
            let lines = i32::from(lines) * if direction == "up" { 1 } else { -1 };
            if let Some(session) = self.terminal_sessions.get_mut(pane_id)
                && session.mode == TerminalSessionMode::Control
                && let Err(message) = session.scroll(live::ScrollRequest {
                    lines,
                    ..Default::default()
                })
            {
                self.set_error("terminal.scroll_failed", message, true);
                return true;
            }
        }
        changed
    }
    /// Whether a scroll must be withheld because the pane reported no size,
    /// and whether saying so changed the snapshot.
    ///
    /// `None` means the pane can be scrolled. The attach is held back until
    /// the same size arrives, so a pane without one has nothing to write to,
    /// and the guessed 24x80 that used to stand in only ever resized the PTY
    /// to a grid it was not running at. Every scroll producer goes through
    /// here so the wait is said once per pane rather than once per producer.
    pub(super) fn scroll_withheld_for_missing_size(&mut self, pane_id: &str) -> Option<bool> {
        if self.terminal_sizes.contains_key(pane_id) {
            return None;
        }
        if self.panes_scrolled_before_size.insert(pane_id.to_owned()) {
            self.push_diagnostic(
                "terminal.scroll_deferred",
                format!(
                    "Pane {pane_id} was scrolled before its view reported a size; nothing was sent"
                ),
            );
            return Some(true);
        }
        Some(false)
    }
    pub fn ingest_pane_control_result(
        &mut self,
        action: PaneControlAction,
        result: Result<PaneControlOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        match (action, result) {
            (
                PaneControlAction::Project { pane_id },
                Ok(PaneControlOutcome::Projected { layout }),
            ) => {
                if self.snapshot.terminal.pane_id.as_deref() != Some(pane_id.as_str()) {
                    self.push_diagnostic(
                        "pane.projection.stale",
                        format!("Ignored stale projection for pane {pane_id}"),
                    );
                    return false;
                }
                if !layout.pane_ids().contains(&pane_id.as_str()) {
                    self.set_error(
                        "pane.projection_mismatch",
                        format!("Projected layout does not contain pane {pane_id}"),
                        true,
                    );
                    return true;
                }
                self.push_diagnostic(
                    "pane.projection.ready",
                    format!("Pane {pane_id} projected in {elapsed_ms} ms"),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_projection",
                    "kind": "pane.projection_ready",
                    "pane_id": pane_id,
                    "duration_ms": elapsed_ms,
                }));
                self.apply_pane_layout(layout, false);
                true
            }
            (PaneControlAction::MoveToNewTab { pane_id, .. }, outcome) => {
                match outcome {
                    Ok(_) => {
                        self.pane_relocations_in_flight.remove(&pane_id);
                        self.push_diagnostic(
                            "lineage.relocated",
                            format!(
                                "Delegated pane {pane_id} moved to its own tab in {elapsed_ms} ms"
                            ),
                        );
                    }
                    Err(error) => {
                        // The request's stamp stays, so the next attempt waits
                        // out RELOCATION_RETRY_INTERVAL_MS: dropping it here
                        // re-sent a refused move on every tick.
                        // Deliberately not a `pane.` diagnostic: the pane
                        // header reads those, and this failure must not
                        // appear over a child the operator never asked to
                        // move (PRD B2).
                        self.push_diagnostic(
                            "lineage.relocate_failed",
                            format!("Could not move delegated pane {pane_id}: {error}"),
                        );
                    }
                }
                true
            }
            (PaneControlAction::Focus { pane_id }, Ok(PaneControlOutcome::Acknowledged { .. })) => {
                self.push_diagnostic(
                    "pane.focus",
                    format!(
                        "Pane {pane_id} focus acknowledged in {elapsed_ms} ms; awaiting authoritative event"
                    ),
                );
                true
            }
            (
                PaneControlAction::Split {
                    pane_id, direction, ..
                },
                Ok(PaneControlOutcome::Acknowledged { created_pane_id }),
            ) => {
                let Some(created_pane_id) = created_pane_id else {
                    self.set_error(
                        "pane.split_invalid_response",
                        "Pane split completed without a created pane id",
                        true,
                    );
                    return true;
                };
                self.push_diagnostic(
                    format!("pane.split.{}", direction.as_str()),
                    format!(
                        "Pane {pane_id} split {} to {created_pane_id} in {elapsed_ms} ms",
                        direction.as_str()
                    ),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_control",
                    "kind": "pane.split_ready",
                    "pane_id": pane_id,
                    "created_pane_id": created_pane_id,
                    "direction": direction.as_str(),
                    "duration_ms": elapsed_ms,
                }));
                true
            }
            (
                PaneControlAction::Resize {
                    pane_id,
                    direction,
                    amount,
                },
                Ok(PaneControlOutcome::Acknowledged { .. }),
            ) => {
                self.push_diagnostic(
                    "pane.resize",
                    format!(
                        "Pane {pane_id} resize {} by {amount:.3} acknowledged in {elapsed_ms} ms; awaiting authoritative event",
                        direction.as_str()
                    ),
                );
                true
            }
            (
                PaneControlAction::ToggleZoom { pane_id },
                Ok(PaneControlOutcome::Acknowledged { .. }),
            ) => {
                self.push_diagnostic(
                    "pane.zoom_toggled",
                    format!(
                        "Pane {pane_id} zoom acknowledged in {elapsed_ms} ms; awaiting authoritative event"
                    ),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_control",
                    "kind": "pane.zoom_ready",
                    "pane_id": pane_id,
                    "duration_ms": elapsed_ms,
                }));
                true
            }
            (PaneControlAction::Close { pane_id }, Ok(PaneControlOutcome::Acknowledged { .. })) => {
                self.push_diagnostic(
                    "pane.close",
                    format!(
                        "Pane {pane_id} close acknowledged in {elapsed_ms} ms; awaiting authoritative event"
                    ),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "pane_control",
                    "kind": "pane.close_ready",
                    "pane_id": pane_id,
                    "duration_ms": elapsed_ms,
                }));
                true
            }
            (PaneControlAction::Project { .. }, Ok(PaneControlOutcome::Acknowledged { .. }))
            | (
                PaneControlAction::Focus { .. }
                | PaneControlAction::Split { .. }
                | PaneControlAction::Resize { .. }
                | PaneControlAction::ToggleZoom { .. }
                | PaneControlAction::Close { .. },
                Ok(PaneControlOutcome::Projected { .. }),
            ) => {
                self.set_error(
                    "pane.control_invalid_outcome",
                    "Pane control returned an outcome for the wrong operation class",
                    false,
                );
                true
            }
            (PaneControlAction::Project { .. }, Err(message)) => {
                self.set_error("pane.projection_failed", message, true);
                true
            }
            (PaneControlAction::Focus { pane_id }, Err(message)) => {
                // Hide keeps the pane it focused. The refusal is reported and
                // the wait ends, so the next Herdr event naming another pane
                // is read as the authority it is rather than as a late answer.
                self.clear_refused_view_focus(ViewFocusSlot::Pane, &pane_id, &message);
                self.set_error("pane.focus_failed", message, true);
                true
            }
            (PaneControlAction::Split { .. }, Err(message)) => {
                self.set_error("pane.split_failed", message, true);
                true
            }
            (PaneControlAction::Resize { .. }, Err(message)) => {
                self.set_error("pane.resize_failed", message, true);
                true
            }
            (PaneControlAction::ToggleZoom { .. }, Err(message)) => {
                self.set_error("pane.zoom_failed", message, true);
                true
            }
            (PaneControlAction::Close { .. }, Err(message)) => {
                self.set_error("pane.close_failed", message, true);
                true
            }
        }
    }
    /// The grid a frame has to arrive at to be drawn: the view's own grid
    /// while one is known, else the settled size the attach asked for.
    pub(super) fn expected_terminal_size(&self, pane_id: &str) -> Option<(u16, u16)> {
        self.terminal_view_sizes
            .get(pane_id)
            .or_else(|| self.terminal_sizes.get(pane_id))
            .copied()
    }
    /// Appends only the decoded frame bytes when the delivering official
    /// terminal session is still the current generation and mode.
    /// None retires the reader; Some(false) keeps reading a held frame without
    /// publishing it. Skipping a foreign grid must not terminate observation.
    pub fn ingest_terminal_session_frame(
        &mut self,
        pane_id: &str,
        generation: u64,
        mode: TerminalSessionMode,
        bytes: &[u8],
        frame: crate::model::TerminalFrame,
    ) -> Option<bool> {
        if self.terminal_session_generations.get(pane_id) != Some(&generation)
            || self
                .terminal_sessions
                .get(pane_id)
                .is_none_or(|session| session.mode != mode)
        {
            return None;
        }
        let expected = self.expected_terminal_size(pane_id);
        let arrived = (frame.height, frame.width);
        if expected != Some(arrived) {
            self.terminal_frames_need_full.insert(pane_id.to_owned());
            // Logged to the file and stderr sink only, once per foreign grid.
            // A push into the snapshot's diagnostics would restamp the
            // revisioned rest section on every frame of a mismatch burst.
            if self
                .terminal_foreign_frame_sizes
                .insert(pane_id.to_owned(), arrived)
                != Some(arrived)
            {
                crate::diagnostic!(serde_json::json!({
                    "component": "terminal", "kind": "terminal.frame_geometry_mismatch",
                    "pane_id": pane_id, "frame": [frame.width, frame.height],
                    "expected": expected.map(|(height, width)| [width, height]),
                }));
            }
            return Some(false);
        }
        self.terminal_foreign_frame_sizes.remove(pane_id);
        if self.terminal_frames_need_full.contains(pane_id) && !frame.full {
            return Some(false);
        }
        // Preserve the last valid view during a retry. Reset the parser only
        // as part of the replacement full frame, so no empty canvas is exposed.
        let reset_bytes = self
            .terminal_frames_need_full
            .remove(pane_id)
            .then(|| [b"\x1bc".as_slice(), bytes].concat());
        let bytes = reset_bytes.as_deref().unwrap_or(bytes);
        if mode == TerminalSessionMode::Control {
            if let Some(recovery) = self.terminal_recovery.remove(pane_id) {
                crate::diagnostic!(serde_json::json!({
                    "kind": "terminal.control_frame_ready", "pane_id": pane_id,
                    "generation": generation, "occurred_at": unix_milliseconds(),
                    "retries": recovery.retries,
                    "last_attempt_at_unix_ms": recovery.last_attempt_at_unix_ms,
                    "rows": frame.height, "cols": frame.width,
                }));
            }
            if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
                lifecycle.message = None;
                lifecycle.retry_decision = "none";
            }
            self.sync_transport_projection(pane_id);
        }
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(bytes));
        self.snapshot
            .terminal
            .chunks
            .last_mut()
            .expect("just appended frame")
            .frame = Some(frame);
        Some(true)
    }
    /// Handles a `terminal.closed` envelope or stdout EOF. An owner conflict
    /// falls back exactly once to Herdr's concurrent read-only observer; every
    /// other close ends only the transport, never the authoritative pane.
    /// Whether this pane is on its way out: Hide asked Herdr to close it, or
    /// Herdr has already stopped listing it in any tab's layout.
    pub(super) fn pane_is_going_away(&self, pane_id: &str) -> bool {
        if self.panes_closing.contains(pane_id) {
            return true;
        }
        // An empty layout list is a session that has not arrived, not a pane
        // that left one.
        !self.snapshot.pane_layouts.is_empty() && self.layout_holding_pane(pane_id).is_none()
    }
    pub fn ingest_terminal_session_closed(
        &mut self,
        pane_id: &str,
        generation: u64,
        mode: TerminalSessionMode,
        reason: Option<String>,
    ) -> bool {
        if self.terminal_session_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        if self
            .terminal_sessions
            .get(pane_id)
            .is_none_or(|session| session.mode != mode)
        {
            return false;
        }
        let _ended_session = self.terminal_sessions.remove(pane_id);
        let attempt = self
            .terminal_session_lifecycles
            .get(pane_id)
            .map_or(1, |lifecycle| lifecycle.attempt);
        let category = live::terminal_closed_category(reason.as_deref());
        let message = reason
            .unwrap_or_else(|| format!("Pane {pane_id} terminal {} session ended", mode.as_str()));

        if mode == TerminalSessionMode::Control && category == "owner_conflict" {
            crate::diagnostic!(serde_json::json!({
                "component": "terminal_session",
                "kind": "terminal.control_owner_conflict",
                "pane_id": pane_id,
                "generation": generation,
                "attempt": attempt,
                "mode": mode.as_str(),
                "duration_ms": 0,
                "exit_category": category,
                "retry_decision": self.terminal_retry_decision(pane_id, "observe_once"),
            }));
            self.schedule_terminal_recovery(
                pane_id,
                "Another client owns terminal control; viewing read-only".to_owned(),
            );
            let retry_message = self.terminal_recovery.get(pane_id).map(|r| r.message());
            self.start_terminal_session(
                pane_id,
                TerminalSessionMode::Observe,
                attempt,
                "observe_once",
                retry_message,
            );
            return true;
        }

        // A pane that is going away, either because Hide asked or because
        // Herdr has already stopped reporting it, ends its transport as a
        // consequence of the close. It is not a failure, so nothing is drawn
        // over the pane's last frame and no notice is appended to it: the pane
        // keeps what it was showing until it is removed. Every other reason
        // still reports itself.
        if self.pane_is_going_away(pane_id) {
            self.panes_closing.remove(pane_id);
            self.terminal_session_lifecycles.insert(
                pane_id.to_owned(),
                TerminalSessionLifecycle {
                    state: "closing",
                    message: None,
                    generation,
                    attempt,
                    mode: Some(mode),
                    exit_category: Some(category.to_owned()),
                    retry_decision: "none",
                },
            );
            self.sync_transport_projection(pane_id);
            crate::diagnostic!(serde_json::json!({
                "component": "terminal_session",
                "kind": "terminal.session_closed_with_pane",
                "pane_id": pane_id,
                "generation": generation,
                "attempt": attempt,
                "mode": mode.as_str(),
                "duration_ms": 0,
                "exit_category": category,
                "retry_decision": "none",
            }));
            return true;
        }

        self.terminal_session_lifecycles.insert(
            pane_id.to_owned(),
            TerminalSessionLifecycle {
                state: "ended",
                message: Some(message.clone()),
                generation,
                attempt,
                mode: Some(mode),
                exit_category: Some(category.to_owned()),
                retry_decision: "manual",
            },
        );
        self.schedule_terminal_recovery(pane_id, message.clone());
        self.sync_transport_projection(pane_id);
        let notice = format!("\r\n[{message}]\r\n");
        self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(notice.as_bytes()));
        crate::diagnostic!(serde_json::json!({
            "component": "terminal_session",
            "kind": "terminal.session_ended",
            "pane_id": pane_id,
            "generation": generation,
            "attempt": attempt,
            "mode": mode.as_str(),
            "duration_ms": 0,
            "exit_category": category,
            "retry_decision": self.terminal_retry_decision(pane_id, "manual"),
        }));
        true
    }
    pub fn ingest_terminal_session_write_failure(
        &mut self,
        pane_id: &str,
        generation: u64,
        message: String,
    ) -> bool {
        if self.terminal_session_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        self.set_error("terminal.write_failed", message.clone(), true);
        if !pane_id.starts_with("remote:") {
            self.terminal_sessions.remove(pane_id);
            if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
                lifecycle.state = "unavailable";
            }
            self.schedule_terminal_recovery(pane_id, message.clone());
            self.sync_transport_projection(pane_id);
        }
        crate::diagnostic!(serde_json::json!({
            "component": "terminal_session",
            "kind": "terminal.control_write_failed",
            "pane_id": pane_id,
            "generation": generation,
            "message": message,
            "retry_decision": self.terminal_retry_decision(pane_id, "manual"),
        }));
        true
    }
    /// Drops the rendered terminal state before selecting a pane in another
    /// checkout. Herdr's globally focused pane may belong to another
    /// workspace, so retaining the attach set here would let the next sync
    /// update redraw stale terminal content while the selected checkout has
    /// no pane yet.
    ///
    /// The layouts are not dropped. They describe every tab in the session,
    /// they are Herdr's and not this selection's, and emptying them to mark a
    /// selection in progress is what made the canvas pass through a blank
    /// frame on the way to the tab the operator asked for.
    pub(super) fn clear_terminal_projection(&mut self) {
        self.snapshot.zoomed = None;
        self.snapshot.terminal.panes.clear();
        self.snapshot.terminal.closed = false;
        self.snapshot.terminal.exit_code = None;
    }
    /// Points the terminal projection at another pane without discarding
    /// anything Herdr has said.
    ///
    /// Zoom is re-read from that pane's own layout rather than carried over,
    /// so leaving a zoomed tab does not leak its zoom into the next one.
    pub(super) fn select_terminal_pane(&mut self, pane_id: Option<String>) {
        let zoomed = pane_id
            .as_deref()
            .and_then(|pane_id| self.layout_holding_pane(pane_id))
            .and_then(|layout| layout.zoomed.then(|| layout.focused_pane_id.clone()));
        let pane_ids = pane_id
            .as_deref()
            .and_then(|pane_id| self.layout_holding_pane(pane_id))
            .map(|layout| {
                layout
                    .pane_ids()
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.snapshot.terminal.pane_id = pane_id.clone();
        self.snapshot.focused.surface = Surface::Terminal;
        self.snapshot.focused.pane_id = pane_id.clone();
        self.snapshot.ui_state.selected_pane_id = pane_id;
        self.snapshot.zoomed = zoomed;
        for pane_id in pane_ids {
            self.ensure_terminal_pane(&pane_id);
        }
        self.sync_focused_terminal_projection();
    }
    pub(super) fn reset_terminal_projection(&mut self, pane_id: Option<String>) {
        self.clear_terminal_projection();
        self.select_terminal_pane(pane_id);
    }
    /// A launcher result is a local projection anchor, not a Herdr focus
    /// request. Keep it authoritative over an older terminal pane while the
    /// next event-stream projection catches up, and make the missing layout
    /// visible instead of retaining unrelated same-cwd content.
    pub(super) fn apply_selected_pane_anchor(&mut self, pane_id: Option<String>) {
        let layout_contains_pane = pane_id
            .as_deref()
            .is_some_and(|selected_pane_id| self.layout_holding_pane(selected_pane_id).is_some());
        self.snapshot.terminal.pane_id = pane_id.clone();
        self.snapshot.focused.surface = Surface::Terminal;
        self.snapshot.focused.pane_id = pane_id.clone();
        if !layout_contains_pane {
            self.clear_terminal_projection();
            if let Some(pane_id) = pane_id.as_deref() {
                self.set_error(
                    "pane.projection_unavailable",
                    format!(
                        "Selected pane {pane_id} is not present in the Herdr session; terminal projection is waiting"
                    ),
                    true,
                );
            }
        }
        self.sync_focused_terminal_projection();
    }
    /// Opens or focuses the file tab for one path in a checkout Hide is
    /// already showing. The caller owns the context check and the persistence,
    /// because a reveal has already made that decision by the time it gets
    /// here and would otherwise make it twice.
    /// The tab the operator is looking at: the focused checkout's visible tab.
    ///
    /// Another checkout's visible tab is that checkout's memory, not a tab on
    /// screen, so it does not renew an attach.
    pub(super) fn focused_visible_tab_id(&self) -> Option<String> {
        self.snapshot
            .navigator
            .focused_checkout_id
            .as_deref()
            .and_then(|checkout_id| self.visible_tab_ids.get(checkout_id))
            .cloned()
    }
    /// Records that a tab was on screen and releases whatever fell out of the
    /// window that leaves.
    pub(super) fn track_visible_tab_attachments(&mut self) -> bool {
        let known_tabs = self
            .snapshot
            .pane_layouts
            .iter()
            .map(|layout| layout.tab_id.clone())
            .collect::<HashSet<_>>();
        // A tab Herdr no longer reports cannot come back, so holding its slot
        // would shrink the window for the tabs that can.
        self.recent_visible_tabs
            .retain(|tab_id| known_tabs.contains(tab_id));
        if let Some(tab_id) = self.focused_visible_tab_id()
            && self.recent_visible_tabs.first() != Some(&tab_id)
        {
            self.recent_visible_tabs.retain(|held| held != &tab_id);
            self.recent_visible_tabs.insert(0, tab_id);
        }
        self.recent_visible_tabs.truncate(ATTACHED_TAB_LIMIT);
        self.release_sessions_outside_attach_window()
    }
    /// Ends the terminal session of every pane whose tab has left the attach
    /// window.
    ///
    /// The pane keeps its projection entry, carrying `released`, because the
    /// sidebar and the pane header read their state from there and a missing
    /// entry would read as a failure rather than as a pane nobody is watching.
    /// The shell drops the canvas and the buffered bytes on that state, so the
    /// tab redraws from Herdr's own frame when it is next shown.
    pub(super) fn release_sessions_outside_attach_window(&mut self) -> bool {
        let window = self
            .recent_visible_tabs
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let attached = self
            .snapshot
            .pane_layouts
            .iter()
            .filter(|layout| window.contains(&layout.tab_id))
            .flat_map(|layout| layout.pane_ids())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        // A pane no layout claims is not a pane that left the window; the
        // session reconcile owns those and drops them with the session.
        let placed = self
            .snapshot
            .pane_layouts
            .iter()
            .flat_map(|layout| layout.pane_ids())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let releasing = self
            .terminal_session_lifecycles
            .iter()
            .filter(|(_, lifecycle)| lifecycle.state != "released")
            .map(|(pane_id, _)| pane_id)
            .filter(|pane_id| !pane_id.starts_with("remote:"))
            .filter(|pane_id| placed.contains(*pane_id) && !attached.contains(*pane_id))
            .cloned()
            .collect::<Vec<_>>();
        if releasing.is_empty() {
            return false;
        }
        for pane_id in releasing {
            let _released_session = self.terminal_sessions.remove(&pane_id);
            self.panes_awaiting_size.remove(&pane_id);
            self.terminal_recovery.remove(&pane_id);
            let attempt = self
                .terminal_session_lifecycles
                .get(&pane_id)
                .map_or(0, |lifecycle| lifecycle.attempt);
            let generation = self
                .terminal_session_generations
                .get(&pane_id)
                .copied()
                .unwrap_or_default();
            self.terminal_session_lifecycles.insert(
                pane_id.clone(),
                TerminalSessionLifecycle {
                    state: "released",
                    message: Some(format!(
                        "Pane {pane_id} was detached after its tab left the last {ATTACHED_TAB_LIMIT} shown"
                    )),
                    generation,
                    attempt,
                    mode: None,
                    exit_category: None,
                    retry_decision: "on_next_visit",
                },
            );
            self.sync_transport_projection(&pane_id);
            self.push_diagnostic(
                "terminal.session_released",
                format!("Released the terminal session for pane {pane_id}"),
            );
            crate::diagnostic!(serde_json::json!({
                "component": "terminal_session",
                "kind": "terminal.session_released",
                "pane_id": pane_id,
                "generation": generation,
                "attempt": attempt,
                "retry_decision": "on_next_visit",
            }));
        }
        true
    }
    pub(super) fn terminal_retry_decision(
        &self,
        pane_id: &str,
        fallback: &'static str,
    ) -> &'static str {
        self.terminal_recovery
            .get(pane_id)
            .map_or(fallback, |r| r.decision())
    }
    pub(super) fn schedule_terminal_recovery(&mut self, pane_id: &str, reason: String) {
        // Remote reconnect policy is owned by its existing transport.
        if pane_id.starts_with("remote:") {
            return;
        }
        let recovery = self
            .terminal_recovery
            .entry(pane_id.to_owned())
            .or_insert_with(|| {
                crate::terminal_recovery::Recovery::new(Instant::now(), reason.clone())
            });
        recovery.reason = reason;
        if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
            lifecycle.message = Some(recovery.message());
            lifecycle.retry_decision = recovery.decision();
        }
    }
    pub(crate) fn maintain_terminals(&mut self, now: Instant) -> bool {
        let visible = self
            .focused_visible_tab_id()
            .and_then(|tab_id| {
                self.snapshot
                    .pane_layouts
                    .iter()
                    .find(|layout| layout.tab_id == tab_id)
                    .map(|layout| {
                        layout
                            .pane_ids()
                            .into_iter()
                            .map(str::to_owned)
                            .collect::<HashSet<_>>()
                    })
            })
            .unwrap_or_default();
        let mut changed = false;
        for pane_id in &visible {
            if self
                .terminal_session_lifecycles
                .get(pane_id)
                .is_some_and(|lifecycle| lifecycle.state == "released")
            {
                self.request_terminal_control(pane_id);
                changed = true;
            }
        }
        let due = self
            .terminal_recovery
            .iter()
            .filter(|(pane_id, recovery)| {
                visible.contains(*pane_id) && recovery.due.is_some_and(|due| now >= due)
            })
            .map(|(pane_id, _)| pane_id.clone())
            .collect::<Vec<_>>();
        for pane_id in due {
            if self.pane_is_going_away(&pane_id) {
                self.terminal_recovery.remove(&pane_id);
                continue;
            }
            let retry = self
                .terminal_recovery
                .get_mut(&pane_id)
                .expect("collected recovery")
                .advance(now);
            let message = self.terminal_recovery[&pane_id].message();
            if retry && self.terminal_sizes.contains_key(&pane_id) {
                let attempt = self
                    .terminal_session_lifecycles
                    .get(&pane_id)
                    .map_or(1, |l| l.attempt + 1);
                self.start_terminal_session(
                    &pane_id,
                    TerminalSessionMode::Control,
                    attempt,
                    "automatic_bounded",
                    Some(message.clone()),
                );
            } else if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(&pane_id) {
                lifecycle.message = Some(message.clone());
                lifecycle.retry_decision = if retry { "automatic_bounded" } else { "manual" };
                if !retry && lifecycle.state != "observing" {
                    lifecycle.state = "unavailable";
                    self.terminal_sessions.remove(&pane_id);
                    // A late spawn cannot revive an exhausted attempt.
                    self.terminal_session_generations.remove(&pane_id);
                }
                self.sync_transport_projection(&pane_id);
            }
            self.push_diagnostic(
                if retry {
                    "terminal.retrying"
                } else {
                    "terminal.retries_exhausted"
                },
                format!("Pane {pane_id}: {message}"),
            );
            changed = true;
        }
        changed
    }
    /// Starts one control attempt. Repeated sync updates are no-ops while any
    /// official control or observer session is starting or active.
    pub(super) fn request_terminal_control(&mut self, pane_id: &str) {
        let native_content = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .find(|pane| pane.id == pane_id)
            .is_some_and(|pane| !pane.content.is_terminal());
        if native_content {
            // A host may report its browser identity after the first layout.
            // Release any early PTY attachment rather than holding invisible
            // terminal control behind the native content surface.
            self.terminal_sessions.remove(pane_id);
            self.terminal_session_lifecycles.remove(pane_id);
            self.terminal_sizes.remove(pane_id);
            self.panes_awaiting_size.remove(pane_id);
            self.terminal_recovery.remove(pane_id);
            return;
        }
        let current = self
            .terminal_session_lifecycles
            .get(pane_id)
            .cloned()
            .unwrap_or_default();
        if !terminal_control_request_allowed(
            current.state,
            self.terminal_sessions.contains_key(pane_id),
        ) {
            self.sync_transport_projection(pane_id);
            return;
        }
        // Herdr sizes the PTY from the attach, so attaching before a view has
        // reported a size costs a full frame at a guessed size and a second
        // one after the resize. The pane's own view reports within a frame of
        // the layout arriving, and the resize handler starts the attach then.
        if !self.terminal_sizes.contains_key(pane_id) {
            if self.panes_awaiting_size.insert(pane_id.to_owned()) {
                self.push_diagnostic(
                    "terminal.attach_deferred",
                    format!(
                        "Pane {pane_id} is waiting for its view to report a size before attaching"
                    ),
                );
            }
            self.terminal_session_lifecycles
                .entry(pane_id.to_owned())
                .or_default()
                .state = "waiting_size";
            self.schedule_terminal_recovery(
                pane_id,
                "Waiting for the pane view to report its size".to_owned(),
            );
            self.sync_transport_projection(pane_id);
            return;
        }
        let attempt = current.attempt.saturating_add(1);
        self.start_terminal_session(
            pane_id,
            TerminalSessionMode::Control,
            attempt,
            if attempt == 1 {
                "automatic_initial"
            } else {
                "manual"
            },
            None,
        );
    }
    pub(super) fn terminal_session_context(&self, pane_id: &str) -> Option<TerminalSessionContext> {
        if pane_id.starts_with("remote:") {
            self.remote_terminals
                .iter()
                .find_map(|(target_id, context)| {
                    remote_pane_source_id(target_id, pane_id).map(|source_pane_id| {
                        TerminalSessionContext::Remote {
                            context: context.clone(),
                            source_pane_id: source_pane_id.to_owned(),
                        }
                    })
                })
        } else {
            self.live
                .as_ref()
                .cloned()
                .map(TerminalSessionContext::Local)
        }
    }
    pub(super) fn start_terminal_session(
        &mut self,
        pane_id: &str,
        mode: TerminalSessionMode,
        attempt: u64,
        retry_decision: &'static str,
        message: Option<String>,
    ) {
        self.terminal_frames_need_full.insert(pane_id.to_owned());
        self.next_terminal_session_generation =
            self.next_terminal_session_generation.saturating_add(1);
        let generation = self.next_terminal_session_generation;
        self.terminal_session_generations
            .insert(pane_id.to_owned(), generation);
        let _retired_session = self.terminal_sessions.remove(pane_id);
        self.terminal_session_lifecycles.insert(
            pane_id.to_owned(),
            TerminalSessionLifecycle {
                state: "starting",
                message,
                generation,
                attempt,
                mode: Some(mode),
                exit_category: None,
                retry_decision,
            },
        );
        if mode == TerminalSessionMode::Control {
            self.schedule_terminal_recovery(
                pane_id,
                "Waiting for the first terminal frame".to_owned(),
            );
            if let Some(recovery) = self.terminal_recovery.get_mut(pane_id) {
                recovery.last_attempt_at_unix_ms = Some(unix_milliseconds());
            }
        }
        let decision = self.terminal_retry_decision(pane_id, retry_decision);
        if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
            lifecycle.retry_decision = decision;
        }
        self.sync_transport_projection(pane_id);
        self.push_diagnostic(
            "terminal.session_requested",
            format!(
                "Starting terminal {} session for pane {pane_id}",
                mode.as_str()
            ),
        );
        // The view's grid is the one the frame guard accepts, so it is the
        // grid the attach asks for. Starting at a settled size the view has
        // already left holds every frame until a resize, and a pane that is
        // not drawn sends none: five attempts at 50x25 against a 41x18 view,
        // then retries exhausted (2026-09-14).
        if let Some(view) = self.terminal_view_sizes.get(pane_id).copied() {
            self.terminal_sizes.insert(pane_id.to_owned(), view);
        }
        #[cfg(test)]
        if self.suppress_terminal_session_workers {
            self.terminal_sessions.insert(
                pane_id.to_owned(),
                TerminalSession::test_stub(pane_id, generation, mode),
            );
            if let Some(lifecycle) = self.terminal_session_lifecycles.get_mut(pane_id) {
                lifecycle.state = match mode {
                    TerminalSessionMode::Control => "controlling",
                    TerminalSessionMode::Observe => "observing",
                };
            }
            self.sync_transport_projection(pane_id);
            return;
        }
        let context = self.terminal_session_context(pane_id);
        let Some(context) = context else {
            let message = if pane_id.starts_with("remote:") {
                format!("Pane {pane_id} has no configured remote terminal transport")
            } else {
                "Local terminal sessions require a live Herdr connection".to_owned()
            };
            self.record_terminal_session_failure(
                pane_id,
                generation,
                attempt,
                mode,
                "transport_unavailable",
                &message,
                0,
            );
            self.set_error("terminal.transport_unavailable", message, true);
            return;
        };
        // Herdr sizes the PTY from the attach, so there is no honest size to
        // send when no view has reported one. Every path here has one:
        // `request_terminal_control` holds a pane back until its view reports,
        // and an observe session only follows a control session that already
        // had a size. A pane that arrives here without one is a routing bug,
        // and saying so beats attaching at a guess and hiding it.
        let Some((rows, cols)) = self.terminal_sizes.get(pane_id).copied() else {
            let message =
                format!("Pane {pane_id} has no reported terminal size, so it cannot be attached");
            self.record_terminal_session_failure(
                pane_id,
                generation,
                attempt,
                mode,
                "size_unknown",
                &message,
                0,
            );
            self.set_error("terminal.size_unknown", message, true);
            return;
        };
        if let Err(message) =
            live::spawn_terminal_session(context, pane_id.to_owned(), generation, mode, rows, cols)
        {
            self.record_terminal_session_failure(
                pane_id,
                generation,
                attempt,
                mode,
                "worker_start_failed",
                &message,
                0,
            );
            let notice = format!(
                "\r\n[Terminal {} session for {pane_id} failed: {message}]\r\n",
                mode.as_str()
            );
            self.append_terminal_chunk(pane_id.to_owned(), live::encode_base64(notice.as_bytes()));
            self.set_error("terminal.session_worker_failed", message, true);
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record_terminal_session_failure(
        &mut self,
        pane_id: &str,
        generation: u64,
        attempt: u64,
        mode: TerminalSessionMode,
        category: &str,
        message: &str,
        elapsed_ms: u128,
    ) {
        self.terminal_session_lifecycles.insert(
            pane_id.to_owned(),
            TerminalSessionLifecycle {
                state: "unavailable",
                message: Some(message.to_owned()),
                generation,
                attempt,
                mode: Some(mode),
                exit_category: Some(category.to_owned()),
                retry_decision: "manual",
            },
        );
        self.schedule_terminal_recovery(pane_id, format!("Terminal start refused: {message}"));
        self.sync_transport_projection(pane_id);
        crate::diagnostic!(serde_json::json!({
            "component": "terminal_session",
            "kind": "terminal.session_unavailable",
            "pane_id": pane_id,
            "generation": generation,
            "attempt": attempt,
            "mode": mode.as_str(),
            "duration_ms": elapsed_ms,
            "exit_category": category,
            "retry_decision": self.terminal_retry_decision(pane_id, "manual"),
        }));
    }
    #[allow(clippy::too_many_arguments)]
    pub fn ingest_terminal_session_spawn(
        &mut self,
        generation: u64,
        pane_id: &str,
        mode: TerminalSessionMode,
        result: Result<TerminalSession, String>,
        elapsed_ms: u128,
        worker_runtime: Weak<Mutex<Runtime>>,
        notifier: crate::ffi::ChangeNotifier,
    ) -> bool {
        if self.terminal_session_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        if self
            .terminal_session_lifecycles
            .get(pane_id)
            .is_none_or(|lifecycle| lifecycle.mode != Some(mode))
        {
            return false;
        }
        if !pane_id.starts_with("remote:")
            && !self.snapshot.pane_layouts.is_empty()
            && self.layout_holding_pane(pane_id).is_none()
        {
            return false;
        }
        match result {
            Ok(session) => {
                if mode == TerminalSessionMode::Control
                    && let Some((rows, cols)) = self.terminal_sizes.get(pane_id).copied()
                    && let Err(message) = session.resize(rows, cols)
                {
                    self.set_error("terminal.resize_after_attach_failed", message, true);
                }
                self.terminal_sessions.insert(pane_id.to_owned(), session);
                let reader_result = self
                    .terminal_sessions
                    .get_mut(pane_id)
                    .expect("terminal session was just inserted")
                    .start_reader(worker_runtime, notifier);
                if let Err(message) = reader_result {
                    let _failed_session = self.terminal_sessions.remove(pane_id);
                    let attempt = self
                        .terminal_session_lifecycles
                        .get(pane_id)
                        .map_or(1, |lifecycle| lifecycle.attempt);
                    self.record_terminal_session_failure(
                        pane_id,
                        generation,
                        attempt,
                        mode,
                        "reader_start_failed",
                        &message,
                        elapsed_ms,
                    );
                    self.set_error("terminal.session_reader_failed", message, true);
                    return true;
                }
                let attempt = self
                    .terminal_session_lifecycles
                    .get(pane_id)
                    .map_or(1, |lifecycle| lifecycle.attempt);
                let message = self
                    .terminal_session_lifecycles
                    .get(pane_id)
                    .and_then(|lifecycle| lifecycle.message.clone());
                let state = match mode {
                    TerminalSessionMode::Control => "controlling",
                    TerminalSessionMode::Observe => "observing",
                };
                self.terminal_session_lifecycles.insert(
                    pane_id.to_owned(),
                    TerminalSessionLifecycle {
                        state,
                        message,
                        generation,
                        attempt,
                        mode: Some(mode),
                        exit_category: None,
                        retry_decision: self.terminal_retry_decision(
                            pane_id,
                            if mode == TerminalSessionMode::Observe {
                                "manual"
                            } else {
                                "none"
                            },
                        ),
                    },
                );
                self.sync_transport_projection(pane_id);
                self.push_diagnostic(
                    "terminal.session_ready",
                    format!(
                        "Pane {pane_id} terminal {} session ready in {elapsed_ms} ms",
                        mode.as_str()
                    ),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "terminal_session",
                    "kind": "terminal.session_ready",
                    "pane_id": pane_id,
                    "generation": generation,
                    "attempt": attempt,
                    "mode": mode.as_str(),
                    "duration_ms": elapsed_ms,
                    "exit_category": null,
                    "retry_decision": self.terminal_retry_decision(pane_id, if mode == TerminalSessionMode::Observe { "manual" } else { "none" }),
                    "last_attempt_at_unix_ms": self.terminal_recovery.get(pane_id).and_then(|r| r.last_attempt_at_unix_ms),
                }));
                true
            }
            Err(message) => {
                let attempt = self
                    .terminal_session_lifecycles
                    .get(pane_id)
                    .map_or(1, |lifecycle| lifecycle.attempt);
                self.record_terminal_session_failure(
                    pane_id,
                    generation,
                    attempt,
                    mode,
                    "spawn_failed",
                    &message,
                    elapsed_ms,
                );
                let notice = format!(
                    "\r\n[Terminal {} session for {pane_id} failed: {message}]\r\n",
                    mode.as_str()
                );
                self.append_terminal_chunk(
                    pane_id.to_owned(),
                    live::encode_base64(notice.as_bytes()),
                );
                self.set_error("terminal.session_failed", message, true);
                true
            }
        }
    }
    /// Routes key bytes only to an official controller. The actual pipe write
    /// runs on the session writer thread, outside the runtime mutex.
    pub(super) fn write_terminal_control(
        &mut self,
        pane_id: &str,
        bytes_base64: &str,
        trace: Option<crate::model::TerminalInputTrace>,
    ) {
        let bytes = match live::decode_base64(bytes_base64) {
            Ok(bytes) => bytes,
            Err(message) => {
                self.set_error("terminal.invalid_input", message, false);
                return;
            }
        };
        match self.terminal_sessions.get(pane_id) {
            Some(session) if session.mode == TerminalSessionMode::Control => {
                if let Err(message) = session.write_bytes(&bytes, trace) {
                    self.set_error("terminal.write_failed", message, true);
                }
            }
            Some(_) => {
                self.set_error(
                    "terminal.read_only",
                    format!(
                        "Pane {pane_id} is read-only because another client owns terminal control; use Reconnect to try again"
                    ),
                    true,
                );
            }
            None => {
                self.set_error(
                    "terminal.unavailable",
                    format!("Pane {pane_id} has no terminal session; use Reconnect to try again"),
                    true,
                );
            }
        }
    }
    pub(crate) fn ingest_terminal_input_sent(
        &mut self,
        pane_id: &str,
        generation: u64,
        sent: crate::model::TerminalInputSent,
    ) -> bool {
        if self.terminal_session_generations.get(pane_id) != Some(&generation) {
            return false;
        }
        self.append_terminal_chunk(pane_id.to_owned(), String::new());
        self.snapshot
            .terminal
            .chunks
            .last_mut()
            .expect("just appended input trace")
            .input_sent = Some(sent);
        true
    }
    pub(super) fn append_terminal_chunk(&mut self, pane_id: String, bytes_base64: String) {
        self.snapshot.terminal.sequence = self.snapshot.terminal.sequence.saturating_add(1);
        self.snapshot.terminal.chunks.push(TerminalChunk {
            pane_id,
            sequence: self.snapshot.terminal.sequence,
            bytes_base64,
            frame: None,
            input_sent: None,
        });
        const RETAINED_TERMINAL_CHUNKS: usize = 512;
        if self.snapshot.terminal.chunks.len() > RETAINED_TERMINAL_CHUNKS {
            let excess = self.snapshot.terminal.chunks.len() - RETAINED_TERMINAL_CHUNKS;
            self.snapshot.terminal.chunks.drain(..excess);
        }
    }
}
