//! File tabs on an SSH device (PRD S5.5 B8-B15, B34): the read runs on a
//! worker and lands in the checkout that asked, and a save is checked
//! against the revision the draft was based on, runs one at a time, and is
//! read back rather than resent when its answer is lost.
//!
//! The device is a double of the helper connection behind the same
//! `HostChannel` boundary: it answers with the helper's own dispatch, so the
//! file work is real, and it can hold a request or lose an answer.

use super::*;
use crate::host_access::{HostAnswer, HostCallError, HostChannel, InProcessHost};
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
    pins: Mutex<HashMap<String, hide_host::RootIdentity>>,
    closes: Mutex<Vec<&'static str>>,
}

impl FakeDevice {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(Answer::Normally),
            gate: Mutex::new(Gate::default()),
            released: Condvar::new(),
            saves: Mutex::new(Vec::new()),
            pins: Mutex::new(HashMap::new()),
            closes: Mutex::new(Vec::new()),
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
    fn close(&self, _reason: &str) {
        self.closes.lock().unwrap().push("close");
    }

    fn close_when_idle(&self, _reason: &str) {
        self.closes.lock().unwrap().push("when_idle");
    }

    fn call(&self, call: Call, timeout: Duration) -> Result<HostAnswer, HostCallError> {
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

    fn pinned(&self, root: &str) -> Option<hide_host::RootIdentity> {
        self.pins.lock().unwrap().get(root).copied()
    }

    fn pin(&self, root: &str, identity: Option<hide_host::RootIdentity>) {
        let mut pins = self.pins.lock().unwrap();
        match identity {
            Some(identity) => pins.insert(root.to_owned(), identity),
            None => pins.remove(root),
        };
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

/// A read still running when its device is removed opens no tab when its
/// answer lands.
#[test]
fn a_device_read_that_lands_after_the_device_was_removed_opens_nothing() {
    let f = Fixture::new();
    f.device.hold();
    f.open("a.txt");
    {
        let mut runtime = f.shared.lock().unwrap();
        runtime.retire_device_editor_tabs(DEVICE);
        assert!(runtime.snapshot.editor.opening.is_empty());
    }
    f.device.release();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while f.device.waiting() > 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    let runtime = f.shared.lock().unwrap();
    assert!(runtime.snapshot.editor.tabs.is_empty());
    assert!(runtime.snapshot.editor.document.is_none());
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
/// old file, so the draft is kept, the tab says the save has not reached the
/// file yet (it may still), nothing is resent, and Retry saves.
#[test]
fn a_save_whose_answer_was_lost_before_it_landed_is_read_back_as_not_saved_yet() {
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
    assert_eq!(
        document.save.map(|save| save.state).as_deref(),
        Some("not_applied"),
        "the tab says the save has not reached the file"
    );
    assert_eq!(f.device.saves().len(), 1, "never resent");

    f.device.answer(Answer::Normally);
    f.save("a.txt", "new\n");
    f.wait_for_document("a.txt", "the retried save", |document| !document.dirty);
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "new\n"
    );
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
    assert_eq!(
        shown.save.map(|save| save.state).as_deref(),
        Some("refused")
    );
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

/// B52: withdrawing consent while a save waits for the helper drops that save
/// unsent, so it cannot go out on its own if consent is given again.
#[test]
fn withdrawing_consent_drops_a_save_waiting_for_the_helper() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.set_phase(hosts::HostPhase::Connecting);
    f.save("a.txt", "mine\n");
    {
        let mut runtime = f.shared.lock().unwrap();
        runtime
            .snapshot
            .ui_state
            .device_registrations
            .push(crate::model::DeviceRegistration {
                id: DEVICE.to_owned(),
                label: DEVICE.to_owned(),
                ssh_alias: Some(DEVICE.to_owned()),
                herdr_socket_path: None,
                host_consent: None,
            });
        runtime.set_host_consent(DEVICE, false);
    }
    let shown = f.shown();
    assert!(shown.dirty);
    assert_eq!(shown.contents_utf8.as_deref(), Some("mine\n"));
    assert_eq!(
        shown.save.map(|save| (
            save.state,
            save.message
                .unwrap_or_default()
                .contains("no longer allowed")
        )),
        Some(("refused".to_owned(), true)),
        "the save no longer waits and the tab says why"
    );
    assert_eq!(f.last_error().as_deref(), Some("file.save_unavailable"));
    f.set_phase(f.ready());
    f.shared.lock().unwrap().settle_device_saves(DEVICE);
    assert!(
        f.device.saves().is_empty(),
        "nothing goes out once allowed again"
    );
}

/// B52: a save already running when consent is withdrawn lands with its
/// real result; the connection is closed only once it is idle, and no new
/// save is sent.
#[test]
fn withdrawing_consent_lets_a_running_save_land() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.device.hold();
    f.save("a.txt", "mine\n");
    {
        let mut runtime = f.shared.lock().unwrap();
        runtime
            .snapshot
            .ui_state
            .device_registrations
            .push(crate::model::DeviceRegistration {
                id: DEVICE.to_owned(),
                label: DEVICE.to_owned(),
                ssh_alias: Some(DEVICE.to_owned()),
                herdr_socket_path: None,
                host_consent: None,
            });
        runtime.set_host_consent(DEVICE, false);
    }
    assert_eq!(*f.device.closes.lock().unwrap(), vec!["when_idle"]);
    f.device.release();
    f.wait_for_document("a.txt", "the running save", |document| {
        !document.dirty && document.save.is_none()
    });
    assert_eq!(f.device.saves(), vec!["mine\n".to_owned()]);
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "mine\n"
    );
    f.save("a.txt", "after\n");
    assert_eq!(f.device.saves().len(), 1, "nothing new is sent");
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

/// B16, B18, B34: an Explorer change on a device's checkout is made by that
/// device's host, off the lock, and an open tab of the item follows it so its
/// next save lands on the new path; a change the tree asked for on another
/// device than the one in front is refused and changes nothing.
#[test]
fn a_device_explorer_change_runs_on_its_host_and_the_open_tab_follows_it() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    let root = f.root.to_string_lossy().into_owned();

    f.dispatch(
        "file_create",
        serde_json::json!({"root": root, "parent": root, "name": "planted.txt"}),
    );
    f.wait("the refusal", |runtime| {
        runtime
            .snapshot
            .explorer_operation
            .as_ref()
            .is_some_and(|operation| operation.phase == "failed")
    });
    assert!(
        !f.root.join("planted.txt").exists(),
        "the tree that asked is not the one in front"
    );

    f.dispatch(
        "path_rename",
        serde_json::json!({"root": root, "path": f.path("a.txt"), "name": "b.txt", "device_id": DEVICE}),
    );
    f.wait("the rename", |runtime| {
        runtime
            .snapshot
            .explorer_operation
            .as_ref()
            .is_some_and(|operation| operation.phase == "finished")
    });
    assert!(f.root.join("b.txt").is_file() && !f.root.join("a.txt").exists());
    let tab_id = Runtime::file_tab_id(WORKSPACE, CHECKOUT, &f.path("a.txt"));
    assert_eq!(
        f.shared.lock().unwrap().document_places[&tab_id].relative,
        "b.txt"
    );
    f.dispatch(
        "file_save",
        serde_json::json!({"tab_id": tab_id, "path": f.path("b.txt"), "contents_utf8": "new\n"}),
    );
    f.wait("the save", |runtime| {
        runtime
            .editor_documents
            .get(&tab_id)
            .is_some_and(|document| {
                document.revision.as_deref()
                    == Some(hide_host::document::revision_of(b"new\n").as_str())
            })
    });
    assert_eq!(
        std::fs::read_to_string(f.root.join("b.txt")).unwrap(),
        "new\n"
    );
    assert!(
        !f.root.join("a.txt").exists(),
        "the save did not recreate the old path"
    );
}

/// A device folder removed and made again at the same path after it was
/// listed is a different folder: a change or an open names the root the
/// listing pinned, so it is refused there instead of landing in the new one.
#[test]
fn a_device_folder_replaced_after_it_was_listed_takes_no_change_or_open() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap().join("checkout");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "first").unwrap();
    let device = FakeDevice::new();
    let root_text = root.to_string_lossy().into_owned();
    crate::host_access::list_folder(device.as_ref(), &root_text, "").unwrap();

    std::fs::rename(&root, dir.path().join("moved")).unwrap();
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "second").unwrap();
    let place = files::DocumentRoot {
        device_id: DEVICE.to_owned(),
        path: root_text.clone(),
        identity: None,
    };
    let rename = files::ExplorerOperation::rename(&root, &root.join("a.txt"), "b.txt").unwrap();
    assert!(files::apply_explorer_operation(device.as_ref(), &place, &rename).is_err());
    assert!(root.join("a.txt").is_file(), "the new folder is untouched");

    let refused = files::open_document(
        device.as_ref(),
        &place,
        &root.join("a.txt").to_string_lossy(),
    );
    assert!(refused.is_err());
    // The refused open unpinned the root; the operator's next open adopts
    // the folder now at that path.
    let (document, _) = files::open_document(
        device.as_ref(),
        &place,
        &root.join("a.txt").to_string_lossy(),
    )
    .unwrap();
    assert_eq!(document.contents_utf8.as_deref(), Some("second"));
}

