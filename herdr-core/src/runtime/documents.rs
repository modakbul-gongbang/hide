//! File tabs on any device: where a document is read from, and how its
//! saves run and settle (PRD S5.5 B8-B15, B33, B34).
//!
//! A tab's document names the device, the checkout root and the root
//! identity it was read under ([`DocumentPlace`]). This machine answers in
//! process, as the local open always has; a device answers through its
//! helper on a worker, and the tab appears when the answer does, in the
//! checkout that asked even if the operator has moved on.
//!
//! One save per document runs at a time, and the newest draft that arrives
//! meanwhile waits behind it (D-15); an older waiting draft is replaced, not
//! sent. A save whose answer is lost is `unknown`: the draft stays, no other
//! save is sent for that document, and the file is read back (after any save
//! still running in its folder has finished) to judge it saved, not saved,
//! or overtaken by another change. Nothing is ever resent on its own.

use std::sync::Arc;

use super::editor::PreparedFileTab;
use super::*;
use crate::files::{DocumentPlace, DocumentRoot, OpenFailure, SaveOutcome};
use crate::host_access::{HostCallError, HostChannel};
use crate::model::{EditorConflictSnapshot, EditorOpeningSnapshot, EditorSaveSnapshot};

#[derive(Default)]
pub(super) struct SaveSlot {
    running: bool,
    queued: Option<PendingSave>,
    unsettled: Option<UnsettledSave>,
    checking: bool,
    /// A read-back was asked for while one was running, for instance because
    /// the helper came back; the running one may have used the old
    /// connection, so a failed answer starts another instead of waiting.
    recheck: bool,
    /// Why the unknown save is still unknown, for the tab to show.
    waiting: Option<String>,
}

struct PendingSave {
    contents: String,
    close_after: bool,
}

#[derive(Clone, PartialEq)]
pub(super) struct UnsettledSave {
    contents: String,
    expected: String,
}

pub(super) struct SaveRequest {
    contents: String,
    expected: String,
    close_after: bool,
    place: DocumentPlace,
}

pub(super) struct OpenRequest {
    generation: u64,
    workspace_id: String,
    checkout_id: String,
    path: String,
    preview: bool,
    /// Replaces the open tab's document (Reload) instead of adding a tab.
    reload: bool,
}

type OpenResult = Result<(EditorDocumentSnapshot, DocumentPlace), OpenFailure>;

