use super::*;
use crate::model::AsyncOperationSnapshot;
use crate::terminal_attachments::{self as ingress, AttachmentFile};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct PendingAttachment {
    pub operation: AsyncOperationSnapshot,
    pane_id: String,
    terminal_generation: Option<u64>,
    remote: Option<(String, u64)>,
    clipboard: bool,
    clipboard_preparing: bool,
    bracketed: bool,
    paths: Vec<String>,
    prepared: Option<Arc<Vec<AttachmentFile>>>,
    queued: Vec<u8>,
    rejected_bytes: usize,
    cancelled: Arc<AtomicBool>,
}

impl Runtime {
    pub(crate) fn take_attachment_worker(&mut self) -> Option<thread::JoinHandle<()>> {
        if let Some(pending) = &mut self.attachment {
            pending.cancelled.store(true, Ordering::Release);
            pending.operation.stage = "cancelling".to_owned();
            pending.queued.clear();
        }
        // A failed intent can outlive its transfer thread and still own staged
        // files. Start one cleanup worker now so destruction takes and joins it.
        if self
            .attachment
            .as_ref()
            .is_some_and(|pending| !pending.clipboard_preparing)
            && self
                .attachment_worker
                .as_ref()
                .is_none_or(|worker| worker.is_finished())
        {
            self.start_attachment_cleanup();
        }
        self.attachment_worker.take()
    }

    pub(super) fn reconcile_attachment_target(&mut self) -> bool {
        let Some(pending) = self.attachment.as_ref() else {
            return false;
        };
        if self.attachment_target_valid(pending) || pending.operation.stage == "retired" {
            return false;
        }
        let pending = self.attachment.as_mut().expect("checked above");
        pending.cancelled.store(true, Ordering::Release);
        pending.queued.clear();
        pending.operation.stage = "retired".to_owned();
        self.fail_attachment("The original terminal closed or reconnected. Held input was discarded and nothing was sent. Cancel and paste again.", false);
        if !self
            .attachment
            .as_ref()
            .is_some_and(|pending| pending.clipboard_preparing)
            && self
                .attachment_worker
                .as_ref()
                .is_none_or(|worker| worker.is_finished())
        {
            self.start_attachment_cleanup();
        }
        true
    }

    pub(super) fn tick_attachment(&mut self) -> bool {
        if self
            .attachment
            .as_ref()
            .is_some_and(|pending| pending.operation.stage == "retry_wait")
            && self
                .attachment_worker
                .as_ref()
                .is_none_or(|worker| worker.is_finished())
        {
            self.start_attachment_worker();
            return true;
        }
        false
    }

    fn attachment_target_valid(&self, pending: &PendingAttachment) -> bool {
        if self.close_operation_holds_pane(&pending.pane_id) {
            return false;
        }
        if self
            .terminal_session_generations
            .get(&pending.pane_id)
            .copied()
            != pending.terminal_generation
        {
            return false;
        }
        if !self
            .snapshot
            .terminal
            .panes
            .iter()
            .any(|pane| pane.pane_id == pending.pane_id)
        {
            return false;
        }
        if let Some((target, generation)) = &pending.remote {
            if self.remote_connection_generations.get(target) != Some(generation) {
                return false;
            }
            if !self
                .snapshot
                .status
                .remote
                .iter()
                .any(|remote| remote.target_id == *target && remote.state == "connected")
            {
                return false;
            }
        }
        match self.terminal_sessions.get(&pending.pane_id) {
            Some(session) => session.mode == TerminalSessionMode::Control,
            None => {
                self.live.is_none()
                    && pending.remote.is_none()
                    && !pending.pane_id.starts_with("remote:")
            }
        }
    }

