//! File tabs on an SSH device (PRD S5.5 B8-B15, B34): the read runs on a
//! worker and lands in the checkout that asked, and a save is checked
//! against the revision the draft was based on, runs one at a time, and is
//! read back rather than resent when its answer is lost.
//!
//! The device is a double of the helper connection behind the same
//! `HostChannel` boundary: it answers with the helper's own dispatch, so the
//! file work is real, and it can hold a request or lose an answer.

use super::*;
use crate::host_access::{HostCallError, HostChannel, InProcessHost};
use hide_host::protocol::Call;
use serde_json::Value;
use std::sync::Condvar;

const DEVICE: &str = "device-a";
const WORKSPACE: &str = "remote:device-a:workspace:1";
const CHECKOUT: &str = "remote:device-a:checkout:1";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Answer {
    Normally,
    /// The write happens and its answer never arrives.
    LoseAfterEffect,
    /// The connection drops before the write reaches the device.
    LoseBeforeEffect,
    /// The write happens, its answer is lost, and the link stays down.
    LoseAfterEffectThenDrop,
    /// Every request fails without being sent: the device is unreachable.
    Unreachable,
    /// The next request fails as unreachable, on a connection that has
    /// since been replaced; the ones after it are answered.
    UnreachableOnce,
}

#[derive(Default)]
struct Gate {
    held: bool,
    waiting: usize,
}

pub(super) struct FakeDevice {
    answer: Mutex<Answer>,
    gate: Mutex<Gate>,
    released: Condvar,
    saves: Mutex<Vec<String>>,
}

impl FakeDevice {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(Answer::Normally),
            gate: Mutex::new(Gate::default()),
            released: Condvar::new(),
            saves: Mutex::new(Vec::new()),
        })
    }

    fn answer(&self, answer: Answer) {
        *self.answer.lock().unwrap() = answer;
    }

    pub(super) fn hold(&self) {
        self.gate.lock().unwrap().held = true;
    }

    pub(super) fn release(&self) {
        self.gate.lock().unwrap().held = false;
        self.released.notify_all();
    }

    fn waiting(&self) -> usize {
        self.gate.lock().unwrap().waiting
    }

    fn saves(&self) -> Vec<String> {
        self.saves.lock().unwrap().clone()
    }
}

impl HostChannel for FakeDevice {
    fn call(&self, call: Call, timeout: Duration) -> Result<Value, HostCallError> {
        {
            let mut gate = self.gate.lock().unwrap();
            gate.waiting += 1;
            while gate.held {
                gate = self.released.wait(gate).unwrap();
            }
            gate.waiting -= 1;
        }
        let answer = *self.answer.lock().unwrap();
        let is_save = matches!(call, Call::Save { .. });
        if let Call::Save { contents, .. } = &call {
            self.saves.lock().unwrap().push(contents.clone());
        }
        match answer {
            Answer::Unreachable => Err(HostCallError::NotConnected(
                "The device helper is not connected".to_owned(),
            )),
            Answer::UnreachableOnce => {
                *self.answer.lock().unwrap() = Answer::Normally;
                Err(HostCallError::NotConnected(
                    "The device helper is not connected".to_owned(),
                ))
            }
            Answer::LoseBeforeEffect if is_save => Err(HostCallError::Unknown(
                "The connection ended before the device answered".to_owned(),
            )),
            Answer::LoseAfterEffectThenDrop if is_save => {
                let _ = InProcessHost.call(call, timeout);
                *self.answer.lock().unwrap() = Answer::Unreachable;
                Err(HostCallError::Unknown(
                    "The connection ended before the device answered".to_owned(),
                ))
            }
            Answer::LoseAfterEffect if is_save => {
                let _ = InProcessHost.call(call, timeout);
                Err(HostCallError::Unknown(
                    "The connection ended before the device answered".to_owned(),
                ))
            }
            _ => InProcessHost.call(call, timeout),
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    shared: Arc<Mutex<Runtime>>,
    device: Arc<FakeDevice>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("a.txt"), "old\n").unwrap();
        let mut runtime = runtime();
        let mut remote = workspace(
            WORKSPACE,
            "Remote",
            &root.to_string_lossy(),
            vec![checkout(WORKSPACE, CHECKOUT, &root.to_string_lossy(), None)],
        );
        remote.device_id = DEVICE.to_owned();
        remote.remote_target_id = Some(DEVICE.to_owned());
        runtime.snapshot.navigator.workspaces = vec![remote];
        runtime.snapshot.navigator.focused_workspace_id = Some(WORKSPACE.to_owned());
        runtime.snapshot.navigator.focused_checkout_id = Some(CHECKOUT.to_owned());
        let device = FakeDevice::new();
        runtime.device_hosts.insert(
            DEVICE.to_owned(),
            hosts::DeviceHost {
                phase: hosts::HostPhase::Ready {
                    host: device.clone(),
                    platform: "macos aarch64".to_owned(),
                    helper_path: "/fake/hide-host-helper".to_owned(),
                },
                generation: 1,
            },
        );
        let shared = Arc::new(Mutex::new(runtime));
        shared
            .lock()
            .unwrap()
            .install_worker_context(Arc::downgrade(&shared), crate::ffi::ChangeNotifier::noop());
        Self {
            _dir: dir,
            root,
            shared,
            device,
        }
    }

