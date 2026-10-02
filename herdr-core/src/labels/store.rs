//! What the label workers remember across a daemon restart (PRD
//! labels-in-hided D-08): per pane, the proven session owner and its label,
//! which turns were already analyzed, where the conversation was last read,
//! and when the pane last changed state.
//!
//! The file is `labels.json` beside `core-state.json`, written whole through
//! a temporary file and a rename, mode 0600, never under the runtime mutex.
//! It holds no conversation text: labels are the provider's short summaries,
//! positions are byte offsets and file identities, turns are hashes. A file
//! that cannot be read starts empty with a diagnostic (B21); nothing in it is
//! shown until the pane's current session reference proves it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hide_session::ConversationCheckpoint;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub(crate) const LABELS_FILE: &str = "labels.json";
const SCHEMA_VERSION: u32 = 1;
/// The store key of this machine's Herdr server.
pub(crate) const LOCAL_TARGET: &str = "local";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct PaneRecord {
    /// The provider's native session, proven from transcript metadata
    /// (`label_reference_token(provider, "id", native id)`).
    #[serde(default)]
    pub(crate) owner: Option<String>,
    /// The Herdr reference under which `owner` was last proven. A path
    /// reference names the same session as its id only once a read has
    /// shown it; an id reference proves itself.
    #[serde(default)]
    pub(crate) proven_reference: Option<String>,
    #[serde(default)]
    pub(crate) task: Option<String>,
    #[serde(default)]
    pub(crate) progress: String,
    #[serde(default)]
    pub(crate) expected_reply: String,
    /// The last analysis read a question to the operator.
    #[serde(default)]
    pub(crate) question: bool,
    /// The last human event that fed task analysis.
    #[serde(default)]
    pub(crate) task_input_cursor: Option<u64>,
    /// The turn whose start was named and the turn whose end was judged.
    #[serde(default)]
    pub(crate) analysis_turn_start: Option<u64>,
    #[serde(default)]
    pub(crate) analysis_turn_end: Option<u64>,
    /// The reference whose file `checkpoint` and `anchor` belong to.
    #[serde(default)]
    pub(crate) read_reference: Option<String>,
    /// Where the last read stopped.
    #[serde(default)]
    pub(crate) checkpoint: Option<ConversationCheckpoint>,
    /// The last human record read, so a restarted reader recovers the
    /// current turn without reading the conversation from its start.
    #[serde(default)]
    pub(crate) anchor: Option<ConversationCheckpoint>,
    #[serde(default)]
    pub(crate) incarnation: Option<String>,
    /// Herdr's per-pane state counter and when the core saw it move, which
    /// is the pane's elapsed time and recency whichever session it runs.
    #[serde(default)]
    pub(crate) state_change_seq: u64,
    #[serde(default)]
    pub(crate) changed_unix_ms: u64,
    #[serde(default)]
    pub(crate) agent_status: Option<String>,
}

/// Whether `reference` names the session a label was made for: its owner,
/// or a path reference a read has shown to be that owner.
pub(crate) fn proves(
    owner: Option<&str>,
    proven_reference: Option<&str>,
    reference: Option<&str>,
) -> bool {
    let (Some(reference), Some(owner)) = (reference, owner) else {
        return false;
    };
    owner == reference || proven_reference == Some(reference)
}

impl PaneRecord {
    pub(crate) fn first_seen(state_change_seq: u64, now_unix_ms: u64) -> Self {
        Self {
            state_change_seq,
            changed_unix_ms: now_unix_ms,
            ..Self::default()
        }
    }

    /// Whether the label belongs to the session the pane runs now. With no
    /// current reference nothing is shown, and the proof is kept for when
    /// the same reference returns (session-label-isolation B9).
    pub(crate) fn proven_for(&self, reference: Option<&str>) -> bool {
        proves(
            self.owner.as_deref(),
            self.proven_reference.as_deref(),
            reference,
        )
    }

    /// A new session in the pane: nothing the old one said or decided stays.
    pub(crate) fn reset_session(&mut self, owner: Option<String>) {
        self.owner = owner;
        self.proven_reference = None;
        self.task = None;
        self.progress.clear();
        self.expected_reply.clear();
        self.question = false;
        self.reset_analysis();
    }

    /// The same session's transcript started over (replaced or truncated):
    /// the label stays, the turn bookkeeping does not.
    pub(crate) fn reset_analysis(&mut self) {
        self.task_input_cursor = None;
        self.analysis_turn_start = None;
        self.analysis_turn_end = None;
    }