impl Runtime {
    /// The checkout a file belongs to, as its device reaches it. This
    /// machine's checkouts resolve through the roots hided pinned when it
    /// opened them, so a replaced checkout refuses the read.
    pub(super) fn document_root(
        &self,
        workspace_id: &str,
        checkout_id: &str,
    ) -> Result<DocumentRoot, String> {
        let (workspace, checkout) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .and_then(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == checkout_id)
                    .map(|checkout| (workspace, checkout))
            })
            .ok_or_else(|| "The file's project or checkout is no longer available".to_owned())?;
        let device_id = workspace.device_id.clone();
        if device_id == workspace::LOCAL_DEVICE_ID
            && let Some(roots) = self.file_roots.as_ref()
        {
            let (path, identity) =
                roots
                    .pinned_root(Path::new(&checkout.path))
                    .ok_or_else(|| {
                        "The checkout is not one this daemon has opened, so nothing was read"
                            .to_owned()
                    })?;
            return Ok(DocumentRoot {
                device_id,
                path: path.to_string_lossy().into_owned(),
                identity: Some(identity),
            });
        }
        Ok(DocumentRoot {
            device_id,
            path: checkout.path.clone(),
            identity: None,
        })
    }

    /// Reads a file for a new tab: in place on this machine, or starts the
    /// device read that shows the tab later.
    pub(super) fn read_file_tab(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
    ) -> Result<PreparedFileTab, String> {
        let root = self.document_root(workspace_id, checkout_id)?;
        let channel = self.device_channel(&root.device_id)?;
        let tab_id = Self::file_tab_id(workspace_id, checkout_id, path);
        if !channel.in_process() {
            return Ok(PreparedFileTab::Reading { root, channel });
        }
        files::open_document(channel.as_ref(), &root, path)
            .map(|(document, place)| PreparedFileTab::Read {
                tab_id,
                document: Box::new(document),
                place,
            })
            .map_err(|failure| failure.message())
    }

    pub(super) fn start_document_open(
        &mut self,
        root: DocumentRoot,
        channel: Arc<dyn HostChannel>,
        request: OpenRequestFields,
    ) {
        let tab_id = Self::file_tab_id(&request.workspace_id, &request.checkout_id, &request.path);
        if !request.reload && self.document_opens.contains_key(&tab_id) {
            return;
        }
        let Some(context) = self.worker_context.clone() else {
            self.set_error(
                "file.open_worker_unavailable",
                "The file reader is unavailable; the file was not opened",
                true,
            );
            return;
        };
        self.next_document_generation += 1;
        let generation = self.next_document_generation;
        self.document_opens.insert(
            tab_id.clone(),
            OpenRequest {
                generation,
                workspace_id: request.workspace_id,
                checkout_id: request.checkout_id,
                path: request.path.clone(),
                preview: request.preview,
                reload: request.reload,
            },
        );
        self.sync_opening_snapshot();
        crate::diagnostic!(serde_json::json!({
            "component": "documents",
            "kind": "file.open_requested",
            "device": root.device_id,
            "generation": generation,
            "reload": request.reload,
        }));
        let path = request.path;
        let spawned = thread::Builder::new()
            .name("herdr-core-file-open".to_owned())
            .spawn(move || {
                let result = files::open_document(channel.as_ref(), &root, &path);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_document_open(&tab_id, generation, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawned {
            let tab_id = self
                .document_opens
                .iter()
                .find(|(_, open)| open.generation == generation)
                .map(|(tab_id, _)| tab_id.clone());
            if let Some(tab_id) = tab_id {
                self.document_opens.remove(&tab_id);
            }
            self.sync_opening_snapshot();
            self.set_error(
                "file.open_worker_failed",
                format!("The file reader could not start: {error}"),
                true,
            );
        }
    }

    fn sync_opening_snapshot(&mut self) {
        let mut opening: Vec<_> = self.document_opens.values().collect();
        opening.sort_by_key(|open| open.generation);
        self.snapshot.editor.opening = opening
            .into_iter()
            .filter(|open| !open.reload)
            .map(|open| EditorOpeningSnapshot {
                workspace_id: open.workspace_id.clone(),
                checkout_id: open.checkout_id.clone(),
                path: open.path.clone(),
            })
            .collect();
    }

    /// A device read came back. An answer for a request that was replaced is
    /// dropped. The tab is shown where it was asked for; it takes the screen
    /// only while that checkout is still the one in front (B34).
    pub(super) fn ingest_document_open(
        &mut self,
        tab_id: &str,
        generation: u64,
        result: OpenResult,
    ) -> bool {
        if self.document_opens.get(tab_id).map(|open| open.generation) != Some(generation) {
            return false;
        }
        let request = self
            .document_opens
            .remove(tab_id)
            .expect("the request was just found");
        self.sync_opening_snapshot();
        let (document, place) = match result {
            Ok(opened) => opened,
            Err(failure) => {
                self.set_error(
                    if request.reload {
                        "file.reload_failed"
                    } else {
                        "file.open_failed"
                    },
                    failure.message(),
                    true,
                );
                return true;
            }
        };
        let open = self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id);
        if request.reload {
            if open {
                self.replace_document(tab_id, document, place);
            }
            return true;
        }
        if open {
            return true;
        }
        let in_front = self.snapshot.navigator.focused_workspace_id.as_deref()
            == Some(request.workspace_id.as_str())
            && self.snapshot.navigator.focused_checkout_id.as_deref()
                == Some(request.checkout_id.as_str());
        let prepared = PreparedFileTab::Read {
            tab_id: tab_id.to_owned(),
            document: Box::new(document),
            place,
        };
        if in_front {
            self.show_file_tab(
                prepared,
                &request.workspace_id,
                &request.checkout_id,
                &request.path,
                request.preview,
            );
        } else {
            self.insert_file_tab(
                prepared,
                &request.workspace_id,
                &request.checkout_id,
                &request.path,
                request.preview,
            );
        }
        self.persist_current_ui_state();
        true
    }

    /// Puts a freshly read document in place of the tab's current one: the
    /// operator chose Reload, so the draft and any save state go with it.
    fn replace_document(
        &mut self,
        tab_id: &str,
        document: EditorDocumentSnapshot,
        place: DocumentPlace,
    ) {
        self.editor_documents.insert(tab_id.to_owned(), document);
        self.document_places.insert(tab_id.to_owned(), place);
        self.document_saves.remove(tab_id);
        self.sync_file_tab_dirty(tab_id);
        self.sync_active_editor_document();
    }

    /// Reload: reads the file again under the checkout's current root, so a
    /// checkout that was replaced is adopted only on this explicit request.
    pub(super) fn reload_document(&mut self, tab_id: &str) -> bool {
        let Some(tab) = self
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .cloned()
        else {
            self.set_error(
                "file.reload_failed",
                "The file tab is no longer open",
                false,
            );
            return true;
        };
        let prepared = self.read_file_tab(&tab.workspace_id, &tab.checkout_id, &tab.path);
        match prepared {
            Ok(PreparedFileTab::Read {
                document, place, ..
            }) => self.replace_document(tab_id, *document, place),
            Ok(PreparedFileTab::Reading { root, channel }) => self.start_document_open(
                root,
                channel,
                OpenRequestFields {
                    workspace_id: tab.workspace_id,
                    checkout_id: tab.checkout_id,
                    path: tab.path,
                    preview: false,
                    reload: true,
                },
            ),
            Ok(PreparedFileTab::Open(_)) => {}
            Err(message) => self.set_error("file.reload_failed", message, true),
        }
        true
    }

    /// Keep Editing: the draft stays and is now judged against what the file
    /// holds, so the next save replaces that version on purpose.
    pub(super) fn keep_editing_document(&mut self, tab_id: &str) -> bool {
        let Some(document) = self.editor_documents.get_mut(tab_id) else {
            self.set_error(
                "file.conflict_without_document",
                "The active file tab has no document state",
                false,
            );
            return true;
        };
        if let Some(conflict) = document.conflict.take()
            && let Some(disk) = conflict.disk_revision
        {
            document.revision = Some(disk);
        }
        self.sync_active_editor_document();
        true
    }

    /// A save the operator asked for, or the save a close waits on.
    pub(super) fn request_file_save(
        &mut self,
        payload: FileSavePayload,
        close_after: bool,
    ) -> bool {
        let Some(tab_id) = self
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.id == payload.tab_id && tab.path == payload.path)
            .map(|tab| tab.id.clone())
        else {
            self.set_error("file.save_rejected", "The save target is not open", false);
            return true;
        };
        let Some(document) = self.editor_documents.get_mut(&tab_id) else {
            self.set_error(
                "file.save_rejected",
                "The save target has no document state",
                false,
            );
            return true;
        };
        if let Err(message) = files::check_editable(document, "the draft was preserved") {
            self.set_error("file.save_rejected", message, false);
            return true;
        }
        document.contents_utf8 = Some(payload.contents_utf8.clone());
        document.dirty = true;
        self.sync_file_tab_dirty(&tab_id);
        let slot = self.document_saves.entry(tab_id.clone()).or_default();
        if slot.unsettled.is_some() {
            self.set_error(
                "file.save_unsettled",
                "The last save's result is still unknown, so this save was not sent. Hide reads the file back first; the draft is kept.",
                true,
            );
            self.start_save_settle(&tab_id);
            self.sync_save_snapshot(&tab_id);
            return true;
        }
        if slot.running {
            slot.queued = Some(PendingSave {
                contents: payload.contents_utf8,
                close_after,
            });
            self.sync_save_snapshot(&tab_id);
            return true;
        }
        self.start_document_save(&tab_id, payload.contents_utf8, close_after);
        true
    }

    fn start_document_save(&mut self, tab_id: &str, contents: String, close_after: bool) {
        let Some(place) = self.document_places.get(tab_id).cloned() else {
            self.set_error(
                "file.save_rejected",
                "The file tab has no location to save to; the draft was preserved",
                false,
            );
            return;
        };
        let Some(expected) = self
            .editor_documents
            .get(tab_id)
            .and_then(|document| document.revision.clone())
        else {
            self.set_error(
                "file.save_rejected",
                "The file's revision was never read; the draft was preserved",
                false,
            );
            return;
        };
        let channel = match self.device_channel(&place.device_id) {
            Ok(channel) => channel,
            Err(message) => {
                self.set_error(
                    "file.save_unavailable",
                    format!("{message}; nothing was sent and the draft was preserved"),
                    true,
                );
                return;
            }
        };
        let Some(context) = self.worker_context.clone() else {
            self.set_error(
                "file.save_worker_unavailable",
                "The file save worker is unavailable; the draft was preserved",
                true,
            );
            return;
        };
        self.document_saves
            .entry(tab_id.to_owned())
            .or_default()
            .running = true;
        self.sync_save_snapshot(tab_id);
        crate::diagnostic!(serde_json::json!({
            "component": "documents",
            "kind": "file.save_started",
            "device": place.device_id,
            "path": place.relative,
        }));
        let request = SaveRequest {
            contents,
            expected,
            close_after,
            place,
        };
        let worker_tab = tab_id.to_owned();
        let spawned = thread::Builder::new()
            .name("herdr-core-file-save".to_owned())
            .spawn(move || {
                let outcome = files::save_document(
                    channel.as_ref(),
                    &request.place,
                    &request.contents,
                    &request.expected,
                );
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_document_save(&worker_tab, request, outcome),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawned {
            if let Some(slot) = self.document_saves.get_mut(tab_id) {
                slot.running = false;
            }
            self.sync_save_snapshot(tab_id);
            self.set_error(
                "file.save_worker_failed",
                format!("The file save worker could not start: {error}; the draft was preserved"),
                true,
            );
        }
    }

    pub(super) fn ingest_document_save(
        &mut self,
        tab_id: &str,
        request: SaveRequest,
        outcome: SaveOutcome,
    ) -> bool {
        let queued = match self.document_saves.get_mut(tab_id) {
            Some(slot) => {
                slot.running = false;
                slot.queued.take()
            }
            None => None,
        };
        let path = format!("{}/{}", request.place.root.path, request.place.relative);
        let Some(document) = self.editor_documents.get_mut(tab_id) else {
            self.document_saves.remove(tab_id);
            self.push_diagnostic(
                "file.save_after_close",
                format!(
                    "A save of {path} finished after its tab closed: {}",
                    outcome_word(&outcome)
                ),
            );
            return true;
        };
        let mut next = None;
        let mut close = false;
        match outcome {
            SaveOutcome::Saved(saved) => {
                document.revision = Some(saved.revision);
                document.opened_modified_at_unix_ms = Some(saved.modified_at_unix_ms);
                document.conflict = None;
                if document.contents_utf8.as_deref() == Some(request.contents.as_str()) {
                    document.dirty = false;
                }
                let dirty = document.dirty;
                self.push_diagnostic("file.save_ready", format!("Saved {path}"));
                match queued {
                    Some(waiting) if waiting.contents != request.contents => next = Some(waiting),
                    Some(waiting) => close = waiting.close_after && !dirty,
                    None => close = request.close_after && !dirty,
                }
            }
            SaveOutcome::Conflict {
                disk_revision,
                message,
            } => {
                document.conflict = Some(EditorConflictSnapshot {
                    opened_revision: request.expected,
                    disk_revision,
                });
                document.dirty = true;
                self.set_error("file.save_conflict", message, true);
            }
            SaveOutcome::Refused(message) => {
                self.set_error("file.save_failed", message, true);
            }
            SaveOutcome::Unknown(reason) => {
                let slot = self.document_saves.entry(tab_id.to_owned()).or_default();
                slot.unsettled = Some(UnsettledSave {
                    contents: request.contents,
                    expected: request.expected,
                });
                self.set_error(
                    "file.save_unknown",
                    format!(
                        "{reason}. The draft is kept, and Hide reads the file back to learn whether the save landed."
                    ),
                    true,
                );
                self.sync_save_snapshot(tab_id);
                self.start_save_settle(tab_id);
            }
        }
        self.sync_file_tab_dirty(tab_id);
        self.sync_save_snapshot(tab_id);
        if let Some(next) = next {
            self.start_document_save(tab_id, next.contents, next.close_after);
        } else if close {
            return self.close_file_tab_now(tab_id);
        }
        true
    }

    /// Reads back the file of a save whose answer was lost. Waits, without
    /// sending anything, while the device cannot be reached.
    pub(super) fn start_save_settle(&mut self, tab_id: &str) {
        let Some(slot) = self.document_saves.get(tab_id) else {
            return;
        };
        let Some(unsettled) = slot.unsettled.clone() else {
            return;
        };
        if slot.checking {
            if let Some(slot) = self.document_saves.get_mut(tab_id) {
                slot.recheck = true;
            }
            return;
        }
        let Some(place) = self.document_places.get(tab_id).cloned() else {
            return;
        };
        let channel = match self.device_channel(&place.device_id) {
            Ok(channel) => channel,
            Err(message) => {
                if let Some(slot) = self.document_saves.get_mut(tab_id) {
                    slot.waiting = Some(message);
                }
                self.sync_save_snapshot(tab_id);
                return;
            }
        };
        let Some(context) = self.worker_context.clone() else {
            return;
        };
        if let Some(slot) = self.document_saves.get_mut(tab_id) {
            slot.checking = true;
            slot.recheck = false;
        }
        self.sync_save_snapshot(tab_id);
        let worker_tab = tab_id.to_owned();
        let spawned = thread::Builder::new()
            .name("herdr-core-file-settle".to_owned())
            .spawn(move || {
                let result = files::revision_now(channel.as_ref(), &place);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_save_settle(&worker_tab, unsettled, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if spawned.is_err() {
            if let Some(slot) = self.document_saves.get_mut(tab_id) {
                slot.checking = false;
            }
            self.sync_save_snapshot(tab_id);
        }
    }

    pub(super) fn ingest_save_settle(
        &mut self,
        tab_id: &str,
        unsettled: UnsettledSave,
        result: Result<Option<String>, HostCallError>,
    ) -> bool {
        let Some(slot) = self.document_saves.get_mut(tab_id) else {
            return false;
        };
        slot.checking = false;
        if slot.unsettled.as_ref() != Some(&unsettled) {
            return false;
        }
        let Some(document) = self.editor_documents.get_mut(tab_id) else {
            self.document_saves.remove(tab_id);
            return false;
        };
        let saved = hide_host::document::revision_of(unsettled.contents.as_bytes());
        match result {
            Ok(Some(revision)) if revision == saved => {
                slot.unsettled = None;
                slot.waiting = None;
                document.revision = Some(revision);
                document.conflict = None;
                if document.contents_utf8.as_deref() == Some(unsettled.contents.as_str()) {
                    document.dirty = false;
                }
                let path = document.path.clone();
                self.push_diagnostic(
                    "file.save_settled_saved",
                    format!("The unanswered save of {path} had reached the file"),
                );
            }
            Ok(Some(revision)) if revision == unsettled.expected => {
                slot.unsettled = None;
                slot.waiting = None;
                self.set_error(
                    "file.save_not_applied",
                    "The unanswered save did not reach the file. The draft is kept; save again when ready.",
                    true,
                );
            }
            Ok(disk_revision) => {
                slot.unsettled = None;
                slot.waiting = None;
                document.dirty = true;
                document.conflict = Some(EditorConflictSnapshot {
                    opened_revision: unsettled.expected,
                    disk_revision,
                });
                self.set_error(
                    "file.save_conflict",
                    "The file changed while the save's result was unknown; choose Reload or Keep Editing. The draft is kept.",
                    true,
                );
            }
            Err(HostCallError::Refused(error)) => {
                slot.unsettled = None;
                slot.waiting = None;
                document.dirty = true;
                document.conflict = Some(EditorConflictSnapshot {
                    opened_revision: unsettled.expected,
                    disk_revision: None,
                });
                self.set_error(
                    "file.save_conflict",
                    format!(
                        "{}; the unanswered save cannot be read back. The draft is kept.",
                        error.message
                    ),
                    true,
                );
            }
            Err(error) => {
                slot.unsettled = Some(unsettled);
                slot.waiting = Some(error.to_string());
                if slot.recheck {
                    self.sync_save_snapshot(tab_id);
                    self.start_save_settle(tab_id);
                    return true;
                }
            }
        }
        self.sync_file_tab_dirty(tab_id);
        self.sync_save_snapshot(tab_id);
        true
    }

    /// A device's helper came back: every save on it whose answer was lost
    /// is read back now.
    pub(super) fn settle_device_saves(&mut self, device_id: &str) {
        let waiting: Vec<String> = self
            .document_saves
            .iter()
            .filter(|(tab_id, slot)| {
                slot.unsettled.is_some()
                    && self
                        .document_places
                        .get(*tab_id)
                        .is_some_and(|place| place.device_id == device_id)
            })
            .map(|(tab_id, _)| tab_id.clone())
            .collect();
        for tab_id in waiting {
            self.start_save_settle(&tab_id);
        }
    }

    fn sync_save_snapshot(&mut self, tab_id: &str) {
        let save = self.document_saves.get(tab_id).and_then(|slot| {
            if slot.unsettled.is_some() {
                Some(EditorSaveSnapshot {
                    state: if slot.checking { "checking" } else { "unknown" }.to_owned(),
                    message: Some(match &slot.waiting {
                        Some(reason) => format!(
                            "The last save's result is unknown; waiting to read the file back: {reason}"
                        ),
                        None => "The last save's result is unknown; reading the file back".to_owned(),
                    }),
                })
            } else if slot.running {
                Some(EditorSaveSnapshot {
                    state: "saving".to_owned(),
                    message: None,
                })
            } else {
                None
            }
        });
        if let Some(document) = self.editor_documents.get_mut(tab_id) {
            document.save = save;
        }
        self.sync_active_editor_document();
    }

    /// Forgets a closed tab's place; a save still running settles into
    /// nothing when it answers.
    pub(super) fn forget_document(&mut self, tab_id: &str) {
        self.document_places.remove(tab_id);
        if let Some(slot) = self.document_saves.get(tab_id)
            && !slot.running
            && !slot.checking
        {
            self.document_saves.remove(tab_id);
        }
        if self.document_opens.remove(tab_id).is_some() {
            self.sync_opening_snapshot();
        }
    }
}

/// What a device read is for, as the caller that starts it names it.
pub(super) struct OpenRequestFields {
    pub(super) workspace_id: String,
    pub(super) checkout_id: String,
    pub(super) path: String,
    pub(super) preview: bool,
    pub(super) reload: bool,
}

fn outcome_word(outcome: &SaveOutcome) -> &'static str {
    match outcome {
        SaveOutcome::Saved(_) => "saved",
        SaveOutcome::Conflict { .. } => "conflict",
        SaveOutcome::Refused(_) => "refused",
        SaveOutcome::Unknown(_) => "unknown",
    }
}
