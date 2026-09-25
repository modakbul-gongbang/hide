use std::sync::Arc;

use super::*;

/// Turns a taken delta into the bytes the shell reads.
///
/// It takes the payload and nothing else. That signature is the guarantee the
/// runtime mutex is not held here: there is no runtime in scope to lock. The
/// caller takes a payload under the lock, drops the guard, and calls this.
pub fn serialize_snapshot_delta(
    payload: &crate::model::SnapshotDeltaPayload,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&crate::model::SnapshotDeltaWire::borrow(payload))
}

/// Revision bookkeeping for the delta snapshot wire. Revisions are stamped
/// lazily at read time by comparing live sections against the last stamped
/// copy, so mutation sites carry no dirty-tracking obligations.
#[derive(Default)]
pub(super) struct DeltaState {
    revision: u64,
    rest_revision: u64,
    editor_revision: u64,
    changes_revision: u64,
    /// Reference-counted so a delta can carry the section out of the lock
    /// without copying it. The runtime never mutates one in place: a changed
    /// section becomes a new `Arc`, which leaves any payload already handed
    /// out holding the state it was taken at.
    last_rest: Option<Arc<crate::model::RestSections>>,
    last_editor: Option<Arc<crate::model::EditorSnapshot>>,
    last_changes: Option<Arc<crate::model::ChangesSnapshot>>,
    /// The View documents on screen, each stamped with its own revision
    /// (PRD S7 contract 3.1), so a keystroke re-sends only its document.
    /// Empty in a shell without View areas.
    documents: HashMap<String, StampedDocument>,
    documents_visible: Vec<String>,
    documents_visible_revision: u64,
}

struct StampedDocument {
    revision: u64,
    document: Arc<EditorDocumentSnapshot>,
}

impl Runtime {
    /// Takes one delta response for the snapshot wire: sections whose revision
    /// passed `have_revision`, plus terminal chunks past `have_sequence`.
    /// Reading is idempotent - the same cursors return the same delta again -
    /// so a caller that failed to apply a response recovers by re-reading with
    /// its unadvanced cursors.
    ///
    /// This is the half that needs the runtime, and it is deliberately the
    /// only half: it stamps revisions and copies out what the wire needs, and
    /// `serialize_snapshot_delta` turns that into bytes with the lock already
    /// released. Serializing here would put the whole navigator, ui state and
    /// terminal output through `serde_json` while every attach thread and the
    /// shell's next read wait on the mutex.
    pub fn snapshot_delta_payload(
        &mut self,
        have_revision: u64,
        have_sequence: u64,
    ) -> crate::model::SnapshotDeltaPayload {
        use crate::model::{RestSections, SnapshotDeltaPayload};

        // A front Workspace moved by Herdr or by a device's own focus, not by
        // an event, is followed here, before anything is stamped.
        self.sync_workspace_view();
        if !self
            .delta
            .last_rest
            .as_ref()
            .is_some_and(|rest| rest.matches(&self.snapshot))
        {
            self.delta.revision += 1;
            self.delta.rest_revision = self.delta.revision;
            self.delta.last_rest = Some(Arc::new(RestSections::capture(&self.snapshot)));
        }
        if self.delta.last_editor.as_deref() != Some(&self.snapshot.editor) {
            self.delta.revision += 1;
            self.delta.editor_revision = self.delta.revision;
            self.delta.last_editor = Some(Arc::new(self.snapshot.editor.clone()));
        }
        if self.delta.last_changes.as_deref() != Some(&self.snapshot.changes) {
            self.delta.revision += 1;
            self.delta.changes_revision = self.delta.revision;
            self.delta.last_changes = Some(Arc::new(self.snapshot.changes.clone()));
        }
        let views = self.separate_view_areas();
        if views {
            self.stamp_view_documents();
        }
        // A cursor from the future has no valid meaning in-process; treat it
        // as a fresh reader so the response converges on full state.
        let have_revision = if have_revision > self.delta.revision {
            0
        } else {
            have_revision
        };

        // Chunks are cloned rather than drained: a caller whose apply failed
        // re-reads with the same cursor and has to get the same bytes back.
        let chunks: Vec<_> = self
            .snapshot
            .terminal
            .chunks
            .iter()
            .filter(|chunk| chunk.sequence > have_sequence)
            .cloned()
            .collect();
        let chunks_dropped = match self.snapshot.terminal.chunks.first() {
            Some(oldest) => have_sequence + 1 < oldest.sequence,
            None => have_sequence < self.snapshot.terminal.sequence,
        };

        SnapshotDeltaPayload {
            schema_version: self.snapshot.schema_version,
            revision: self.delta.revision,
            // The retained copies were compared against the live snapshot
            // above and rebuilt where they differed, so each is the live
            // section and costs a refcount instead of a copy. All three are
            // stamped by that block, so a missing one is a broken invariant
            // and not a section to send as null.
            rest: (self.delta.rest_revision > have_revision).then(|| {
                Arc::clone(
                    self.delta
                        .last_rest
                        .as_ref()
                        .expect("the rest section is stamped before a delta is taken"),
                )
            }),
            editor: (self.delta.editor_revision > have_revision).then(|| {
                Arc::clone(
                    self.delta
                        .last_editor
                        .as_ref()
                        .expect("the editor section is stamped before a delta is taken"),
                )
            }),
            changes: (self.delta.changes_revision > have_revision).then(|| {
                Arc::clone(
                    self.delta
                        .last_changes
                        .as_ref()
                        .expect("the changes section is stamped before a delta is taken"),
                )
            }),
            documents: views
                .then(|| self.view_documents_delta(have_revision))
                .flatten(),
            find: self.snapshot.find.clone(),
            input_generation: self.snapshot.input_generation,
            terminal_sequence: self.snapshot.terminal.sequence,
            chunks,
            chunks_dropped,
        }
    }

