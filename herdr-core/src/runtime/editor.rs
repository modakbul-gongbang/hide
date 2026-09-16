use super::*;

pub(super) enum PreparedFileTab {
    /// The file already has a tab; showing it is a focus.
    Open(String),
    /// The file was read and needs a tab of its own.
    Read {
        tab_id: String,
        document: EditorDocumentSnapshot,
    },
}

impl Runtime {
    pub(super) fn file_tab_id(workspace_id: &str, checkout_id: &str, path: &str) -> String {
        format!("file:{workspace_id}:{checkout_id}:{path}")
    }

    pub(super) fn activate_editor_tab(&mut self, tab_id: &str) -> Result<(), String> {
        let tab = self
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .cloned()
            .ok_or_else(|| format!("Editor tab {tab_id} is not open"))?;
        let document = match tab.kind {
            EditorTabKind::File => Some(
                self.editor_documents
                    .get(tab_id)
                    .cloned()
                    .ok_or_else(|| format!("File tab {tab_id} has no document state"))?,
            ),
            EditorTabKind::Diff => {
                if tab.diff_committed.is_none() {
                    return Err(format!("Diff tab {tab_id} has no comparison scope"));
                }
                None
            }
        };
        if let Some(active_id) = self.snapshot.editor.active_tab_id.as_deref()
            && active_id != tab_id
        {
            self.editor_tab_history.retain(|known| known != active_id);
            self.editor_tab_history.push(active_id.to_owned());
        }
        self.snapshot.editor.active_tab_id = Some(tab_id.to_owned());
        match tab.kind {
            EditorTabKind::File => {
                self.snapshot.editor.document = document;
                self.snapshot.ui_state.selected_path = Some(tab.path);
            }
            EditorTabKind::Diff => {
                let committed = tab.diff_committed.expect("validated above");
                if self.snapshot.changes.selected_path.as_deref() != Some(tab.path.as_str())
                    || self.snapshot.changes.selected_committed != committed
                {
                    self.snapshot.changes.diff = None;
                }
                self.snapshot.changes.selected_path = Some(tab.path);
                self.snapshot.changes.selected_committed = committed;
                self.snapshot.editor.document = None;
                self.snapshot.ui_state.selected_path = None;
            }
        }
        Ok(())
    }

    pub(super) fn deactivate_editor_tab(&mut self) {
        self.snapshot.editor.active_tab_id = None;
        self.snapshot.editor.document = None;
        self.editor_tab_history.clear();
    }

    pub(super) fn sync_active_editor_document(&mut self) {
        self.snapshot.editor.document =
            self.snapshot
                .editor
                .active_tab_id
                .as_deref()
                .and_then(|tab_id| {
                    self.editor_documents.get(tab_id).and_then(|document| {
                        self.snapshot
                            .editor
                            .tabs
                            .iter()
                            .find(|tab| tab.id == tab_id && tab.kind == EditorTabKind::File)
                            .map(|_| document.clone())
                    })
                });
    }

    pub(super) fn sync_file_tab_dirty(&mut self, tab_id: &str) {
        let dirty = self
            .editor_documents
            .get(tab_id)
            .is_some_and(|document| document.dirty);
        if let Some(tab) = self
            .snapshot
            .editor
            .tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
        {
            tab.dirty = dirty;
        }
    }

    pub(super) fn ingest_file_save_result(
        &mut self,
        tab_id: String,
        path: String,
        contents: String,
        editor: EditorDocumentSnapshot,
        result: Result<(), String>,
    ) -> bool {
        let is_current_draft = self.is_current_file_draft(&tab_id, &path, &contents);
        if !is_current_draft {
            self.push_diagnostic(
                "file.save_stale",
                format!("Ignored a completed save for stale draft {path}"),
            );
            return true;
        }
        self.editor_documents.insert(tab_id.clone(), editor);
        self.sync_file_tab_dirty(&tab_id);
        self.sync_active_editor_document();
        match result {
            Ok(()) => {
                self.push_diagnostic("file.save_ready", format!("Saved {path}"));
            }
            Err(message) => self.set_error("file.save_failed", message, true),
        }
        true
    }

