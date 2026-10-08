//! What the label workers remember across a daemon restart (PRD
//! labels-in-hided D-08): per pane, the proven session owner and its label,
//! which turns were already analyzed, where the conversation was last read,
//! and when the pane last changed state.
//!
//! The file is `labels.json` beside `core-state.json`, written whole through
//! a temporary file and a rename, mode 0600, never under the runtime mutex
//! and never on the thread that applies Herdr's events: a save hands the
//! write to the store's own thread, and dropping the store waits for it.
//! Labels are the provider's short summaries, positions are byte offsets and
//! file identities, turns are hashes; the only conversation text is each
//! local pane's last request and reply, capped (`facts`, PRD overview-request-view
//! B31). Device records remain in memory only. A file
//! that cannot be read starts empty with a diagnostic (B21); nothing in it is
//! shown until the pane's current session reference proves it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use hide_session::ConversationCheckpoint;
use hide_session::turns::{TurnTracker, Waiting};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::analysis::LabelEnd;
use super::facts::SessionFacts;

pub(crate) const LABELS_FILE: &str = "labels.json";
const SCHEMA_VERSION: u32 = 1;
/// The store key of the test core's own Herdr server: its node id.
#[cfg(test)]
pub(crate) const LOCAL_TARGET: &str = "test-node";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct PaneRecord {
    /// The provider's native session, proven from transcript metadata
    /// (`label_reference_token(provider, "id", native id)`).
    #[serde(default)]
    pub(crate) owner: Option<String>,
    /// Native metadata proven for `owner`; missing on prior helper answers.
    #[serde(default)]
    pub(crate) native_session_id: Option<String>,
    /// The Pi reader's resolved path, re-proved before any effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) native_source_path: Option<String>,
    /// The Herdr reference under which `owner` was last proven. A path
    /// reference names the same session as its id only once a read has
    /// shown it; an id reference proves itself.
    #[serde(default)]
    pub(crate) proven_reference: Option<String>,
    /// The session's goal (D-38); a v3 record named it `task`.
    #[serde(default, alias = "task")]
    pub(crate) goal: Option<String>,
    /// The last analysis's line; a v3 record's `progress`.
    #[serde(default, alias = "progress")]
    pub(crate) line: String,
    /// How the last analyzed turn ended, until the agent runs again.
    #[serde(default)]
    pub(crate) end: Option<LabelEnd>,
    /// A v3 record's question and the reply it asked for, read only to
    /// carry a pending question into `end` and `line` on load; retired with
    /// the v3 records (`upgrade_v3`).
    #[serde(default, rename = "question", skip_serializing)]
    pub(crate) v3_question: bool,
    #[serde(default, rename = "expected_reply", skip_serializing)]
    pub(crate) v3_expected_reply: String,
    /// The last human event that fed goal analysis.
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
    /// The turn the last read ended in, which the next read continues, for
    /// an agent whose session read reports its turns (`hide_session::turns`).
    #[serde(default)]
    pub(crate) turns: Option<TurnTracker>,
    /// Herdr's `state_change_seq` the last complete read was asked under.
    /// What `turns` says the agent waits for is known for that state only
    /// (PRD codex-plan-approval-hold D-06); `None` while a read is partial.
    #[serde(default)]
    pub(crate) turns_seq: Option<u64>,
    /// Herdr's per-pane state counter and when the core saw it move, which
    /// is the pane's elapsed time and recency whichever session it runs.
    #[serde(default)]
    pub(crate) state_change_seq: u64,
    #[serde(default)]
    pub(crate) changed_unix_ms: u64,
    #[serde(default)]
    pub(crate) agent_status: Option<String>,
    /// What the session says without any AI; see `facts`.
    #[serde(default)]
    pub(crate) facts: SessionFacts,
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

    /// Carries a v3 record's pending question into v5's fields, so a row
    /// that was asking still asks after the upgrade.
    fn upgrade_v3(&mut self) {
        let question = std::mem::take(&mut self.v3_question);
        let reply = std::mem::take(&mut self.v3_expected_reply);
        if self.end.is_none() && question && !reply.trim().is_empty() {
            self.end = Some(LabelEnd::Question);
            self.line = reply;
        }
    }

    /// A new session in the pane: nothing the old one said or decided stays.
    pub(crate) fn reset_session(&mut self, owner: Option<String>) {
        self.owner = owner;
        self.native_session_id = None;
        self.native_source_path = None;
        self.proven_reference = None;
        self.goal = None;
        self.line.clear();
        self.end = None;
        self.facts = SessionFacts::default();
        self.reset_analysis();
    }

    /// The same session's transcript started over (replaced or truncated):
    /// the label stays, the turn bookkeeping does not.
    pub(crate) fn reset_analysis(&mut self) {
        self.task_input_cursor = None;
        self.analysis_turn_start = None;
        self.analysis_turn_end = None;
        self.facts.forget_offsets();
    }

    pub(crate) fn forget_position(&mut self) {
        self.read_reference = None;
        self.checkpoint = None;
        self.anchor = None;
        self.incarnation = None;
        self.turns = None;
        self.turns_seq = None;
    }

    /// What the last complete read says the agent waits for, with the Herdr
    /// state it was read under; the wait is `None` when the records read do
    /// not settle it.
    pub(crate) fn turn_read(&self) -> Option<(u64, Option<Waiting>)> {
        Some((self.turns_seq?, self.turns.as_ref()?.waiting()))
    }

    pub(crate) fn user_turn(&self) -> Option<(u64, hide_session::turns::UserTurnFact)> {
        Some((self.turns_seq?, self.turns.as_ref()?.user_turn()?))
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
    /// The store key of the core's own Herdr server: its node id. Only this
    /// target reaches disk.
    node: String,
    file: Arc<Mutex<LabelsFile>>,
    /// Held from encoding to rename, so two workers saving at once cannot
    /// share the temporary file and the last write carries the newest data.
    writing: Arc<Mutex<()>>,
    /// The thread that writes this machine's records, started by the first
    /// save: a write ends in an fsync, and the workers save from the
    /// coordinator thread that applies Herdr's events (PRD
    /// instant-pane-topology D-13). It ends, after its last write, when the
    /// store is dropped.
    saver: Mutex<Option<Saver>>,
}

/// The saver thread's requests: `None` asks for a save, and `Some` is
/// answered once every save asked for before it is on disk.
type SaveRequest = Option<Sender<()>>;

struct Saver {
    requests: Sender<SaveRequest>,
    worker: JoinHandle<()>,
}

impl LabelStore {
    /// Loads the store beside `state_dir`, or imports the retired plugin's
    /// state from `home` once when there is no store yet (D-11).
    pub(crate) fn open(state_dir: Option<&Path>, home: Option<&Path>, node: &str) -> Self {
        let Some(path) = state_dir.map(|dir| dir.join(LABELS_FILE)) else {
            return Self::in_memory(node);
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
                        file.targets.insert(node.to_owned(), imported);
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
        let mut targets = file.targets;
        // Older versions wrote device facts here. Do not restore them,
        // and the save below removes those records from the existing file.
        targets.retain(|target, _| target == node);
        for record in targets.values_mut().flat_map(BTreeMap::values_mut) {
            record.upgrade_v3();
        }
        let store = Self {
            path: Some(path),
            node: node.to_owned(),
            writing: Arc::new(Mutex::new(())),
            file: Arc::new(Mutex::new(LabelsFile {
                version: SCHEMA_VERSION,
                targets,
            })),
            saver: Mutex::new(None),
        };
        // An import is written at once, so the kit's later removal of the
        // plugin's folder cannot take it away.
        store.save();
        store
    }

    pub(crate) fn in_memory(node: &str) -> Self {
        Self {
            path: None,
            node: node.to_owned(),
            writing: Arc::new(Mutex::new(())),
            file: Arc::new(Mutex::new(LabelsFile {
                version: SCHEMA_VERSION,
                targets: BTreeMap::new(),
            })),
            saver: Mutex::new(None),
        }
    }

    /// The store key of the core's own Herdr server.
    pub(crate) fn node(&self) -> &str {
        &self.node
    }

    /// A worker's starting records.
    pub(crate) fn target(&self, key: &str) -> BTreeMap<String, PaneRecord> {
        self.lock().targets.get(key).cloned().unwrap_or_default()
    }

    /// Replaces a worker's records. Only this machine's records reach disk;
    /// a device's conversation stays available to its worker in memory.
    pub(crate) fn save_target(&self, key: &str, records: &BTreeMap<String, PaneRecord>) {
        {
            let mut file = self.lock();
            if file.targets.get(key) == Some(records) {
                return;
            }
            file.targets.insert(key.to_owned(), records.clone());
        }
        if key == self.node {
            self.save_later();
        }
    }

    /// Returns once every save asked for so far is on disk; the product
    /// waits for its saves only by dropping the store.
    #[cfg(test)]
    pub(crate) fn flush(&self) {
        let saver = self.saver.lock().unwrap_or_else(|error| error.into_inner());
        let Some(saver) = saver.as_ref() else {
            return;
        };
        let (answer, answered) = channel();
        if saver.requests.send(Some(answer)).is_ok() {
            let _ = answered.recv();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LabelsFile> {
        self.file.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Hands the write to the saver thread; saves asked for while one is
    /// being written become one more write.
    fn save_later(&self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let mut saver = self.saver.lock().unwrap_or_else(|error| error.into_inner());
        if saver.is_none() {
            let (requests, receiver) = channel::<SaveRequest>();
            let file = Arc::clone(&self.file);
            let writing = Arc::clone(&self.writing);
            let node = self.node.clone();
            let spawned = std::thread::Builder::new()
                .name("hide-labels-save".to_owned())
                .spawn(move || {
                    while let Ok(request) = receiver.recv() {
                        let mut flushes = Vec::new();
                        let mut save = false;
                        for request in std::iter::once(request).chain(receiver.try_iter()) {
                            match request {
                                None => save = true,
                                Some(answer) => flushes.push(answer),
                            }
                        }
                        if save {
                            write_store(&path, &node, &writing, &file);
                        }
                        for answer in flushes {
                            let _ = answer.send(());
                        }
                    }
                });
            match spawned {
                Ok(worker) => *saver = Some(Saver { requests, worker }),
                Err(error) => {
                    crate::diagnostic!(json!({
                        "component": "labels",
                        "kind": "store.saver_failed",
                        "message": error.to_string(),
                    }));
                    drop(saver);
                    self.save();
                    return;
                }
            }
        }
        if let Some(running) = saver.as_ref()
            && running.requests.send(None).is_err()
        {
            // The saver ended (a write panicked): this save is written here,
            // and the next one starts a new saver.
            crate::diagnostic!(json!({
                "component": "labels",
                "kind": "store.saver_ended",
            }));
            *saver = None;
            drop(saver);
            self.save();
        }
    }

    fn save(&self) {
        if let Some(path) = self.path.as_deref() {
            write_store(path, &self.node, &self.writing, &self.file);
        }
    }
}

impl Drop for LabelStore {
    fn drop(&mut self) {
        let saver = self
            .saver
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(Saver { requests, worker }) = saver {
            drop(requests);
            if worker.join().is_err() {
                crate::diagnostic!(json!({
                    "component": "labels",
                    "kind": "store.saver_join_failed",
                }));
            }
        }
    }
}

/// Encodes this machine's records and writes them in place of the file.
fn write_store(path: &Path, node: &str, writing: &Mutex<()>, file: &Mutex<LabelsFile>) {
    let _writing = writing.lock().unwrap_or_else(|error| error.into_inner());
    let encoded = {
        let file = file.lock().unwrap_or_else(|error| error.into_inner());
        // Filter at the final serialization boundary as well: a later
        // local save must never serialize device entries held in memory.
        #[derive(Serialize)]
        struct LocalFile<'a> {
            version: u32,
            targets: BTreeMap<&'a str, &'a BTreeMap<String, PaneRecord>>,
        }
        serde_json::to_vec(&LocalFile {
            version: SCHEMA_VERSION,
            targets: file
                .targets
                .iter()
                .filter(|(target, _)| target.as_str() == node)
                .map(|(target, records)| (target.as_str(), records))
                .collect(),
        })
    };
    let bytes = match encoded {
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
            goal: Some("라벨 저장소 확인".to_owned()),
            state_change_seq: 3,
            changed_unix_ms: 1_000,
            ..PaneRecord::default()
        }
    }

    #[test]
    fn a_saved_target_survives_a_reopen() {
        let root = tempfile::tempdir().unwrap();
        let store = LabelStore::open(Some(root.path()), None, LOCAL_TARGET);
        let mut records = BTreeMap::new();
        records.insert("w1:p1".to_owned(), record("v1:owner"));
        store.save_target(LOCAL_TARGET, &records);
        // Dropping the store waits for the write it handed off.
        drop(store);
        let reopened = LabelStore::open(Some(root.path()), None, LOCAL_TARGET);
        assert_eq!(reopened.target(LOCAL_TARGET), records);
        assert!(hide_platform::fs::private::is_private(&root.path().join(LABELS_FILE)).unwrap());
    }

    #[test]
    fn device_conversation_stays_in_memory_even_after_a_local_save() {
        let root = tempfile::tempdir().unwrap();
        let store = LabelStore::open(Some(root.path()), None, LOCAL_TARGET);
        let mut device = record("remote-owner");
        device.facts = serde_json::from_value(json!({
            "operator_request": {"text":"private remote operator", "at_unix_ms":1, "requester":{"kind":"operator"}},
            "other_request": {"text":"private remote sender", "at_unix_ms":2, "requester":{"kind":"agent"}},
            "reply": {"text":"private remote reply", "at_unix_ms":3}
        })).unwrap();
        let remote = BTreeMap::from([("w2:p1".to_owned(), device)]);
        store.save_target("device:mini", &remote);
        assert_eq!(store.target("device:mini"), remote);
        let local = BTreeMap::from([("w1:p1".to_owned(), record("local-owner"))]);
        store.save_target(LOCAL_TARGET, &local);
        store.flush();
        let disk = std::fs::read_to_string(root.path().join(LABELS_FILE)).unwrap();
        assert!(
            !disk.contains("private remote"),
            "device conversation reached disk"
        );
        let reopened = LabelStore::open(Some(root.path()), None, LOCAL_TARGET);
        assert_eq!(reopened.target(LOCAL_TARGET), local);
        assert!(reopened.target("device:mini").is_empty());
        assert_eq!(store.target("device:mini"), remote);
    }

    #[test]
    fn opening_a_legacy_store_removes_device_records_from_disk() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(LABELS_FILE);
        std::fs::write(&path, serde_json::to_vec(&json!({"version":1,"targets":{
            LOCAL_TARGET:{"w1:p1":record("local-owner")},
            "device:mini":{"w2:p1":{"facts":{"reply":{"text":"legacy private remote","at_unix_ms":1}}}}
        }})).unwrap()).unwrap();
        let opened = LabelStore::open(Some(root.path()), None, LOCAL_TARGET);
        assert_eq!(opened.target(LOCAL_TARGET)["w1:p1"], record("local-owner"));
        assert!(opened.target("device:mini").is_empty());
        assert!(
            !std::fs::read_to_string(path)
                .unwrap()
                .contains("legacy private remote")
        );
    }

    #[test]
    fn a_v3_record_keeps_its_goal_and_its_pending_question() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(LABELS_FILE),
            r#"{"version":1,"targets":{"test-node":{
                "w1:p1":{"owner":"v1:a","task":"요청 보기 만들기","progress":"테스트 중","question":true,"expected_reply":"A/B 선택"},
                "w1:p2":{"owner":"v1:b","task":"라벨 저장소 확인","progress":"끝남"}}}}"#,
        )
        .unwrap();
        let records = LabelStore::open(Some(root.path()), None, LOCAL_TARGET).target(LOCAL_TARGET);
        let asking = &records["w1:p1"];
        assert_eq!(asking.goal.as_deref(), Some("요청 보기 만들기"));
        assert_eq!(asking.end, Some(LabelEnd::Question));
        assert_eq!(asking.line, "A/B 선택");
        let done = &records["w1:p2"];
        assert_eq!(done.end, None);
        assert_eq!(done.line, "끝남");
        let written = std::fs::read_to_string(root.path().join(LABELS_FILE)).unwrap();
        assert!(!written.contains("expected_reply"), "{written}");
    }

    #[test]
    fn a_corrupt_file_starts_empty() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(LABELS_FILE), b"{not json").unwrap();
        let store = LabelStore::open(Some(root.path()), None, LOCAL_TARGET);
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
