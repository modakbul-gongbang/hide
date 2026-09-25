//! Core-owned records for every Herdr-owned mutation still in flight.
//!
//! A split, zoom, resize, tab move, remote mutation or close is one record
//! from the moment the operator asks until fresh topology confirms it, with
//! a host connection generation, target and conflict-scope ids, phase, stage,
//! start time and absolute deadline. Transport success is never topology
//! truth, and an expired or ambiguous mutation becomes an explicit unknown
//! that is never resent; `docs/ARCHITECTURE.md` owns the reasons. The close
//! lifecycle itself lives beside reopen in `editor.rs`.

use super::*;

/// One user close from intent through authoritative topology confirmation.
/// The captured item is held here as a reservation until the request reaches
/// the front of `close_capture_order`; it never competes with the confirmed
/// twenty-entry undo stack while its result is unknown.
#[derive(Clone, Debug)]
pub(super) struct PendingClose {
    pub(super) request: live::CloseCaptureRequest,
    pub(super) target_id: String,
    pub(super) scope_id: String,
    /// The full pane set observed when the user approved this close. A later
    /// pane addition or removal changes the target range, so the old approval
    /// cannot be applied to it.
    pub(super) scope_pane_ids: Vec<String>,
    pub(super) pane_ids: Vec<String>,
    /// Agent panes that required the approval when this close was requested.
    /// A newly protected pane invalidates the approval even when the tab's
    /// pane set itself is unchanged.
    pub(super) protected_agent_pane_ids: Vec<String>,
    pub(super) label: String,
    pub(super) item: Option<ClosedItem>,
    pub(super) phase: String,
    pub(super) stage: String,
    pub(super) started_at_unix_ms: u64,
    pub(super) deadline_at_unix_ms: Option<u64>,
    pub(super) connection_generation: u64,
    pub(super) message: Option<String>,
    pub(super) retryable: bool,
    pub(super) selection_restore: Option<CloseSelectionRestore>,
}

#[derive(Clone, Debug)]
pub(super) struct PendingPaneOperation {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) target_id: String,
    pub(super) scope_id: String,
    pub(super) connection_generation: u64,
    pub(super) phase: String,
    pub(super) stage: String,
    pub(super) started_at_unix_ms: u64,
    pub(super) deadline_at_unix_ms: Option<u64>,
    pub(super) message: Option<String>,
    pub(super) retryable: bool,
    pub(super) created_pane_id: Option<String>,
    pub(super) queued_resize: Option<QueuedResize>,
    pub(super) baseline_signature: PaneTopologySignature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ResizeAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct QueuedResize {
    pub(super) axis: ResizeAxis,
    pub(super) delta: f32,
}

impl QueuedResize {
    pub(super) fn from_action(action: &PaneControlAction) -> Option<Self> {
        let PaneControlAction::Resize {
            direction, amount, ..
        } = action
        else {
            return None;
        };
        let (axis, sign) = match direction {
            PaneResizeDirection::Left => (ResizeAxis::Horizontal, -1.0),
            PaneResizeDirection::Right => (ResizeAxis::Horizontal, 1.0),
            PaneResizeDirection::Up => (ResizeAxis::Vertical, -1.0),
            PaneResizeDirection::Down => (ResizeAxis::Vertical, 1.0),
        };
        Some(Self {
            axis,
            delta: sign * *amount,
        })
    }