    fn path(&self, name: &str) -> String {
        self.root.join(name).to_string_lossy().into_owned()
    }

    fn dispatch(&self, kind: &str, payload: Value) {
        let event =
            serde_json::json!({"schema_version": SCHEMA_VERSION, "kind": kind, "payload": payload});
        self.shared
            .lock()
            .unwrap()
            .dispatch_json(&serde_json::to_vec(&event).unwrap());
    }

    fn open(&self, name: &str) {
        self.dispatch(
            "file_open",
            serde_json::json!({"path": self.path(name), "workspace_id": WORKSPACE, "checkout_id": CHECKOUT}),
        );
    }

    fn save(&self, name: &str, contents: &str) {
        self.dispatch(
            "file_save",
            serde_json::json!({
                "tab_id": Runtime::file_tab_id(WORKSPACE, CHECKOUT, &self.path(name)),
                "path": self.path(name),
                "contents_utf8": contents,
            }),
        );
    }

    fn document(&self, name: &str) -> Option<EditorDocumentSnapshot> {
        let tab_id = Runtime::file_tab_id(WORKSPACE, CHECKOUT, &self.path(name));
        self.shared
            .lock()
            .unwrap()
            .editor_documents
            .get(&tab_id)
            .cloned()
    }

    fn wait(&self, what: &str, mut ready: impl FnMut(&Runtime) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if ready(&self.shared.lock().unwrap()) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_document(
        &self,
        name: &str,
        what: &str,
        ready: impl Fn(&EditorDocumentSnapshot) -> bool,
    ) {
        let tab_id = Runtime::file_tab_id(WORKSPACE, CHECKOUT, &self.path(name));
        self.wait(what, |runtime| {
            runtime.editor_documents.get(&tab_id).is_some_and(&ready)
        });
    }

    fn last_error(&self) -> Option<String> {
        self.shared
            .lock()
            .unwrap()
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.clone())
    }

    fn open_and_wait(&self, name: &str) -> EditorDocumentSnapshot {
        self.open(name);
        self.wait_for_document(name, "the device read", |_| true);
        self.document(name).unwrap()
    }
}

#[test]
fn a_device_file_is_read_on_a_worker_and_shows_as_a_tab_when_it_arrives() {
    let f = Fixture::new();
    f.device.hold();
    f.open("a.txt");
    {
        let runtime = f.shared.lock().unwrap();
        assert!(
            runtime.snapshot.editor.tabs.is_empty(),
            "no tab before the answer"
        );
        assert_eq!(runtime.snapshot.editor.opening.len(), 1);
        assert_eq!(runtime.snapshot.editor.opening[0].path, f.path("a.txt"));
    }
    f.device.release();
    f.wait("the tab", |runtime| {
        runtime.snapshot.editor.document.is_some()
    });
    let runtime = f.shared.lock().unwrap();
    assert!(runtime.snapshot.editor.opening.is_empty());
    let document = runtime.snapshot.editor.document.as_ref().unwrap();
    assert_eq!(document.contents_utf8.as_deref(), Some("old\n"));
    assert_eq!(
        document.revision.as_deref(),
        Some(hide_host::document::revision_of(b"old\n").as_str())
    );
}