    /// Stamps each visible View document whose contents differ from its last
    /// stamped copy, and the visible set when it changed. At most one
    /// document per View area is compared, the comparison the editor section
    /// makes for its one document.
    fn stamp_view_documents(&mut self) {
        let visible = self.visible_view_documents();
        for tab_id in &visible {
            let Some(document) = self.editor_documents.get(tab_id) else {
                continue;
            };
            if self
                .delta
                .documents
                .get(tab_id)
                .is_some_and(|stamped| *stamped.document == *document)
            {
                continue;
            }
            self.delta.revision += 1;
            self.delta.documents.insert(
                tab_id.clone(),
                StampedDocument {
                    revision: self.delta.revision,
                    document: Arc::new(document.clone()),
                },
            );
        }
        // A document off screen is dropped, so it is sent again, whole, when
        // it comes back: the shell drops it too.
        self.delta
            .documents
            .retain(|tab_id, _| visible.contains(tab_id));
        if visible != self.delta.documents_visible {
            self.delta.revision += 1;
            self.delta.documents_visible_revision = self.delta.revision;
            self.delta.documents_visible = visible;
        }
    }

    /// The `documents` section for a reader at `have_revision`: absent when
    /// nothing in it changed past that revision, complete for a fresh reader.
    fn view_documents_delta(&self, have_revision: u64) -> Option<crate::model::DocumentsDelta> {
        let changed: Vec<_> = self
            .delta
            .documents_visible
            .iter()
            .filter_map(|tab_id| {
                let stamped = self.delta.documents.get(tab_id)?;
                (stamped.revision > have_revision)
                    .then(|| (tab_id.clone(), Arc::clone(&stamped.document)))
            })
            .collect();
        (have_revision == 0
            || self.delta.documents_visible_revision > have_revision
            || !changed.is_empty())
        .then(|| crate::model::DocumentsDelta {
            visible: self.delta.documents_visible.clone(),
            changed,
        })
    }
}
