use super::*;

pub(super) enum PreparedFileTab {
    /// The file already has a tab; showing it is a focus.
    Open(String),
    /// The file was read and needs a tab of its own.
    Read {
        tab_id: String,
        document: Box<EditorDocumentSnapshot>,
        place: crate::files::DocumentPlace,
    },
    /// The file is being read off the runtime lock; the tab shows when the
    /// read lands.
    Reading {
        root: crate::files::DocumentRoot,
        channel: std::sync::Arc<dyn crate::host_access::HostChannel>,
    },
}

impl Runtime {
    pub(super) fn file_tab_id(workspace_id: &str, checkout_id: &str, path: &str) -> String {
        format!("file:{workspace_id}:{checkout_id}:{path}")
    }

    /// The id a new tab for `path` takes. A renamed or moved tab keeps the id
    /// its old path gave it, so a new file later opened at that old path
    /// would collide with it and be dropped as already open; that tab gets
    /// the first free numbered id instead (B9, B18).
    pub(super) fn new_file_tab_id(
        &self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
    ) -> String {
        let base = Self::file_tab_id(workspace_id, checkout_id, path);
        let taken = |id: &str| {
            self.snapshot
                .editor
                .tabs
                .iter()
                .any(|tab| tab.id == id && tab.path != path)
        };
        if !taken(&base) {
            return base;
        }
        (2..)
            .map(|n| format!("{base}#{n}"))
            .find(|id| !taken(id))
            .expect("an unbounded range yields a free id")
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
            // A restored tab whose file could not be read has no document;
            // showing it shows why (B20).
            EditorTabKind::File if tab.unavailable_reason.is_some() => None,
            EditorTabKind::File => {
                let document = self
                    .editor_documents
                    .get(tab_id)
                    .ok_or_else(|| format!("File tab {tab_id} has no document state"))?;
                // With View areas the documents ride their own snapshot
                // section, one per visible display (PRD S7 A4), so a
                // keystroke re-sends one document, not the editor, and
                // the editor carries no copy.
                (!self.separate_view_areas()).then(|| EditorDocumentSnapshot::clone(document))
            }
            EditorTabKind::Diff => {
                if tab.diff_committed.is_none() {
                    return Err(format!("Diff tab {tab_id} has no comparison scope"));
                }
                None
            }
            EditorTabKind::Session | EditorTabKind::Memory => None,
        };
        if let Some(active_id) = self.snapshot.editor.active_tab_id.as_deref()
            && active_id != tab_id
        {
            self.editor_tab_history.retain(|known| known != active_id);
            self.editor_tab_history.push(active_id.to_owned());
        }
        self.snapshot.editor.active_tab_id = Some(tab_id.to_owned());
        self.snapshot.editor.archive_detail = None;
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
                    let changes = self.snapshot.changes.edit();
                    changes.diff = None;
                    changes.selected_path = Some(tab.path);
                    changes.selected_committed = committed;
                }
                self.snapshot.editor.document = None;
                self.snapshot.ui_state.selected_path = None;
            }
            EditorTabKind::Session | EditorTabKind::Memory => {
                self.snapshot.editor.document = None;
                self.snapshot.editor.archive_detail = self.archive_documents.get(tab_id).cloned();
                self.snapshot.ui_state.selected_path = None;
            }
        }
        Ok(())
    }

    pub(super) fn deactivate_editor_tab(&mut self) {
        self.snapshot.editor.active_tab_id = None;
        self.snapshot.editor.document = None;
        self.snapshot.editor.archive_detail = None;
        self.editor_tab_history.clear();
    }

    pub(super) fn sync_active_editor_document(&mut self) {
        self.snapshot.editor.document = if self.separate_view_areas() {
            None
        } else {
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
                            .map(|_| EditorDocumentSnapshot::clone(document))
                    })
                })
        };
        self.snapshot.editor.archive_detail = self
            .snapshot
            .editor
            .active_tab_id
            .as_deref()
            .and_then(|tab_id| self.archive_documents.get(tab_id).cloned());
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

    /// A close that waits on its save: the tab closes when the save lands
    /// and the draft matches what was written.
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
        self.request_file_save(payload, true)
    }

    pub(super) fn prepare_file_tab(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
    ) -> Result<PreparedFileTab, String> {
        // A tab that could not read its file reads it again on the next open.
        if let Some(tab_id) = self.snapshot.editor.tabs.iter().find_map(|tab| {
            (tab.kind == EditorTabKind::File
                && tab.workspace_id == workspace_id
                && tab.checkout_id == checkout_id
                && tab.path == path
                && tab.unavailable_reason.is_none())
            .then(|| tab.id.clone())
        }) {
            return Ok(PreparedFileTab::Open(tab_id));
        }
        let (root, channel) = self.document_source(workspace_id, checkout_id)?;
        Ok(PreparedFileTab::Reading { root, channel })
    }

    /// Shows a prepared file tab. `preview` is what the click asked for: a
    /// single click wants the checkout's preview slot, every other entry
    /// point wants an ordinary tab (PRD editor-preview-tab D-09). A file that
    /// already has a tab is focused, and an ordinary open of the file that
    /// currently holds the preview slot promotes it where it sits (D-10, B4).
    pub(super) fn show_file_tab(
        &mut self,
        prepared: PreparedFileTab,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        preview: bool,
    ) {
        if self.separate_view_areas() {
            self.show_file_in_view(prepared, workspace_id, checkout_id, path, preview, false);
            return;
        }
        let tab_id = match prepared {
            PreparedFileTab::Open(tab_id) => {
                if !preview {
                    self.promote_editor_tab(&tab_id);
                }
                tab_id
            }
            PreparedFileTab::Reading { root, channel } => {
                self.start_document_open(
                    root,
                    channel,
                    documents::OpenRequestFields {
                        workspace_id: workspace_id.to_owned(),
                        checkout_id: checkout_id.to_owned(),
                        path: path.to_owned(),
                        preview,
                        reload: false,
                        reveal: None,
                        restore: false,
                        placement: None,
                    },
                );
                self.snapshot.ui_state.selected_path = Some(path.to_owned());
                return;
            }
            read @ PreparedFileTab::Read { .. } => {
                match self.insert_file_tab(read, workspace_id, checkout_id, path, preview) {
                    Some(tab_id) => tab_id,
                    None => return,
                }
            }
        };
        if let Err(message) = self.activate_editor_tab(&tab_id) {
            self.set_error("file.focus_failed", message, false);
        }
        self.snapshot.ui_state.selected_path = Some(path.to_owned());
    }

    /// Adds a read document as a tab in its checkout's strip without showing
    /// it. Returns the tab id, or `None` for a preparation that holds no
    /// document.
    pub(super) fn insert_file_tab(
        &mut self,
        prepared: PreparedFileTab,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        preview: bool,
    ) -> Option<String> {
        let PreparedFileTab::Read {
            tab_id,
            document,
            place,
        } = prepared
        else {
            return None;
        };
        self.editor_documents
            .insert(tab_id.clone(), Edited::new(*document));
        self.document_places.insert(tab_id.clone(), place);
        let tab = EditorTabSnapshot {
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
            markdown_live: true,
            wrap: false,
            dirty: false,
            preview,
            unavailable_reason: None,
        };
        self.place_editor_tab(tab);
        Some(tab_id)
    }

    /// Puts a new editor tab into the strip.
    ///
    /// An ordinary tab takes a slot at the end. A preview tab takes the
    /// checkout's preview slot: when the checkout already has a preview tab
    /// that is clean, the new tab replaces it in place, inheriting its strip
    /// slot, and the replaced tab's document, Markdown mode and wrap state
    /// are dropped without a Recent Closed entry (D-04). A dirty preview tab
    /// is never replaced: it is promoted where it sits and the new preview
    /// opens beside it (D-05). File and diff tabs share the one slot (D-02).
    ///
    /// With View areas the preview slots are the areas' (one preview display
    /// each, PRD S7 D-02), so a new tab only joins the editor, and the area
    /// retires the document its preview display showed (`view_areas.rs`).
    pub(super) fn place_editor_tab(&mut self, tab: EditorTabSnapshot) {
        if self.separate_view_areas() {
            self.snapshot.editor.tabs.push(tab);
            self.rebuild_tab_strips();
            return;
        }
        let replaced = tab.preview.then(|| {
            self.snapshot
                .editor
                .tabs
                .iter()
                .position(|held| {
                    held.preview
                        && held.workspace_id == tab.workspace_id
                        && held.checkout_id == tab.checkout_id
                })
                .map(|index| (index, self.snapshot.editor.tabs[index].dirty))
        });
        match replaced.flatten() {
            Some((index, false)) => {
                let old = self.retire_editor_tab(index);
                let old_entry = StripTabSnapshot::editor(&old).id;
                let new_entry = StripTabSnapshot::editor(&tab).id;
                for order in self.checkout_tab_order.values_mut() {
                    for entry in order.iter_mut().filter(|entry| **entry == old_entry) {
                        *entry = new_entry.clone();
                    }
                }
                for pending in self.pending_tab_move.values_mut() {
                    for entry in pending
                        .desired
                        .iter_mut()
                        .filter(|entry| **entry == old_entry)
                    {
                        *entry = new_entry.clone();
                    }
                }
                self.push_diagnostic(
                    "editor.preview_replaced",
                    format!("Preview tab {} replaced by {}", old.id, tab.id),
                );
                self.snapshot.editor.tabs.insert(index, tab);
            }
            Some((index, true)) => {
                let dirty = &mut self.snapshot.editor.tabs[index];
                dirty.preview = false;
                let kept = dirty.id.clone();
                self.push_diagnostic(
                    "editor.preview_kept_dirty",
                    format!("Dirty preview tab {kept} kept and promoted"),
                );
                self.snapshot.editor.tabs.push(tab);
            }
            None => self.snapshot.editor.tabs.push(tab),
        }
        self.rebuild_tab_strips();
    }

    /// Takes an editor tab out of the runtime: its snapshot entry, its
    /// document, its place in the focus history, and, when it was the active
    /// diff, the Changes selection it was showing. The caller decides what
    /// the removal means: a close records it for reopening, a preview
    /// replacement does not.
    pub(super) fn retire_editor_tab(&mut self, index: usize) -> EditorTabSnapshot {
        let tab = self.snapshot.editor.tabs.remove(index);
        let was_active = self.snapshot.editor.active_tab_id.as_deref() == Some(tab.id.as_str());
        self.editor_documents.remove(&tab.id);
        self.forget_document(&tab.id);
        self.archive_documents.remove(&tab.id);
        self.editor_tab_history.retain(|known| known != &tab.id);
        if was_active {
            self.snapshot.editor.active_tab_id = None;
            self.snapshot.editor.document = None;
            self.snapshot.editor.archive_detail = None;
            if tab.kind == EditorTabKind::Diff {
                let changes = self.snapshot.changes.edit();
                changes.selected_path = None;
                changes.diff = None;
            }
        }
        tab
    }

    /// Closes every file and diff tab of a removed device without saving and
    /// drops its closed items. A dirty tab's draft stays in the shell's own
    /// store, where a draft no tab stands for is offered for export (B26).
    pub(super) fn retire_device_editor_tabs(&mut self, device_id: &str) {
        let scope = format!("remote:{device_id}:");
        while let Some(index) = self
            .snapshot
            .editor
            .tabs
            .iter()
            .position(|tab| tab.checkout_id.starts_with(&scope))
        {
            self.retire_editor_tab(index);
        }
        self.recent_closed
            .retain(|item| item.device_id() != device_id);
        self.sync_recent_closed_snapshot();
        self.forget_device_opens(&scope);
    }

    /// Makes a preview tab an ordinary tab in the same slot. Returns whether
    /// anything changed: promoting a tab that is already ordinary is a
    /// no-op, and a tab that is not open is refused with a reason. With View
    /// areas every preview display of the document is kept open instead,
    /// and the tab's flag follows them.
    pub(super) fn promote_editor_tab(&mut self, tab_id: &str) -> bool {
        if !self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
            self.set_error(
                "editor.keep_open_unknown_tab",
                format!("Editor tab {tab_id} is not open"),
                false,
            );
            return true;
        }
        if self.separate_view_areas() {
            return self.promote_view_displays(tab_id);
        }
        let Some(tab) = self
            .snapshot
            .editor
            .tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
        else {
            return false;
        };
        if !tab.preview {
            return false;
        }
        tab.preview = false;
        self.push_diagnostic("editor.preview_promoted", format!("Tab {tab_id} kept open"));
        self.rebuild_tab_strips();
        true
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
        let (device_id, checkout) = self
            .catalog_checkout(&tab.workspace_id, &tab.checkout_id)
            .map(|(workspace, checkout)| (workspace.device_id.clone(), checkout.clone()))
            .ok_or_else(|| {
                "The editor tab's project or checkout is no longer available".to_owned()
            })?;
        // A device's focus is its Herdr session's, which the core follows;
        // showing one of its files moves nothing on this machine.
        if device_id != workspace::LOCAL_DEVICE_ID {
            return self.activate_editor_tab(tab_id);
        }
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
        self.sync_changes_root_path();
        self.select_terminal_pane(pane_id);
        self.operator_focused_pane_id = None;
        self.refresh_pane_read_state();
        self.persist_current_ui_state();
        Ok(())
    }

    pub(super) fn open_file_tab(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        preview: bool,
    ) {
        match self.prepare_file_tab(workspace_id, checkout_id, path) {
            Ok(prepared) => self.show_file_tab(prepared, workspace_id, checkout_id, path, preview),
            // The read failed before anything moved, so an existing preview
            // tab keeps its slot (B15).
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
        preview: bool,
    ) {
        let tab_id = self.insert_diff_tab(workspace_id, checkout_id, path, committed, preview);
        if let Err(message) = self.activate_editor_tab(&tab_id) {
            self.set_error("diff.focus_failed", message, false);
        }
    }

    /// Puts a diff tab in its checkout's strip without showing it, or keeps
    /// the one already there (promoted unless a preview was asked for).
    pub(super) fn insert_diff_tab(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        committed: bool,
        preview: bool,
    ) -> String {
        let tab_id = Self::diff_tab_id(workspace_id, checkout_id, path, committed);
        if self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
            if !preview {
                self.promote_editor_tab(&tab_id);
            }
            return tab_id;
        }
        self.place_editor_tab(EditorTabSnapshot {
            id: tab_id.clone(),
            workspace_id: workspace_id.to_owned(),
            checkout_id: checkout_id.to_owned(),
            path: path.to_owned(),
            label: diff_label(path, committed),
            kind: EditorTabKind::Diff,
            diff_committed: Some(committed),
            markdown_live: true,
            wrap: false,
            dirty: false,
            preview,
            unavailable_reason: None,
        });
        tab_id
    }

    pub(super) fn show_archive_tab(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        detail: ArchiveDetailSnapshot,
        preview: bool,
    ) {
        let kind = if detail.kind == "memory" {
            EditorTabKind::Memory
        } else {
            EditorTabKind::Session
        };
        let tab_id = format!(
            "{}:{}:{}:{}",
            detail.kind, workspace_id, checkout_id, detail.id
        );
        self.archive_documents
            .insert(tab_id.clone(), detail.clone());
        if self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
            if !preview {
                self.promote_editor_tab(&tab_id);
            }
        } else {
            self.place_editor_tab(EditorTabSnapshot {
                id: tab_id.clone(),
                workspace_id: workspace_id.to_owned(),
                checkout_id: checkout_id.to_owned(),
                path: detail.id,
                label: detail.title,
                kind,
                diff_committed: None,
                markdown_live: false,
                wrap: true,
                dirty: false,
                preview,
                unavailable_reason: None,
            });
        }
        if let Err(message) = self.activate_editor_tab(&tab_id) {
            self.set_error("archive.focus_failed", message, false);
        }
    }

    pub(super) fn close_memory_archive_tabs(&mut self) {
        let tab_ids = self
            .snapshot
            .editor
            .tabs
            .iter()
            .filter(|tab| tab.kind == EditorTabKind::Memory)
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        for tab_id in tab_ids {
            self.close_file_tab_now(&tab_id);
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

    /// The device whose closed items the reopen command and the snapshot's
    /// `recent_closed` speak for: the one in front.
    fn reopen_device(&self) -> &str {
        self.snapshot
            .navigator
            .focused_device_id
            .as_deref()
            .unwrap_or(workspace::LOCAL_DEVICE_ID)
    }

    /// The newest closed item the device in front can reopen. One stack keeps
    /// the twenty most recent closes of every device, and reopen never takes
    /// another device's item, so a reopen on one device cannot restore work
    /// on another.
    fn reopenable(&self) -> Option<&ClosedItem> {
        let device = self.reopen_device();
        self.recent_closed
            .iter()
            .rev()
            .find(|item| item.device_id() == device)
    }

    pub(super) fn sync_recent_closed_snapshot(&mut self) {
        // This machine's close reservations block only this machine's reopen.
        let local = self.reopen_device() == workspace::LOCAL_DEVICE_ID;
        let device = self.reopen_device().to_owned();
        self.snapshot.recent_closed.count = self
            .recent_closed
            .iter()
            .filter(|item| item.device_id() == device)
            .count();
        self.snapshot.recent_closed.top_label =
            self.reopenable().map(|item| item.label().to_owned());
        self.snapshot.recent_closed.restoring = self.reopen_in_flight.is_some();
        self.snapshot.recent_closed.pending = self
            .close_capture_order
            .iter()
            .filter_map(|key| {
                self.close_operations
                    .get(key)
                    .map(|operation| (key, operation))
            })
            .map(
                |(key, operation)| crate::model::RecentClosedPendingSnapshot {
                    key: operation.request.key.clone(),
                    target_id: operation.target_id.clone(),
                    label: operation.label.clone(),
                    phase: operation.phase.clone(),
                    checking: self.close_status_checks_in_flight.contains(key),
                    message: operation.message.clone(),
                    retryable: operation.retryable,
                },
            )
            .collect();
        self.snapshot.recent_closed.can_reopen = self.reopenable().is_some()
            && (!local || self.close_capture_order.is_empty())
            && self.reopen_in_flight.is_none();
        self.snapshot.recent_closed.reopen_blocked_reason = self
            .close_capture_order
            .back()
            .filter(|_| local)
            .and_then(|key| self.close_operations.get(key))
            .map(|operation| {
                if operation.phase == "unknown" {
                    "The latest close result needs checking before it can be reopened.".to_owned()
                } else {
                    "The latest close is still being confirmed.".to_owned()
                }
            });
        self.sync_async_operations();
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
        let closed_tab = self.retire_editor_tab(index);
        if closed_tab.kind == EditorTabKind::File {
            // A checkout the catalog no longer lists keeps the item; its
            // reopen then reports that the checkout is unavailable.
            let (device_id, checkout_path) = self
                .catalog_checkout(&closed_tab.workspace_id, &closed_tab.checkout_id)
                .map(|(workspace, checkout)| (workspace.device_id.clone(), checkout.path.clone()))
                .unwrap_or_else(|| (workspace::LOCAL_DEVICE_ID.to_owned(), String::new()));
            let key = self.next_recent_closed_key();
            self.push_recent_closed(ClosedItem::File {
                key,
                device_id,
                workspace_id: closed_tab.workspace_id.clone(),
                checkout_id: closed_tab.checkout_id.clone(),
                checkout_path,
                path: closed_tab.path.clone(),
                label: closed_tab.label.clone(),
            });
        }
        self.rebuild_tab_strips();
        // With View areas the area the closed display was in shows its next
        // display (`view_areas.rs`); the focus history is the one canvas's.
        if was_active && !self.separate_view_areas() {
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
                }
            })
            .collect()
    }

    pub(super) fn close_operation_kind(target: &live::CloseCaptureTarget) -> &'static str {
        match target {
            live::CloseCaptureTarget::Pane { .. } => "pane.close",
            live::CloseCaptureTarget::Tab { .. } => "tab.close",
        }
    }

    pub(super) fn close_operation_pane_ids(
        target: &live::CloseCaptureTarget,
        panes: &[ClosedPane],
    ) -> Vec<String> {
        match target {
            live::CloseCaptureTarget::Pane { pane_id } => vec![pane_id.clone()],
            live::CloseCaptureTarget::Tab { .. } => {
                panes.iter().map(|pane| pane.pane_id.clone()).collect()
            }
        }
    }

    pub(super) fn close_operation_target_present(
        payload: &SessionSnapshotPayload,
        target: &live::CloseCaptureTarget,
        scope_id: &str,
    ) -> bool {
        match target {
            live::CloseCaptureTarget::Pane { pane_id } => {
                if let Some(layout) = payload
                    .layouts
                    .iter()
                    .find(|layout| layout.tab_id == scope_id)
                {
                    return layout.panes.iter().any(|pane| pane.pane_id == *pane_id);
                }
                if payload.panes.iter().any(|pane| pane.pane_id == *pane_id) {
                    return true;
                }
                // Without the target tab's layout, a tab that is still in the
                // snapshot is an incomplete answer, not proof that its pane
                // disappeared. A removed tab is enough to prove the pane is
                // gone because pane closes never move a pane to another tab.
                payload.tabs.iter().any(|tab| tab.tab_id == scope_id)
            }
            live::CloseCaptureTarget::Tab { tab_id } => {
                payload.tabs.iter().any(|tab| tab.tab_id == *tab_id)
            }
        }
    }

    pub(super) fn close_target_present_in_projection(&self, operation: &PendingClose) -> bool {
        match &operation.request.target {
            live::CloseCaptureTarget::Pane { pane_id } => self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .flat_map(|checkout| checkout.tabs.iter())
                .find(|tab| tab.id.as_deref() == Some(operation.scope_id.as_str()))
                .is_some_and(|tab| tab.panes.iter().any(|pane| pane.id == *pane_id)),
            live::CloseCaptureTarget::Tab { tab_id } => self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .flat_map(|checkout| checkout.tabs.iter())
                .any(|tab| tab.id.as_deref() == Some(tab_id.as_str())),
        }
    }

    /// Returns a pre-send close failure when the user-approved target has
    /// changed since the request was captured. The caller must select the new
    /// target again, so an approval for one pane range never silently covers a
    /// later pane or a newly started agent.
    pub(super) fn close_precondition_failure(&self, operation: &PendingClose) -> Option<String> {
        let current_pane_ids = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .find(|tab| tab.id.as_deref() == Some(operation.scope_id.as_str()))
            .map(|tab| {
                let mut ids = tab
                    .panes
                    .iter()
                    .map(|pane| pane.id.clone())
                    .collect::<Vec<_>>();
                ids.sort();
                ids
            });
        let target_is_known =
            current_pane_ids.is_some() || self.close_target_present_in_projection(operation);
        if !target_is_known {
            // A few transport-facing callers can deliver a capture result
            // before the first authoritative projection exists. There is no
            // target identity to compare in that state, so keep the capture
            // path alive and let the later topology confirmation decide.
            return None;
        }
        let activity_status_unknown = self.snapshot.navigator.agents.iter().any(|agent| {
            let in_target = match &operation.request.target {
                live::CloseCaptureTarget::Pane { pane_id } => agent.pane_id == *pane_id,
                live::CloseCaptureTarget::Tab { .. } => operation
                    .scope_pane_ids
                    .iter()
                    .any(|pane_id| pane_id == &agent.pane_id),
            };
            in_target && agent.requires_close_status_check
        });
        if activity_status_unknown {
            return Some(
                "Activity status became unknown while preparing this close; check status before closing"
                    .to_owned(),
            );
        }
        if let Some(mut current_pane_ids) = current_pane_ids {
            if !operation.scope_pane_ids.is_empty() {
                current_pane_ids.sort();
                if current_pane_ids != operation.scope_pane_ids {
                    return Some(
                        "The close target changed before it was sent; select the updated tab or pane again"
                            .to_owned(),
                    );
                }
            }
        } else if !self.close_target_present_in_projection(operation) {
            return Some(
                "The close target disappeared before it was sent; select the current target again"
                    .to_owned(),
            );
        }
        let current_protected_agent_pane_ids = self
            .snapshot
            .navigator
            .agents
            .iter()
            .filter(|agent| {
                let in_target = match &operation.request.target {
                    live::CloseCaptureTarget::Pane { pane_id } => agent.pane_id == *pane_id,
                    live::CloseCaptureTarget::Tab { .. } => operation
                        .scope_pane_ids
                        .iter()
                        .any(|pane_id| pane_id == &agent.pane_id),
                };
                in_target && agent.requires_close_confirmation
            })
            .map(|agent| agent.pane_id.as_str())
            .collect::<Vec<_>>();
        let newly_protected = current_protected_agent_pane_ids.iter().any(|pane_id| {
            !operation
                .protected_agent_pane_ids
                .iter()
                .any(|known| known == pane_id)
        });
        if newly_protected {
            return Some(
                "A new agent needs attention in this tab; review the close confirmation again"
                    .to_owned(),
            );
        }
        None
    }

    pub(super) fn cancel_close_before_effect(&mut self, key: &str, message: String) {
        let Some(operation) = self.close_operations.get_mut(key) else {
            return;
        };
        operation.phase = "refused".to_owned();
        operation.stage = "capture".to_owned();
        operation.message = Some(format!("The close was canceled: {message}"));
        operation.retryable = true;
        operation.deadline_at_unix_ms = None;
        let operation = operation.clone();
        self.clear_close_guards(&operation);
        self.restore_close_selection(operation.selection_restore.as_ref());
        self.set_reopen_notices(vec![live::ReopenNotice {
            pane_id: matches!(
                &operation.request.target,
                live::CloseCaptureTarget::Pane { .. }
            )
            .then_some(operation.target_id.clone()),
            message: operation.message.clone().unwrap_or(message),
        }]);
        self.push_diagnostic(
            "recent_closed.close_canceled",
            format!(
                "{}: target changed before close effect",
                operation.request.key
            ),
        );
        self.promote_close_reservations();
        self.sync_recent_closed_snapshot();
    }

    pub(super) fn close_operation_holds_pane(&self, pane_id: &str) -> bool {
        self.close_operations
            .values()
            .any(|operation| operation.pane_ids.iter().any(|known| known == pane_id))
    }

    pub(super) fn close_selection_state(&self, checkout_id: Option<&str>) -> CloseSelectionState {
        let (active_tab_id, visible_tab_id) = checkout_id
            .map(|checkout_id| {
                let active_tab_id = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .find(|checkout| checkout.id == checkout_id)
                    .and_then(|checkout| checkout.active_tab_id.clone());
                let visible_tab_id = self.visible_tab_ids.get(checkout_id).cloned();
                (active_tab_id, visible_tab_id)
            })
            .unwrap_or((None, None));
        CloseSelectionState {
            focused_checkout_id: self.snapshot.navigator.focused_checkout_id.clone(),
            terminal_pane_id: self.snapshot.terminal.pane_id.clone(),
            focused_pane_id: self.snapshot.focused.pane_id.clone(),
            selected_pane_id: self.snapshot.ui_state.selected_pane_id.clone(),
            active_tab_id,
            visible_tab_id,
        }
    }

    pub(super) fn restore_close_selection(&mut self, restore: Option<&CloseSelectionRestore>) {
        let Some(restore) = restore else {
            return;
        };
        if self.close_selection_state(restore.checkout_id.as_deref()) != restore.after {
            return;
        }
        if let Some(checkout_id) = restore.checkout_id.as_deref() {
            if let Some(checkout) = self
                .snapshot
                .navigator
                .workspaces
                .iter_mut()
                .flat_map(|workspace| workspace.checkouts.iter_mut())
                .find(|checkout| checkout.id == checkout_id)
            {
                checkout.active_tab_id = restore.before.active_tab_id.clone();
            }
            match restore.before.visible_tab_id.clone() {
                Some(tab_id) => {
                    self.visible_tab_ids.insert(checkout_id.to_owned(), tab_id);
                }
                None => {
                    self.visible_tab_ids.remove(checkout_id);
                }
            }
            self.sync_active_tab_projection();
        }
        self.select_terminal_pane(restore.before.terminal_pane_id.clone());
        self.snapshot.focused.pane_id = restore.before.focused_pane_id.clone();
        self.snapshot.ui_state.selected_pane_id = restore.before.selected_pane_id.clone();
    }

    pub(super) fn clear_close_guards(&mut self, operation: &PendingClose) {
        for pane_id in &operation.pane_ids {
            self.panes_closing.remove(pane_id);
        }
    }

    pub(super) fn start_close_capture(
        &mut self,
        target: live::CloseCaptureTarget,
        tab: TabSnapshot,
    ) -> bool {
        let target_id = close_target_id(&target).to_owned();
        let Some(scope_id) = tab.id.clone() else {
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: matches!(&target, live::CloseCaptureTarget::Pane { .. })
                    .then_some(target_id.clone()),
                message: "The close target has no tab identity, so it was not closed".to_owned(),
            }]);
            return true;
        };
        if self
            .close_operations
            .values()
            .any(|operation| operation.scope_id == scope_id || operation.target_id == target_id)
            || self
                .pane_operations
                .values()
                .any(|operation| operation.scope_id == scope_id)
        {
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: matches!(&target, live::CloseCaptureTarget::Pane { .. })
                    .then_some(target_id.clone()),
                message: "This tab already has a close in progress; no second close was sent"
                    .to_owned(),
            }]);
            self.sync_recent_closed_snapshot();
            return false;
        }
        let Some(context) = self.close_context(&tab) else {
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: matches!(&target, live::CloseCaptureTarget::Pane { .. })
                    .then_some(target_id.clone()),
                message:
                    "The closed item could not be recorded because its local context is incomplete"
                        .into(),
            }]);
            return true;
        };
        let Some(live_context) = self.live.as_ref().cloned() else {
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: matches!(&target, live::CloseCaptureTarget::Pane { .. })
                    .then_some(target_id.clone()),
                message: "Closing this item requires the local Herdr connection".into(),
            }]);
            return true;
        };
        let request = live::CloseCaptureRequest {
            key: self.next_recent_closed_key(),
            connection_generation: self.live_generation,
            context,
            panes: self.closed_panes(&tab),
            target,
        };
        let pane_ids = Self::close_operation_pane_ids(&request.target, &request.panes);
        let mut scope_pane_ids = tab
            .panes
            .iter()
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>();
        scope_pane_ids.sort();
        let protected_agent_pane_ids = self
            .snapshot
            .navigator
            .agents
            .iter()
            .filter(|agent| {
                let in_target = match &request.target {
                    live::CloseCaptureTarget::Pane { pane_id } => agent.pane_id == *pane_id,
                    live::CloseCaptureTarget::Tab { .. } => scope_pane_ids
                        .iter()
                        .any(|pane_id| pane_id == &agent.pane_id),
                };
                in_target && agent.requires_close_confirmation
            })
            .map(|agent| agent.pane_id.clone())
            .collect::<Vec<_>>();
        let restore_checkout_id = tab.checkout_id.clone();
        let selection_before = self.close_selection_state(restore_checkout_id.as_deref());
        let label = match &request.target {
            live::CloseCaptureTarget::Pane { pane_id } => tab
                .panes
                .iter()
                .find(|pane| pane.id == *pane_id)
                .and_then(|pane| pane.herdr_label.clone().or(pane.terminal_title.clone()))
                .unwrap_or_else(|| pane_id.clone()),
            live::CloseCaptureTarget::Tab { .. } => {
                tab.label.clone().unwrap_or_else(|| "Tab".to_owned())
            }
        };
        let now = unix_milliseconds();
        self.close_operations.insert(
            request.key.clone(),
            PendingClose {
                target_id: target_id.clone(),
                scope_id,
                scope_pane_ids,
                pane_ids: pane_ids.clone(),
                protected_agent_pane_ids,
                label,
                item: None,
                phase: "preparing".to_owned(),
                stage: "capture".to_owned(),
                started_at_unix_ms: now,
                deadline_at_unix_ms: Some(now.saturating_add(CLOSE_STAGE_TIMEOUT_MS)),
                connection_generation: request.connection_generation,
                message: None,
                retryable: true,
                selection_restore: None,
                request: request.clone(),
            },
        );
        self.close_capture_order.push_back(request.key.clone());
        self.panes_closing.extend(pane_ids);
        self.move_from_closing_target(&request.target, &tab);
        let selection_after = self.close_selection_state(restore_checkout_id.as_deref());
        if selection_before != selection_after
            && let Some(operation) = self.close_operations.get_mut(&request.key)
        {
            operation.selection_restore = Some(CloseSelectionRestore {
                checkout_id: restore_checkout_id,
                before: selection_before,
                after: selection_after,
            });
        }
        self.sync_recent_closed_snapshot();
        if let Err(message) = live::spawn_close_capture(live_context, request.clone()) {
            self.fail_close_operation(
                &request.key,
                format!("The close worker could not start: {message}"),
            );
        }
        true
    }

    /// Keeps the operator on a confirmed alternative as soon as a close is
    /// approved. The alternative is chosen from the existing projection, so
    /// this never invents geometry or follows a global Herdr focus change.
    pub(super) fn move_from_closing_target(
        &mut self,
        target: &live::CloseCaptureTarget,
        tab: &TabSnapshot,
    ) {
        let Some(checkout_id) = tab.checkout_id.as_deref() else {
            return;
        };
        if self.snapshot.navigator.focused_checkout_id.as_deref() != Some(checkout_id) {
            return;
        }
        match target {
            live::CloseCaptureTarget::Pane { pane_id } => {
                let replacement = tab
                    .panes
                    .iter()
                    .map(|pane| pane.id.as_str())
                    .filter(|candidate| *candidate != pane_id)
                    .find(|candidate| !self.panes_closing.contains(*candidate))
                    .map(str::to_owned);
                if self.snapshot.terminal.pane_id.as_deref() == Some(pane_id.as_str()) {
                    self.select_terminal_pane(replacement);
                }
            }
            live::CloseCaptureTarget::Tab { tab_id } => {
                let replacement = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .find(|checkout| checkout.id == checkout_id)
                    .and_then(|checkout| {
                        checkout
                            .tabs
                            .iter()
                            .filter(|candidate| candidate.id.as_deref() != Some(tab_id.as_str()))
                            .filter(|candidate| {
                                candidate.id.as_deref().is_some_and(|candidate_id| {
                                    !self
                                        .close_operations
                                        .values()
                                        .any(|operation| operation.target_id == candidate_id)
                                })
                            })
                            .find_map(|candidate| candidate.id.clone())
                    });
                let Some(replacement) = replacement else {
                    return;
                };
                if let Some(checkout) = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter_mut()
                    .flat_map(|workspace| workspace.checkouts.iter_mut())
                    .find(|checkout| checkout.id == checkout_id)
                {
                    checkout.active_tab_id = Some(replacement.clone());
                }
                self.visible_tab_ids
                    .insert(checkout_id.to_owned(), replacement.clone());
                let pane_id =
                    self.snapshot
                        .navigator
                        .workspaces
                        .iter()
                        .flat_map(|workspace| workspace.checkouts.iter())
                        .find(|checkout| checkout.id == checkout_id)
                        .and_then(|checkout| {
                            checkout.tabs.iter().find(|candidate| {
                                candidate.id.as_deref() == Some(replacement.as_str())
                            })
                        })
                        .and_then(|replacement_tab| {
                            self.tab_focus_pane_id(
                                &replacement,
                                replacement_tab.panes.first().map(|pane| pane.id.clone()),
                            )
                        });
                self.select_terminal_pane(pane_id);
                self.sync_active_tab_projection();
            }
        }
    }

    pub(super) fn fail_close_operation(&mut self, key: &str, message: String) {
        let Some(operation) = self.close_operations.get_mut(key) else {
            return;
        };
        operation.phase = "failed".to_owned();
        operation.stage = "capture".to_owned();
        operation.message = Some(format!(
            "Restore information was not prepared; the item was not closed: {message}"
        ));
        operation.retryable = true;
        operation.deadline_at_unix_ms = None;
        let operation = operation.clone();
        self.clear_close_guards(&operation);
        self.set_reopen_notices(vec![live::ReopenNotice {
            pane_id: matches!(
                &operation.request.target,
                live::CloseCaptureTarget::Pane { .. }
            )
            .then_some(operation.target_id.clone()),
            message: operation.message.clone().unwrap_or(message),
        }]);
        self.push_diagnostic(
            "recent_closed.capture_failed",
            format!(
                "{}: {}",
                operation.request.key,
                operation.message.as_deref().unwrap_or("unknown failure")
            ),
        );
        self.promote_close_reservations();
        self.sync_recent_closed_snapshot();
    }

    pub(super) fn ensure_pending_close_from_request(
        &mut self,
        request: &live::CloseCaptureRequest,
    ) {
        if self.close_operations.contains_key(&request.key) {
            return;
        }
        let target_id = close_target_id(&request.target).to_owned();
        let pane_ids = Self::close_operation_pane_ids(&request.target, &request.panes);
        let now = unix_milliseconds();
        self.close_operations.insert(
            request.key.clone(),
            PendingClose {
                request: request.clone(),
                target_id,
                scope_id: request.context.tab_id.clone(),
                scope_pane_ids: {
                    let mut ids = request.context.pane_ids_before_close.clone();
                    ids.sort();
                    ids
                },
                pane_ids,
                protected_agent_pane_ids: Vec::new(),
                label: request.context.tab_label.clone(),
                item: None,
                phase: "preparing".to_owned(),
                stage: "capture".to_owned(),
                started_at_unix_ms: now,
                deadline_at_unix_ms: Some(now.saturating_add(CLOSE_STAGE_TIMEOUT_MS)),
                connection_generation: request.connection_generation,
                message: None,
                retryable: true,
                selection_restore: None,
            },
        );
        if !self
            .close_capture_order
            .iter()
            .any(|key| key == &request.key)
        {
            self.close_capture_order.push_back(request.key.clone());
        }
    }

    pub(crate) fn ingest_close_capture_result(
        &mut self,
        request: &live::CloseCaptureRequest,
        result: Result<live::CloseCaptureOutcome, String>,
    ) -> (bool, Vec<live::CloseEffectRequest>) {
        self.ensure_pending_close_from_request(request);
        let Some(operation) = self.close_operations.get(&request.key).cloned() else {
            return (false, Vec::new());
        };
        if operation.connection_generation != request.connection_generation
            || request.connection_generation != self.live_generation
            || !matches!(operation.phase.as_str(), "preparing" | "capturing")
        {
            return (false, Vec::new());
        }
        if operation
            .deadline_at_unix_ms
            .is_some_and(|deadline| deadline <= unix_milliseconds())
        {
            self.fail_close_operation(
                &request.key,
                "the restore capture result arrived after its deadline".to_owned(),
            );
            return (true, Vec::new());
        }
        if let Some(message) = self.close_precondition_failure(&operation) {
            self.cancel_close_before_effect(&request.key, message);
            return (true, Vec::new());
        }
        let mut effects = Vec::new();
        match result {
            Ok(outcome) => {
                let Some(operation) = self.close_operations.get_mut(&request.key) else {
                    return (false, Vec::new());
                };
                operation.item = outcome.item;
                operation.phase = "transmitting".to_owned();
                operation.stage = "close_request".to_owned();
                operation.message = None;
                operation.retryable = false;
                operation.deadline_at_unix_ms =
                    Some(unix_milliseconds().saturating_add(CLOSE_STAGE_TIMEOUT_MS));
                self.push_diagnostic(
                    "recent_closed.reserved",
                    format!(
                        "Reserved user close {} before its external effect",
                        request.key
                    ),
                );
                effects.push(live::CloseEffectRequest {
                    key: request.key.clone(),
                    connection_generation: request.connection_generation,
                    target: request.target.clone(),
                });
            }
            Err(message) => self.fail_close_operation(&request.key, message),
        }
        self.sync_recent_closed_snapshot();
        (true, effects)
    }

    pub(crate) fn ingest_close_effect_result(
        &mut self,
        request: &live::CloseEffectRequest,
        result: Result<(), hide_herdr_client::ApiError>,
    ) -> bool {
        if !self.close_operations.contains_key(&request.key) {
            // A result can outlive the runtime that created its reservation.
            // Rehydrate that reservation without putting it back on the
            // confirmed stack, then let the same identity checks and
            // topology gate handle the result.
            let tab_id = match &request.target {
                live::CloseCaptureTarget::Pane { pane_id } => pane_id.clone(),
                live::CloseCaptureTarget::Tab { tab_id } => tab_id.clone(),
            };
            self.ensure_pending_close_from_request(&live::CloseCaptureRequest {
                key: request.key.clone(),
                connection_generation: request.connection_generation,
                context: ClosedContext {
                    workspace_id: String::new(),
                    workspace_label: String::new(),
                    workspace_ids_before_close: Vec::new(),
                    tab_ids_before_close: Vec::new(),
                    pane_ids_before_close: Vec::new(),
                    checkout_id: String::new(),
                    checkout_path: String::new(),
                    tab_id,
                    tab_label: request.key.clone(),
                    tab_index: 0,
                },
                panes: Vec::new(),
                target: request.target.clone(),
            });
            let item = self
                .recent_closed
                .iter()
                .position(|item| item.key() == request.key)
                .and_then(|index| self.recent_closed.remove(index));
            if let Some(operation) = self.close_operations.get_mut(&request.key) {
                operation.item = item;
                operation.phase = "transmitting".to_owned();
                operation.stage = "close_request".to_owned();
                operation.retryable = false;
                operation.deadline_at_unix_ms =
                    Some(unix_milliseconds().saturating_add(CLOSE_STAGE_TIMEOUT_MS));
            }
        }
        let Some(operation) = self.close_operations.get(&request.key).cloned() else {
            return false;
        };
        if operation.connection_generation != request.connection_generation
            || request.connection_generation != self.live_generation
            || close_target_id(&operation.request.target) != close_target_id(&request.target)
            || operation.phase != "transmitting"
        {
            return false;
        }
        let result = if operation
            .deadline_at_unix_ms
            .is_some_and(|deadline| deadline <= unix_milliseconds())
        {
            Err(hide_herdr_client::ApiError::Transport(
                "close result arrived after its deadline".to_owned(),
            ))
        } else {
            result
        };
        let mut schedule_status_check = false;
        match result {
            Ok(()) => {
                if let Some(operation) = self.close_operations.get_mut(&request.key) {
                    operation.phase = "awaiting_topology".to_owned();
                    operation.stage = "topology".to_owned();
                    operation.message = Some(
                        "Close was accepted; waiting for Herdr to confirm the target is gone"
                            .to_owned(),
                    );
                    operation.retryable = false;
                    operation.deadline_at_unix_ms =
                        Some(unix_milliseconds().saturating_add(CLOSE_STAGE_TIMEOUT_MS));
                }
                self.push_diagnostic(
                    "recent_closed.captured",
                    format!("Captured user close {}", request.key),
                );
            }
            Err(hide_herdr_client::ApiError::Remote { code, message }) => {
                if let Some(operation) = self.close_operations.get_mut(&request.key) {
                    operation.phase = "refused".to_owned();
                    operation.stage = "close_request".to_owned();
                    operation.message = Some(format!("The close was refused: {code}: {message}"));
                    operation.retryable = true;
                    operation.deadline_at_unix_ms = None;
                }
                let operation = self.close_operations.get(&request.key).cloned();
                if let Some(operation) = operation.as_ref() {
                    self.clear_close_guards(operation);
                    self.restore_close_selection(operation.selection_restore.as_ref());
                }
                self.set_reopen_notices(vec![live::ReopenNotice {
                    pane_id: matches!(&request.target, live::CloseCaptureTarget::Pane { .. })
                        .then_some(close_target_id(&request.target).to_owned()),
                    message: operation
                        .and_then(|operation| operation.message)
                        .unwrap_or_else(|| format!("The item was not closed: {code}: {message}")),
                }]);
                self.push_diagnostic(
                    "recent_closed.close_failed",
                    format!("{}: {code}: {message}", request.key),
                );
            }
            Err(error) => {
                if let Some(operation) = self.close_operations.get_mut(&request.key) {
                    operation.phase = "unknown".to_owned();
                    operation.stage = "status_check".to_owned();
                    operation.message =
                        Some("Close result is unknown; check status before retrying".to_owned());
                    operation.retryable = true;
                    operation.deadline_at_unix_ms =
                        Some(unix_milliseconds().saturating_add(CLOSE_STAGE_TIMEOUT_MS));
                }
                self.set_reopen_notices(vec![live::ReopenNotice {
                    pane_id: matches!(&request.target, live::CloseCaptureTarget::Pane { .. })
                        .then_some(close_target_id(&request.target).to_owned()),
                    message: format!(
                        "Close result is unknown; the reopen reservation was kept: {error}"
                    ),
                }]);
                self.push_diagnostic(
                    "recent_closed.close_unconfirmed",
                    format!("{}: {error}", request.key),
                );
                schedule_status_check = true;
            }
        }
        if schedule_status_check {
            self.start_close_status_check(&request.key);
        }
        self.promote_close_reservations();
        self.sync_recent_closed_snapshot();
        true
    }

    pub(super) fn promote_close_reservations(&mut self) -> bool {
        let mut changed = false;
        while let Some(key) = self.close_capture_order.front().cloned() {
            let Some(operation) = self.close_operations.get(&key).cloned() else {
                self.close_capture_order.pop_front();
                changed = true;
                continue;
            };
            if !matches!(operation.phase.as_str(), "completed" | "failed" | "refused") {
                break;
            }
            self.close_capture_order.pop_front();
            self.close_operations.remove(&key);
            self.clear_close_guards(&operation);
            if operation.phase == "completed"
                && let Some(item) = operation.item
            {
                push_bounded(&mut self.recent_closed, item);
            }
            changed = true;
        }
        if changed {
            self.sync_recent_closed_snapshot();
        }
        changed
    }

    /// Completes a close only after a fresh topology says the requested
    /// target is absent. A capture failure with an absent target is kept as a
    /// completed, no-item operation, so Hide does not invent restore data for
    /// an external close.
    pub(super) fn mark_close_topology_confirmed(&mut self, key: &str) -> bool {
        let Some(operation) = self.close_operations.get(key).cloned() else {
            return false;
        };
        if matches!(operation.phase.as_str(), "completed" | "failed" | "refused") {
            return false;
        }
        if let Some(current) = self.close_operations.get_mut(key) {
            let had_restore = current.item.is_some();
            current.phase = "completed".to_owned();
            current.stage = "topology".to_owned();
            current.message = (!had_restore).then(|| {
                "The item was already closed; Hide synchronized this view without creating a reopen entry"
                    .to_owned()
            });
            current.retryable = false;
            current.deadline_at_unix_ms = None;
        }
        self.clear_close_guards(&operation);
        if operation.item.is_none() {
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: matches!(
                    &operation.request.target,
                    live::CloseCaptureTarget::Pane { .. }
                )
                .then_some(operation.target_id),
                message: "The item was already closed; Hide synchronized this view without creating a reopen entry".to_owned(),
            }]);
        } else {
            self.snapshot.recent_closed.notices.clear();
        }
        self.push_diagnostic(
            "recent_closed.topology_confirmed",
            format!(
                "Confirmed close {} by fresh topology; restore data {}",
                key,
                if operation.item.is_some() {
                    "is reserved"
                } else {
                    "was unavailable"
                }
            ),
        );
        true
    }

    pub(super) fn observe_close_topology(&mut self, payload: &SessionSnapshotPayload) -> bool {
        let mut changed = false;
        for key in self.close_capture_order.clone() {
            let Some(operation) = self.close_operations.get(&key).cloned() else {
                continue;
            };
            let target_present = Self::close_operation_target_present(
                payload,
                &operation.request.target,
                &operation.scope_id,
            );
            let scope_changed = !operation.scope_pane_ids.is_empty()
                && payload
                    .layouts
                    .iter()
                    .find(|layout| layout.tab_id == operation.scope_id)
                    .is_some_and(|layout| {
                        let mut pane_ids = layout
                            .panes
                            .iter()
                            .map(|pane| pane.pane_id.clone())
                            .collect::<Vec<_>>();
                        pane_ids.sort();
                        pane_ids != operation.scope_pane_ids
                    });
            if matches!(operation.phase.as_str(), "preparing" | "capturing")
                && (!target_present || scope_changed)
            {
                self.cancel_close_before_effect(
                    &key,
                    if target_present {
                        "the tab or pane set changed while restore information was being prepared"
                            .to_owned()
                    } else {
                        "the target disappeared while restore information was being prepared"
                            .to_owned()
                    },
                );
                changed = true;
            } else if !target_present {
                changed |= self.mark_close_topology_confirmed(&key);
            }
        }
        changed |= self.promote_close_reservations();
        if changed {
            self.sync_recent_closed_snapshot();
        }
        changed
    }

    pub(super) fn start_close_status_check(&mut self, key: &str) -> bool {
        self.start_close_status_checks(&[key.to_owned()])
    }

    /// Starts one read for all currently unknown local closes. All of those
    /// reservations share the same Herdr session, so a status check is a
    /// single authoritative snapshot with a fan-out result rather than one
    /// request per pane or tab.
    pub(super) fn start_close_status_checks(&mut self, requested_keys: &[String]) -> bool {
        if requested_keys.is_empty()
            || requested_keys
                .iter()
                .any(|key| self.close_status_checks_in_flight.contains(key))
        {
            return false;
        }
        let requested_unknown = requested_keys.iter().any(|key| {
            self.close_operations
                .get(key)
                .is_some_and(|operation| operation.phase == "unknown")
        });
        if !requested_unknown {
            return false;
        }

        let requests = self
            .close_capture_order
            .iter()
            .filter_map(|key| {
                let operation = self.close_operations.get(key)?;
                (operation.phase == "unknown" && !self.close_status_checks_in_flight.contains(key))
                    .then(|| live::CloseStatusCheckRequest {
                        key: key.clone(),
                        target: operation.request.target.clone(),
                    })
            })
            .collect::<Vec<_>>();
        if requests.is_empty() {
            return false;
        }
        let request_keys = requests
            .iter()
            .map(|request| request.key.clone())
            .collect::<Vec<_>>();
        let Some(context) = self.live.as_ref().cloned() else {
            for key in &request_keys {
                if let Some(operation) = self.close_operations.get_mut(key) {
                    operation.message = Some(
                        "Close result is unknown; reconnect Herdr before checking status"
                            .to_owned(),
                    );
                    operation.retryable = true;
                    operation.deadline_at_unix_ms = None;
                }
            }
            self.sync_recent_closed_snapshot();
            return true;
        };

        for key in &request_keys {
            self.close_status_checks_in_flight.insert(key.clone());
            if let Some(operation) = self.close_operations.get_mut(key) {
                operation.message = Some("Checking the current topology".to_owned());
                operation.stage = "status_check".to_owned();
                operation.retryable = true;
            }
        }
        if let Err(message) =
            live::spawn_close_status_checks(context, self.live_generation, requests)
        {
            for key in &request_keys {
                self.close_status_checks_in_flight.remove(key);
                if let Some(operation) = self.close_operations.get_mut(key) {
                    operation.message = Some(format!(
                        "Close result is unknown; status check could not start: {message}"
                    ));
                    operation.retryable = true;
                    operation.deadline_at_unix_ms = None;
                }
            }
            let notices = request_keys
                .iter()
                .filter_map(|key| {
                    let operation = self.close_operations.get(key)?;
                    Some(live::ReopenNotice {
                        pane_id: matches!(
                            &operation.request.target,
                            live::CloseCaptureTarget::Pane { .. }
                        )
                        .then_some(operation.target_id.clone()),
                        message: operation.message.clone().unwrap_or_default(),
                    })
                })
                .collect();
            self.set_reopen_notices(notices);
            self.sync_recent_closed_snapshot();
            return true;
        }
        self.sync_recent_closed_snapshot();
        true
    }

    pub(crate) fn ingest_close_status_results(
        &mut self,
        connection_generation: u64,
        requests: &[live::CloseStatusCheckRequest],
        result: Result<SessionSnapshotPayload, SessionFetchError>,
    ) -> bool {
        let valid_requests = requests
            .iter()
            .filter_map(|request| {
                let operation = self.close_operations.get(&request.key)?;
                if connection_generation != self.live_generation
                    || operation.connection_generation != connection_generation
                    || close_target_id(&operation.request.target)
                        != close_target_id(&request.target)
                {
                    return None;
                }
                Some((
                    request.key.clone(),
                    request.target.clone(),
                    operation.scope_id.clone(),
                ))
            })
            .collect::<Vec<_>>();
        if valid_requests.is_empty() {
            for request in requests {
                self.close_status_checks_in_flight.remove(&request.key);
            }
            return false;
        }

        let mut changed = false;
        match result {
            Ok(payload) => {
                let target_presence = valid_requests
                    .iter()
                    .map(|(key, target, scope_id)| {
                        (
                            key.clone(),
                            Self::close_operation_target_present(&payload, target, scope_id),
                        )
                    })
                    .collect::<Vec<_>>();
                // The status response is itself a fresh session projection.
                // Apply it before classifying the reservation so the sidebar
                // cannot retain a closed target until an unrelated event.
                changed |= self.ingest_session_with_catalog(Ok(payload), None);
                let mut notices = Vec::new();
                for (key, target_present) in target_presence {
                    if target_present && let Some(operation) = self.close_operations.get_mut(&key) {
                        operation.phase = "unknown".to_owned();
                        operation.stage = "status_check".to_owned();
                        operation.message = Some(
                            "The target is still present; no close was resent. Check status again after resolving the current work"
                                .to_owned(),
                        );
                        operation.retryable = true;
                        operation.deadline_at_unix_ms = None;
                        notices.push(live::ReopenNotice {
                            pane_id: matches!(
                                &operation.request.target,
                                live::CloseCaptureTarget::Pane { .. }
                            )
                            .then_some(operation.target_id.clone()),
                            message: operation.message.clone().unwrap_or_default(),
                        });
                        changed = true;
                    }
                    self.push_diagnostic(
                        "recent_closed.status_checked",
                        format!("Checked close {key}; target_present={target_present}"),
                    );
                }
                if !notices.is_empty() {
                    self.set_reopen_notices(notices);
                }
            }
            Err(error) => {
                let mut notices = Vec::new();
                for (key, _, _) in &valid_requests {
                    if let Some(operation) = self.close_operations.get_mut(key) {
                        operation.phase = "unknown".to_owned();
                        operation.stage = "status_check".to_owned();
                        operation.message = Some(format!(
                            "Close result is unknown; status check failed: {}",
                            error.message()
                        ));
                        operation.retryable = true;
                        operation.deadline_at_unix_ms = None;
                        notices.push(live::ReopenNotice {
                            pane_id: matches!(
                                &operation.request.target,
                                live::CloseCaptureTarget::Pane { .. }
                            )
                            .then_some(operation.target_id.clone()),
                            message: operation.message.clone().unwrap_or_default(),
                        });
                        changed = true;
                    }
                    self.push_diagnostic(
                        "recent_closed.status_check_failed",
                        format!("{key}: {}", error.message()),
                    );
                }
                self.set_reopen_notices(notices);
            }
        }
        for request in requests {
            self.close_status_checks_in_flight.remove(&request.key);
        }
        changed |= self.promote_close_reservations();
        self.sync_recent_closed_snapshot();
        changed || !valid_requests.is_empty()
    }

    pub(crate) fn check_close_status(&mut self, key: &str) -> bool {
        let Some(operation) = self.close_operations.get(key) else {
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: None,
                message: "That close is no longer pending".to_owned(),
            }]);
            return true;
        };
        if operation.phase != "unknown" {
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: matches!(
                    &operation.request.target,
                    live::CloseCaptureTarget::Pane { .. }
                )
                .then_some(operation.target_id.clone()),
                message: "This close is still awaiting its normal confirmation".to_owned(),
            }]);
            return true;
        }
        self.start_close_status_check(key)
    }

    pub(super) fn expire_close_operations(&mut self, now_unix_ms: u64) -> bool {
        let mut check = Vec::new();
        let mut changed = false;
        for key in self.close_capture_order.clone() {
            let Some(operation) = self.close_operations.get(&key).cloned() else {
                continue;
            };
            if self.close_status_checks_in_flight.contains(&key) {
                continue;
            }
            let Some(deadline) = operation.deadline_at_unix_ms else {
                continue;
            };
            if deadline > now_unix_ms {
                continue;
            }
            match operation.phase.as_str() {
                "preparing" | "capturing" => {
                    self.fail_close_operation(
                        &key,
                        "the restore capture deadline expired".to_owned(),
                    );
                    changed = true;
                }
                "transmitting" | "awaiting_topology" => {
                    if let Some(operation) = self.close_operations.get_mut(&key) {
                        operation.phase = "unknown".to_owned();
                        operation.stage = "status_check".to_owned();
                        operation.message = Some(
                            "Close result is unknown; checking the current topology".to_owned(),
                        );
                        operation.retryable = true;
                        operation.deadline_at_unix_ms =
                            Some(now_unix_ms.saturating_add(CLOSE_STAGE_TIMEOUT_MS));
                    }
                    check.push(key);
                    changed = true;
                }
                "unknown" => {
                    if let Some(operation) = self.close_operations.get_mut(&key) {
                        operation.message = Some(
                            "Result check needed; no mutation was resent. Use Check status to retry the read-only check"
                                .to_owned(),
                        );
                        operation.deadline_at_unix_ms = None;
                    }
                    changed = true;
                }
                _ => {}
            }
        }
        if !check.is_empty() {
            changed |= self.start_close_status_checks(&check);
        }
        changed |= self.promote_close_reservations();
        if changed {
            self.sync_recent_closed_snapshot();
        }
        changed
    }

    pub(super) fn reopen_closed(&mut self) -> bool {
        if self.reopen_in_flight.is_some() {
            return false;
        }
        if self.reopen_device() == workspace::LOCAL_DEVICE_ID
            && let Some(key) = self.close_capture_order.back().cloned()
            && let Some(operation) = self.close_operations.get(&key)
        {
            self.set_reopen_notices(vec![live::ReopenNotice {
                pane_id: matches!(
                    &operation.request.target,
                    live::CloseCaptureTarget::Pane { .. }
                )
                .then_some(operation.target_id.clone()),
                message: if operation.phase == "unknown" {
                    "Close result is unknown; check status before reopening".to_owned()
                } else {
                    "Close is still being confirmed; reopening is temporarily unavailable"
                        .to_owned()
                },
            }]);
            self.sync_recent_closed_snapshot();
            return true;
        }
        let Some(item) = self.reopenable().cloned() else {
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
        // With View areas the reopened file takes a display, so a Workspace
        // at its display cap refuses it here and the item stays.
        if self.separate_view_areas()
            && let ClosedItem::File {
                workspace_id,
                checkout_id,
                path,
                ..
            } = &item
            && let Some(workspace) = self.workspace_key(workspace_id, checkout_id)
            && !self.admit_view_open(
                &workspace,
                path,
                crate::view_layout::DisplayKind::File,
                None,
                false,
                false,
            )
        {
            return true;
        }
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
        let spawned = if let ClosedItem::File {
            workspace_id,
            checkout_id,
            ..
        } = &request.item
        {
            let located = self
                .document_root(workspace_id, checkout_id)
                .and_then(|root| Ok((self.device_channel(&root.device_id)?, root)));
            located.and_then(|(channel, root)| {
                self.worker_context
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| "the file worker is unavailable".to_owned())
                    .and_then(|worker| {
                        live::spawn_file_reopen(
                            worker.runtime,
                            worker.notifier,
                            request,
                            root,
                            channel,
                        )
                    })
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
                    live::FileReopenResult::Opened(opened) => {
                        let (document, place) = *opened;
                        if let ClosedItem::File {
                            workspace_id,
                            checkout_id,
                            path,
                            ..
                        } = &request.item
                        {
                            let tab_id = self.new_file_tab_id(workspace_id, checkout_id, path);
                            let prepared = PreparedFileTab::Read {
                                tab_id: tab_id.clone(),
                                document: Box::new(document),
                                place,
                            };
                            self.show_file_tab(prepared, workspace_id, checkout_id, path, false);
                            // With View areas the open can still be refused as
                            // it lands, when the Workspace filled its views
                            // meanwhile: the item stays and says why.
                            if !self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
                                let reason = self
                                    .snapshot
                                    .status
                                    .last_error
                                    .as_ref()
                                    .map_or_else(String::new, |error| error.message.clone());
                                self.set_reopen_notices(vec![live::ReopenNotice {
                                    pane_id: None,
                                    message: format!(
                                        "The file could not be reopened; retry is available: {reason}"
                                    ),
                                }]);
                                self.sync_recent_closed_snapshot();
                                return true;
                            }
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
                    self.yield_surface_to_terminal();
                }
                self.set_reopen_notices(outcome.notices);
            }
        }
        self.sync_recent_closed_snapshot();
        true
    }
}

/// A diff tab's name: the file and the comparison it shows.
pub(super) fn diff_label(path: &str, committed: bool) -> String {
    let scope = if committed {
        "branch diff"
    } else {
        "working diff"
    };
    format!("{} ({scope})", super::workspace_view::file_label(path))
}