/// B34: the answer belongs to the checkout that asked. The operator moved
/// on meanwhile, so the tab joins that checkout without taking the screen.
#[test]
fn a_device_read_that_arrives_after_the_operator_moved_on_does_not_take_the_screen() {
    let f = Fixture::new();
    f.device.hold();
    f.open("a.txt");
    f.shared
        .lock()
        .unwrap()
        .snapshot
        .navigator
        .focused_checkout_id = None;
    f.device.release();
    f.wait("the tab", |runtime| {
        !runtime.snapshot.editor.tabs.is_empty()
    });
    let runtime = f.shared.lock().unwrap();
    assert_eq!(runtime.snapshot.editor.tabs[0].checkout_id, CHECKOUT);
    assert_eq!(runtime.snapshot.editor.active_tab_id, None);
    assert!(runtime.snapshot.editor.document.is_none());
}

#[test]
fn a_device_save_writes_the_draft_and_moves_the_revision() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.save("a.txt", "new\n");
    f.wait_for_document("a.txt", "the save", |document| !document.dirty);
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "new\n"
    );
    let document = f.document("a.txt").unwrap();
    assert_eq!(
        document.revision.as_deref(),
        Some(hide_host::document::revision_of(b"new\n").as_str())
    );
    assert_eq!(document.save, None);
}

/// B13: another writer changed the file; the save is refused, the file and
/// the draft both survive, and Keep Editing makes the next save replace
/// that version on purpose.
#[test]
fn a_device_save_over_a_changed_file_is_a_conflict_until_the_operator_keeps_editing() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    std::fs::write(f.root.join("a.txt"), "theirs\n").unwrap();
    f.save("a.txt", "mine\n");
    f.wait_for_document("a.txt", "the conflict", |document| {
        document.conflict.is_some()
    });
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "theirs\n"
    );
    let document = f.document("a.txt").unwrap();
    assert!(document.dirty);
    assert_eq!(document.contents_utf8.as_deref(), Some("mine\n"));
    let conflict = document.conflict.unwrap();
    assert_eq!(
        conflict.disk_revision.as_deref(),
        Some(hide_host::document::revision_of(b"theirs\n").as_str())
    );
    assert_eq!(f.last_error().as_deref(), Some("file.save_conflict"));

    f.dispatch(
        "file_conflict",
        serde_json::json!({"action": "keep_editing"}),
    );
    f.save("a.txt", "mine\n");
    f.wait_for_document("a.txt", "the save over it", |document| !document.dirty);
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "mine\n"
    );
}

/// B14: the write landed but its answer was lost. The file is read back and
/// the save is recognised as done; nothing is sent twice.
#[test]
fn a_save_whose_answer_was_lost_after_it_landed_is_read_back_as_saved() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.device.answer(Answer::LoseAfterEffect);
    f.save("a.txt", "new\n");
    f.wait_for_document("a.txt", "the read-back", |document| {
        document.save.is_none() && !document.dirty
    });
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "new\n"
    );
    assert_eq!(f.device.saves(), vec!["new\n".to_owned()], "never resent");
    assert_eq!(
        diagnostic_count(&f.shared.lock().unwrap(), "file.save_settled_saved"),
        1
    );
}

/// B14: the connection dropped before the write. The read-back finds the
/// old file, so the draft is kept unsaved and nothing is resent.
#[test]
fn a_save_whose_answer_was_lost_before_it_landed_is_read_back_as_not_saved() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.device.answer(Answer::LoseBeforeEffect);
    f.save("a.txt", "new\n");
    f.wait("the verdict", |runtime| {
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str())
            == Some("file.save_not_applied")
    });
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "old\n"
    );
    let document = f.document("a.txt").unwrap();
    assert!(document.dirty);
    assert_eq!(document.contents_utf8.as_deref(), Some("new\n"));
    assert_eq!(document.save, None);
    assert_eq!(f.device.saves().len(), 1, "never resent");
}