/// Reload and a save both decide which text and revision the tab holds, so
/// one waits for the other instead of taking its state away: a reload
/// landing mid-save would leave the save's revision on the reloaded text,
/// and the next save would overwrite the file without a conflict.
#[test]
fn reload_and_save_do_not_overtake_each_other_on_one_tab() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    f.device.hold();
    f.save("a.txt", "saved\n");
    f.wait("the save to be sent", |_| f.device.waiting() == 1);
    f.dispatch("file_conflict", serde_json::json!({"action": "reload"}));
    assert_eq!(f.last_error().as_deref(), Some("file.reload_busy"));
    f.device.release();
    f.wait_for_document("a.txt", "the save", |document| !document.dirty);
    assert_eq!(
        f.document("a.txt").unwrap().revision.as_deref(),
        Some(hide_host::document::revision_of(b"saved\n").as_str())
    );

    std::fs::write(f.root.join("a.txt"), "outside\n").unwrap();
    f.device.hold();
    f.dispatch("file_conflict", serde_json::json!({"action": "reload"}));
    f.wait("the reload to be sent", |_| f.device.waiting() == 1);
    f.save("a.txt", "typed during reload\n");
    assert_eq!(f.last_error().as_deref(), Some("file.save_during_reload"));
    assert!(
        f.device
            .saves()
            .iter()
            .all(|sent| sent != "typed during reload\n")
    );
    f.device.release();
    f.wait_for_document("a.txt", "the reload", |document| {
        document.contents_utf8.as_deref() == Some("outside\n")
    });
    assert_eq!(
        std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
        "outside\n"
    );
}

