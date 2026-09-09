use super::Runtime;
use crate::attachments::{self, Action, Attachment, Intent, Shelf, State};
use crate::live::TerminalSessionMode;

impl Runtime {
    pub(super) fn handle_attachment(&mut self, intent: Intent) -> bool {
        let pane_id = intent.pane_id.clone();
        let id = intent.id.clone();
        let changed = self.reduce_attachment(intent);
        if changed {
            let state = self
                .snapshot
                .terminal
                .attachments
                .iter()
                .find(|shelf| shelf.pane_id == pane_id)
                .and_then(|shelf| shelf.items.iter().find(|item| item.id == id))
                .map(|item| item.state);
            crate::diagnostics::emit(serde_json::json!({
                "component": "image_attachment", "kind": "attachment.transition",
                "pane_id": pane_id, "attachment_id": id, "state": state,
            }));
        }
        changed
    }

    fn reduce_attachment(&mut self, intent: Intent) -> bool {
        if intent.id.is_empty() || intent.id.len() > 64 || intent.pane_id.len() > 256 {
            self.set_error(
                "attachment.invalid_intent",
                "Invalid attachment identity",
                false,
            );
            return true;
        }
        if self.layout_holding_pane(&intent.pane_id).is_none()
            || self.panes_closing.contains(&intent.pane_id)
        {
            // An asynchronous decoder cannot recreate a retired pane's shelf.
            return false;
        }
        if matches!(intent.action, Action::ReturnToPrompt) {
            return self.return_attachment_prompt(&intent.pane_id);
        }
        if matches!(intent.action, Action::Viewport) {
            if self
                .snapshot
                .terminal
                .attachments
                .iter()
                .any(|s| s.pane_id == intent.pane_id)
            {
                // Presentation availability is not provider/composer authority.
                // Keep it private: only actual handoff/state changes publish rest.
                self.attachment_support.available.insert(
                    intent.pane_id,
                    (
                        intent.active == Some(true),
                        intent.following_bottom == Some(true),
                    ),
                );
            }
            return false;
        }
        let provider = self
            .snapshot
            .navigator
            .agents
            .iter()
            .find(|agent| agent.pane_id == intent.pane_id)
            .map(|agent| (agent.agent_kind.clone(), agent.id.clone()));
        if matches!(intent.action, Action::Stage) {
            let total: usize = self
                .snapshot
                .terminal
                .attachments
                .iter()
                .map(|shelf| shelf.items.len())
                .sum();
            let shelves = &mut self.snapshot.terminal.attachments;
            if !shelves.iter().any(|shelf| shelf.pane_id == intent.pane_id) {
                shelves.push(Shelf {
                    pane_id: intent.pane_id.clone(),
                    items: Vec::new(),
                    notice: None,
                    following_bottom: None,
                    viewport_message: Some("Checking terminal viewport".to_owned()),
                });
            }
            let shelf = shelves
                .iter_mut()
                .find(|shelf| shelf.pane_id == intent.pane_id)
                .unwrap();
            if shelf.items.iter().any(|item| item.id == intent.id) {
                return false;
            }
            if total >= attachments::TOTAL_LIMIT || shelf.items.len() >= attachments::PER_PANE_LIMIT
            {
                let notice = Some("Attachment limit reached: 4 per pane, 16 retained across open panes. Handed-off copies stay until their pane closes.".to_owned());
                if shelf.notice == notice {
                    return false;
                }
                shelf.notice = notice;
                return true;
            }
            shelf.notice = None;
            let supported = provider
                .as_ref()
                .is_some_and(|(kind, _)| matches!(kind.as_str(), "claude" | "codex"));
            shelf.items.push(Attachment {
                id: intent.id, name: intent.name.unwrap_or_else(|| "Local image".to_owned()).chars().take(512).collect(),
                path: None, state: if supported { State::Loading } else { State::Failed },
                message: if supported { "Preparing a private image copy" } else { "Image handoff is unavailable: Herdr must identify this local pane as Claude Code or Codex." }.to_owned(),
                handoff_started: false, removal_error: None,
                provider: provider.as_ref().map(|pair| pair.0.clone()), agent_id: provider.map(|pair| pair.1),
            });
            if supported {
                self.observe_attachment_viewport(&intent.pane_id);
            }
            return true;
        }
        let Some(shelf) = self
            .snapshot
            .terminal
            .attachments
            .iter_mut()
            .find(|shelf| shelf.pane_id == intent.pane_id)
        else {
            return false;
        };
        let Some(index) = shelf.items.iter().position(|item| item.id == intent.id) else {
            return false;
        };
        if matches!(intent.action, Action::Remove) {
            let item = &mut shelf.items[index];
            if item.handoff_started {
                let error = Some(
                    attachments::provider_removal_failure(item.provider.as_deref()).to_owned(),
                );
                if item.removal_error == error {
                    return false;
                }
                item.removal_error = error;
            } else {
                shelf.items.remove(index);
            }
            shelf.notice = None;
            return true;
        }
        let item = &mut shelf.items[index];
        match intent.action {
            Action::Prepared => {
                if item.state != State::Loading {
                    return false;
                }
                // A fresh preparation result supersedes an earlier Enter refusal.
                shelf.notice = None;
                if let Some(error) = intent.error {
                    item.state = State::Failed;
                    item.message = error.chars().take(1024).collect();
                } else if let Some(path) =
                    intent.path.filter(|path| attachments::paste(path).is_ok())
                {
                    item.path = Some(path);
                    item.state = State::Pending;
                    item.message =
                        "Not sent. Remove with X, or press Enter to add to the provider prompt."
                            .to_owned();
                } else {
                    item.state = State::Failed;
                    item.message = "The prepared image has no safe local path.".to_owned();
                }
                true
            }
            _ => false,
        }
    }
    /// The only attachment hook in ordinary input. All non-Enter bytes pass unchanged.
    /// First Enter hands off pending images without submitting; a later Enter is ordinary input.
    pub(super) fn intercept_attachment_enter(&mut self, pane_id: &str, bytes_base64: &str) -> bool {
        // Canonical base64 for a single carriage return, as sent by SwiftTerm.
        // No decode, allocation, collection scan or publication for other input.
        if bytes_base64 != "DQ==" {
            return false;
        }
        let Some(shelf) = self
            .snapshot
            .terminal
            .attachments
            .iter_mut()
            .find(|s| s.pane_id == pane_id)
        else {
            return false;
        };
        if !shelf
            .items
            .iter()
            .any(|item| !item.handoff_started || item.state == State::Queued)
        {
            return false;
        }
        let reason = if shelf.items.iter().any(|item| item.state == State::Queued) {
            Some(
                "Image handoff is still queued. Check the provider's image indicators before pressing Enter again.",
            )
        } else if self.snapshot.ui_state.selected_pane_id.as_deref() != Some(pane_id)
            || shelf.following_bottom != Some(true)
            || !self
                .attachment_support
                .available
                .get(pane_id)
                .is_some_and(|state| state.0 && state.1)
        {
            Some("Return to prompt to add images, or use Remove to cancel them. Nothing was submitted.")
        } else if shelf.items.iter().any(|item| item.state == State::Loading) {
            Some("Images are still preparing. Press Enter again when ready; nothing was submitted.")
        } else if shelf
            .items
            .iter()
            .any(|item| !item.handoff_started && item.state == State::Failed)
        {
            Some("Remove the failed images before continuing. Nothing was submitted.")
        } else {
            None
        };
        if let Some(reason) = reason {
            shelf.notice = Some(reason.to_owned());
        } else {
            shelf.notice = None;
            self.advance_attachment_handoffs(pane_id);
        }
        true
    }