/// B14: while the device cannot be reached the result stays unknown, a new
/// save is refused rather than sent, and the read-back runs when the helper
/// is back.
#[test]
fn an_unknown_save_waits_for_the_device_and_blocks_the_next_save_until_read_back() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.device.answer(Answer::LoseAfterEffectThenDrop);
    f.save("a.txt", "new\n");
    f.wait_for_document("a.txt", "the refused read-back", |document| {
        document.save.as_ref().is_some_and(|save| {
            save.state == "unknown" && save.message.as_deref().unwrap_or("").contains("waiting")
        })
    });

    // The refused save asks for a read-back, which is still on its way to
    // the old connection when the helper comes back.
    f.device.hold();
    f.save("a.txt", "newer\n");
    assert_eq!(f.last_error().as_deref(), Some("file.save_unsettled"));
    f.wait("the read-back to reach the device", |_| {
        f.device.waiting() == 1
    });
    f.shared.lock().unwrap().settle_device_saves(DEVICE);
    f.device.answer(Answer::UnreachableOnce);
    f.device.release();
    assert_eq!(
        f.device.saves(),
        vec!["new\n".to_owned()],
        "the second save was not sent"
    );
    f.wait_for_document("a.txt", "the read-back after reconnect", |document| {
        document.save.is_none()
    });
    let document = f.document("a.txt").unwrap();
    assert_eq!(
        document.revision.as_deref(),
        Some(hide_host::document::revision_of(b"new\n").as_str())
    );
    assert!(document.dirty, "the newer draft is still unsaved");
    assert_eq!(document.contents_utf8.as_deref(), Some("newer\n"));
}

/// D-15: one save per document runs; drafts that arrive meanwhile collapse
/// to the newest, which is sent against the revision the first one wrote.
#[test]
fn drafts_saved_while_a_save_runs_collapse_to_the_newest() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.device.hold();
    f.save("a.txt", "first\n");
    f.wait("the first save to reach the device", |_| {
        f.device.waiting() == 1
    });
    f.save("a.txt", "second\n");
    f.save("a.txt", "third\n");
    assert_eq!(
        f.document("a.txt")
            .unwrap()
            .save
            .map(|save| save.state)
            .as_deref(),
        Some("saving")
    );
    f.device.release();
    f.wait_for_document("a.txt", "the last save", |document| {
        !document.dirty && document.save.is_none()
    });
    assert_eq!(
        f.device.saves(),
        vec!["first\n".to_owned(), "third\n".to_owned()]
    );
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "third\n"
    );
}

/// B52: without a helper (no consent, or revoked) nothing is read or sent,
/// and the reason names what to do.
#[test]
fn a_device_without_a_helper_reads_and_sends_nothing() {
    let f = Fixture::new();
    f.shared.lock().unwrap().device_hosts.clear();
    f.open("a.txt");
    let runtime = f.shared.lock().unwrap();
    assert!(runtime.snapshot.editor.tabs.is_empty());
    assert!(runtime.snapshot.editor.opening.is_empty());
    let error = runtime.snapshot.status.last_error.as_ref().unwrap();
    assert_eq!(error.kind, "file.open_failed");
    assert!(f.device.saves().is_empty());
}

impl Fixture {
    fn set_phase(&self, phase: hosts::HostPhase) {
        self.shared
            .lock()
            .unwrap()
            .device_hosts
            .get_mut(DEVICE)
            .unwrap()
            .phase = phase;
    }

    fn ready(&self) -> hosts::HostPhase {
        hosts::HostPhase::Ready {
            host: self.device.clone(),
            platform: "macos aarch64".to_owned(),
            helper_path: "/fake/hide-host-helper".to_owned(),
        }
    }

    /// The document the shell draws, not the core's own copy.
    fn shown(&self) -> EditorDocumentSnapshot {
        self.shared
            .lock()
            .unwrap()
            .snapshot
            .editor
            .document
            .clone()
            .unwrap()
    }
}