/// B19, B22: a device checkout's History is read by that device's helper,
/// and an answer for the same path on another device is never shown: moving
/// to this machine's checkout at that path drops the device's list in the
/// same frame, and a device read that lands afterwards is refused.
#[test]
fn a_device_checkouts_history_comes_from_its_helper_and_stays_with_its_device() {
    let f = Fixture::new();
    let git = |arguments: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&f.root)
                .args(arguments)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    let mut runtime = f.shared.lock().unwrap();
    runtime.snapshot.ui_state.right_panel_visible = true;
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Changes;
    runtime.sync_changes_root_path();
    let request = runtime.changes_request().unwrap();
    assert_eq!(request.root.device_id, DEVICE);
    let device_answer = || crate::changes::ChangesAnswer {
        key: Some(request.key()),
        changes: crate::changes::read(&request),
    };
    let listed = device_answer();
    assert_eq!(listed.changes.unavailable_reason, None);
    assert_eq!(listed.changes.entries[0].relative_path, "a.txt");
    assert!(runtime.ingest_changes(listed));

    let local_id = "workspace:local-same-path";
    let local_checkout = "checkout:local-same-path";
    let path = f.root.to_string_lossy().into_owned();
    runtime.snapshot.navigator.workspaces.push(workspace(
        local_id,
        "Local",
        &path,
        vec![checkout(local_id, local_checkout, &path, None)],
    ));
    runtime.snapshot.navigator.focused_workspace_id = Some(local_id.to_owned());
    runtime.snapshot.navigator.focused_checkout_id = Some(local_checkout.to_owned());
    runtime.sync_changes_root_path();
    assert_eq!(
        runtime.snapshot.navigator.changes_root_path.as_deref(),
        Some(path.as_str())
    );
    assert_eq!(
        runtime.snapshot.changes,
        crate::model::ChangesSnapshot::default()
    );
    assert!(!runtime.ingest_changes(device_answer()));
    assert_eq!(
        runtime.snapshot.changes,
        crate::model::ChangesSnapshot::default()
    );
}