    pub(crate) fn forget_position(&mut self) {
        self.read_reference = None;
        self.checkpoint = None;
        self.anchor = None;
        self.incarnation = None;
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LabelsFile {
    version: u32,
    #[serde(default)]
    targets: BTreeMap<String, BTreeMap<String, PaneRecord>>,
}

/// The daemon's one `labels.json`, shared by the worker of every Herdr
/// server it follows. Each worker owns its target's entry.
pub(crate) struct LabelStore {
    path: Option<PathBuf>,
    file: Mutex<LabelsFile>,
    /// Held from encoding to rename, so two workers saving at once cannot
    /// share the temporary file and the last write carries the newest data.
    writing: Mutex<()>,
}

impl LabelStore {
    /// Loads the store beside `state_dir`, or imports the retired plugin's
    /// state from `home` once when there is no store yet (D-11).
    pub(crate) fn open(state_dir: Option<&Path>, home: Option<&Path>) -> Self {
        let Some(path) = state_dir.map(|dir| dir.join(LABELS_FILE)) else {
            return Self::in_memory();
        };
        let file = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<LabelsFile>(&bytes) {
                Ok(file) if file.version == SCHEMA_VERSION => file,
                Ok(file) => {
                    crate::diagnostic!(json!({
                        "component": "labels",
                        "kind": "store.version_unknown",
                        "version": file.version,
                    }));
                    LabelsFile::default()
                }
                Err(error) => {
                    crate::diagnostic!(json!({
                        "component": "labels",
                        "kind": "store.unreadable",
                        "message": error.to_string(),
                    }));
                    LabelsFile::default()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut file = LabelsFile::default();
                if let Some(home) = home {
                    let imported = super::import::plugin_state(home);
                    if !imported.is_empty() {
                        crate::diagnostic!(json!({
                            "component": "labels",
                            "kind": "store.imported",
                            "panes": imported.len(),
                        }));
                        file.targets.insert(LOCAL_TARGET.to_owned(), imported);
                    }
                }
                file
            }
            Err(error) => {
                crate::diagnostic!(json!({
                    "component": "labels",
                    "kind": "store.unreadable",
                    "message": error.to_string(),
                }));
                LabelsFile::default()
            }
        };
        let store = Self {
            path: Some(path),
            writing: Mutex::new(()),
            file: Mutex::new(LabelsFile {
                version: SCHEMA_VERSION,
                targets: file.targets,
            }),
        };
        // An import is written at once, so the kit's later removal of the
        // plugin's folder cannot take it away.
        store.save();
        store
    }

    pub(crate) fn in_memory() -> Self {
        Self {
            path: None,
            writing: Mutex::new(()),
            file: Mutex::new(LabelsFile {
                version: SCHEMA_VERSION,
                targets: BTreeMap::new(),
            }),
        }
    }

    /// A worker's starting records.
    pub(crate) fn target(&self, key: &str) -> BTreeMap<String, PaneRecord> {
        self.lock().targets.get(key).cloned().unwrap_or_default()
    }

    /// Replaces a worker's records and writes the file.
    pub(crate) fn save_target(&self, key: &str, records: &BTreeMap<String, PaneRecord>) {
        {
            let mut file = self.lock();
            if file.targets.get(key) == Some(records) {
                return;
            }
            file.targets.insert(key.to_owned(), records.clone());
        }
        self.save();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LabelsFile> {
        self.file.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn save(&self) {
        let Some(path) = self.path.as_deref() else {
            return;
        };
        let _writing = self
            .writing
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let bytes = match serde_json::to_vec(&*self.lock()) {
            Ok(bytes) => bytes,
            Err(error) => {
                crate::diagnostic!(json!({
                    "component": "labels",
                    "kind": "store.encode_failed",
                    "message": error.to_string(),
                }));
                return;
            }
        };
        if let Err(error) = write_private(path, &bytes) {
            crate::diagnostic!(json!({
                "component": "labels",
                "kind": "store.write_failed",
                "message": error.to_string(),
            }));
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| std::io::Error::other("labels path has no directory"))?;
    std::fs::create_dir_all(directory)?;
    hide_platform::fs::atomic::write_file(path, bytes, hide_platform::fs::Access::Private)
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(owner: &str) -> PaneRecord {
        PaneRecord {
            owner: Some(owner.to_owned()),
            task: Some("라벨 저장소 확인".to_owned()),
            state_change_seq: 3,
            changed_unix_ms: 1_000,
            ..PaneRecord::default()
        }
    }

    #[test]
    fn a_saved_target_survives_a_reopen() {
        let root = tempfile::tempdir().unwrap();
        let store = LabelStore::open(Some(root.path()), None);
        let mut records = BTreeMap::new();
        records.insert("w1:p1".to_owned(), record("v1:owner"));
        store.save_target(LOCAL_TARGET, &records);
        let reopened = LabelStore::open(Some(root.path()), None);
        assert_eq!(reopened.target(LOCAL_TARGET), records);
        assert!(hide_platform::fs::private::is_private(&root.path().join(LABELS_FILE)).unwrap());
    }

    #[test]
    fn a_corrupt_file_starts_empty() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(LABELS_FILE), b"{not json").unwrap();
        let store = LabelStore::open(Some(root.path()), None);
        assert!(store.target(LOCAL_TARGET).is_empty());
    }

    #[test]
    fn a_label_is_shown_only_under_the_reference_that_proves_it() {
        let mut record = record("v1:id-a");
        assert!(record.proven_for(Some("v1:id-a")));
        assert!(!record.proven_for(None));
        assert!(!record.proven_for(Some("v1:path-a")));
        record.proven_reference = Some("v1:path-a".to_owned());
        assert!(record.proven_for(Some("v1:path-a")));
        assert!(!record.proven_for(Some("v1:id-b")));
        let mut ownerless = record.clone();
        ownerless.owner = None;
        assert!(!ownerless.proven_for(Some("v1:path-a")));
    }
}