/// A save asked for while the helper is still connecting waits for it
/// rather than failing, keeps only the newest draft, and goes out once the
/// helper is ready; the tab shows the draft and why it has not saved yet.
#[test]
fn a_save_while_the_helper_connects_waits_and_goes_out_when_it_is_ready() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.set_phase(hosts::HostPhase::Connecting);
    f.save("a.txt", "first\n");
    f.save("a.txt", "second\n");
    let shown = f.shown();
    assert!(shown.dirty);
    assert_eq!(shown.contents_utf8.as_deref(), Some("second\n"));
    assert_eq!(
        shown.save.map(|save| save.state).as_deref(),
        Some("waiting")
    );
    assert!(
        f.device.saves().is_empty(),
        "nothing is sent while connecting"
    );

    f.set_phase(f.ready());
    f.shared.lock().unwrap().settle_device_saves(DEVICE);
    f.wait_for_document("a.txt", "the waiting save", |document| {
        !document.dirty && document.save.is_none()
    });
    assert_eq!(f.device.saves(), vec!["second\n".to_owned()]);
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "second\n"
    );
}

/// The helper never came: the waiting save is dropped unsent, and the draft
/// stays in the tab with the reason.
#[test]
fn a_save_waiting_for_a_helper_that_fails_is_not_sent_and_keeps_the_draft() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.set_phase(hosts::HostPhase::Connecting);
    f.save("a.txt", "mine\n");
    f.shared
        .lock()
        .unwrap()
        .release_held_saves(DEVICE, "The SSH connection timed out");
    f.set_phase(hosts::HostPhase::Unavailable("timed out".to_owned()));
    let shown = f.shown();
    assert!(shown.dirty);
    assert_eq!(shown.contents_utf8.as_deref(), Some("mine\n"));
    assert_eq!(shown.save, None);
    assert_eq!(f.last_error().as_deref(), Some("file.save_unavailable"));
    f.shared.lock().unwrap().settle_device_saves(DEVICE);

    // A save refused outright still shows the draft it was given.
    f.save("a.txt", "again\n");
    let shown = f.shown();
    assert!(shown.dirty);
    assert_eq!(shown.contents_utf8.as_deref(), Some("again\n"));
    assert_eq!(f.last_error().as_deref(), Some("file.save_unavailable"));
    assert!(f.device.saves().is_empty());
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "old\n"
    );
}

/// B47: this machine's disk is read on a worker too, so a slow volume holds
/// no runtime lock. An unrelated action applies while the read waits, and a
/// reveal of that file moves the screen only when the read lands.
#[test]
fn a_slow_local_read_blocks_nothing_and_a_reveal_moves_only_when_it_lands() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("nested/b.txt"), "local\n").unwrap();
    let root_text = root.to_string_lossy().into_owned();
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "workspace:local",
        "Local",
        &root_text,
        vec![checkout(
            "workspace:local",
            "checkout:local",
            &root_text,
            None,
        )],
    )];
    runtime.snapshot.navigator.focused_workspace_id = Some("workspace:local".to_owned());
    runtime.snapshot.navigator.focused_checkout_id = Some("checkout:local".to_owned());
    runtime.snapshot.navigator.root_path = Some(root_text.clone());
    runtime.snapshot.ui_state.right_panel_visible = false;
    let disk = FakeDevice::new();
    runtime.local_host = disk.clone();
    let shared = Arc::new(Mutex::new(runtime));
    shared
        .lock()
        .unwrap()
        .install_worker_context(Arc::downgrade(&shared), crate::ffi::ChangeNotifier::noop());
    let dispatch = |event: Vec<u8>| shared.lock().unwrap().dispatch_json(&event);
    let file = root.join("nested/b.txt");

    disk.hold();
    dispatch(reveal_event(
        "workspace:local",
        "checkout:local",
        &file,
        false,
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while disk.waiting() == 0 {
        assert!(Instant::now() < deadline, "the read never reached the disk");
        thread::sleep(Duration::from_millis(5));
    }
    dispatch(
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "ui_state_update",
            "payload": {
                "expanded_paths": [],
                "collapsed_workspace_ids": ["workspace:local"],
                "collapsed_checkout_ids": [],
                "selected_path": null,
                "selected_pane_id": null,
                "shortcut_bindings": {}
            }
        }))
        .unwrap(),
    );
    {
        let runtime = shared.lock().unwrap();
        assert_eq!(
            runtime.snapshot.ui_state.collapsed_workspace_ids,
            vec!["workspace:local".to_owned()]
        );
        assert!(!runtime.snapshot.ui_state.right_panel_visible);
        assert_eq!(runtime.snapshot.ui_state.selected_path, None);
        assert!(runtime.snapshot.editor.tabs.is_empty());
        assert_eq!(runtime.snapshot.editor.opening.len(), 1);
    }

    disk.release();
    let deadline = Instant::now() + Duration::from_secs(5);
    while shared.lock().unwrap().snapshot.editor.document.is_none() {
        assert!(Instant::now() < deadline, "the local read never landed");
        thread::sleep(Duration::from_millis(5));
    }
    let runtime = shared.lock().unwrap();
    let file_text = file.to_string_lossy().into_owned();
    assert!(runtime.snapshot.ui_state.right_panel_visible);
    assert_eq!(
        runtime.snapshot.ui_state.selected_path.as_deref(),
        Some(file_text.as_str())
    );
    assert!(
        runtime
            .snapshot
            .ui_state
            .expanded_paths
            .contains(&root.join("nested").to_string_lossy().into_owned())
    );
    let document = runtime.snapshot.editor.document.as_ref().unwrap();
    assert_eq!(document.contents_utf8.as_deref(), Some("local\n"));
}