    pub(super) fn is_current_file_draft(&self, tab_id: &str, path: &str, contents: &str) -> bool {
        self.snapshot.editor.tabs.iter().any(|tab| {
            tab.id == tab_id
                && tab.path == path
                && self
                    .editor_documents
                    .get(tab_id)
                    .and_then(|document| document.contents_utf8.as_deref())
                    == Some(contents)
        })
    }

    pub(super) fn ingest_file_save_then_close_result(
        &mut self,
        tab_id: String,
        path: String,
        contents: String,
        editor: EditorDocumentSnapshot,
        result: Result<(), String>,
    ) -> bool {
        let close_after_save =
            result.is_ok() && self.is_current_file_draft(&tab_id, &path, &contents);
        self.ingest_file_save_result(tab_id.clone(), path, contents, editor, result);
        if close_after_save {
            self.close_file_tab_now(&tab_id)
        } else {
            true
        }
    }

    pub(super) fn start_file_save_then_close(
        &mut self,
        close_tab_id: String,
        payload: FileSavePayload,
    ) -> bool {
        if payload.tab_id != close_tab_id {
            self.set_error(
                "file.close_save_mismatch",
                "The pending save did not belong to the file being closed",
                false,
            );
            return true;
        }
        let Some(tab) = self
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.id == payload.tab_id && tab.path == payload.path)
        else {
            self.set_error(
                "file.close_unknown_tab",
                "The save target is not open",
                false,
            );
            return true;
        };
        let tab_id = tab.id.clone();
        let Some(document) = self.editor_documents.get_mut(&tab_id) else {
            self.set_error(
                "file.close_save_rejected",
                "The save target has no document state",
                false,
            );
            return true;
        };
        document.contents_utf8 = Some(payload.contents_utf8.clone());
        document.dirty = true;
        self.sync_file_tab_dirty(&tab_id);
        self.sync_active_editor_document();
        let Some(context) = self.worker_context.clone() else {
            self.set_error(
                "file.close_save_worker_unavailable",
                "The file stayed open because its pending save could not start",
                true,
            );
            return true;
        };
        let path = payload.path;
        let contents = payload.contents_utf8;
        let expected_modified_at = payload.expected_modified_at_unix_ms;
        let mut editor = self
            .editor_documents
            .get(&tab_id)
            .cloned()
            .expect("the close-save document was validated");
        match thread::Builder::new()
            .name("herdr-core-file-save-close".to_owned())
            .spawn(move || {
                let result = files::save(
                    &mut editor,
                    Path::new(&path),
                    contents.clone(),
                    expected_modified_at,
                );
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard
                        .ingest_file_save_then_close_result(tab_id, path, contents, editor, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            }) {
            Ok(_) => true,
            Err(error) => {
                self.set_error(
                    "file.close_save_worker_failed",
                    format!(
                        "The file stayed open because its pending save could not start: {error}"
                    ),
                    true,
                );
                true
            }
        }
    }

    pub(super) fn prepare_file_tab(
        &self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
    ) -> Result<PreparedFileTab, String> {
        if let Some(tab_id) = self.snapshot.editor.tabs.iter().find_map(|tab| {
            (tab.kind == EditorTabKind::File
                && tab.workspace_id == workspace_id
                && tab.checkout_id == checkout_id
                && tab.path == path)
                .then(|| tab.id.clone())
        }) {
            return Ok(PreparedFileTab::Open(tab_id));
        }
        files::open(Path::new(path)).map(|document| PreparedFileTab::Read {
            tab_id: Self::file_tab_id(workspace_id, checkout_id, path),
            document,
        })
    }