/// B23, B32, B34: a device's closed file is reopened from that device and
/// through its host, never from this machine, and a reopen on the device does
/// not take this machine's newer close off its stack.
#[test]
fn a_closed_device_file_reopens_only_on_its_device_and_leaves_this_machines_close() {
    let f = Fixture::new();
    f.open_and_wait("a.txt");
    let tab_id = Runtime::file_tab_id(WORKSPACE, CHECKOUT, &f.path("a.txt"));
    f.dispatch("file_close", serde_json::json!({"tab_id": tab_id}));
    {
        let mut runtime = f.shared.lock().unwrap();
        assert!(runtime.snapshot.editor.tabs.is_empty());
        // This machine closed a file after the device's.
        runtime.push_recent_closed(super::closed_file("local-close", "/repo/local.txt"));
        runtime.snapshot.navigator.devices.push(DeviceSnapshot {
            id: DEVICE.to_owned(),
            label: "Device".to_owned(),
            kind: "remote".to_owned(),
            state: "connected".to_owned(),
            message: None,
            problem: None,
            ssh_alias: Some(DEVICE.to_owned()),
            herdr_socket_path: None,
            agent_count: 0,
            test: None,
            host: Default::default(),
        });
        assert_eq!(runtime.snapshot.recent_closed.count, 1);
        assert_eq!(
            runtime.snapshot.recent_closed.top_label.as_deref(),
            Some("local.txt"),
            "this machine in front offers only its own close"
        );
    }

    f.dispatch("focus_device", serde_json::json!({"device_id": DEVICE}));
    {
        let runtime = f.shared.lock().unwrap();
        assert_eq!(runtime.snapshot.recent_closed.count, 1);
        assert_eq!(
            runtime.snapshot.recent_closed.top_label.as_deref(),
            Some("a.txt")
        );
        assert!(runtime.snapshot.recent_closed.can_reopen);
    }
    f.dispatch("reopen_closed", serde_json::json!({}));
    f.wait("the device file back in its tab", |runtime| {
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.id == tab_id)
    });
    let runtime = f.shared.lock().unwrap();
    assert_eq!(
        runtime.editor_documents[&tab_id].contents_utf8.as_deref(),
        Some("old\n"),
        "read back through the device's host"
    );
    assert_eq!(runtime.snapshot.recent_closed.count, 0);
    assert_eq!(runtime.recent_closed.len(), 1);
    assert_eq!(runtime.recent_closed[0].label(), "local.txt");
}

/// S6 B20, B21: after a restart a device Workspace's View tabs wait for the
/// device helper rather than coming back unavailable while it connects.
#[test]
fn a_device_workspaces_view_tabs_wait_for_its_helper_after_a_restart() {
    use crate::workspace_views::{ViewTabKind, ViewTabRecord};
    let f = Fixture::new();
    let root = f.root.to_string_lossy().into_owned();
    let views_dir = tempfile::tempdir().unwrap();
    {
        let mut runtime = f.shared.lock().unwrap();
        runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
            target_id: DEVICE.to_owned(),
            state: "connected".to_owned(),
            message: None,
            herdr_version: None,
            session: None,
            files: RemoteFileListSnapshot::idle(),
            catalog: crate::model::DeviceCatalogSnapshot {
                state: "ready".to_owned(),
                ..Default::default()
            },
        });
        runtime.device_hosts.get_mut(DEVICE).unwrap().phase = hosts::HostPhase::Connecting;
        let mut store =
            WorkspaceViewStore::open(views_dir.path().join("views.json"), Default::default()).0;
        let record = ViewTabRecord {
            path: f.path("a.txt"),
            kind: ViewTabKind::File,
            committed: None,
            preview: false,
        };
        let entry = store.views.entry(DEVICE, &root);
        // A diff tab comes back at once, so the editor changes while the
        // file is still being read.
        let diff = ViewTabRecord {
            path: f.path("a.txt"),
            kind: ViewTabKind::Diff,
            committed: Some(false),
            preview: false,
        };
        entry.tabs = vec![diff, record.clone()];
        entry.active = Some(record);
        runtime.workspace_views = Some(store);
        runtime.sync_workspace_view();
        assert!(
            runtime.snapshot.editor.tabs.is_empty(),
            "nothing is restored, or marked unavailable, while the helper connects"
        );
        runtime.device_hosts.get_mut(DEVICE).unwrap().phase = hosts::HostPhase::Ready {
            host: f.device.clone(),
            platform: "macos aarch64".to_owned(),
            helper_path: "/fake/hide-host-helper".to_owned(),
        };
        runtime.snapshot.status.remote[0].catalog.state = "resolving".to_owned();
        assert!(
            !runtime.restore_front_when_ready(),
            "the device's checkouts are not in their Projects yet"
        );
        runtime.snapshot.status.remote[0].catalog.state = "ready".to_owned();
        f.device.hold();
        assert!(runtime.restore_front_when_ready());
        runtime.sync_workspace_view();
        let saved = &runtime
            .workspace_views
            .as_ref()
            .unwrap()
            .views
            .get(DEVICE, &root)
            .unwrap()
            .tabs;
        assert_eq!(
            saved.len(),
            2,
            "a tab still being read back stays in the saved list"
        );
    }
    f.device.release();
    f.wait_for_document("a.txt", "the restored device file", |_| true);
    let runtime = f.shared.lock().unwrap();
    let tab = runtime
        .snapshot
        .editor
        .tabs
        .iter()
        .find(|tab| tab.path == f.path("a.txt"))
        .expect("the restored tab");
    assert_eq!(tab.unavailable_reason, None);
}