    pub(super) fn begin_attachment(&mut self, payload: AttachmentPayload) -> bool {
        let mut operation = AsyncOperationSnapshot {
            id: payload.request_id.clone(),
            kind: "terminal.attachment".to_owned(),
            target_id: payload.pane_id.clone(),
            scope_id: self
                .pane_operation_scope(&payload.pane_id)
                .unwrap_or_else(|| payload.pane_id.clone()),
            phase: "pending".to_owned(),
            stage: if payload.clipboard {
                "clipboard"
            } else {
                "preparing"
            }
            .to_owned(),
            started_at_unix_ms: unix_milliseconds(),
            deadline_at_unix_ms: None,
            message: Some("Preparing files…".to_owned()),
            retryable: false,
        };
        if self
            .attachment
            .as_ref()
            .is_some_and(|pending| pending.operation.id == payload.request_id)
        {
            return false;
        }
        let refusal = if !ingress::valid_request_id(&payload.request_id) {
            Some("Invalid attachment request identity.")
        } else if self.attachment.is_some()
            || self
                .attachment_worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
        {
            Some(
                "Another file transfer is pending. Finish or cancel that transfer before pasting more files.",
            )
        } else if payload
            .paths
            .iter()
            .any(|path| path.len() > ingress::MAX_PATH_BYTES)
        {
            Some(
                "A selected file path exceeds the 4096-byte path limit. Move the file to a shorter path.",
            )
        } else if payload.paths.len() > ingress::MAX_FILES
            || (!payload.clipboard && payload.paths.is_empty())
        {
            Some("Choose between 1 and 8 regular files.")
        } else {
            None
        };
        if let Some(message) = refusal {
            operation.phase = "refused".to_owned();
            operation.message = Some(message.to_owned());
            self.attachment_rejection = Some(operation);
            self.sync_async_operations();
            return true;
        }
        let remote = self
            .remote_file_transports
            .keys()
            .find(|target| remote_pane_source_id(target, &payload.pane_id).is_some())
            .and_then(|target| {
                self.remote_connection_generations
                    .get(target)
                    .map(|generation| (target.clone(), *generation))
            });
        let pending = PendingAttachment {
            operation,
            terminal_generation: self
                .terminal_session_generations
                .get(&payload.pane_id)
                .copied(),
            pane_id: payload.pane_id,
            remote,
            clipboard: payload.clipboard,
            clipboard_preparing: payload.clipboard,
            bracketed: payload.bracketed_paste,
            paths: payload.paths,
            prepared: None,
            queued: Vec::new(),
            rejected_bytes: 0,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let valid = self.attachment_target_valid(&pending)
            && (!pending.pane_id.starts_with("remote:") || pending.remote.is_some());
        self.attachment = Some(pending);
        self.attachment_rejection = None;
        if !valid {
            self.attachment
                .as_mut()
                .expect("assigned above")
                .clipboard_preparing = false;
            self.fail_attachment("The original terminal is unavailable. Reconnect, cancel this transfer and paste again.", false);
        } else if !payload.clipboard {
            self.start_attachment_worker();
        }
        self.sync_async_operations();
        true
    }

    pub(super) fn attachment_ready(&mut self, payload: AttachmentCompletionPayload) -> bool {
        let Some(pending) = self.attachment.as_mut().filter(|pending| {
            pending.operation.id == payload.request_id
                && pending.pane_id == payload.pane_id
                && pending.clipboard_preparing
                && ["clipboard", "cancelling", "retired"]
                    .contains(&pending.operation.stage.as_str())
        }) else {
            return false;
        };
        pending.clipboard_preparing = false;
        if ["cancelling", "retired"].contains(&pending.operation.stage.as_str()) {
            self.attachment = None;
            self.sync_async_operations();
            return true;
        }
        if pending.operation.phase == "failed" {
            return true;
        }
        if let Some(error) = payload.error {
            self.fail_attachment(&error, false);
        } else {
            pending.paths = vec![
                ingress::clipboard_path(&self.state_path, &payload.request_id)
                    .to_string_lossy()
                    .into_owned(),
            ];
            self.start_attachment_worker();
        }
        self.sync_async_operations();
        true
    }

    fn fail_attachment(&mut self, message: &str, retryable: bool) {
        if let Some(pending) = self.attachment.as_mut() {
            pending.operation.phase = "failed".to_owned();
            pending.operation.retryable = retryable;
            pending.operation.message = Some(format!(
                "{message} Files were not pasted.{}{}",
                if pending.operation.stage == "retired" {
                    ""
                } else if !retryable {
                    " Typed input is held. Cancel to discard it."
                } else {
                    " Typed input is held until retry succeeds or you cancel."
                },
                if pending.rejected_bytes > 0 {
                    format!(
                        " {} further input bytes were refused at the 64 KiB limit.",
                        pending.rejected_bytes
                    )
                } else {
                    String::new()
                }
            ));
        }
        self.sync_async_operations();
    }

    pub(super) fn hold_attachment_input(&mut self, payload: &KeyPayload) -> Option<bool> {
        let pending = self.attachment.as_mut().filter(|pending| {
            pending.pane_id == payload.pane_id && pending.operation.stage != "retired"
        })?;
        if pending.operation.stage == "cancelling" {
            let message = "Cancellation is finishing. New terminal input was refused; wait for this notice to close before typing.";
            if pending.operation.message.as_deref() == Some(message) {
                return Some(false);
            }
            pending.operation.message = Some(message.to_owned());
            self.sync_async_operations();
            return Some(true);
        }
        // Bound decoding before allocating, including a large ordinary text paste.
        let padding = if payload.bytes_base64.ends_with("==") {
            2
        } else if payload.bytes_base64.ends_with('=') {
            1
        } else {
            0
        };
        let decoded_length = (payload.bytes_base64.len() / 4 * 3).saturating_sub(padding);
        if pending.queued.len().saturating_add(decoded_length) > ingress::MAX_QUEUED_INPUT {
            pending.rejected_bytes = pending.rejected_bytes.saturating_add(decoded_length);
            pending.cancelled.store(true, Ordering::Release);
            self.fail_attachment(
                "Held input reached its 64 KiB limit. Cancel and paste again.",
                false,
            );
            return Some(true);
        }
        let bytes = match live::decode_base64(&payload.bytes_base64) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.fail_attachment("Invalid terminal input was refused.", false);
                return Some(true);
            }
        };
        if pending.queued.len().saturating_add(bytes.len()) > ingress::MAX_QUEUED_INPUT {
            pending.rejected_bytes = pending.rejected_bytes.saturating_add(bytes.len());
            pending.cancelled.store(true, Ordering::Release);
            self.fail_attachment(
                "Held input reached its 64 KiB limit. Cancel and paste again.",
                false,
            );
            Some(true)
        } else {
            pending.queued.extend_from_slice(&bytes);
            Some(false)
        }
    }

    pub(super) fn attachment_action(&mut self, payload: AttachmentActionPayload) -> bool {
        if self.attachment_rejection.as_ref().is_some_and(|rejection| {
            rejection.id == payload.request_id && rejection.target_id == payload.pane_id
        }) {
            self.attachment_rejection = None;
            self.sync_async_operations();
            return true;
        }
        let Some(pending) = self.attachment.as_ref().filter(|pending| {
            pending.operation.id == payload.request_id && pending.pane_id == payload.pane_id
        }) else {
            return false;
        };
        match payload.action.as_str() {
            "cancel" => {
                pending.cancelled.store(true, Ordering::Release);
                if self
                    .attachment_worker
                    .as_ref()
                    .is_some_and(|worker| !worker.is_finished())
                    || pending.clipboard_preparing
                {
                    let pending = self.attachment.as_mut().expect("matched above");
                    pending.operation.phase = "pending".to_owned();
                    pending.operation.stage = "cancelling".to_owned();
                    pending.operation.message =
                        Some("Cancelling transfer and discarding held input…".to_owned());
                    pending.queued.clear();
                } else {
                    self.start_attachment_cleanup();
                }
            }
            "retry" if pending.operation.retryable && pending.operation.phase == "failed" => {
                if !self.attachment_target_valid(pending) {
                    self.fail_attachment(
                        "The terminal connection changed. Cancel and paste again.",
                        false,
                    );
                } else if self
                    .attachment_worker
                    .as_ref()
                    .is_some_and(|worker| !worker.is_finished())
                {
                    let pending = self.attachment.as_mut().expect("matched above");
                    pending.operation.stage = "retry_wait".to_owned();
                    pending.operation.phase = "pending".to_owned();
                    pending.operation.retryable = false;
                    pending.operation.message = Some(
                        "Waiting for the previous transfer to finish before retrying…".to_owned(),
                    );
                } else {
                    self.start_attachment_worker();
                }
            }
            _ => return false,
        }
        self.sync_async_operations();
        true
    }

    fn start_attachment_cleanup(&mut self) {
        let Some(context) = self.worker_context.clone() else {
            self.attachment = None;
            return;
        };
        let Some(pending) = self.attachment.as_mut() else {
            return;
        };
        let request_id = pending.operation.id.clone();
        let clipboard = pending.clipboard;
        let files = pending.prepared.take();
        let transport = pending
            .remote
            .as_ref()
            .and_then(|(target, _)| self.remote_file_transports.get(target))
            .cloned();
        let state_path = self.state_path.clone();
        pending.queued.clear();
        pending.operation.stage = "cancelling".to_owned();
        pending.operation.phase = "pending".to_owned();
        pending.operation.retryable = false;
        pending.operation.message =
            Some("Cancelling transfer and discarding held input…".to_owned());
        match thread::Builder::new()
            .name("hide-attachment-cleanup".to_owned())
            .spawn(move || {
                if clipboard {
                    ingress::remove_clipboard(&state_path, &request_id);
                }
                if let (Some(transport), Some(files)) = (transport, files) {
                    transport.remove_attachments(&request_id, &files);
                }
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                if let Ok(mut runtime) = runtime.lock()
                    && runtime
                        .attachment
                        .as_ref()
                        .is_some_and(|pending| pending.operation.id == request_id)
                {
                    runtime.attachment = None;
                    runtime.sync_async_operations();
                }
                context.notifier.notify();
            }) {
            Ok(worker) => self.attachment_worker = Some(worker),
            Err(_) => {
                self.attachment = None;
                crate::diagnostic!(
                    serde_json::json!({"kind":"terminal.attachment.cleanup_worker_failed"})
                );
            }
        }
    }

    fn start_attachment_worker(&mut self) {
        let Some(context) = self.worker_context.clone() else {
            self.fail_attachment("The file transfer worker is unavailable.", false);
            return;
        };
        if self
            .attachment_worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return;
        }
        let Some(pending) = self.attachment.as_mut() else {
            return;
        };
        pending.operation.phase = "pending".to_owned();
        pending.operation.stage = "transfer".to_owned();
        pending.operation.retryable = false;
        pending.operation.message = Some(
            if pending.remote.is_some() {
                "Uploading files… Temporary files expire after 24 hours on a later paste."
            } else if pending.clipboard {
                "Preparing image… Temporary files expire after 24 hours on a later paste."
            } else {
                "Preparing files…"
            }
            .to_owned(),
        );
        pending.cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = pending.cancelled.clone();
        let request_id = pending.operation.id.clone();
        let paths = pending.paths.clone();
        let prepared = pending.prepared.clone();
        let transport = pending
            .remote
            .as_ref()
            .and_then(|(target, _)| self.remote_file_transports.get(target))
            .cloned();
        let clipboard = pending.clipboard;
        let state_path = self.state_path.clone();
        match thread::Builder::new()
            .name("hide-terminal-attachment".to_owned())
            .spawn(move || {
                let prepared = match prepared {
                    Some(files) => Ok(files),
                    None => ingress::read_sources(&paths, &cancelled).map(Arc::new),
                };
                let result =
                    prepared
                        .as_ref()
                        .map_err(Clone::clone)
                        .and_then(|files| match &transport {
                            Some(transport) => {
                                transport.stage_attachments(&request_id, files, &cancelled)
                            }
                            None => Ok(files.iter().map(|file| file.path.clone()).collect()),
                        });
                let remote_uploaded = transport.is_some() && result.is_ok();
                let cleanup_files = prepared.as_ref().ok().cloned();
                let Some(runtime) = context.runtime.upgrade() else {
                    if clipboard {
                        ingress::remove_clipboard(&state_path, &request_id);
                    }
                    if let (Some(transport), Some(files)) = (transport, cleanup_files) {
                        transport.remove_attachments(&request_id, &files);
                    }
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut runtime) => {
                        runtime.finish_attachment(&request_id, prepared.ok(), result)
                    }
                    Err(_) => return,
                };
                let cleaned_cancellation = cancelled.load(Ordering::Acquire);
                if clipboard && (cleaned_cancellation || remote_uploaded) {
                    ingress::remove_clipboard(&state_path, &request_id);
                }
                if cleaned_cancellation
                    && let (Some(transport), Some(files)) = (&transport, &cleanup_files)
                {
                    transport.remove_attachments(&request_id, files);
                }
                // All I/O is finished before releasing worker ownership. An immediate
                // retry is either admitted now or handed off here, even without a live tick.
                let draining_cleanup = match runtime.lock() {
                    Ok(mut runtime) => runtime.complete_attachment_worker(cleaned_cancellation),
                    Err(_) => false,
                };
                // Drop already owns this worker's JoinHandle. Complete any cancellation
                // that arrived after the first check here, never in an unjoined child.
                if draining_cleanup {
                    if clipboard {
                        ingress::remove_clipboard(&state_path, &request_id);
                    }
                    if let (Some(transport), Some(files)) = (&transport, &cleanup_files) {
                        transport.remove_attachments(&request_id, files);
                    }
                }
                if changed {
                    context.notifier.notify();
                }
            }) {
            Ok(worker) => self.attachment_worker = Some(worker),
            Err(_) => {
                self.fail_attachment("Could not start the file transfer worker. Retry.", true)
            }
        }
        self.sync_async_operations();
    }

    fn complete_attachment_worker(&mut self, cleaned_cancellation: bool) -> bool {
        // None means destruction took the handle and is joining this exact worker.
        let draining = self.attachment_worker.take().is_none();
        let needs_cleanup = self.attachment.as_ref().is_some_and(|pending| {
            ["cancelling", "retired"].contains(&pending.operation.stage.as_str())
        });
        if needs_cleanup {
            if draining || cleaned_cancellation {
                self.attachment = None;
                self.sync_async_operations();
            } else {
                self.start_attachment_cleanup();
            }
        } else if !draining
            && self
                .attachment
                .as_ref()
                .is_some_and(|pending| pending.operation.stage == "retry_wait")
        {
            self.start_attachment_worker();
        }
        draining && needs_cleanup && !cleaned_cancellation
    }

    fn finish_attachment(
        &mut self,
        request_id: &str,
        prepared: Option<Arc<Vec<AttachmentFile>>>,
        result: Result<Vec<String>, String>,
    ) -> bool {
        let Some(pending) = self
            .attachment
            .as_ref()
            .filter(|pending| pending.operation.id == request_id)
        else {
            return false;
        };
        if pending.operation.stage == "cancelling" {
            self.attachment = None;
            self.sync_async_operations();
            return true;
        }
        if !self.attachment_target_valid(pending) || pending.operation.stage == "retired" {
            let pending = self.attachment.as_mut().expect("matched");
            pending.cancelled.store(true, Ordering::Release);
            pending.prepared = None;
            pending.queued.clear();
            pending.operation.stage = "retired".to_owned();
            self.fail_attachment("The original terminal closed or reconnected. No files or held input were sent. Paste again in the new terminal.", false);
            return true;
        }
        let pending = self.attachment.as_mut().expect("matched");
        pending.prepared = prepared;
        if pending.rejected_bytes > 0 {
            self.fail_attachment(
                "Held input exceeded the 64 KiB limit. Cancel and paste again.",
                false,
            );
            return true;
        }
        match result {
            Err(error) => {
                crate::diagnostic!(
                    serde_json::json!({"kind":"terminal.attachment.failed", "request_id":request_id, "pane_id":pending.pane_id, "error":error})
                );
                self.fail_attachment(&error, true);
            }
            Ok(paths) => {
                let mut bytes = match ingress::paste_bytes(&paths, pending.bracketed) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        self.fail_attachment(&error, false);
                        return true;
                    }
                };
                bytes.extend_from_slice(&pending.queued);
                let pane_id = pending.pane_id.clone();
                if let Some(session) = self.terminal_sessions.get(&pane_id) {
                    if session.write_bytes(&bytes, None).is_err() {
                        self.fail_attachment(
                            "Terminal input was not accepted. Check the connection and retry.",
                            true,
                        );
                        return true;
                    }
                } else {
                    self.append_terminal_chunk(pane_id, live::encode_base64(&bytes));
                }
                self.attachment = None;
            }
        }
        self.sync_async_operations();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "01234567-0123-0123-0123-0123456789ab";
    const OTHER_ID: &str = "01234567-0123-0123-0123-0123456789ac";

    fn runtime() -> Runtime {
        let mut runtime = Runtime::new(
            CoreOptions {
                schema_version: SCHEMA_VERSION,
                herdr_socket_path: None,
                herdr_bin_path: None,
                app_state_path: std::env::temp_dir()
                    .join(format!(
                        "hide-attachment-state-{}-{}.json",
                        std::process::id(),
                        unix_milliseconds()
                    ))
                    .to_string_lossy()
                    .into_owned(),
                host_helper_dir: None,
                host_helper_root: None,
                workspace_views_path: None,
            },
            environment::EnvironmentReport {
                statuses: Vec::new(),
                home_path: None,
                chromux_enabled: false,
                herdr_socket_path_override: None,
                codex_home: None,
            },
        );
        runtime.ensure_terminal_pane("pane-one");
        runtime
    }

    fn begin(runtime: &mut Runtime, id: &str, pane: &str) {
        assert!(runtime.begin_attachment(AttachmentPayload {
            request_id: id.to_owned(),
            pane_id: pane.to_owned(),
            bracketed_paste: true,
            clipboard: true,
            paths: Vec::new()
        }));
    }

    fn key(pane: &str, bytes: &[u8]) -> KeyPayload {
        KeyPayload {
            pane_id: pane.to_owned(),
            bytes_base64: live::encode_base64(bytes),
            input_trace: None,
        }
    }

    #[test]
    fn failed_attachment_holds_enter_and_success_releases_in_order_without_extra_publication() {
        let mut runtime = runtime();
        begin(&mut runtime, ID, "pane-one");
        assert_eq!(
            runtime.hold_attachment_input(&key("pane-one", b" AFTER\r")),
            Some(false)
        );
        assert_eq!(
            runtime.hold_attachment_input(&key("different-pane", b"ordinary")),
            None
        );
        runtime.finish_attachment(
            ID,
            None,
            Err("A file exceeds the 20 MiB attachment limit.".to_owned()),
        );
        assert!(runtime.snapshot.terminal.chunks.is_empty());
        assert!(
            runtime
                .attachment
                .as_ref()
                .unwrap()
                .operation
                .message
                .as_ref()
                .unwrap()
                .contains("20 MiB")
        );
        assert_eq!(
            runtime.hold_attachment_input(&key("pane-one", b"next")),
            Some(false)
        );
        runtime.finish_attachment(ID, None, Ok(vec!["/remote/private/image.png".to_owned()]));
        assert!(runtime.attachment.is_none());
        assert_eq!(
            live::decode_base64(
                &runtime
                    .snapshot
                    .terminal
                    .chunks
                    .last()
                    .unwrap()
                    .bytes_base64
            )
            .unwrap(),
            b"\x1b[200~\"/remote/private/image.png\"\x1b[201~ AFTER\rnext"
        );
    }

    #[test]
    fn input_limit_is_visible_and_never_sends_partial_input() {
        let mut runtime = runtime();
        begin(&mut runtime, ID, "pane-one");
        assert_eq!(
            runtime.hold_attachment_input(&key("pane-one", &vec![b'x'; ingress::MAX_QUEUED_INPUT])),
            Some(false)
        );
        assert_eq!(
            runtime.hold_attachment_input(&key("pane-one", b"\r")),
            Some(true)
        );
        let pending = runtime.attachment.as_ref().unwrap();
        assert_eq!(pending.queued.len(), ingress::MAX_QUEUED_INPUT);
        assert!(
            pending
                .operation
                .message
                .as_ref()
                .unwrap()
                .contains("1 further input bytes were refused")
        );
        runtime.finish_attachment(ID, None, Ok(vec!["/remote/file.png".to_owned()]));
        assert!(runtime.snapshot.terminal.chunks.is_empty());
        assert!(!runtime.attachment.as_ref().unwrap().operation.retryable);
    }

    #[test]
    fn competing_paste_dismissal_preserves_original_and_cancel_discards_held_input() {
        let mut runtime = runtime();
        begin(&mut runtime, ID, "pane-one");
        runtime.hold_attachment_input(&key("pane-one", b"\r"));
        begin(&mut runtime, OTHER_ID, "pane-one");
        assert_eq!(
            runtime.attachment_rejection.as_ref().unwrap().phase,
            "refused"
        );
        runtime.attachment_action(AttachmentActionPayload {
            request_id: OTHER_ID.to_owned(),
            pane_id: "pane-one".to_owned(),
            action: "cancel".to_owned(),
        });
        assert_eq!(runtime.attachment.as_ref().unwrap().queued, b"\r");
        runtime.attachment_action(AttachmentActionPayload {
            request_id: ID.to_owned(),
            pane_id: "pane-one".to_owned(),
            action: "cancel".to_owned(),
        });
        assert!(runtime.attachment.as_ref().unwrap().queued.is_empty());
        runtime.attachment_ready(AttachmentCompletionPayload {
            request_id: ID.to_owned(),
            pane_id: "pane-one".to_owned(),
            error: None,
        });
        assert!(runtime.attachment.is_none());
        assert!(runtime.snapshot.terminal.chunks.is_empty());
    }

    #[test]
    fn changed_generation_retires_intent_and_unknown_remote_never_falls_back_locally() {
        let mut runtime = runtime();
        begin(&mut runtime, ID, "pane-one");
        runtime.hold_attachment_input(&key("pane-one", b"\r"));
        runtime
            .terminal_session_generations
            .insert("pane-one".to_owned(), 99);
        assert!(runtime.reconcile_attachment_target());
        assert_eq!(
            runtime.attachment.as_ref().unwrap().operation.stage,
            "retired"
        );
        assert!(runtime.attachment.as_ref().unwrap().queued.is_empty());
        runtime.attachment_ready(AttachmentCompletionPayload {
            request_id: ID.to_owned(),
            pane_id: "pane-one".to_owned(),
            error: None,
        });
        assert!(runtime.attachment.is_none());
        assert!(runtime.snapshot.terminal.chunks.is_empty());
        runtime.ensure_terminal_pane("remote:missing:pane");
        begin(&mut runtime, OTHER_ID, "remote:missing:pane");
        assert_eq!(
            runtime.attachment.as_ref().unwrap().operation.phase,
            "failed"
        );
        assert!(!runtime.attachment.as_ref().unwrap().operation.retryable);
        assert!(runtime.snapshot.terminal.chunks.is_empty());
    }

    #[test]
    fn cancel_after_completion_and_retirement_release_the_admission_slot() {
        for retire in [false, true] {
            let mut runtime = runtime();
            begin(&mut runtime, ID, "pane-one");
            runtime.attachment.as_mut().unwrap().clipboard_preparing = false;
            runtime.attachment.as_mut().unwrap().operation.stage = "transfer".to_owned();
            let gate = Arc::new(std::sync::Barrier::new(2));
            let worker_gate = gate.clone();
            runtime.attachment_worker = Some(thread::spawn(move || {
                worker_gate.wait();
            }));
            runtime.finish_attachment(ID, None, Err("Transfer failed.".to_owned()));
            if retire {
                runtime
                    .terminal_session_generations
                    .insert("pane-one".to_owned(), 99);
                assert!(runtime.reconcile_attachment_target());
            } else {
                runtime.attachment_action(AttachmentActionPayload {
                    request_id: ID.to_owned(),
                    pane_id: "pane-one".to_owned(),
                    action: "cancel".to_owned(),
                });
            }
            gate.wait();
            assert!(!runtime.complete_attachment_worker(false));
            assert!(runtime.attachment.is_none());
            runtime.ensure_terminal_pane("pane-two");
            begin(&mut runtime, OTHER_ID, "pane-two");
            assert_eq!(
                runtime.attachment.as_ref().unwrap().operation.phase,
                "pending"
            );
            assert!(runtime.snapshot.terminal.chunks.is_empty());
        }
    }

    #[test]
    fn destruction_drains_its_joined_worker_without_spawning_cleanup_or_retry() {
        let mut runtime = runtime();
        begin(&mut runtime, ID, "pane-one");
        runtime.attachment.as_mut().unwrap().clipboard_preparing = false;
        let gate = Arc::new(std::sync::Barrier::new(2));
        let worker_gate = gate.clone();
        let worker = thread::spawn(move || {
            worker_gate.wait();
        });
        runtime.attachment_worker = Some(worker);
        let joined = runtime.take_attachment_worker().unwrap();
        assert!(
            runtime.complete_attachment_worker(false),
            "the original joined worker owns remaining cleanup"
        );
        assert!(runtime.attachment.is_none() && runtime.attachment_worker.is_none());
        gate.wait();
        joined.join().unwrap();
        assert!(runtime.snapshot.terminal.chunks.is_empty());
    }
}