    pub(super) fn show_file_tab(
        &mut self,
        prepared: PreparedFileTab,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
    ) {
        let tab_id = match prepared {
            PreparedFileTab::Open(tab_id) => tab_id,
            PreparedFileTab::Read { tab_id, document } => {
                self.editor_documents.insert(tab_id.clone(), document);
                self.snapshot.editor.tabs.push(EditorTabSnapshot {
                    id: tab_id.clone(),
                    workspace_id: workspace_id.to_owned(),
                    checkout_id: checkout_id.to_owned(),
                    path: path.to_owned(),
                    label: Path::new(path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .filter(|name| !name.is_empty())
                        .unwrap_or(path)
                        .to_owned(),
                    kind: EditorTabKind::File,
                    diff_committed: None,
                    markdown_preview: true,
                    wrap: false,
                    dirty: false,
                });
                // A new file tab takes a slot at the end of the strip.
                self.rebuild_tab_strips();
                tab_id
            }
        };
        if let Err(message) = self.activate_editor_tab(&tab_id) {
            self.set_error("file.focus_failed", message, false);
        }
        self.snapshot.ui_state.selected_path = Some(path.to_owned());
    }

    pub(super) fn focus_editor_tab_context(&mut self, tab_id: &str) -> Result<(), String> {
        let tab = self
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .cloned()
            .ok_or_else(|| format!("File tab {tab_id} is not open"))?;
        let checkout = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == tab.workspace_id)
            .and_then(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == tab.checkout_id)
            })
            .cloned()
            .ok_or_else(|| {
                "The editor tab's project or checkout is no longer available".to_owned()
            })?;
        self.activate_editor_tab(tab_id)?;
        let pane_id = checkout.active_tab_id.as_deref().and_then(|id| {
            let first = checkout
                .tabs
                .iter()
                .find(|tab| tab.id.as_deref() == Some(id))
                .and_then(|tab| tab.panes.first())
                .map(|pane| pane.id.clone());
            self.tab_focus_pane_id(id, first)
        });
        self.snapshot.navigator.focused_workspace_id = Some(tab.workspace_id);
        self.snapshot.navigator.focused_checkout_id = Some(tab.checkout_id);
        self.snapshot.navigator.root_path = Some(checkout.path);
        self.select_terminal_pane(pane_id);
        self.operator_focused_pane_id = None;
        self.refresh_pane_read_state();
        self.persist_current_ui_state();
        Ok(())
    }

    pub(super) fn open_file_tab(&mut self, workspace_id: &str, checkout_id: &str, path: &str) {
        match self.prepare_file_tab(workspace_id, checkout_id, path) {
            Ok(prepared) => self.show_file_tab(prepared, workspace_id, checkout_id, path),
            Err(message) => self.set_error("file.open_failed", message, true),
        }
    }

    pub(super) fn diff_tab_id(
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        committed: bool,
    ) -> String {
        let scope = if committed { "committed" } else { "working" };
        format!("diff:{workspace_id}:{checkout_id}:{scope}:{path}")
    }

    pub(super) fn show_diff_tab(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        committed: bool,
    ) {
        let tab_id = Self::diff_tab_id(workspace_id, checkout_id, path, committed);
        if !self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
            let name = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or(path);
            let scope = if committed {
                "branch diff"
            } else {
                "working diff"
            };
            self.snapshot.editor.tabs.push(EditorTabSnapshot {
                id: tab_id.clone(),
                workspace_id: workspace_id.to_owned(),
                checkout_id: checkout_id.to_owned(),
                path: path.to_owned(),
                label: format!("{name} ({scope})"),
                kind: EditorTabKind::Diff,
                diff_committed: Some(committed),
                markdown_preview: true,
                wrap: false,
                dirty: false,
            });
            self.rebuild_tab_strips();
        }
        if let Err(message) = self.activate_editor_tab(&tab_id) {
            self.set_error("diff.focus_failed", message, false);
        }
    }

    pub(super) fn next_recent_closed_key(&mut self) -> String {
        self.recent_closed_sequence += 1;
        format!(
            "reopen-{}-{}",
            unix_milliseconds(),
            self.recent_closed_sequence
        )
    }

    pub(super) fn sync_recent_closed_snapshot(&mut self) {
        self.snapshot.recent_closed.count = self.recent_closed.len();
        self.snapshot.recent_closed.top_label = self
            .recent_closed
            .back()
            .map(|item| item.label().to_owned());
        self.snapshot.recent_closed.restoring = self.reopen_in_flight.is_some();
    }

    pub(super) fn push_recent_closed(&mut self, item: ClosedItem) {
        push_bounded(&mut self.recent_closed, item);
        self.sync_recent_closed_snapshot();
    }

    pub(super) fn consume_recent_closed(&mut self, key: &str) {
        if let Some(index) = self.recent_closed.iter().position(|item| item.key() == key) {
            self.recent_closed.remove(index);
        }
    }

    pub(super) fn close_file_tab_now(&mut self, tab_id: &str) -> bool {
        let Some(index) = self
            .snapshot
            .editor
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id)
        else {
            self.set_error(
                "file.close_unknown_tab",
                format!("File tab {tab_id} is not open"),
                false,
            );
            return true;
        };
        let was_active = self.snapshot.editor.active_tab_id.as_deref() == Some(tab_id);
        let closed_tab = self.snapshot.editor.tabs.remove(index);
        if closed_tab.kind == EditorTabKind::File {
            let key = self.next_recent_closed_key();
            let checkout_path = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .find(|checkout| checkout.id == closed_tab.checkout_id)
                .map(|checkout| checkout.path.clone())
                .unwrap_or_default();
            self.push_recent_closed(ClosedItem::File {
                key,
                workspace_id: closed_tab.workspace_id.clone(),
                checkout_id: closed_tab.checkout_id.clone(),
                checkout_path,
                path: closed_tab.path.clone(),
                label: closed_tab.label.clone(),
            });
        }
        self.rebuild_tab_strips();
        self.editor_documents.remove(tab_id);
        self.editor_tab_history.retain(|known| known != tab_id);
        if was_active {
            self.snapshot.editor.active_tab_id = None;
            self.snapshot.editor.document = None;
            if closed_tab.kind == EditorTabKind::Diff {
                self.snapshot.changes.selected_path = None;
                self.snapshot.changes.diff = None;
            }
            while let Some(previous_id) = self.editor_tab_history.pop() {
                if self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .any(|tab| tab.id == previous_id)
                {
                    if let Err(message) = self.activate_editor_tab(&previous_id) {
                        self.set_error("editor.focus_failed", message, false);
                    }
                    break;
                }
            }
        }
        self.persist_current_ui_state();
        true
    }

    pub(super) fn set_reopen_notices(&mut self, notices: Vec<live::ReopenNotice>) {
        self.snapshot.recent_closed.notices = notices
            .into_iter()
            .map(|notice| crate::model::RecentClosedNoticeSnapshot {
                pane_id: notice.pane_id,
                message: notice.message,
            })
            .collect();
    }

    pub(super) fn close_context(&self, tab: &TabSnapshot) -> Option<ClosedContext> {
        let tab_id = tab.id.as_ref()?;
        let project_workspace_id = tab.workspace_id.as_ref()?;
        let checkout_id = tab.checkout_id.as_ref()?;
        let (session_workspace_id, tab_order) = self
            .herdr_workspace_tab_order
            .iter()
            .find(|(_, ids)| ids.iter().any(|id| id == tab_id))?;
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.id == *project_workspace_id)
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == *checkout_id)
                    .map(|checkout| ClosedContext {
                        workspace_id: session_workspace_id.clone(),
                        workspace_label: workspace.label.clone(),
                        workspace_ids_before_close: self
                            .snapshot
                            .navigator
                            .workspaces
                            .iter()
                            .flat_map(|workspace| workspace.session_workspace_ids.iter().cloned())
                            .collect(),
                        tab_ids_before_close: tab_order.clone(),
                        pane_ids_before_close: tab
                            .panes
                            .iter()
                            .map(|pane| pane.id.clone())
                            .collect(),
                        checkout_id: checkout_id.clone(),
                        checkout_path: checkout.path.clone(),
                        tab_id: tab_id.clone(),
                        tab_label: tab.label.clone().unwrap_or_else(|| "Tab".into()),
                        tab_index: tab_order.iter().position(|id| id == tab_id).unwrap_or(0),
                    })
            })
    }

    pub(super) fn closed_panes(&self, tab: &TabSnapshot) -> Vec<ClosedPane> {
        tab.panes
            .iter()
            .map(|pane| {
                let agent = self
                    .snapshot
                    .navigator
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == pane.id)
                    .map(|agent| ClosedAgent {
                        kind: agent.agent_kind.clone(),
                        session_id: agent.session_id.clone(),
                    });
                ClosedPane {
                    pane_id: pane.id.clone(),
                    label: pane
                        .herdr_label
                        .clone()
                        .or_else(|| pane.terminal_title.clone()),
                    cwd: pane.cwd.clone(),
                    agent,
                    browser: matches!(
                        pane.content,
                        crate::pane_content::PaneContent::Browser { .. }
                    ),
                }
            })
            .collect()
    }

    pub(super) fn start_close_capture(
        &mut self,
        target: live::CloseCaptureTarget,
        tab: TabSnapshot,
    ) -> bool {
        let target_id = match &target {
            live::CloseCaptureTarget::Pane { pane_id } => pane_id,
            live::CloseCaptureTarget::Tab { tab_id } => tab_id,
        }
        .clone();
        if !self.close_captures_in_flight.insert(target_id.clone()) {
            return false;
        }
        let Some(context) = self.close_context(&tab) else {
            self.close_captures_in_flight.remove(&target_id);
            self.panes_closing.remove(&target_id);
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: None,
                message:
                    "The closed item could not be recorded because its local context is incomplete"
                        .into(),
            }]);
            return true;
        };
        let Some(live_context) = self.live.as_ref().cloned() else {
            self.close_captures_in_flight.remove(&target_id);
            self.panes_closing.remove(&target_id);
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: None,
                message: "Closing this item requires the local Herdr connection".into(),
            }]);
            return true;
        };
        let request = live::CloseCaptureRequest {
            key: self.next_recent_closed_key(),
            context,
            panes: self.closed_panes(&tab),
            target,
        };
        self.close_capture_order.push_back(request.key.clone());
        if let Err(message) = live::spawn_close_capture(live_context, request.clone()) {
            self.close_captures_in_flight.remove(&target_id);
            self.panes_closing.remove(&target_id);
            self.close_capture_order
                .retain(|pending| pending != &request.key);
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: None,
                message: format!("The close worker could not start: {message}"),
            }]);
        }
        true
    }

    pub(crate) fn ingest_close_capture_result(
        &mut self,
        request: &live::CloseCaptureRequest,
        result: Result<live::CloseCaptureOutcome, String>,
    ) -> (bool, Vec<live::CloseEffectRequest>) {
        self.close_capture_results
            .insert(request.key.clone(), (request.clone(), result));
        let mut effects = Vec::new();
        while let Some(key) = self.close_capture_order.front().cloned() {
            let Some((request, result)) = self.close_capture_results.remove(&key) else {
                break;
            };
            self.close_capture_order.pop_front();
            match result {
                Ok(outcome) => {
                    if let Some(item) = outcome.item {
                        self.push_recent_closed(item);
                    }
                    self.snapshot.recent_closed.notices.clear();
                    self.push_diagnostic(
                        "recent_closed.reserved",
                        format!(
                            "Reserved user close {} before its external effect",
                            request.key
                        ),
                    );
                    effects.push(live::CloseEffectRequest {
                        key: request.key,
                        target: request.target,
                    });
                }
                Err(message) => {
                    let target_id = match &request.target {
                        live::CloseCaptureTarget::Pane { pane_id } => pane_id,
                        live::CloseCaptureTarget::Tab { tab_id } => tab_id,
                    };
                    self.close_captures_in_flight.remove(target_id);
                    if let live::CloseCaptureTarget::Pane { pane_id } = &request.target {
                        self.panes_closing.remove(pane_id);
                    }
                    self.set_reopen_notices(vec![live::ReopenNotice {
                        pane_id: match &request.target {
                            live::CloseCaptureTarget::Pane { pane_id } => Some(pane_id.clone()),
                            live::CloseCaptureTarget::Tab { .. } => None,
                        },
                        message: format!("The item was not closed: {message}"),
                    }]);
                    self.push_diagnostic(
                        "recent_closed.capture_failed",
                        format!("{}: {message}", request.key),
                    );
                }
            }
        }
        self.sync_recent_closed_snapshot();
        (true, effects)
    }

    pub(crate) fn ingest_close_effect_result(
        &mut self,
        request: &live::CloseEffectRequest,
        result: Result<(), hide_herdr_client::ApiError>,
    ) -> bool {
        let target_id = match &request.target {
            live::CloseCaptureTarget::Pane { pane_id } => pane_id,
            live::CloseCaptureTarget::Tab { tab_id } => tab_id,
        };
        self.close_captures_in_flight.remove(target_id);
        match result {
            Ok(()) => {
                self.snapshot.recent_closed.notices.clear();
                self.push_diagnostic(
                    "recent_closed.captured",
                    format!("Captured user close {}", request.key),
                );
            }
            Err(hide_herdr_client::ApiError::Remote { code, message }) => {
                self.consume_recent_closed(&request.key);
                if let live::CloseCaptureTarget::Pane { pane_id } = &request.target {
                    self.panes_closing.remove(pane_id);
                }
                self.set_reopen_notices(vec![live::ReopenNotice {
                    pane_id: match &request.target {
                        live::CloseCaptureTarget::Pane { pane_id } => Some(pane_id.clone()),
                        live::CloseCaptureTarget::Tab { .. } => None,
                    },
                    message: format!("The item was not closed: {code}: {message}"),
                }]);
                self.push_diagnostic(
                    "recent_closed.close_failed",
                    format!("{}: {code}: {message}", request.key),
                );
            }
            Err(error) => {
                if let live::CloseCaptureTarget::Pane { pane_id } = &request.target {
                    self.panes_closing.remove(pane_id);
                }
                self.set_reopen_notices(vec![live::ReopenNotice {
                    pane_id: match &request.target {
                        live::CloseCaptureTarget::Pane { pane_id } => Some(pane_id.clone()),
                        live::CloseCaptureTarget::Tab { .. } => None,
                    },
                    message: format!(
                        "Hide could not confirm whether the item closed; its reopen entry was kept: {error}"
                    ),
                }]);
                self.push_diagnostic(
                    "recent_closed.close_unconfirmed",
                    format!("{}: {error}", request.key),
                );
            }
        }
        self.sync_recent_closed_snapshot();
        true
    }

    pub(super) fn reopen_closed(&mut self) -> bool {
        if self.reopen_in_flight.is_some() {
            return false;
        }
        let Some(item) = self.recent_closed.back().cloned() else {
            return false;
        };
        if let ClosedItem::File {
            workspace_id,
            checkout_id,
            path,
            ..
        } = &item
            && let Some(tab_id) = self.snapshot.editor.tabs.iter().find_map(|tab| {
                (tab.kind == EditorTabKind::File
                    && tab.workspace_id == *workspace_id
                    && tab.checkout_id == *checkout_id
                    && tab.path == *path)
                    .then(|| tab.id.clone())
            })
        {
            self.consume_recent_closed(item.key());
            match self.focus_editor_tab_context(&tab_id) {
                Ok(()) => self.snapshot.recent_closed.notices.clear(),
                Err(message) => self.set_reopen_notices(vec![live::ReopenNotice {
                    pane_id: None,
                    message: format!(
                        "The file reopened, but its project context was unavailable: {message}"
                    ),
                }]),
            }
            self.sync_recent_closed_snapshot();
            return true;
        }
        let (workspace_exists, tab_exists, fallback_pane_id) = match &item {
            ClosedItem::Pane {
                context, placement, ..
            } => {
                let tab = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .find(|tab| tab.id.as_deref() == Some(context.tab_id.as_str()));
                let fallback = tab.and_then(|tab| {
                    placement
                        .neighbor_pane_id
                        .as_ref()
                        .filter(|neighbor| tab.panes.iter().any(|pane| pane.id == **neighbor))
                        .cloned()
                        .or_else(|| tab.panes.first().map(|pane| pane.id.clone()))
                });
                (
                    self.snapshot.navigator.workspaces.iter().any(|workspace| {
                        workspace
                            .session_workspace_ids
                            .contains(&context.workspace_id)
                    }),
                    tab.is_some(),
                    fallback,
                )
            }
            ClosedItem::Tab { context, .. } => (
                self.snapshot.navigator.workspaces.iter().any(|workspace| {
                    workspace
                        .session_workspace_ids
                        .contains(&context.workspace_id)
                }),
                false,
                None,
            ),
            ClosedItem::File { .. } => (true, true, None),
        };
        let key = item.key().to_owned();
        self.reopen_in_flight = Some(key.clone());
        self.set_reopen_notices(vec![live::ReopenNotice {
            pane_id: fallback_pane_id.clone(),
            message: format!("Reopening {}…", item.label()),
        }]);
        self.sync_recent_closed_snapshot();
        let request = live::ReopenRequest {
            item,
            workspace_exists,
            tab_exists,
            fallback_pane_id,
        };
        let spawned = if matches!(&request.item, ClosedItem::File { .. }) {
            self.worker_context
                .as_ref()
                .cloned()
                .ok_or_else(|| "the file worker is unavailable".to_owned())
                .and_then(|worker| {
                    live::spawn_file_reopen(worker.runtime, worker.notifier, request)
                })
        } else {
            self.live
                .as_ref()
                .cloned()
                .ok_or_else(|| {
                    "the local Herdr connection is unavailable; retry when it returns".to_owned()
                })
                .and_then(|context| live::spawn_reopen(context, request))
        };
        if let Err(message) = spawned {
            self.reopen_in_flight = None;
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: None,
                message: format!("Reopen could not start; retry is available: {message}"),
            }]);
            self.sync_recent_closed_snapshot();
        }
        true
    }

    pub fn ingest_reopen_result(
        &mut self,
        request: &live::ReopenRequest,
        result: Result<live::FileReopenResultOrHerdr, String>,
    ) -> bool {
        let key = request.item.key();
        if self.reopen_in_flight.as_deref() != Some(key) {
            return false;
        }
        self.reopen_in_flight = None;
        match result {
            Err(message) => {
                self.set_reopen_notices(vec![live::ReopenNotice {
                    pane_id: request.fallback_pane_id.clone(),
                    message: format!("Reopen failed; retry is available: {message}"),
                }]);
            }
            Ok(live::FileReopenResultOrHerdr::File(file)) => {
                match file {
                    live::FileReopenResult::Opened(document) => {
                        if let ClosedItem::File {
                            workspace_id,
                            checkout_id,
                            path,
                            ..
                        } = &request.item
                        {
                            let tab_id = Self::file_tab_id(workspace_id, checkout_id, path);
                            let prepared = PreparedFileTab::Read {
                                tab_id: tab_id.clone(),
                                document,
                            };
                            self.show_file_tab(prepared, workspace_id, checkout_id, path);
                            if let Err(message) = self.focus_editor_tab_context(&tab_id) {
                                self.set_reopen_notices(vec![live::ReopenNotice {
                                pane_id: None,
                                message: format!("The file reopened, but its project context was unavailable: {message}"),
                            }]);
                            } else {
                                self.snapshot.recent_closed.notices.clear();
                            }
                            self.consume_recent_closed(key);
                        }
                    }
                    live::FileReopenResult::Missing => {
                        self.consume_recent_closed(key);
                        self.set_reopen_notices(vec![live::ReopenNotice {
                        pane_id: None,
                        message: "The file was deleted. Restore it from Finder Trash to open it again.".into(),
                    }]);
                    }
                    live::FileReopenResult::Failed(message) => {
                        self.set_reopen_notices(vec![live::ReopenNotice {
                            pane_id: None,
                            message: format!(
                                "The file could not be reopened; retry is available: {message}"
                            ),
                        }]);
                    }
                }
            }
            Ok(live::FileReopenResultOrHerdr::Herdr(outcome)) => {
                if outcome.consumed {
                    self.consume_recent_closed(key);
                }
                if let Some(pane_id) = outcome.focused_pane_id {
                    self.snapshot.terminal.pane_id = Some(pane_id.clone());
                    self.snapshot.focused.surface = Surface::Terminal;
                    self.snapshot.focused.pane_id = Some(pane_id.clone());
                    self.snapshot.ui_state.selected_pane_id = Some(pane_id);
                    self.deactivate_editor_tab();
                }
                self.set_reopen_notices(outcome.notices);
            }
        }
        self.sync_recent_closed_snapshot();
        true
    }
}