    pub(super) fn into_action(self, pane_id: String) -> Option<PaneControlAction> {
        if self.delta.abs() < 0.001 {
            return None;
        }
        let (direction, amount) = match (self.axis, self.delta.is_sign_positive()) {
            (ResizeAxis::Horizontal, true) => (PaneResizeDirection::Right, self.delta.abs()),
            (ResizeAxis::Horizontal, false) => (PaneResizeDirection::Left, self.delta.abs()),
            (ResizeAxis::Vertical, true) => (PaneResizeDirection::Down, self.delta.abs()),
            (ResizeAxis::Vertical, false) => (PaneResizeDirection::Up, self.delta.abs()),
        };
        Some(PaneControlAction::Resize {
            pane_id,
            direction,
            amount: amount.min(0.5),
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct PendingRemoteOperation {
    pub(super) id: String,
    pub(super) target_id: String,
    pub(super) scope_id: String,
    pub(super) connection_generation: u64,
    pub(super) kind: String,
    pub(super) phase: String,
    pub(super) stage: String,
    pub(super) started_at_unix_ms: u64,
    pub(super) deadline_at_unix_ms: Option<u64>,
    pub(super) message: Option<String>,
    pub(super) retryable: bool,
    pub(super) created_pane_id: Option<String>,
    pub(super) baseline_zoomed: Option<bool>,
}

pub(super) type PaneRect = (String, u16, u16, u16, u16);
pub(super) type SplitRect = (u8, u32, u16, u16, u16, u16);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PaneTopologySignature {
    pub(super) zoomed: bool,
    pub(super) focused_pane_id: String,
    pub(super) pane_ids: Vec<String>,
    pub(super) splits: Vec<(u8, u32)>,
    /// Raw geometry is present only on a signature taken from Herdr's fresh
    /// session payload. Model projections intentionally do not invent it.
    pub(super) pane_rects: Option<Vec<PaneRect>>,
    pub(super) split_rects: Option<Vec<SplitRect>>,
}

pub(super) struct TabMoveResultContext<'a> {
    pub(super) checkout_id: &'a str,
    pub(super) tab_id: &'a str,
    pub(super) expected_order: &'a [String],
    pub(super) generation: u64,
    pub(super) connection_generation: u64,
    pub(super) elapsed_ms: u128,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct CloseSelectionState {
    pub(super) focused_checkout_id: Option<String>,
    pub(super) terminal_pane_id: Option<String>,
    pub(super) focused_pane_id: Option<String>,
    pub(super) selected_pane_id: Option<String>,
    pub(super) active_tab_id: Option<String>,
    pub(super) visible_tab_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CloseSelectionRestore {
    pub(super) checkout_id: Option<String>,
    pub(super) before: CloseSelectionState,
    pub(super) after: CloseSelectionState,
}

pub(super) fn close_target_id(target: &live::CloseCaptureTarget) -> &str {
    match target {
        live::CloseCaptureTarget::Pane { pane_id } => pane_id,
        live::CloseCaptureTarget::Tab { tab_id } => tab_id,
    }
}

pub(super) fn remote_pane_id(target_id: &str, pane_id: &str) -> String {
    format!("remote:{target_id}:pane:{pane_id}")
}

/// Copies each pane's status word and close-confirmation answer from the agent
/// rows that carry the read axis.
///

#[derive(Clone, Debug)]
pub(super) struct RemoteMutationDescriptor {
    pub(super) kind: String,
    pub(super) target_id: String,
    pub(super) scope_id: String,
    pub(super) baseline_zoomed: Option<bool>,
}

pub(super) fn remote_mutation_descriptor(
    session: &RemoteSessionSnapshot,
    request: &RemoteControlRequest,
) -> Option<RemoteMutationDescriptor> {
    let kind = request.mutation_kind()?.to_owned();
    match request {
        RemoteControlRequest::SplitPane { pane_id, .. }
        | RemoteControlRequest::TogglePaneZoom { pane_id, .. }
        | RemoteControlRequest::ClosePane { pane_id, .. } => {
            let tab = session
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .flat_map(|checkout| checkout.tabs.iter())
                .find(|tab| tab.panes.iter().any(|pane| pane.id == *pane_id))?;
            let scope_id = tab.id.clone()?;
            let baseline_zoomed = (kind == "pane.zoom").then(|| {
                session
                    .pane_layouts
                    .iter()
                    .find(|layout| layout.tab_id == scope_id)
                    .is_some_and(|layout| layout.zoomed)
            });
            Some(RemoteMutationDescriptor {
                kind,
                target_id: pane_id.clone(),
                scope_id,
                baseline_zoomed,
            })
        }
        RemoteControlRequest::CloseTab { tab_id, .. } => {
            let _tab = session
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .flat_map(|checkout| checkout.tabs.iter())
                .find(|tab| tab.id.as_deref() == Some(tab_id.as_str()))?;
            Some(RemoteMutationDescriptor {
                kind,
                target_id: tab_id.clone(),
                scope_id: tab_id.clone(),
                baseline_zoomed: None,
            })
        }
        RemoteControlRequest::FocusPane { .. }
        | RemoteControlRequest::FocusWorkspace { .. }
        | RemoteControlRequest::FocusTab { .. }
        | RemoteControlRequest::CreateTab { .. } => None,
    }
}

impl Runtime {
    pub(crate) fn request_status_refresh(&mut self) -> bool {
        self.status_refresh_requested = true;
        self.push_diagnostic(
            "session.status_refresh_requested",
            "A read-only session refresh was requested by the shell",
        );
        true
    }

    pub(crate) fn take_status_refresh_request(&mut self) -> bool {
        let requested = self.status_refresh_requested;
        self.status_refresh_requested = false;
        requested
    }

    /// Advances every bounded asynchronous operation from a coordinator timer.
    /// Snapshot publication is an output of this work, never its clock: an
    /// idle Herdr session must still turn an expired acknowledgement into an
    /// explicit unknown result at its deadline.
    pub(crate) fn tick_async_operations(&mut self, now_unix_ms: u64) -> bool {
        let mut changed = false;
        changed |= self.expire_pending_view_focus(now_unix_ms);
        changed |= self.expire_tab_moves(now_unix_ms);
        changed |= self.expire_close_operations(now_unix_ms);
        changed |= self.expire_pane_operations(now_unix_ms);
        changed |= self.expire_remote_operations(now_unix_ms);
        changed |= self.reconcile_attachment_target();
        changed |= self.tick_attachment();
        changed |= self.tick_project_memory(now_unix_ms);
        changed |= self.reattach_resized_observers(now_unix_ms);
        if changed {
            self.sync_recent_closed_snapshot();
            self.sync_async_operations();
        }
        changed
    }

    pub(super) fn insert_remote_operation(
        &mut self,
        key: &(String, String),
        descriptor: RemoteMutationDescriptor,
        started_at_unix_ms: u64,
        connection_generation: u64,
    ) {
        if self.remote_operations.len() >= 128 {
            let oldest = self
                .remote_operations
                .iter()
                .min_by_key(|(_, operation)| operation.started_at_unix_ms)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.remote_operations.remove(&oldest);
            }
        }
        self.remote_operations.insert(
            key.clone(),
            PendingRemoteOperation {
                id: format!("remote.control:{}:{}", key.0, key.1),
                target_id: descriptor.target_id,
                scope_id: descriptor.scope_id,
                connection_generation,
                kind: descriptor.kind,
                phase: "transmitting".to_owned(),
                stage: "request".to_owned(),
                started_at_unix_ms,
                deadline_at_unix_ms: Some(
                    started_at_unix_ms.saturating_add(CLOSE_STAGE_TIMEOUT_MS),
                ),
                message: None,
                retryable: false,
                created_pane_id: None,
                baseline_zoomed: descriptor.baseline_zoomed,
            },
        );
        self.sync_async_operations();
    }

    pub(super) fn fail_remote_operation(
        &mut self,
        target_id: &str,
        request_id: &str,
        message: String,
    ) {
        let key = (target_id.to_owned(), request_id.to_owned());
        let Some(operation) = self.remote_operations.get(&key).cloned() else {
            return;
        };
        if let Some(current) = self.remote_operations.get_mut(&key) {
            current.phase = "failed".to_owned();
            current.stage = "request".to_owned();
            current.deadline_at_unix_ms = None;
            current.message = Some(message.clone());
            current.retryable = true;
        }
        self.push_diagnostic(
            "remote.control.failed",
            format!(
                "{} {} (request {}): {message}",
                operation.kind, operation.target_id, request_id
            ),
        );
        self.set_error(
            "remote.control.failed",
            format!("{} for {target_id} failed: {message}", operation.kind),
            true,
        );
        self.sync_async_operations();
    }

    pub(super) fn acknowledge_remote_operation(
        &mut self,
        target_id: &str,
        request_id: &str,
        created_pane_id: Option<String>,
    ) -> bool {
        let key = (target_id.to_owned(), request_id.to_owned());
        let Some(operation) = self.remote_operations.get(&key).cloned() else {
            return false;
        };
        if operation.kind == "pane.split" && created_pane_id.is_none() {
            self.fail_remote_operation(
                target_id,
                request_id,
                "Remote Herdr acknowledged split without a created pane id".to_owned(),
            );
            return true;
        }
        if let Some(current) = self.remote_operations.get_mut(&key) {
            current.phase = "awaiting_topology".to_owned();
            current.stage = "topology".to_owned();
            current.message = Some(
                "Request accepted; waiting for the remote Herdr topology to confirm it".to_owned(),
            );
            current.retryable = false;
            current.created_pane_id = created_pane_id
                .as_deref()
                .map(|pane_id| remote_pane_id(target_id, pane_id));
            current.deadline_at_unix_ms =
                Some(unix_milliseconds().saturating_add(CLOSE_STAGE_TIMEOUT_MS));
        }
        self.sync_async_operations();
        true
    }

    pub(super) fn observe_remote_operations(&mut self, session: &RemoteSessionSnapshot) -> bool {
        let mut completed = Vec::new();
        for (key, operation) in &self.remote_operations {
            if !matches!(operation.phase.as_str(), "awaiting_topology" | "unknown") {
                continue;
            }
            let confirmed = match operation.kind.as_str() {
                "pane.close" => !session
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .flat_map(|tab| tab.panes.iter())
                    .any(|pane| pane.id == operation.target_id),
                "tab.close" => !session
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .any(|tab| tab.id.as_deref() == Some(operation.target_id.as_str())),
                "pane.split" => operation.created_pane_id.as_ref().is_some_and(|created| {
                    session
                        .workspaces
                        .iter()
                        .flat_map(|workspace| workspace.checkouts.iter())
                        .flat_map(|checkout| checkout.tabs.iter())
                        .any(|tab| {
                            tab.id.as_deref() == Some(operation.scope_id.as_str())
                                && tab.panes.iter().any(|pane| pane.id == *created)
                        })
                }),
                "pane.zoom" => operation.baseline_zoomed.is_some_and(|baseline| {
                    session
                        .pane_layouts
                        .iter()
                        .find(|layout| layout.tab_id == operation.scope_id)
                        .is_some_and(|layout| layout.zoomed != baseline)
                }),
                _ => false,
            };
            if confirmed {
                completed.push(key.clone());
            }
        }
        let changed = !completed.is_empty();
        for key in completed {
            if let Some(operation) = self.remote_operations.remove(&key) {
                self.push_diagnostic(
                    "remote.control.topology_confirmed",
                    format!(
                        "Confirmed {} for {} on {} from fresh topology",
                        operation.kind, operation.target_id, key.0
                    ),
                );
            }
        }
        if changed {
            self.sync_async_operations();
            return true;
        }
        false
    }

    pub(super) fn expire_remote_operations(&mut self, now_unix_ms: u64) -> bool {
        let mut changed = false;
        for operation in self.remote_operations.values_mut() {
            let Some(deadline) = operation.deadline_at_unix_ms else {
                continue;
            };
            if deadline > now_unix_ms {
                continue;
            }
            if matches!(
                operation.phase.as_str(),
                "transmitting" | "awaiting_topology"
            ) {
                operation.phase = "unknown".to_owned();
                operation.stage = "status_check".to_owned();
                operation.message = Some(
                    "Remote result is unknown; no mutation was resent and a fresh topology is required"
                        .to_owned(),
                );
                operation.retryable = false;
                operation.deadline_at_unix_ms = None;
                changed = true;
            }
        }
        if changed {
            self.sync_async_operations();
        }
        changed
    }

    pub(super) fn mark_remote_operation_unknown(
        &mut self,
        key: &(String, String),
        message: String,
    ) -> bool {
        let operation_id = {
            let Some(operation) = self.remote_operations.get_mut(key) else {
                return false;
            };
            if matches!(operation.phase.as_str(), "completed" | "failed" | "refused") {
                return false;
            }
            operation.phase = "unknown".to_owned();
            operation.stage = "status_check".to_owned();
            operation.message = Some(message);
            operation.retryable = false;
            operation.deadline_at_unix_ms = None;
            operation.id.clone()
        };
        self.push_diagnostic(
            "remote.control.unknown",
            format!("Remote operation {operation_id} requires fresh topology"),
        );
        self.sync_async_operations();
        true
    }

    pub(super) fn mark_tab_move_unknown(
        &mut self,
        checkout_id: &str,
        generation: u64,
        connection_generation: u64,
        reason: String,
    ) -> bool {
        let Some(pending) = self.pending_tab_move.get_mut(checkout_id) else {
            return false;
        };
        if pending.generation != generation
            || pending.connection_generation != connection_generation
            || matches!(pending.phase.as_str(), "completed" | "failed" | "refused")
        {
            return false;
        }
        pending.phase = "unknown".to_owned();
        pending.stage = "status_check".to_owned();
        pending.message = Some(format!(
            "Tab move result is unknown; no mutation was resent: {reason}"
        ));
        pending.retryable = false;
        pending.deadline_at_unix_ms = None;
        self.push_diagnostic(
            "tab.move.unknown",
            format!("Tab move for {checkout_id} requires fresh topology; mutation was not resent"),
        );
        self.set_error(
            "tab.move_unknown",
            format!("Tab move result is unknown; no move was resent: {reason}"),
            true,
        );
        self.sync_async_operations();
        true
    }

    /// The confirmable shape of a model layout: pane ids, split geometry, zoom
    /// and focus. It carries no PTY rectangles, so a resize cannot be
    /// confirmed against it; only a fresh session layout can do that.
    pub(super) fn model_layout_signature(layout: &PaneLayoutSnapshot) -> PaneTopologySignature {
        fn collect(
            node: &PaneLayoutNodeSnapshot,
            pane_ids: &mut Vec<String>,
            splits: &mut Vec<(u8, u32)>,
        ) {
            match node {
                PaneLayoutNodeSnapshot::Pane { pane_id } => pane_ids.push(pane_id.clone()),
                PaneLayoutNodeSnapshot::Split {
                    direction,
                    ratio,
                    first,
                    second,
                } => {
                    splits.push((
                        match direction {
                            crate::model::PaneLayoutDirection::Right => 0,
                            crate::model::PaneLayoutDirection::Down => 1,
                        },
                        ratio.to_bits(),
                    ));
                    collect(first, pane_ids, splits);
                    collect(second, pane_ids, splits);
                }
            }
        }

        let mut signature = PaneTopologySignature {
            zoomed: layout.zoomed,
            focused_pane_id: layout.focused_pane_id.clone(),
            ..PaneTopologySignature::default()
        };
        collect(&layout.root, &mut signature.pane_ids, &mut signature.splits);
        signature.pane_ids.sort();
        signature.splits.sort_unstable();
        signature
    }

    pub(super) fn session_layout_signature(
        layout: &crate::sidebar::SessionLayoutPayload,
    ) -> PaneTopologySignature {
        let mut signature = PaneTopologySignature {
            zoomed: layout.zoomed,
            focused_pane_id: layout.focused_pane_id.clone(),
            pane_ids: layout
                .panes
                .iter()
                .map(|pane| pane.pane_id.clone())
                .collect(),
            splits: layout
                .splits
                .iter()
                .map(|split| {
                    (
                        match split.direction {
                            crate::model::PaneLayoutDirection::Right => 0,
                            crate::model::PaneLayoutDirection::Down => 1,
                        },
                        split.ratio.to_bits(),
                    )
                })
                .collect(),
            pane_rects: Some(
                layout
                    .panes
                    .iter()
                    .map(|pane| {
                        (
                            pane.pane_id.clone(),
                            pane.rect.x,
                            pane.rect.y,
                            pane.rect.width,
                            pane.rect.height,
                        )
                    })
                    .collect(),
            ),
            split_rects: Some(
                layout
                    .splits
                    .iter()
                    .map(|split| {
                        (
                            match split.direction {
                                crate::model::PaneLayoutDirection::Right => 0,
                                crate::model::PaneLayoutDirection::Down => 1,
                            },
                            split.ratio.to_bits(),
                            split.rect.x,
                            split.rect.y,
                            split.rect.width,
                            split.rect.height,
                        )
                    })
                    .collect(),
            ),
        };
        signature.pane_ids.sort();
        signature.splits.sort_unstable();
        signature.pane_rects.as_mut().expect("set above").sort();
        signature
            .split_rects
            .as_mut()
            .expect("set above")
            .sort_unstable();
        signature
    }

    pub(super) fn pane_operation_scope(&self, pane_id: &str) -> Option<String> {
        self.snapshot
            .pane_layouts
            .iter()
            .find(|layout| layout.pane_ids().contains(&pane_id))
            .map(|layout| layout.tab_id.clone())
    }

    pub(super) fn pane_operation_baseline(&self, pane_id: &str) -> Option<PaneTopologySignature> {
        self.snapshot
            .pane_layouts
            .iter()
            .find(|layout| layout.pane_ids().contains(&pane_id))
            .map(|layout| {
                self.confirmed_pane_layout_signatures
                    .get(&layout.tab_id)
                    .cloned()
                    .unwrap_or_else(|| Self::model_layout_signature(layout))
            })
    }

    pub(super) fn begin_pane_operation(
        &mut self,
        context: LiveContext,
        action: PaneControlAction,
    ) -> bool {
        let kind = action.kind();
        let target_id = action.pane_id().to_owned();
        let Some(scope_id) = self.pane_operation_scope(&target_id) else {
            self.set_error(
                "pane.operation_scope_unavailable",
                format!("Herdr has no confirmed layout for pane {target_id}"),
                true,
            );
            return true;
        };
        let Some(baseline_signature) = self.pane_operation_baseline(&target_id) else {
            self.set_error(
                "pane.operation_scope_unavailable",
                format!("Herdr has no confirmed layout for pane {target_id}"),
                true,
            );
            return true;
        };
        self.pane_operations.retain(|_, operation| {
            !(operation.scope_id == scope_id
                && matches!(operation.phase.as_str(), "failed" | "refused"))
        });
        if kind == "pane.resize"
            && let Some(existing) = self
                .pane_operations
                .values_mut()
                .find(|operation| operation.scope_id == scope_id)
            && existing.kind == "pane.resize"
            && existing.target_id == target_id
            && matches!(
                existing.phase.as_str(),
                "transmitting" | "awaiting_topology"
            )
        {
            let Some(next) = QueuedResize::from_action(&action) else {
                return true;
            };
            if let Some(queued) = existing.queued_resize.as_mut() {
                if queued.axis != next.axis {
                    self.set_error(
                        "pane.operation_in_progress",
                        format!(
                            "A resize is already running for pane {}; the new axis was not sent",
                            target_id
                        ),
                        false,
                    );
                    return true;
                }
                queued.delta = (queued.delta + next.delta).clamp(-0.5, 0.5);
            } else {
                existing.queued_resize = Some(next);
            }
            existing.message = Some(
                "The latest resize position will be sent after Herdr confirms this one".to_owned(),
            );
            self.sync_recent_closed_snapshot();
            return true;
        }
        if self
            .pane_operations
            .values()
            .any(|operation| operation.scope_id == scope_id)
            || self
                .close_operations
                .values()
                .any(|operation| operation.scope_id == scope_id)
        {
            self.set_error(
                "pane.operation_in_progress",
                format!(
                    "{} is already running for tab {}; the new request was not queued",
                    kind, scope_id
                ),
                false,
            );
            return true;
        }
        self.next_async_operation_id = self.next_async_operation_id.saturating_add(1).max(1);
        let id = format!(
            "pane-op-{}-{}",
            unix_milliseconds(),
            self.next_async_operation_id
        );
        let now = unix_milliseconds();
        self.pane_operations.insert(
            id.clone(),
            PendingPaneOperation {
                id: id.clone(),
                kind: kind.to_owned(),
                target_id,
                scope_id,
                connection_generation: self.live_generation,
                phase: "transmitting".to_owned(),
                stage: "request".to_owned(),
                started_at_unix_ms: now,
                deadline_at_unix_ms: Some(now.saturating_add(CLOSE_STAGE_TIMEOUT_MS)),
                message: None,
                retryable: false,
                created_pane_id: None,
                queued_resize: None,
                baseline_signature,
            },
        );
        self.sync_recent_closed_snapshot();
        if let Err(message) =
            live::spawn_pane_control_with_generation(context, action, Some(self.live_generation))
        {
            self.fail_pane_operation(&id, message);
        }
        true
    }

    pub(super) fn fail_pane_operation(&mut self, id: &str, message: String) {
        let Some(operation) = self.pane_operations.get(id).cloned() else {
            return;
        };
        if let Some(current) = self.pane_operations.get_mut(id) {
            current.phase = "failed".to_owned();
            current.stage = "request".to_owned();
            current.deadline_at_unix_ms = None;
            current.message = Some(message.clone());
            current.retryable = true;
            current.created_pane_id = None;
            current.queued_resize = None;
        }
        self.set_error(
            "pane.operation_failed",
            format!(
                "{} for {} failed: {message}",
                operation.kind, operation.target_id
            ),
            true,
        );
        self.push_diagnostic(
            "pane.operation_failed",
            format!("{} {}: {message}", operation.kind, operation.target_id),
        );
        self.sync_recent_closed_snapshot();
    }

    pub(super) fn mark_pane_operation_unknown(&mut self, id: &str, message: String) -> bool {
        let operation_id = {
            let Some(operation) = self.pane_operations.get_mut(id) else {
                return false;
            };
            if matches!(operation.phase.as_str(), "completed" | "failed" | "refused") {
                return false;
            }
            operation.phase = "unknown".to_owned();
            operation.stage = "status_check".to_owned();
            operation.message = Some(format!(
                "{}; no mutation was resent and a fresh topology is required",
                message
            ));
            operation.retryable = false;
            operation.deadline_at_unix_ms = None;
            operation.id.clone()
        };
        self.push_diagnostic(
            "pane.operation.unknown",
            format!("Pane operation {operation_id} requires fresh topology"),
        );
        self.sync_async_operations();
        true
    }

    pub(super) fn ingest_pane_mutation_result(
        &mut self,
        id: &str,
        result: Result<PaneControlOutcome, live::ControlFailure>,
        elapsed_ms: u128,
    ) -> bool {
        let Some(operation) = self.pane_operations.get(id).cloned() else {
            return false;
        };
        if operation.phase != "transmitting" {
            return false;
        }
        if operation
            .deadline_at_unix_ms
            .is_some_and(|deadline| deadline <= unix_milliseconds())
        {
            return self.mark_pane_operation_unknown(
                id,
                "Pane mutation result arrived after its deadline".to_owned(),
            );
        }
        match result {
            Ok(PaneControlOutcome::Acknowledged { created_pane_id }) => {
                if operation.kind == "pane.split" && created_pane_id.is_none() {
                    self.fail_pane_operation(
                        id,
                        "Herdr acknowledged split without a created pane id".to_owned(),
                    );
                    return true;
                }
                if let Some(current) = self.pane_operations.get_mut(id) {
                    current.phase = "awaiting_topology".to_owned();
                    current.stage = "topology".to_owned();
                    current.message = Some(
                        "Request accepted; waiting for Herdr to confirm the new topology"
                            .to_owned(),
                    );
                    current.retryable = false;
                    current.created_pane_id = created_pane_id;
                    current.deadline_at_unix_ms =
                        Some(unix_milliseconds().saturating_add(CLOSE_STAGE_TIMEOUT_MS));
                }
                self.push_diagnostic(
                    "pane.operation.accepted",
                    format!(
                        "{} for {} acknowledged in {elapsed_ms} ms; awaiting topology",
                        operation.kind, operation.target_id
                    ),
                );
            }
            Ok(PaneControlOutcome::Projected { .. }) => self.fail_pane_operation(
                id,
                "Herdr returned a projection outcome for a mutation request".to_owned(),
            ),
            Err(error) if error.is_ambiguous() => {
                self.mark_pane_operation_unknown(id, error.message().to_owned());
            }
            Err(error) => self.fail_pane_operation(id, error.message().to_owned()),
        }
        self.sync_recent_closed_snapshot();
        true
    }

    pub(super) fn observe_pane_operations(&mut self, payload: &SessionSnapshotPayload) -> bool {
        let mut completed = Vec::new();
        let mut changed = false;
        for (id, operation) in &self.pane_operations {
            if !matches!(operation.phase.as_str(), "awaiting_topology" | "unknown") {
                continue;
            }
            let Some(layout) = payload
                .layouts
                .iter()
                .find(|layout| layout.tab_id == operation.scope_id)
            else {
                continue;
            };
            if !layout
                .panes
                .iter()
                .any(|pane| pane.pane_id == operation.target_id)
            {
                continue;
            }
            let split_ready = operation.kind == "pane.split"
                && operation
                    .created_pane_id
                    .as_deref()
                    .is_some_and(|created| layout.panes.iter().any(|pane| pane.pane_id == created));
            let current = Self::session_layout_signature(layout);
            let topology_changed = match operation.kind.as_str() {
                // A split is confirmed by the exact pane Herdr said it
                // created. A changed focus or ratio alone is not proof that
                // this split landed.
                "pane.split" => split_ready,
                // Zoom has no geometry contract of its own. The zoom bit is
                // the authoritative confirmation, while a focus change is a
                // separate event and must not settle it.
                "pane.zoom" => current.zoomed != operation.baseline_signature.zoomed,
                // Resize must be proven by fresh raw rectangles. A no-op
                // acknowledgement or a focus-only event cannot settle a
                // resize, and a model projection has no rectangles to invent.
                "pane.resize" => {
                    operation.baseline_signature.pane_rects.is_some()
                        && operation.baseline_signature.pane_rects != current.pane_rects
                        || operation.baseline_signature.split_rects.is_some()
                            && operation.baseline_signature.split_rects != current.split_rects
                }
                _ => current != operation.baseline_signature,
            };
            if topology_changed {
                completed.push((id.clone(), current));
            }
        }
        let mut restart = Vec::new();
        for (id, current_signature) in completed {
            let Some(operation) = self.pane_operations.get_mut(&id) else {
                continue;
            };
            if let Some(queued) = operation.queued_resize.take()
                && let Some(action) = queued.into_action(operation.target_id.clone())
            {
                operation.connection_generation = self.live_generation;
                operation.phase = "transmitting".to_owned();
                operation.stage = "request".to_owned();
                operation.started_at_unix_ms = unix_milliseconds();
                operation.deadline_at_unix_ms = Some(
                    operation
                        .started_at_unix_ms
                        .saturating_add(CLOSE_STAGE_TIMEOUT_MS),
                );
                operation.message = Some("Sending the latest resize position…".to_owned());
                operation.retryable = false;
                operation.created_pane_id = None;
                operation.baseline_signature = current_signature;
                restart.push((id.clone(), action));
                changed = true;
                continue;
            }
            let Some(operation) = self.pane_operations.remove(&id) else {
                continue;
            };
            self.push_diagnostic(
                "pane.operation.topology_confirmed",
                format!(
                    "Confirmed {} for {} from fresh topology",
                    operation.kind, operation.target_id
                ),
            );
            changed = true;
        }
        for (id, action) in restart {
            let Some(context) = self.live.as_ref().cloned() else {
                self.fail_pane_operation(
                    &id,
                    "The latest resize could not be sent because Herdr disconnected".to_owned(),
                );
                continue;
            };
            if let Err(message) = live::spawn_pane_control_with_generation(
                context,
                action,
                Some(self.live_generation),
            ) {
                self.fail_pane_operation(&id, message);
            }
        }
        if changed {
            self.sync_recent_closed_snapshot();
        }
        changed
    }

    pub(super) fn sync_async_operations(&mut self) {
        let mut operations = self
            .close_capture_order
            .iter()
            .filter_map(|key| {
                self.close_operations
                    .get(key)
                    .map(|operation| (key, operation))
            })
            .map(|(_key, operation)| crate::model::AsyncOperationSnapshot {
                id: operation.request.key.clone(),
                kind: Self::close_operation_kind(&operation.request.target).to_owned(),
                target_id: operation.target_id.clone(),
                scope_id: operation.scope_id.clone(),
                phase: operation.phase.clone(),
                stage: operation.stage.clone(),
                started_at_unix_ms: operation.started_at_unix_ms,
                deadline_at_unix_ms: operation.deadline_at_unix_ms,
                message: operation.message.clone(),
                retryable: operation.retryable,
            })
            .collect::<Vec<_>>();
        operations.extend(self.pane_operations.values().map(|operation| {
            crate::model::AsyncOperationSnapshot {
                id: operation.id.clone(),
                kind: operation.kind.clone(),
                target_id: operation.target_id.clone(),
                scope_id: operation.scope_id.clone(),
                phase: operation.phase.clone(),
                stage: operation.stage.clone(),
                started_at_unix_ms: operation.started_at_unix_ms,
                deadline_at_unix_ms: operation.deadline_at_unix_ms,
                message: operation.message.clone(),
                retryable: operation.retryable,
            }
        }));
        operations.extend(
            self.pending_tab_move
                .iter()
                .map(
                    |(checkout_id, operation)| crate::model::AsyncOperationSnapshot {
                        id: format!("tab.move:{checkout_id}:{}", operation.generation),
                        kind: "tab.move".to_owned(),
                        target_id: operation.target_id.clone(),
                        scope_id: checkout_id.clone(),
                        phase: operation.phase.clone(),
                        stage: operation.stage.clone(),
                        started_at_unix_ms: operation.started_at_unix_ms,
                        deadline_at_unix_ms: operation.deadline_at_unix_ms,
                        message: operation.message.clone(),
                        retryable: operation.retryable,
                    },
                ),
        );
        operations.extend(self.remote_operations.values().map(|operation| {
            crate::model::AsyncOperationSnapshot {
                id: operation.id.clone(),
                kind: operation.kind.clone(),
                target_id: operation.target_id.clone(),
                scope_id: operation.scope_id.clone(),
                phase: operation.phase.clone(),
                stage: operation.stage.clone(),
                started_at_unix_ms: operation.started_at_unix_ms,
                deadline_at_unix_ms: operation.deadline_at_unix_ms,
                message: operation.message.clone(),
                retryable: operation.retryable,
            }
        }));
        if let Some(attachment) = &self.attachment {
            operations.push(attachment.operation.clone());
        }
        if let Some(rejection) = &self.attachment_rejection {
            operations.push(rejection.clone());
        }
        operations.sort_by(|left, right| {
            left.started_at_unix_ms
                .cmp(&right.started_at_unix_ms)
                .then_with(|| left.id.cmp(&right.id))
        });
        self.snapshot.status.async_operations = operations;
    }

    pub(super) fn expire_pane_operations(&mut self, now_unix_ms: u64) -> bool {
        let mut changed = false;
        for id in self.pane_operations.keys().cloned().collect::<Vec<_>>() {
            let Some(operation) = self.pane_operations.get(&id).cloned() else {
                continue;
            };
            let Some(deadline) = operation.deadline_at_unix_ms else {
                continue;
            };
            if deadline > now_unix_ms {
                continue;
            }
            let Some(current) = self.pane_operations.get_mut(&id) else {
                continue;
            };
            match current.phase.as_str() {
                "transmitting" | "awaiting_topology" => {
                    current.phase = "unknown".to_owned();
                    current.stage = "status_check".to_owned();
                    current.message = Some(
                        "Pane operation result is unknown; no mutation was resent. Waiting for a fresh topology check"
                            .to_owned(),
                    );
                    current.retryable = false;
                    current.queued_resize = None;
                    current.deadline_at_unix_ms =
                        Some(now_unix_ms.saturating_add(CLOSE_STAGE_TIMEOUT_MS));
                    self.push_diagnostic(
                        "pane.operation.unconfirmed",
                        format!(
                            "{} for {} exceeded its deadline; mutation was not resent",
                            operation.kind, operation.target_id
                        ),
                    );
                    changed = true;
                }
                "unknown" => {
                    current.message = Some(
                        "Result check needed; no mutation was resent and the confirmed topology is unchanged"
                            .to_owned(),
                    );
                    current.deadline_at_unix_ms = None;
                    current.retryable = false;
                    self.push_diagnostic(
                        "pane.operation.status_needed",
                        format!(
                            "{} for {} remains unconfirmed after the read-only check",
                            operation.kind, operation.target_id
                        ),
                    );
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            self.sync_recent_closed_snapshot();
        }
        changed
    }

    pub(super) fn expire_tab_moves(&mut self, now_unix_ms: u64) -> bool {
        let mut changed = false;
        let checkout_ids = self.pending_tab_move.keys().cloned().collect::<Vec<_>>();
        for checkout_id in checkout_ids {
            let Some(pending) = self.pending_tab_move.get(&checkout_id).cloned() else {
                continue;
            };
            let Some(deadline) = pending.deadline_at_unix_ms else {
                continue;
            };
            if deadline > now_unix_ms {
                continue;
            }
            let Some(current) = self.pending_tab_move.get_mut(&checkout_id) else {
                continue;
            };
            match current.phase.as_str() {
                "transmitting" | "awaiting_topology" => {
                    current.phase = "unknown".to_owned();
                    current.stage = "status_check".to_owned();
                    current.message = Some(
                        "Tab move result is unknown; no mutation was resent. Waiting for a fresh topology check"
                            .to_owned(),
                    );
                    current.retryable = false;
                    current.deadline_at_unix_ms =
                        Some(now_unix_ms.saturating_add(CLOSE_STAGE_TIMEOUT_MS));
                    self.push_diagnostic(
                        "tab.move.unconfirmed",
                        format!(
                            "Tab move for {} exceeded its deadline; mutation was not resent",
                            pending.target_id
                        ),
                    );
                    changed = true;
                }
                "unknown" => {
                    current.message = Some(
                        "Result check needed; no mutation was resent and the confirmed order is unchanged"
                            .to_owned(),
                    );
                    current.deadline_at_unix_ms = None;
                    current.retryable = false;
                    self.push_diagnostic(
                        "tab.move.status_needed",
                        format!(
                            "Tab move for {} remains unconfirmed after the topology check",
                            pending.target_id
                        ),
                    );
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            self.sync_async_operations();
        }
        changed
    }
}
