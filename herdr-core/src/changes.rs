//! The right panel's changes view: one checkout's Git working-tree state, read
//! by the host that holds the checkout (PRD S5.5 B19-B22).
//!
//! The read is `hide_host::git` behind the checkout's `HostChannel`: this
//! machine's in process, a device's through its helper, so a local and a
//! device checkout answer one contract. It runs on a reader thread, never
//! under the runtime mutex and never on a Herdr session's coordinator: a
//! device's History does not depend on this machine running Herdr.
//! [`ChangesPump`] drives [`ChangesReader`], which recomputes only when the
//! request changes or the refresh window lapses, and reads nothing at all
//! while neither Changes nor Explorer is visible and no diff tab needs it.
//! Each answer names the device and folder it describes, so the runtime drops
//! one that arrives after the operator moved on.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::Duration;

use hide_host::git::{ChangedFile, Changes, FileStatus};
use hide_host::protocol::Call;

use crate::ffi::ChangeNotifier;
use crate::files::DocumentRoot;
use crate::host_access::{HostCallError, HostChannel, call_as};
use crate::model::{
    ChangedFileDiffSnapshot, ChangedFileSnapshot, ChangedFileStatus, ChangesSnapshot,
};
use crate::reader::BackgroundRead;
use crate::runtime::Runtime;

/// How stale the list may be while the view is open. Short enough that an edit
/// made in a terminal pane shows up by the time the operator looks over, long
/// enough that it is nowhere near a per-tick fork.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// How often the pump asks the runtime what History needs and collects a
/// finished read: the latency between selecting a row and its read starting,
/// and between a read finishing and the screen showing it. One brief lock per
/// wake; the read itself runs on the reader's thread.
const PUMP_TICK: Duration = Duration::from_millis(250);

/// How long one read may take on the checkout's host before it is reported
/// as unavailable. A device's Git runs over its SSH connection.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The host a request reads through, compared by identity: a reconnect gives
/// a device a new channel, which is a new request and reads again.
#[derive(Clone)]
pub struct ChannelRef(pub Arc<dyn HostChannel>);

impl PartialEq for ChannelRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ChannelRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ChannelRef")
    }
}

/// What the runtime wants read: the checkout, the folder under it the view
/// describes, the file whose diff to fetch, and what the committed group is
/// measured against. Absent while neither Changes nor Explorer is showing and
/// no diff tab is active, which keeps the reader out of the common
/// hidden-panel path.
#[derive(Clone, Debug, PartialEq)]
pub struct ChangesRequest {
    /// The checkout's root on its device, which the host opens and runs Git
    /// in.
    pub root: DocumentRoot,
    /// The channel to that device's host, or why there is none now.
    pub channel: Result<ChannelRef, String>,
    /// The folder the view describes: the checkout root, or a registered
    /// folder below it.
    pub root_path: PathBuf,
    pub selected_path: Option<String>,
    /// Whether the selection is in the committed group, which decides what
    /// its diff is taken against.
    pub selected_committed: bool,
    /// What the committed group is measured against, from the worktree
    /// reader's answer. `None` lets the host use the repository's default
    /// branch, and without one only the uncommitted group is produced.
    pub base_branch: Option<String>,
}

impl ChangesRequest {
    pub fn key(&self) -> ChangesKey {
        ChangesKey {
            device_id: self.root.device_id.clone(),
            root_path: self.root_path.to_string_lossy().into_owned(),
        }
    }
}

/// Which device's folder an answer describes. The same path on two devices
/// is two keys.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangesKey {
    pub device_id: String,
    pub root_path: String,
}

pub struct ChangesAnswer {
    /// `None` for the closed view's empty projection.
    pub key: Option<ChangesKey>,
    pub changes: ChangesSnapshot,
}

pub struct ChangesReader {
    inner: BackgroundRead<Option<ChangesRequest>, ChangesAnswer>,
}

impl ChangesReader {
    pub fn new() -> Self {
        Self {
            inner: BackgroundRead::new(
                REFRESH_INTERVAL,
                Duration::ZERO,
                |request: &Option<ChangesRequest>| {
                    match request {
                        Some(request) => ChangesAnswer {
                            key: Some(request.key()),
                            changes: read(request),
                        },
                        // The view is closed, so there is nothing to describe. An
                        // empty projection also drops the retained diff text off
                        // the wire.
                        None => ChangesAnswer {
                            key: None,
                            changes: ChangesSnapshot::default(),
                        },
                    }
                },
            ),
        }
    }