/// A device file tab is drawn in that device checkout's strip, after the
/// host's Herdr tabs, and a session sync that brings only Herdr's tabs keeps
/// it there without announcing a change on every poll.
#[test]
fn a_device_file_tab_joins_the_device_strip_and_survives_session_syncs() {
    let f = Fixture::new();
    let session = {
        let mut runtime = f.shared.lock().unwrap();
        let remote = runtime.snapshot.navigator.workspaces.remove(0);
        let session = RemoteSessionSnapshot {
            workspaces: vec![remote],
            agents: Vec::new(),
            active_tab_ids: Default::default(),
            focused_workspace_id: Some(WORKSPACE.to_owned()),
            focused_checkout_id: Some(CHECKOUT.to_owned()),
            focused_tab_id: None,
            focused_pane_id: None,
            pane_layouts: Vec::new(),
        };
        runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
            target_id: DEVICE.to_owned(),
            state: "connected".to_owned(),
            message: None,
            herdr_version: None,
            session: Some(session.clone()),
            files: RemoteFileListSnapshot::idle(),
            catalog: Default::default(),
        });
        runtime.snapshot.navigator.focused_device_id = Some(DEVICE.to_owned());
        runtime.snapshot.navigator.focused_workspace_id = None;
        runtime.snapshot.navigator.focused_checkout_id = None;
        session
    };
    f.open_and_wait("a.txt");
    let tab_id = Runtime::file_tab_id(WORKSPACE, CHECKOUT, &f.path("a.txt"));
    let strip_ids = |runtime: &Runtime| {
        runtime.snapshot.status.remote[0]
            .session
            .as_ref()
            .unwrap()
            .workspaces[0]
            .checkouts[0]
            .strip
            .iter()
            .map(|entry| (entry.kind, entry.source_id.clone()))
            .collect::<Vec<_>>()
    };
    let mut runtime = f.shared.lock().unwrap();
    assert_eq!(
        strip_ids(&runtime),
        vec![(StripTabKind::File, tab_id.clone())]
    );
    assert_eq!(
        runtime.snapshot.editor.active_tab_id.as_deref(),
        Some(tab_id.as_str())
    );

    runtime.ingest_remote_session(DEVICE, Ok(session.clone()));
    assert_eq!(
        strip_ids(&runtime),
        vec![(StripTabKind::File, tab_id.clone())]
    );
    assert!(
        !runtime.ingest_remote_session(DEVICE, Ok(session)),
        "an unchanged host session is not a change"
    );
}