    /// One explicitly requested ordered handoff per stable ID to the active pane.
    /// A failed/partial write is never automatically retried.
    fn advance_attachment_handoffs(&mut self, pane_id: &str) -> bool {
        if self.snapshot.ui_state.selected_pane_id.as_deref() != Some(pane_id)
            || self.panes_closing.contains(pane_id)
            || !self
                .attachment_support
                .available
                .get(pane_id)
                .is_some_and(|state| state.0)
        {
            return false;
        }
        let provider = self
            .snapshot
            .navigator
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
            .map(|agent| (agent.agent_kind.clone(), agent.id.clone()));
        let Some(shelf) = self
            .snapshot
            .terminal
            .attachments
            .iter_mut()
            .find(|s| s.pane_id == pane_id)
        else {
            return false;
        };

        let mut changed = false;
        // Preserve drop order even if an asynchronous preparation completes later.
        for item in &mut shelf.items {
            if item.state == State::Loading {
                break;
            }
            if item.state != State::Pending {
                continue;
            }
            changed = true;
            if provider != item.provider.clone().zip(item.agent_id.clone()) {
                item.state = State::Failed;
                item.message =
                    "The pane's provider changed. Remove this image and drop it again.".to_owned();
                continue;
            }
            let result = match (self.terminal_sessions.get(pane_id), item.path.as_deref()) {
                (Some(session), Some(path)) if session.mode == TerminalSessionMode::Control => {
                    attachments::provider_delivery(item.provider.as_deref(), path).map_err(str::to_owned).and_then(|bytes| session.write_bytes(&bytes, Some(crate::model::TerminalInputTrace {
                        id: 0, started_ns: crate::live::monotonic_ns(), attachment_id: Some(item.id.clone()),
                    })))
                }
                _ => Err("Terminal control is unavailable or read-only. Reconnect, then remove and drop the image again.".to_owned()),
            };
            match result {
                Ok(()) => {
                    item.handoff_started = true;
                    item.state = State::Queued;
                    item.message =
                        "Waiting for terminal transport; provider receipt is unconfirmed"
                            .to_owned();
                }
                Err(message) => {
                    item.state = State::Failed;
                    item.message = message;
                }
            }
            crate::diagnostics::emit(serde_json::json!({
                "component": "image_attachment", "kind": "attachment.handoff",
                "pane_id": pane_id, "attachment_id": item.id, "state": item.state,
            }));
        }
        changed
    }
}