    /// An answer on the wake its read finishes, `None` otherwise. A changed
    /// request always starts a read, so selecting a file does not wait out
    /// the window.
    pub fn read_if_due(&mut self, request: Option<ChangesRequest>) -> Option<ChangesAnswer> {
        self.inner.poll(request)
    }
}

impl Default for ChangesReader {
    fn default() -> Self {
        Self::new()
    }
}

/// The one thread that keeps History current for the checkout in front,
/// whichever device holds it. It lives with the core, not with a Herdr
/// session, so a hided that serves only devices still reads their History.
pub(crate) struct ChangesPump {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ChangesPump {
    pub fn spawn(runtime: Weak<Mutex<Runtime>>, notifier: ChangeNotifier) -> std::io::Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("changes-pump".into())
            .spawn(move || {
                let mut reader = ChangesReader::new();
                while matches!(
                    receiver.recv_timeout(PUMP_TICK),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    let Some(runtime) = runtime.upgrade() else {
                        break;
                    };
                    let Ok(request) = runtime.lock().map(|mut runtime| runtime.changes_request())
                    else {
                        break;
                    };
                    let Some(answer) = reader.read_if_due(request) else {
                        continue;
                    };
                    let Ok(changed) = runtime
                        .lock()
                        .map(|mut runtime| runtime.ingest_changes(answer))
                    else {
                        break;
                    };
                    drop(runtime);
                    if changed {
                        notifier.notify();
                    }
                }
            })?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for ChangesPump {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Reads `request` on its checkout's host and projects the answer. Blocks on
/// the channel; a refusal or a lost answer is a stated reason, never an empty
/// list, so a failure is not shown as a clean tree.
pub fn read(request: &ChangesRequest) -> ChangesSnapshot {
    let root_path = request.root_path.to_string_lossy().into_owned();
    let unavailable = |reason: String| ChangesSnapshot {
        root_path: Some(root_path.clone()),
        unavailable_reason: Some(reason),
        ..ChangesSnapshot::default()
    };
    let channel = match &request.channel {
        Ok(channel) => Arc::clone(&channel.0),
        Err(reason) => return unavailable(reason.clone()),
    };
    let Some(scope) = request
        .root_path
        .strip_prefix(&request.root.path)
        .ok()
        .map(|scope| scope.to_string_lossy().into_owned())
    else {
        return unavailable(
            "This History folder no longer matches its registered checkout".to_owned(),
        );
    };
    let selected = request.selected_path.as_ref().and_then(|path| {
        Path::new(path)
            .strip_prefix(&request.root_path)
            .ok()
            .map(|relative| relative.to_string_lossy().into_owned())
    });
    let root = match crate::files::root_ref(channel.as_ref(), &request.root) {
        Ok(root) => root,
        Err(error) => return unavailable(reason(error)),
    };
    let answer: Result<Changes, _> = call_as(
        channel.as_ref(),
        Call::Changes {
            root,
            scope,
            selected,
            committed: request.selected_committed,
            base: request.base_branch.clone(),
        },
        READ_TIMEOUT,
    );
    let changes = match answer {
        Ok(changes) => changes,
        Err(error) => return unavailable(reason(error)),
    };
    let project = |files: Vec<ChangedFile>| {
        files
            .into_iter()
            .map(|file| snapshot_of(&request.root_path, file))
            .collect::<Vec<_>>()
    };
    let entries = project(changes.entries);
    let committed = changes.committed.map(project);
    // A base the host could not resolve means there is nothing to compare
    // against, so the committed group is absent rather than empty.
    let base_branch = committed.as_ref().and(changes.base);
    let diff = changes.diff.map(|diff| ChangedFileDiffSnapshot {
        path: absolute(&request.root_path, &diff.path),
        text: diff.text,
        notice: diff.notice,
    });
    ChangesSnapshot {
        root_path: Some(root_path),
        selected_path: diff.as_ref().map(|diff| diff.path.clone()),
        selected_committed: request.selected_committed && diff.is_some(),
        entries,
        committed: committed.unwrap_or_default(),
        base_branch,
        diff,
        unavailable_reason: None,
        stale_reason: None,
    }
}

fn reason(error: HostCallError) -> String {
    match error {
        HostCallError::Refused(error) => error.message,
        other => other.to_string(),
    }
}

fn absolute(root_path: &Path, relative: &str) -> String {
    root_path.join(relative).to_string_lossy().into_owned()
}

fn snapshot_of(root_path: &Path, file: ChangedFile) -> ChangedFileSnapshot {
    ChangedFileSnapshot {
        path: absolute(root_path, &file.path),
        relative_path: file.path,
        previous_relative_path: file.previous,
        status: match file.status {
            FileStatus::Modified => ChangedFileStatus::Modified,
            FileStatus::Added => ChangedFileStatus::Added,
            FileStatus::Deleted => ChangedFileStatus::Deleted,
            FileStatus::Untracked => ChangedFileStatus::Untracked,
            FileStatus::Renamed => ChangedFileStatus::Renamed,
            FileStatus::Conflict => ChangedFileStatus::Conflict,
        },
        added_lines: file.added,
        removed_lines: file.removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_access::InProcessHost;

    fn git(directory: &Path, arguments: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {arguments:?}");
    }

    fn request(checkout: &Path, folder: &Path, selected: Option<&Path>) -> ChangesRequest {
        ChangesRequest {
            root: DocumentRoot {
                device_id: crate::workspace::LOCAL_DEVICE_ID.to_owned(),
                path: checkout.to_string_lossy().into_owned(),
                identity: None,
            },
            channel: Ok(ChannelRef(Arc::new(InProcessHost))),
            root_path: folder.to_path_buf(),
            selected_path: selected.map(|path| path.to_string_lossy().into_owned()),
            selected_committed: false,
            base_branch: None,
        }
    }

    #[test]
    fn a_registered_folder_projects_absolute_paths_under_itself() {
        let temporary = tempfile::tempdir().unwrap();
        let checkout = temporary.path().canonicalize().unwrap();
        git(&checkout, &["init", "-q"]);
        let folder = checkout.join("registered");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("inside.txt"), "INSIDE\n").unwrap();
        std::fs::write(checkout.join("outside.txt"), "OUTSIDE\n").unwrap();

        let listed = read(&request(
            &checkout,
            &folder,
            Some(&folder.join("inside.txt")),
        ));
        assert_eq!(listed.unavailable_reason, None);
        assert_eq!(listed.entries.len(), 1);
        assert_eq!(listed.entries[0].relative_path, "inside.txt");
        assert_eq!(
            listed.entries[0].path,
            folder.join("inside.txt").to_string_lossy()
        );
        assert_eq!(listed.entries[0].status, ChangedFileStatus::Untracked);
        let diff = listed.diff.unwrap();
        assert_eq!(diff.path, folder.join("inside.txt").to_string_lossy());
        assert!(diff.text.contains("INSIDE"));
        assert_eq!(listed.selected_path.as_deref(), Some(diff.path.as_str()));
        assert!(listed.committed.is_empty());
        assert_eq!(listed.base_branch, None);
    }

    #[test]
    fn a_host_that_cannot_answer_states_why_instead_of_a_clean_tree() {
        let temporary = tempfile::tempdir().unwrap();
        let folder = temporary.path().canonicalize().unwrap();
        let mut unready = request(&folder, &folder, None);
        unready.channel = Err("The device helper is not ready".to_owned());
        let projected = read(&unready);
        assert_eq!(
            projected.unavailable_reason.as_deref(),
            Some("The device helper is not ready")
        );
        assert!(projected.entries.is_empty());

        // A folder with its own `.git` pointing nowhere is never a
        // repository, whatever contains the temporary directory.
        std::fs::write(folder.join(".git"), "gitdir: /nonexistent\n").unwrap();
        let projected = read(&request(&folder, &folder, None));
        assert!(projected.unavailable_reason.is_some());
        assert_eq!(projected.root_path.as_deref(), folder.to_str());
    }

    #[test]
    fn a_closed_view_publishes_an_empty_projection_without_a_key() {
        let mut reader = ChangesReader::new();
        for _ in 0..500 {
            if let Some(answer) = reader.read_if_due(None) {
                assert_eq!(answer.key, None);
                assert_eq!(answer.changes, ChangesSnapshot::default());
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("the reader never answered");
    }
}