impl Runtime {
    pub(super) fn attachment_transport_result(
        &mut self,
        pane_id: &str,
        sent: &crate::model::TerminalInputSent,
    ) -> Option<bool> {
        if let Some(id) = &sent.attachment_id {
            if let Some(shelf) = self
                .snapshot
                .terminal
                .attachments
                .iter_mut()
                .find(|shelf| shelf.pane_id == pane_id)
            {
                let Some(item) = shelf.items.iter_mut().find(|item| &item.id == id) else {
                    return Some(false);
                };
                if item.state != crate::attachments::State::Queued {
                    return Some(false);
                }
                item.state = if sent.outcome == "completed" {
                    crate::attachments::State::HandoffUnconfirmed
                } else {
                    crate::attachments::State::Failed
                };
                item.message = if sent.outcome == "completed" {
                    "Passed to terminal transport. Provider receipt is unconfirmed; check the prompt before pressing Enter."
                } else { "Terminal write failed; delivery may be partial. Check the prompt before trying again." }.to_owned();
                crate::diagnostics::emit(serde_json::json!({
                    "component": "image_attachment", "kind": "attachment.transport_result",
                    "pane_id": pane_id, "attachment_id": id, "state": item.state,
                }));
                if !shelf.items.iter().any(|item| item.state == State::Queued) {
                    shelf.notice = None;
                }
                return Some(true);
            }
            return Some(false);
        }
        None
    }
    pub(super) fn release_attachment_transports(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let mut changed = false;
        for shelf in &mut self.snapshot.terminal.attachments {
            if !keep(&shelf.pane_id) {
                for item in &mut shelf.items {
                    if item.state == State::Queued {
                        item.state = State::Failed;
                        item.message = "Terminal transport ended or was released. Delivery may be partial; check the prompt before trying again.".to_owned();
                        changed = true;
                    }
                }
            }
        }
        changed
    }
}

#[derive(Default)]
pub(super) struct Support {
    generation: u64,
    available: std::collections::HashMap<String, (bool, bool)>,
    watches: std::collections::HashMap<String, (u64, crate::viewport::Watch)>,
}

impl Runtime {
    pub(super) fn retain_attachment_panes(&mut self, keep: impl Fn(&str) -> bool) {
        self.snapshot
            .terminal
            .attachments
            .retain(|shelf| keep(&shelf.pane_id));
        self.attachment_support.watches.retain(|pane, _| keep(pane));
        self.attachment_support
            .available
            .retain(|pane, _| keep(pane));
    }

    fn observe_attachment_viewport(&mut self, pane_id: &str) {
        if self.attachment_support.watches.contains_key(pane_id) {
            return;
        }
        if self.attachment_support.watches.len() >= attachments::TOTAL_LIMIT {
            if let Some(shelf) = self
                .snapshot
                .terminal
                .attachments
                .iter_mut()
                .find(|s| s.pane_id == pane_id)
            {
                shelf.viewport_message = Some(
                    "Viewport capacity is full. Close an unused attachment pane and try again."
                        .to_owned(),
                );
            }
            return;
        }
        let Some(context) = self.live.clone() else {
            if let Some(shelf) = self
                .snapshot
                .terminal
                .attachments
                .iter_mut()
                .find(|s| s.pane_id == pane_id)
            {
                shelf.viewport_message = Some(
                    "A local live terminal viewport is required for image handoff.".to_owned(),
                );
            }
            return;
        };
        self.attachment_support.generation += 1;
        let generation = self.attachment_support.generation;
        let pane = pane_id.to_owned();
        let result =
            crate::viewport::watch(context.api_connector.clone(), pane.clone(), move |state| {
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = runtime
                    .lock()
                    .is_ok_and(|mut runtime| runtime.attachment_viewport(&pane, generation, state));
                if changed {
                    context.notifier.notify();
                }
            });
        match result {
            Ok(watch) => {
                self.attachment_support
                    .watches
                    .insert(pane_id.to_owned(), (generation, watch));
            }
            Err(error) => {
                if let Some(shelf) = self
                    .snapshot
                    .terminal
                    .attachments
                    .iter_mut()
                    .find(|s| s.pane_id == pane_id)
                {
                    shelf.following_bottom = None;
                    shelf.viewport_message = Some(error);
                }
            }
        }
    }

    fn attachment_viewport(
        &mut self,
        pane_id: &str,
        generation: u64,
        state: Result<bool, String>,
    ) -> bool {
        if self
            .attachment_support
            .watches
            .get(pane_id)
            .map(|entry| entry.0)
            != Some(generation)
        {
            return false;
        }
        let Some(shelf) = self
            .snapshot
            .terminal
            .attachments
            .iter_mut()
            .find(|s| s.pane_id == pane_id)
        else {
            return false;
        };
        let (following, message) = match state {
            Ok(true) => (Some(true), None),
            Ok(false) => (
                Some(false),
                Some("Return to the prompt to review images in the native composer.".to_owned()),
            ),
            Err(error) => (None, Some(format!("Viewport unavailable: {error}"))),
        };
        if shelf.following_bottom == following && shelf.viewport_message == message {
            return false;
        }
        crate::diagnostics::emit(serde_json::json!({
            "component": "image_attachment", "kind": "attachment.viewport",
            "pane_id": pane_id, "generation": generation, "following_bottom": following,
            "message": message,
        }));
        shelf.following_bottom = following;
        shelf.viewport_message = message;
        true
    }

    fn return_attachment_prompt(&mut self, pane_id: &str) -> bool {
        if !self
            .snapshot
            .terminal
            .attachments
            .iter()
            .any(|s| s.pane_id == pane_id)
        {
            return false;
        }
        if self.snapshot.terminal.attachments.iter().any(|s| {
            s.pane_id == pane_id
                && s.viewport_message.as_deref() == Some("Returning to the terminal prompt")
        }) {
            return false;
        }
        // The pinned control stream resets host scrollback on input, including
        // empty bytes. No character, provider command, or Enter is delivered.
        let result = self
            .terminal_sessions
            .get(pane_id)
            .ok_or_else(|| {
                "Terminal control is unavailable. Reconnect before returning to the prompt."
                    .to_owned()
            })
            .and_then(|session| session.write_bytes(&[], None));
        let shelf = self
            .snapshot
            .terminal
            .attachments
            .iter_mut()
            .find(|s| s.pane_id == pane_id)
            .unwrap();
        let succeeded = result.is_ok();
        shelf.following_bottom = None;
        shelf.viewport_message = Some(match result {
            Ok(()) => "Returning to the terminal prompt".to_owned(),
            Err(error) => error,
        });
        self.attachment_support.watches.remove(pane_id);
        if succeeded {
            self.observe_attachment_viewport(pane_id);
        }
        true
    }
}
